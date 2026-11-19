//! AI economy (04b §5-§6): worker allocation (`aiJobMix`, `aiNode`, `aiAssignPawns`), building placement
//! (`aiBuild`), plan / houses / camps / rebuild / farms, tech steps, extra factories, training (`aiNextUnit`),
//! market.

use super::intel::Intel;
use crate::World;
use crate::cmd::{Command, TradeOp};
use crate::defs::*;
use crate::math::{V2, v2};
use crate::slab::{NodeId, UnitId};
use crate::world::{GatherTarget, Order};
use crate::TILE;

/// Cost of a plan step (`(build ? BUILD_DEF : TECH_DEF)[id].cost`).
pub fn step_cost(s: PlanStep) -> Cost {
    match s {
        PlanStep::Build(k) => k.def().cost,
        PlanStep::Tech(t) => t.def().cost,
    }
}

/// First index of a build step for `k` in `AI_PLAN` (`AI_PLAN.findIndex(s => s[1] == k)`).
pub fn plan_index_of(k: BuildingKind) -> Option<usize> {
    AI_PLAN.iter().position(|s| *s == PlanStep::Build(k))
}

/// Builders sent to a new site: 2 when `work > 15`, else 1.
pub fn builders_for(k: BuildingKind) -> usize {
    if k.def().work > 15.0 { 2 } else { 1 }
}

/// Desired pawns for the faction's age (`L.pawns[f.age - 1]`).
pub fn pawns_wanted(level: &AiLevelDef, age: u8) -> u16 {
    level.pawns[(age.clamp(1, 3) - 1) as usize]
}

impl World {
    /// Live own pawns from the census list (dead/removed ones skipped).
    pub(crate) fn ai_live_pawns(&self, f: FactionIdx) -> Vec<UnitId> {
        self.factions[f as usize]
            .pawns
            .iter()
            .copied()
            .filter(|id| self.unit_alive(*id))
            .collect()
    }

    /// `aiJobMix(f)` (04b §6.1): normalized share per resource (index = `Res`). Reads the previous `ai.mix`.
    pub fn ai_job_mix(&self, f: FactionIdx) -> [f32; 4] {
        let fac = &self.factions[f as usize];
        let ai = &fac.ai;
        let l = ai.level.def();
        let mut need = [0.5f32, 0.15, 0.35, if fac.age > 1 { 0.12 } else { 0.0 }];
        let add = |need: &mut [f32; 4], c: &Cost, w: f32| {
            let s = c.total() as f32;
            if s <= 0.0 {
                return;
            }
            for r in Res::ALL {
                need[*r as usize] += w * c.get(*r) as f32 / s;
            }
        };
        let step = AI_PLAN.get(ai.step as usize).copied();
        if let Some(s) = step {
            add(&mut need, &step_cost(s), 1.0);
        }
        if fac.age == 1 && step != Some(PlanStep::Tech(TechId::Age2)) {
            add(&mut need, &TechId::Age2.def().cost, 0.6);
        }
        let pawns = fac.pawns.len();
        if pawns < pawns_wanted(l, fac.age) as usize {
            add(&mut need, &UnitKind::Pawn.def().cost, 0.5);
        }
        if let Some(mix) = &ai.mix {
            let k = if fac.age > 1 { 1.5 } else { 0.8 };
            for t in UnitKind::ALL {
                let m = mix[*t as usize];
                if m > 0.0 {
                    add(&mut need, &t.def().cost, m * k);
                }
            }
        }
        let mut all = 0.0;
        for r in Res::ALL {
            let have = fac.res[*r];
            if have > 800.0 {
                need[*r as usize] *= 0.25;
            } else if have > 400.0 {
                need[*r as usize] *= 0.5;
            }
            all += need[*r as usize];
        }
        if all > 0.0 {
            for v in need.iter_mut() {
                *v /= all;
            }
        }
        need
    }

    /// `aiNode(f, res, p)` (04b §6.1): valid, roomy node of `res` minimizing
    /// `dist(node, castle) + 0.3 * dist(node, pawn)`, below 2200 (no fog for the AI).
    pub fn ai_node(&self, f: FactionIdx, res: Res, p: V2) -> Option<GatherTarget> {
        let c = self.ai_castle(f)?;
        let mut best = None;
        let mut bd = 2200.0f32;
        if res == Res::Meat {
            for (id, s) in self.sheep.iter() {
                if s.dead || s.users >= SHEEP_USERS {
                    continue;
                }
                let d = s.pos.dist(c) + s.pos.dist(p) * 0.3;
                if d < bd {
                    bd = d;
                    best = Some(GatherTarget::Sheep(id));
                }
            }
            return best;
        }
        for (i, n) in self.nodes.iter().enumerate() {
            if n.res() != res || !n.valid() || !n.room() {
                continue;
            }
            // cheap reject before the two square roots
            let dc = n.pos.dist(c);
            if dc >= bd {
                continue;
            }
            let d = dc + n.pos.dist(p) * 0.3;
            if d < bd {
                bd = d;
                best = Some(GatherTarget::Node(NodeId(i as u32)));
            }
        }
        best
    }

