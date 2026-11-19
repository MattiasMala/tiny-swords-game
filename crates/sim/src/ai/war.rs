//! AI warfare (04b §9): spare soldiers, target choice, wave launch / march / assault / retreat, defense
//! (rally, recall, emergency tower), capture squads, strays.

use super::intel::Intel;
use super::{AiWave, WavePhase};
use crate::World;
use crate::cmd::OrderKind;
use crate::defs::*;
use crate::events::{GameEvent, Msg, dur};
use crate::math::{V2, v2};
use crate::slab::UnitId;
use crate::world::Order;

/// Result of `aiSpare` (04b §9.1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spare {
    /// free fighters minus the garrison: melee first, strongest first
    pub squad: Vec<UnitId>,
    /// free monks at home
    pub monks: Vec<UnitId>,
    /// own fighters alive (`aiFighter`: rams, monks, scouts excluded)
    pub total: u32,
}

impl World {
    /// Unit is a member of the running wave.
    pub(crate) fn ai_in_wave(&self, f: FactionIdx, id: UnitId) -> bool {
        self.factions[f as usize]
            .ai
            .wave
            .as_ref()
            .is_some_and(|w| w.ids.contains(&id))
    }

    /// Own castle front point `(c.x, c.y1 + dy)`.
    fn castle_front(&self, f: FactionIdx, dy: f32) -> Option<V2> {
        let c = self.factions.get(f as usize)?.castle?;
        self.buildings
            .get(c)
            .filter(|b| !b.dead)
            .map(|b| v2(b.pos.x, b.rect.y1 + dy))
    }

    /// `aiSpare(f, intel)` (04b §9.1).
    pub fn ai_spare(&self, f: FactionIdx, intel: &Intel) -> Spare {
        let fac = &self.factions[f as usize];
        let l = fac.ai.level.def();
        let mut fighters: Vec<(UnitId, bool, f32)> = Vec::new();
        let mut monks = Vec::new();
        for &id in &intel.home {
            let Some(u) = self.units.get(id) else {
                continue;
            };
            if u.dead || !u.order.is_idle() || u.target.is_some() || self.ai_in_wave(f, id) {
                continue;
            }
            if u.def().has_dmg() {
                fighters.push((id, u.def().is_ranged(), self.ai_worth(id)));
            }
            if u.def().is_healer() {
                monks.push(id);
            }
        }
        let mut total = 0u32;
        for id in &fac.soldiers {
            if self.units.get(*id).is_some_and(|u| !u.dead && ai_fighter(u.kind)) {
                total += 1;
            }
        }
        let mut busy = 0i32;
        for &id in &intel.home {
            let Some(u) = self.units.get(id) else {
                continue;
            };
            if u.def().has_dmg()
                && !fighters.iter().any(|(s, _, _)| *s == id)
                && !self.ai_in_wave(f, id)
            {
                busy += 1;
            }
        }
        let recent = if self.time - fac.ai.attacked_t < 90.0 {
            0.15
        } else {
            0.0
        };
        let keep = 2i32.max((total as f32 * (l.garrison + recent)).round() as i32);
        // stable: melee first, then worth descending
        fighters.sort_by(|a, b| {
            (a.1 as u8)
                .cmp(&(b.1 as u8))
                .then(b.2.partial_cmp(&a.2).unwrap_or(core::cmp::Ordering::Equal))
        });
        let n = (fighters.len() as i32 - (keep - busy).max(0)).max(0) as usize;
        Spare {
            squad: fighters.into_iter().take(n).map(|(id, _, _)| id).collect(),
            monks,
            total,
        }
    }

    /// `chooseTarget(f, intel, power)` (04b §9.2): weighted draw with the sim RNG (addenda A4).
    pub fn ai_choose_target(&mut self, f: FactionIdx, intel: Option<&Intel>, power: f32) -> Option<FactionIdx> {
        let c = self.ai_castle(f)?;
        let mut opts: Vec<FactionIdx> = Vec::new();
        let mut w: Vec<f32> = Vec::new();
        let mut total = 0.0f32;
        for o in &self.factions {
            if o.idx == f || !o.alive {
                continue;
            }
            let oc = self.ai_castle(o.idx).unwrap_or(c);
            let mut v = 1.0 / (1.0 + oc.dist2(c) / 4e6);
            if let Some(e) = intel.and_then(|i| i.foe(o.idx)) {
                let denom = power + e.defense;
                v *= 0.3 + if denom > 0.0 { power / denom } else { 0.0 };
            }
            if self.time - o.raided_t < RAID_SPACING {
                v *= 0.1;
            }
            total += v;
            opts.push(o.idx);
            w.push(v);
        }
        let mut r = self.rng.f32() * total;
        for (i, o) in opts.iter().enumerate() {
            r -= w[i];
            if r <= 0.0 {
                return Some(*o);
            }
        }
        opts.first().copied()
    }