    /// `aiAssignPawns(f, dt)` (04b §6.1).
    pub fn ai_assign_pawns(&mut self, f: FactionIdx, dt: f32) {
        let pawns = self.ai_live_pawns(f);
        let n = pawns.len() as f32;
        let mut jobs = [0f32; 4];
        let mut idlers: Vec<UnitId> = Vec::new();
        for &p in &pawns {
            match &self.units.get(p).map(|u| &u.order) {
                Some(Order::Gather(g)) => jobs[g.res as usize] += 1.0,
                Some(Order::Idle { .. }) => idlers.push(p),
                _ => {}
            }
        }
        let mix = self.ai_job_mix(f);
        let lack = |jobs: &[f32; 4], k: Res| mix[k as usize] * n - jobs[k as usize];
        let neediest = |jobs: &[f32; 4]| {
            let mut a = Res::ALL[0];
            for &b in &Res::ALL[1..] {
                if lack(jobs, b) > lack(jobs, a) {
                    a = b;
                }
            }
            a
        };
        let ai = &mut self.factions[f as usize].ai;
        ai.shuffle_t -= dt;
        if ai.shuffle_t <= 0.0 && idlers.is_empty() {
            ai.shuffle_t = 6.0;
            let mut over = Res::ALL[0];
            for &b in &Res::ALL[1..] {
                if lack(&jobs, b) < lack(&jobs, over) {
                    over = b;
                }
            }
            if lack(&jobs, over) < -1.0 && lack(&jobs, neediest(&jobs)) > 1.0 {
                let p = pawns.iter().copied().find(|id| {
                    self.units.get(*id).is_some_and(|u| {
                        u.carry.is_none() && matches!(&u.order, Order::Gather(g) if g.res == over)
                    })
                });
                if let Some(p) = p {
                    jobs[over as usize] -= 1.0;
                    idlers.push(p);
                }
            }
        }
        for p in idlers {
            let Some(pos) = self.units.get(p).map(|u| u.pos) else {
                continue;
            };
            let mut order: [Res; 4] = [Res::Wood, Res::Gold, Res::Meat, Res::Stone];
            // stable sort by lack descending
            order.sort_by(|a, b| {
                lack(&jobs, *b)
                    .partial_cmp(&lack(&jobs, *a))
                    .unwrap_or(core::cmp::Ordering::Equal)
            });
            for res in order {
                let Some(node) = self.ai_node(f, res, pos) else {
                    continue;
                };
                if let Some(o) = self.gather_order(node) {
                    self.set_order(p, o);
                    jobs[res as usize] += 1.0;
                    break;
                }
            }
        }
    }

    /// `aiBuild(f, kind, toward)` (04b §5): pick a spot (depot: next to the closest resource without a drop
    /// site; tower: toward the enemy) and place a site with the 1-2 nearest pawns. True if the site was placed.
    pub fn ai_build(&mut self, f: FactionIdx, kind: BuildingKind, toward: Option<V2>) -> bool {
        let def = kind.def();
        let Some(c) = self.ai_castle(f) else {
            return false;
        };
        let pawns = self.ai_live_pawns(f);
        {
            let fac = &self.factions[f as usize];
            if fac.age < def.age || !fac.res.can_afford(&def.cost) || pawns.is_empty() {
                return false;
            }
        }
        let (bx, by) = self.factions[f as usize].base;
        let (mut x, mut y, mut rmin) = (bx, by + 1, 4);
        if kind == BuildingKind::Depot {
            let mut best: Option<V2> = None;
            let mut bd = f32::INFINITY;
            let drops: Vec<crate::math::Rect> = self
                .buildings
                .values()
                .filter(|b| b.faction == f && !b.dead && !b.def().drop.is_empty())
                .map(|b| b.rect)
                .collect();
            for n in &self.nodes {
                // trees, golds, stones: the node vector is already in this order
                if !n.valid() {
                    continue;
                }
                let d = n.pos.dist(c);
                if d < 420.0 || d > bd || d > (BUILD_RADIUS - 4.0) * TILE {
                    continue;
                }
                if drops.iter().any(|r| r.dist(n.pos) < 380.0) {
                    continue;
                }
                bd = d;
                best = Some(n.pos);
            }
            if let Some(p) = best {
                x = (p.x / TILE).floor() as i32;
                y = (p.y / TILE).floor() as i32;
                rmin = 2;
            }
        } else if kind == BuildingKind::Tower {
            let foe = toward.or_else(|| {
                self.ai_nearest_enemy(f)
                    .and_then(|o| self.ai_castle(o))
            });
            if let Some(foe) = foe {
                let a = (foe.y - c.y).atan2(foe.x - c.x);
                // JS Math.round (halves toward +inf, e.g. -3.5 → -3)
                x += super::util::js_round(a.cos() * 7.0);
                y += super::util::js_round(a.sin() * 6.0);
                rmin = 1;
            }
        }
        let Some((sx, sy)) = self.ai_find_spot(kind, x, y, rmin, 18) else {
            return false;
        };
        let spot = v2(sx as f32 * TILE, sy as f32 * TILE);
        let mut near: Vec<(UnitId, f32)> = pawns
            .iter()
            .filter_map(|id| self.units.get(*id).map(|u| (*id, u.pos.dist(spot))))
            .collect();
        near.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(core::cmp::Ordering::Equal));
        let builders: Vec<UnitId> = near
            .iter()
            .take(builders_for(kind))
            .map(|(id, _)| *id)
            .collect();
        self.exec_command(
            f,
            &Command::Place {
                kind,
                cx: sx,
                cy: sy,
                builders,
            },
        )
        .is_ok()
    }

    /// Plan, houses, camps, rebuilding, extra farms and the tech step (04b §4, §6.2). `open` = open sites
    /// counted at the start of the think; returns the plan step cost used by defense and training.
    pub fn ai_plan(&mut self, f: FactionIdx, open: u32) -> Option<Cost> {
        let l = self.factions[f as usize].ai.level.def();
        // skip rule (one per think)
        {
            let fac = &mut self.factions[f as usize];
            if let Some(PlanStep::Build(k)) = AI_PLAN.get(fac.ai.step as usize).copied() {
                let cap = match k {
                    BuildingKind::Farm => Some(l.farms),
                    BuildingKind::Tower => Some(l.towers),
                    _ => None,
                };
                if let Some(cap) = cap
                    && fac.has[k as usize] + fac.sites[k as usize] >= cap
                {
                    fac.ai.step += 1;
                }
            }
        }
        let step = AI_PLAN
            .get(self.factions[f as usize].ai.step as usize)
            .copied();
        let cost = step.map(step_cost);
        let coming_pawns = self.ai_queued(f, true);
        let coming_troops = self.ai_queued(f, false);
        let fac = &self.factions[f as usize];
        let (age, pawn_cap, troop_cap) = (fac.age, fac.pawn_cap, fac.troop_cap);
        let (pawn_n, troop_n) = (fac.pawn_n as u32, fac.troop_n as u32);
        let has = fac.has;
        let sites = fac.sites;
        let ai_step = fac.ai.step as usize;
        let wanted = pawns_wanted(l, age);
        if sites[BuildingKind::House as usize] == 0
            && pawn_cap < PAWN_MAX
            && pawn_n + coming_pawns + 2 >= pawn_cap as u32
            && wanted + 2 > pawn_cap
        {
            self.ai_build(f, BuildingKind::House, None);
        } else if sites[BuildingKind::Camp as usize] == 0
            && troop_cap < TROOP_MAX
            && troop_n + coming_troops + 2 >= troop_cap as u32
            && l.army + 2 > troop_cap
        {
            self.ai_build(f, BuildingKind::Camp, None);
        } else if open < 2 {
            let lost = [BuildingKind::Farm, BuildingKind::Barracks, BuildingKind::Depot]
                .into_iter()
                .find(|k| {
                    has[*k as usize] == 0
                        && sites[*k as usize] == 0
                        && plan_index_of(*k).is_some_and(|i| i < ai_step)
                });
            let farms = has[BuildingKind::Farm as usize] + sites[BuildingKind::Farm as usize];
            if let Some(k) = lost {
                self.ai_build(f, k, None);
            } else if farms > 0
                && farms < l.farms
                && sites[BuildingKind::Farm as usize] == 0
                && farms as f32 * 2.5 + 2.0
                    < self.ai_job_mix(f)[Res::Meat as usize] * self.factions[f as usize].pawns.len() as f32
            {
                self.ai_build(f, BuildingKind::Farm, None);
            } else if let Some(PlanStep::Build(k)) = step
                && self.ai_build(f, k, None)
            {
                self.factions[f as usize].ai.step += 1;
            }
        }
        if let Some(PlanStep::Tech(goal)) = step {
            self.ai_tech_step(f, goal, open);
        }
        cost
    }

    /// Tech step of the plan (04b §6.2): prerequisites first, missing research building built first.
    fn ai_tech_step(&mut self, f: FactionIdx, goal: TechId, open: u32) {
        let fac = &self.factions[f as usize];
        let mut id = goal;
        while let Some(r) = id.def().req
            && !fac.techs.has(r)
        {
            id = r;
        }
        let t = id.def();
        let b = self.free_building(f, t.at);
        if fac.techs.has(goal) || fac.busy.has(goal) {
            self.factions[f as usize].ai.step += 1;
        } else if fac.busy.has(id) {
            // prerequisite in progress
        } else if let Some(b) = b {
            let qlen = self.buildings.get(b).map_or(usize::MAX, |b| b.queue.len());
            if qlen < 2
                && self
                    .exec_command(f, &Command::Research { building: b, tech: id })
                    .is_ok()
                && id == goal
            {
                self.factions[f as usize].ai.step += 1;
            }
        } else if fac.sites[t.at as usize] == 0 && open < 2 {
            self.ai_build(f, t.at, None);
        }
    }

    /// Extra factories when rich (04b §6.3).
    pub fn ai_factories(&mut self, f: FactionIdx, open: u32) {
        let fac = &self.factions[f as usize];
        let Some(mix) = fac.ai.mix else {
            return;
        };
        if open >= 2
            || !Res::ALL
                .iter()
                .all(|k| *k == Res::Stone || fac.res[*k] > 500.0)
        {
            return;
        }
        let mut top: Option<UnitKind> = None;
        for t in UnitKind::ALL {
            let m = mix[*t as usize];
            if m > 0.0 && top.is_none_or(|b| m > mix[b as usize]) {
                top = Some(*t);
            }
        }
        let Some(top) = top else {
            return;
        };
        let kind = top.def().from;
        let l = fac.ai.level.def();
        if fac.has[kind as usize] + fac.sites[kind as usize] < l.factories {
            self.ai_build(f, kind, None);
        }
    }

    /// `aiNextUnit(f, intel, cheap)` (04b §6.5): monks, then siege, then fighters by lag behind the mix.
    pub fn ai_next_unit(&self, f: FactionIdx, intel: &Intel, cheap: bool) -> Option<UnitKind> {
        let fac = &self.factions[f as usize];
        let l = fac.ai.level.def();
        let mut have = [0u32; 9];
        for id in &fac.soldiers {
            if let Some(u) = self.units.get(*id)
                && !u.dead
            {
                have[u.kind as usize] += 1;
            }
        }
        for b in self.buildings.values() {
            if b.faction != f || b.dead {
                continue;
            }
            for q in &b.queue {
                if let crate::cmd::QueueWhat::Unit(k) = q.what {
                    have[k as usize] += 1;
                }
            }
        }
        let ok = |t: UnitKind| {
            let d = t.def();
            fac.has[d.from as usize] > 0 && fac.age >= d.age
        };
        let fighters: u32 = UnitKind::ALL
            .iter()
            .filter(|t| ai_fighter(**t))
            .map(|t| have[*t as usize])
            .sum();
        let mut list: Vec<UnitKind> = Vec::with_capacity(8);
        for &t in UnitKind::ALL {
            if t.def().is_healer() && ok(t) && have[t as usize] < (l.monks as u32).min(fighters / 5) {
                list.push(t);
            }
        }
        let siege: Vec<UnitKind> = UnitKind::ALL
            .iter()
            .copied()
            .filter(|t| t.def().siege && ok(*t))
            .collect();
        let sieges: u32 = siege.iter().map(|t| have[*t as usize]).sum();
        if !siege.is_empty() && fighters >= 6 && sieges < (l.siege as u32).min(fighters / 6) {
            let hunted = |t: UnitKind| {
                if intel.total == 0.0 {
                    return 0.0;
                }
                let mut v = 0.0;
                for e in &intel.foes {
                    for k in UnitKind::ALL {
                        let m = e.mix[*k as usize];
                        if m != 0.0 && k.def().bonus_vs(t) > 1.0 {
                            v += m;
                        }
                    }
                }
                v / intel.total
            };
            let mut opts: Vec<UnitKind> = siege
                .iter()
                .copied()
                .filter(|t| have[*t as usize] == 0 && (t.def().only_buildings || hunted(*t) < 0.3))
                .collect();
            // onlyBuildings first (stable)
            opts.sort_by_key(|t| if t.def().only_buildings { 0 } else { 1 });
            list.extend(opts);
        }
        if let Some(want) = &fac.ai.mix {
            let lag = |t: UnitKind| want[t as usize] * (fighters as f32 + 1.0) - have[t as usize] as f32;
            let mut keys: Vec<UnitKind> = UnitKind::ALL
                .iter()
                .copied()
                .filter(|t| want[*t as usize] > 0.0)
                .collect();
            keys.sort_by(|a, b| lag(*b).partial_cmp(&lag(*a)).unwrap_or(core::cmp::Ordering::Equal));
            list.extend(keys);
        }
        if cheap {
            list.into_iter().find(|t| fac.res.can_afford(&t.def().cost))
        } else {
            list.first().copied()
        }
    }

    /// Training (04b §6.4): at most one unit per think, pawns first.
    pub fn ai_train(&mut self, f: FactionIdx, intel: &Intel, step_cost: Option<Cost>) {
        let Some(castle) = self.factions[f as usize].castle else {
            return;
        };
        let coming_pawns = self.ai_queued(f, true);
        let coming_troops = self.ai_queued(f, false);
        let fac = &self.factions[f as usize];
        let l = fac.ai.level.def();
        let soldiers = fac.soldiers.len();
        if (fac.pawn_n as u32 + coming_pawns) < pawns_wanted(l, fac.age).min(fac.pawn_cap) as u32
            && !(intel.danger > intel.home_value && soldiers < 4)
        {
            if self.buildings.get(castle).is_some_and(|c| c.queue.len() < 2) {
                let _ = self.exec_command(
                    f,
                    &Command::Train {
                        building: castle,
                        unit: UnitKind::Pawn,
                    },
                );
            }
        } else if (fac.troop_n as u32 + coming_troops) < l.army.min(fac.troop_cap) as u32 {
            let mut t = self.ai_next_unit(f, intel, false);
            let fac = &self.factions[f as usize];
            if let Some(k) = t
                && fac.res.can_afford(&k.def().cost)
            {
                self.factions[f as usize].ai.lack_t = self.time;
            } else if self.time - fac.ai.lack_t > 12.0 {
                t = self.ai_next_unit(f, intel, true);
            }
            let Some(t) = t else {
                return;
            };
            let d = t.def();
            let Some(b) = self.free_building(f, d.from) else {
                return;
            };
            let fac = &self.factions[f as usize];
            let hold = if fac.age == 1 && !fac.busy.has(TechId::Age2) {
                Some(TechId::Age2.def().cost)
            } else {
                step_cost
            };
            let saving = hold.is_some_and(|h| {
                intel.danger == 0.0
                    && fac.soldiers.len() >= 3
                    && Res::ALL.iter().any(|k| {
                        let c = d.cost.get(*k) as f64;
                        c > 0.0 && fac.res[*k] - c < h.get(*k) as f64
                    })
            });
            if !saving && self.buildings.get(b).is_some_and(|b| b.queue.len() < 2) {
                let _ = self.exec_command(f, &Command::Train { building: b, unit: t });
            }
        }
    }

    /// Market (04b §6.6): one trade per think.
    pub fn ai_market(&mut self, f: FactionIdx) {
        let fac = &self.factions[f as usize];
        if fac.has[BuildingKind::Market as usize] == 0 {
            return;
        }
        let Some(market) = self.free_building(f, BuildingKind::Market) else {
            return;
        };
        let l = fac.ai.level.def();
        let gold = fac.res[Res::Gold];
        let limit = if gold < 80.0 { 450.0 } else { 700.0 };
        let extra = [Res::Wood, Res::Meat, Res::Stone]
            .into_iter()
            .find(|k| fac.res[*k] > limit && sell_price(fac.trades[*k as usize]) >= 40.0);
        if let Some(res) = extra
            && gold < if l.trade { 250.0 } else { 80.0 }
        {
            let _ = self.exec_command(
                f,
                &Command::Trade {
                    market,
                    op: TradeOp::Sell,
                    res,
                },
            );
        } else if l.trade
            && fac.res[Res::Meat] < 150.0
            && gold > 450.0
            && buy_price(fac.trades[Res::Meat as usize]) <= 170.0
        {
            let _ = self.exec_command(
                f,
                &Command::Trade {
                    market,
                    op: TradeOp::Buy,
                    res: Res::Meat,
                },
            );
        }
    }
}