    /// `aiLaunch(f, intel)` (04b §9.3). True if a wave left.
    pub fn ai_launch(&mut self, f: FactionIdx, intel: &Intel) -> bool {
        let Spare {
            mut squad,
            monks,
            total,
        } = self.ai_spare(f, intel);
        let coming_troops = self.ai_queued(f, false);
        let (mut wave_need, due_t, capped) = {
            let fac = &self.factions[f as usize];
            let ai = &fac.ai;
            let capped = fac.troop_n as u32 + coming_troops >= ai.level.def().army as u32;
            (ai.wave_need, ai.due_t, capped)
        };
        // DEVIATION (reviewer fix): `army` counts monks and siege, the garrison stays home, so a full army can have
        // fewer spare fighters than the capped `waveNeed` (easy: 9 troops = 7 fighters + monk + ram, keep 2 →
        // squad 6 < 7; hard: squad 19-23 < 24) and the JS AI never attacks again. With the army at its cap (no
        // more soldiers can come) the size floor falls back to the initial one; the power check still applies.
        if capped {
            wave_need = wave_need.min(AI_WAVE_NEED_START);
        }
        if squad.len() < wave_need as usize {
            return false;
        }
        let power: f32 = squad.iter().map(|id| self.ai_worth(*id)).sum();
        let Some(target) = self.ai_choose_target(f, Some(intel), power) else {
            return false;
        };
        let Some(def) = intel.foe(target).map(|e| e.defense) else {
            return false;
        };
        let l = self.factions[f as usize].ai.level.def();
        let full = total + coming_troops >= l.army as u32;
        let late = self.time - due_t.unwrap_or(self.time);
        if power < l.odds * def * 0.5f32.max(if full { 0.6 } else { 1.0 } - late / 300.0) {
            return false;
        }
        let take = (monks.len() as f32 * 0.7).ceil() as usize;
        squad.extend(monks.into_iter().take(take));
        let mut ids: Vec<UnitId> = Vec::with_capacity(squad.len());
        for id in squad {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let gap = self.rng.range(l.gap.0, l.gap.1);
        let now = self.time;
        {
            let ai = &mut self.factions[f as usize].ai;
            ai.wave = Some(AiWave {
                ids,
                target,
                start: power,
                phase: WavePhase::March,
                leg_t: -99.0,
                t0: now,
            });
            ai.due_t = None;
            ai.target = Some(target);
            ai.wave_t = gap;
            ai.wave_need = l.wave_need_cap().min(ai.wave_need + 2);
        }
        self.factions[target as usize].raided_t = now;
        if self.factions[target as usize].human {
            let color = self.factions[f as usize].color;
            self.emit(GameEvent::Say {
                to: Some(target),
                msg: Msg::Attacks { color },
                color: Some(f),
                dur: dur::ATTACKS,
            });
            self.emit(GameEvent::Play {
                to: target,
                sound: Sound::Horn,
            });
        }
        true
    }

    /// `aiMarch(f, intel)` (04b §9.4): legs toward the target, assault within 900 px, retreat when decimated,
    /// reinforcements.
    pub fn ai_march(&mut self, f: FactionIdx, intel: &Intel) {
        let Some(mut w) = self.factions[f as usize].ai.wave.take() else {
            return;
        };
        let Some(home_pt) = self.castle_front(f, RECALL_DY) else {
            return;
        };
        let l = self.factions[f as usize].ai.level.def();
        // live members (others dropped from the set)
        w.ids.retain(|id| self.units.get(*id).is_some_and(|u| !u.dead && u.faction == f));
        let units = w.ids.clone();
        let fighters: Vec<UnitId> = units
            .iter()
            .copied()
            .filter(|id| self.units.get(*id).is_some_and(|u| u.def().has_dmg()))
            .collect();
        if fighters.is_empty() {
            if !units.is_empty() {
                self.order_units(f, &units, OrderKind::AMove, home_pt.x, home_pt.y, None);
            }
            // wave stays None
            return;
        }
        let mut value = 0.0;
        let mut cen = V2::ZERO;
        for &id in &fighters {
            value += self.ai_worth(id);
            if let Some(u) = self.units.get(id) {
                cen += u.pos;
            }
        }
        cen /= fighters.len() as f32;
        let target_ok = |s: &World, t: FactionIdx| s.factions[t as usize].alive && s.ai_castle(t).is_some();
        if !target_ok(self, w.target) {
            let next = if value >= 0.5 * w.start {
                self.ai_nearest_enemy(f)
            } else {
                None
            };
            let next = next.filter(|n| self.ai_castle(*n).is_some_and(|p| p.dist(cen) <= 2600.0));
            let Some(next) = next else {
                self.order_units(f, &units, OrderKind::AMove, home_pt.x, home_pt.y, None);
                return;
            };
            w.target = next;
            self.factions[f as usize].ai.target = Some(next);
            w.phase = WavePhase::March;
            w.leg_t = -99.0;
        }
        let tgt = self.factions[w.target as usize]
            .castle
            .and_then(|c| self.buildings.get(c))
            .filter(|b| !b.dead)
            .map(|b| (b.pos, b.rect.y1, b.hp, b.max_hp));
        let Some((tc, tc_y1, tc_hp, tc_max)) = tgt else {
            // unreachable: target_ok checked the castle; keep the wave for the next think
            self.factions[f as usize].ai.wave = Some(w);
            return;
        };
        let d_t = tc.dist(cen);
        if value < l.retreat * w.start && tc_hp > 0.3 * tc_max {
            self.order_units(f, &units, OrderKind::Move, home_pt.x, home_pt.y, None);
            let ai = &mut self.factions[f as usize].ai;
            ai.wave_t = ai.wave_t.max(40.0);
            return;
        }
        let assault_pt = v2(tc.x, tc_y1 + 30.0);
        if w.phase == WavePhase::March {
            if d_t < 900.0 {
                w.phase = WavePhase::Assault;
                self.order_units(f, &units, OrderKind::AMove, assault_pt.x, assault_pt.y, None);
                self.factions[f as usize].ai.wave = Some(w);
                return;
            }
            let waiting = fighters
                .iter()
                .filter(|id| {
                    self.units
                        .get(**id)
                        .is_some_and(|u| u.order.is_idle() && u.target.is_none())
                })
                .count();
            if waiting as f32 >= 0.75 * fighters.len() as f32 || self.time - w.leg_t > 35.0 {
                let step = 700.0f32.min(d_t - 500.0);
                let p = self.map.nearest_passable(cen + (tc - cen) / d_t * step);
                self.order_units(f, &units, OrderKind::AMove, p.x, p.y, None);
                w.leg_t = self.time;
            }
            self.factions[f as usize].ai.wave = Some(w);
            return;
        }
        // assault
        let idle: Vec<UnitId> = units
            .iter()
            .copied()
            .filter(|id| {
                self.units.get(*id).is_some_and(|u| {
                    u.order.is_idle() && u.target.is_none() && u.pos.dist(tc) > 200.0
                })
            })
            .collect();
        if !idle.is_empty() {
            self.order_units(f, &idle, OrderKind::AMove, assault_pt.x, assault_pt.y, None);
        }
        if l.reinforce && intel.danger == 0.0 {
            // aiSpare must see the current wave membership
            self.factions[f as usize].ai.wave = Some(w);
            let squad = self.ai_spare(f, intel).squad;
            let Some(mut w2) = self.factions[f as usize].ai.wave.take() else {
                return;
            };
            if squad.len() >= 3 {
                for &s in &squad {
                    if !w2.ids.contains(&s) {
                        w2.ids.push(s);
                    }
                    w2.start += self.ai_worth(s);
                }
                self.order_units(f, &squad, OrderKind::AMove, assault_pt.x, assault_pt.y, None);
            }
            w = w2;
        }
        self.factions[f as usize].ai.wave = Some(w);
    }

    /// `aiDefend(f, intel, stepCost)` (04b §9.5).
    pub fn ai_defend(&mut self, f: FactionIdx, intel: &Intel, step_cost: Option<Cost>) {
        if intel.danger == 0.0 {
            return;
        }
        let Some(at) = intel.danger_at else {
            return;
        };
        self.factions[f as usize].ai.attacked_t = self.time;
        let l = self.factions[f as usize].ai.level.def();
        if l.rally {
            let free: Vec<UnitId> = intel
                .home
                .iter()
                .copied()
                .filter(|id| {
                    self.units.get(*id).is_some_and(|u| {
                        !u.dead
                            && u.def().has_dmg()
                            && !u.def().only_buildings
                            && u.target.is_none()
                            && u.order.is_idle()
                    }) && !self.ai_in_wave(f, *id)
                })
                .collect();
            if !free.is_empty() {
                self.order_units(f, &free, OrderKind::AMove, at.x, at.y, None);
            }
        }
        if l.recall
            && let Some(w) = &self.factions[f as usize].ai.wave
            && intel.danger > 0.8 * intel.home_value + 250.0
        {
            let t = w.target;
            let castle = self.factions[t as usize]
                .castle
                .and_then(|c| self.buildings.get(c))
                .filter(|b| !b.dead);
            let go_home = !self.factions[t as usize].alive
                || castle.is_none_or(|c| c.hp > 0.3 * c.max_hp);
            if go_home {
                let ids = w.ids.clone();
                self.order_units(f, &ids, OrderKind::AMove, at.x, at.y, None);
                let ai = &mut self.factions[f as usize].ai;
                ai.wave = None;
                ai.wave_t = ai.wave_t.max(30.0);
            }
        }
        let fac = &self.factions[f as usize];
        let tower = BuildingKind::Tower;
        if fac.sites[tower as usize] == 0
            && fac.has[tower as usize] < l.towers
            && fac.res[Res::Stone] - tower.def().cost.stone as f64
                >= step_cost.map_or(0.0, |c| c.stone as f64)
        {
            self.ai_build(f, tower, Some(at));
        }
    }

    /// `aiCapture(f, intel)` (04b §9.6): a squad of 3 toward the closest point not ours.
    pub fn ai_capture(&mut self, f: FactionIdx, intel: &Intel) {
        let ai = &self.factions[f as usize].ai;
        if !ai.level.def().capture || ai.wave.is_some() || intel.danger != 0.0 || self.time < ai.capture_t {
            return;
        }
        self.factions[f as usize].ai.capture_t = self.time + 25.0;
        let squad = self.ai_spare(f, intel).squad;
        if squad.len() < 3 {
            return;
        }
        let Some(c) = self.ai_castle(f) else {
            return;
        };
        let age = self.factions[f as usize].age;
        let mut best = None;
        let mut bd = 2200.0 + 600.0 * age as f32;
        for p in &self.points {
            let d = p.pos.dist(c);
            if p.owner != Some(f) && d < bd {
                bd = d;
                best = Some(v2(p.pos.x, p.rect.y1 + 40.0));
            }
        }
        if let Some(p) = best {
            self.order_units(f, &squad[..3], OrderKind::AMove, p.x, p.y, None);
        }
    }

    /// Strays (04b §9.7): idle soldiers far from home (not in the wave, not on a point being captured) go back.
    pub fn ai_strays(&mut self, f: FactionIdx) {
        let Some(c) = self.ai_castle(f) else {
            return;
        };
        let Some(home_pt) = self.castle_front(f, RECALL_DY) else {
            return;
        };
        let stray: Vec<UnitId> = self.factions[f as usize]
            .soldiers
            .iter()
            .copied()
            .filter(|id| {
                let Some(u) = self.units.get(*id) else {
                    return false;
                };
                !u.dead
                    && matches!(u.order, Order::Idle { .. })
                    && u.target.is_none()
                    && !self.ai_in_wave(f, *id)
                    && u.pos.dist(c) > 900.0
                    && !self
                        .points
                        .iter()
                        .any(|p| p.owner != Some(f) && p.pos.dist(u.pos) < CAPTURE_RADIUS)
            })
            .collect();
        if !stray.is_empty() {
            self.order_units(f, &stray, OrderKind::AMove, home_pt.x, home_pt.y, None);
        }
    }
}
