// OWNER: S3 (economy) — see docs/spec/03b-gather-buildings.md §2 (nodes: nodeValid/Room, chopTree, updateResources
// for trees/veins, pickups §2.4), §3 (claim, gatherOrder, setOrder side effects), §4 (dropSite, findNode,
// gatherStep, deposit, workOn), §5 (updatePawn), §6 (threatNear, shelterFor, enter/leaveShelter, shelterStep,
// resumeWork, embolden), §4.7 nodeNear; addenda G2-G8. Sheep live in sheep.rs; §12 estimateRates is in faction.rs (S4).
//
// Everything here is a complete port. Notes:
// * `threat_near` reads the spatial grid (rebuilt at the start of the unit phase) with a safety margin and re-checks
//   exact positions; `threat_near_scan` is the exact full scan used outside the unit phase (commands).
// * Pickups and capture scans iterate units in slab order ("first unit in G.units order wins").
#![allow(dead_code)]

use crate::TICK_DT;
use crate::World;
use crate::defs::*;
use crate::events::{GameEvent, Msg};
use crate::math::{V2, v2};
use crate::path::FieldTarget;
use crate::slab::{BuildingId, NodeId, PickupId, UnitId};
use crate::unit::NavResult;
use crate::world::{Carry, GatherOrder, GatherTarget, Order, Pickup, UnitState};

/// Extra px around a grid query: units move a little between the grid rebuild and the query.
const GRID_MARGIN: f32 = 32.0;

impl World {
    /// `updatePawn(u, dt)` (§5): fear check every 0.4 s → shelter; tool; pause; gather / build|repair / drop /
    /// idle / move.
    pub fn update_pawn(&mut self, id: UnitId) {
        let dt = TICK_DT;
        let Some(u) = self.units.get_mut(id) else {
            return;
        };
        u.fear_t -= dt;
        if u.fear_t <= 0.0 {
            u.fear_t = FEAR_PERIOD;
            let (f, kind, pos, inside, brave) = (u.faction, u.kind, u.pos, u.inside, u.brave);
            let centre = inside
                .and_then(|b| self.buildings.get(b))
                .map(|b| b.pos)
                .unwrap_or(pos);
            let extra = if inside.is_some() || brave {
                2.0 * crate::TILE
            } else {
                0.0
            };
            let r = self.factions[f as usize].mods.unit_sight_px(kind) + extra;
            let seen = self.threat_near(f, centre, r);
            let Some(u) = self.units.get_mut(id) else {
                return;
            };
            let mut flee = false;
            if let Order::Shelter { calm, .. } = &mut u.order {
                *calm = if seen { 0.0 } else { *calm + FEAR_PERIOD };
            } else if u.brave {
                u.calm_t = if seen { 0.0 } else { u.calm_t + FEAR_PERIOD };
                if u.calm_t >= CALM_TIME {
                    u.brave = false;
                }
            } else {
                flee = seen;
            }
            if flee && let Some(to) = self.shelter_for(id) {
                let back = self.units.get(id).map(|u| u.order.clone());
                if let Some(back) = back {
                    self.set_order(
                        id,
                        Order::Shelter {
                            building: Some(to),
                            back: Box::new(back),
                            calm: 0.0,
                        },
                    );
                }
            }
        }
        let Some(u) = self.units.get_mut(id) else {
            return;
        };
        if matches!(u.order, Order::Shelter { .. }) {
            self.shelter_step(id);
            return;
        }
        u.tool = match &u.order {
            Order::Gather(g) => Some(g.res.tool()),
            Order::Build { .. } | Order::Repair { .. } => Some(PawnTool::Hammer),
            _ => None,
        };
        if u.pause_t > 0.0 {
            u.pause_t -= dt;
            return;
        }
        match u.order.clone() {
            Order::Gather(_) => self.gather_step(id),
            Order::Build { .. } | Order::Repair { .. } => self.build_step(id),
            Order::Drop { building } => {
                let r = if self.building_alive(building) {
                    match self.field_of(id, FieldTarget::Building(building)) {
                        Some(fr) => self.nav_to(id, fr),
                        None => NavResult::Blocked,
                    }
                } else {
                    NavResult::Blocked
                };
                if r == NavResult::Arrived {
                    let ok = match (
                        self.units.get(id).and_then(|u| u.carry),
                        self.buildings.get(building),
                    ) {
                        (Some(c), Some(b)) => b.def().accepts(c.res),
                        _ => false,
                    };
                    if ok {
                        self.deposit(id);
                    }
                }
                if r != NavResult::Moving {
                    self.idle(id);
                }
            }
            Order::Idle { .. } => {}
            Order::Move(m) | Order::AMove(m) => {
                if self.travel(id, m.to, TRAVEL_NEAR) {
                    self.idle(id);
                }
            }
            // pawns never get combat/heal/explore orders; if one slips through, stand still
            _ => self.idle(id),
        }
    }

    /// `gatherStep(u, o, dt)` (§4.3).
    pub fn gather_step(&mut self, id: UnitId) {
        let dt = TICK_DT;
        let Some(u) = self.units.get(id) else { return };
        let Order::Gather(o) = u.order else { return };
        let f = u.faction;
        // deliver first
        if let Some(c) = u.carry {
            let pos = u.pos;
            if let Some(site) = self.drop_site_at(f, pos, c.res)
                && let Some(fr) = self.field_of(id, FieldTarget::Building(site))
                && self.nav_to(id, fr) == NavResult::Arrived
            {
                self.deposit(id);
            }
            return;
        }
        let mut n = u.node.filter(|n| self.node_valid(*n));
        if n.is_none() {
            n = self.find_node(id);
            self.claim(id, n);
            if n.is_none() {
                if self.factions[f as usize].human {
                    if let Some(u) = self.units.get_mut(id) {
                        u.skip.clear();
                        u.pause_t = PAUSE_NO_NODE;
                    }
                } else {
                    self.idle(id);
                }
                return;
            }
        }
        let Some(mut n) = n else { return };
        // `None` = unreachable, `Some(at)` otherwise
        let at: Option<bool> = match n {
            GatherTarget::Sheep(_) => {
                let Some(upos) = self.units.get(id).map(|u| u.pos) else {
                    return;
                };
                let mut d = self
                    .gather_pos(n)
                    .map(|p| p.dist(upos))
                    .unwrap_or(f32::INFINITY);
                let first = match self.units.get(id).map(|u| &u.order) {
                    Some(Order::Gather(g)) => g.first,
                    _ => None,
                };
                if d > 120.0 && Some(n) != first {
                    let look = {
                        let u = self.units.get_mut(id).expect("checked");
                        u.look_t -= dt;
                        if u.look_t <= 0.0 {
                            u.look_t = 0.5;
                            true
                        } else {
                            false
                        }
                    };
                    if look {
                        let m = self.find_node(id);
                        let dm = m
                            .and_then(|m| self.gather_pos(m))
                            .map(|p| p.dist(upos))
                            .unwrap_or(f32::INFINITY);
                        if let Some(m) = m
                            && dm < d * 0.6
                        {
                            self.claim(id, Some(m));
                            n = m;
                            d = dm;
                        }
                    }
                }
                if d < SHEEP_ARRIVE {
                    Some(true)
                } else {
                    match self.gather_pos(n) {
                        Some(p) if self.approach(id, p) => Some(false),
                        _ => None,
                    }
                }
            }
            GatherTarget::Node(nid) => match self.field_of(id, FieldTarget::Node(nid)) {
                Some(fr) => match self.nav_to(id, fr) {
                    NavResult::Arrived => Some(true),
                    NavResult::Moving => Some(false),
                    NavResult::Blocked => None,
                },
                None => None,
            },
        };
        let Some(at) = at else {
            if let Some(u) = self.units.get_mut(id)
                && !u.skip.contains(&n)
            {
                u.skip.push(n);
            }
            self.claim(id, None);
            return;
        };
        if !at {
            return;
        }
        let Some(npos) = self.gather_pos(n) else {
            return;
        };
        self.work_on(id, npos);
        let mods = self.factions[f as usize].mods;
        let res = o.res;
        {
            let Some(u) = self.units.get_mut(id) else {
                return;
            };
            u.work_t += dt * mods.gather[res as usize];
            if u.work_t < res.job_time() {
                return;
            }
        }
        let cap = mods.carry_cap();
        let amount = match n {
            GatherTarget::Node(nid) => {
                let left = self
                    .nodes
                    .get(nid.0 as usize)
                    .map(|n| n.amount)
                    .unwrap_or(0.0);
                let amount = cap.min(left);
                self.take_from_node(nid, amount);
                amount
            }
            GatherTarget::Sheep(s) => {
                self.kill_sheep(s);
                SHEEP_MEAT
            }
        };
        if !self.node_valid(n) {
            self.claim(id, None);
        }
        if let Some(u) = self.units.get_mut(id) {
            u.carry = Some(Carry { res, amount });
            u.work_t = 0.0;
            u.pause_t = PAUSE_HARVEST;
        }
    }

    /// Removes `amount` from a tree (`chopTree`: stump + regrow timer + dust2) or a vein.
    pub fn take_from_node(&mut self, n: NodeId, amount: f32) {
        let Some(node) = self.nodes.get_mut(n.0 as usize) else {
            return;
        };
        node.amount -= amount;
        if node.kind == NodeKind::Tree {
            if node.amount <= 0.0 {
                node.amount = 0.0;
                node.regrow = TREE_REGROW;
                let p = node.pos;
                self.emit(GameEvent::fx(FxKind::Dust2, p.x, p.y));
            }
        } else if node.amount < 0.0 {
            node.amount = 0.0;
        }
    }

    /// Iterates the nodes of a resource (`nodesOf(res)`): trees / golds / stones in id order, or the live sheep
    /// slab for meat. Yields `(target, position)`.
    pub fn nodes_of(&self, res: Res) -> impl Iterator<Item = (GatherTarget, V2)> + '_ {
        let meat = res == Res::Meat;
        let nodes = self
            .nodes
            .iter()
            .enumerate()
            .filter(move |(_, n)| !meat && n.res() == res)
            .map(|(i, n)| (GatherTarget::Node(NodeId(i as u32)), n.pos));
        let sheep = self
            .sheep
            .iter()
            .filter(move |_| meat)
            .map(|(id, s)| (GatherTarget::Sheep(id), s.pos));
        nodes.chain(sheep)
    }

    /// `findNode(u, o)` (§4.2): player's pick, else nearest free explored node within 900 px of the origin
    /// (+0.3 × distance to the pawn), humans widen to `WORK_RADIUS` around the castle (always for meat). The
    /// widened search moves the order's origin to the found node and clears `first`.
    pub fn find_node(&mut self, id: UnitId) -> Option<GatherTarget> {
        let u = self.units.get(id)?;
        let Order::Gather(o) = u.order else {
            return None;
        };
        let f = u.faction;
        let upos = u.pos;
        let castle = self.factions[f as usize]
            .castle
            .and_then(|c| self.buildings.get(c))
            .filter(|c| !c.dead)
            .map(|c| c.pos);
        let wide = self.factions[f as usize].human && castle.is_some();
        let room =
            |g: GatherTarget| self.node_valid(g) && self.node_room(g) && !u.skip.contains(&g);
        if let Some(first) = o.first
            && room(first)
        {
            return Some(first);
        }
        let mut best = None;
        let mut bd = f32::INFINITY;
        if !(wide && o.res == Res::Meat) {
            for (g, p) in self.nodes_of(o.res) {
                let d0 = p.dist(o.origin);
                if d0 > NODE_SEARCH || !room(g) || !self.explored(f, p) {
                    continue;
                }
                let d = d0 + p.dist(upos) * 0.3;
                if d < bd {
                    bd = d;
                    best = Some(g);
                }
            }
            if best.is_some() || !wide {
                return best;
            }
        }
        let c = castle?;
        let mut best_pos = V2::ZERO;
        for (g, p) in self.nodes_of(o.res) {
            if p.dist(c) > WORK_RADIUS || !room(g) || !self.explored(f, p) {
                continue;
            }
            let d = p.dist(upos);
            if d < bd {
                bd = d;
                best = Some(g);
                best_pos = p;
            }
        }
        if best.is_some()
            && let Some(u) = self.units.get_mut(id)
            && let Order::Gather(o) = &mut u.order
        {
            o.first = None;
            o.origin = best_pos;
        }
        best
    }

    /// `deposit(u)` (§4.4): res += load, gathered, float `+N` + coin sound for humans, pause 0.4.
    pub fn deposit(&mut self, id: UnitId) {
        let Some(u) = self.units.get_mut(id) else {
            return;
        };
        let Some(c) = u.carry.take() else { return };
        u.pause_t = PAUSE_DELIVER;
        let (f, pos) = (u.faction, u.pos);
        let fac = &mut self.factions[f as usize];
        fac.res[c.res] += c.amount as f64;
        fac.gathered += c.amount as f64;
        if fac.human {
            self.emit(GameEvent::Float {
                to: f,
                x: pos.x.round(),
                y: (pos.y - 70.0).round(),
                amount: c.amount.round() as i32,
                res: c.res,
            });
            self.emit(GameEvent::Sound {
                sound: Sound::Coin,
                x: pos.x,
                y: pos.y,
            });
        }
    }

    /// `workOn(u, target, dt)` (§4.5): face target, `chop` every 0.6 s of work, state Work, anim Work + tool.
    pub fn work_on(&mut self, id: UnitId, at: V2) {
        let dt = TICK_DT;
        let Some(u) = self.units.get_mut(id) else {
            return;
        };
        u.flip = at.x < u.pos.x;
        let chop = ((u.work_t + dt) / CHOP_EVERY).floor() != (u.work_t / CHOP_EVERY).floor();
        u.state = UnitState::Work;
        if u.anim != AnimKind::Work {
            u.anim = AnimKind::Work;
            u.anim_t = 0.0;
        }
        let pos = u.pos;
        if chop {
            self.emit(GameEvent::Sound {
                sound: Sound::Chop,
                x: pos.x,
                y: pos.y,
            });
        }
    }

    /// `claim(u, n)`: book `n` (users++), release the previous booking (users--).
    pub fn claim(&mut self, id: UnitId, n: Option<GatherTarget>) {
        let Some(u) = self.units.get(id) else { return };
        if u.node == n {
            return;
        }
        if let Some(old) = u.node {
            self.adjust_users(old, -1);
        }
        if let Some(new) = n {
            self.adjust_users(new, 1);
        }
        if let Some(u) = self.units.get_mut(id) {
            u.node = n;
        }
    }

    fn adjust_users(&mut self, g: GatherTarget, d: i32) {
        match g {
            GatherTarget::Node(n) => {
                if let Some(n) = self.nodes.get_mut(n.0 as usize) {
                    n.users = (n.users as i32 + d).clamp(0, 255) as u8;
                }
            }
            GatherTarget::Sheep(s) => {
                if let Some(s) = self.sheep.get_mut(s) {
                    s.users = (s.users as i32 + d).clamp(0, 255) as u8;
                }
            }
        }
    }

    /// `gatherOrder(n)` = `{k:'gather', res, first: n, px: n.x, py: n.y}`.
    pub fn gather_order(&self, g: GatherTarget) -> Option<Order> {
        let pos = self.gather_pos(g)?;
        let res = match g {
            GatherTarget::Node(n) => self.nodes.get(n.0 as usize)?.res(),
            GatherTarget::Sheep(_) => Res::Meat,
        };
        Some(Order::Gather(GatherOrder {
            res,
            first: Some(g),
            origin: pos,
        }))
    }

    /// `nodeValid(n)`: tree wood > 0, vein amount > 0, sheep alive.
    pub fn node_valid(&self, g: GatherTarget) -> bool {
        match g {
            GatherTarget::Node(n) => self.nodes.get(n.0 as usize).is_some_and(|n| n.valid()),
            GatherTarget::Sheep(s) => self.sheep.get(s).is_some_and(|s| !s.dead),
        }
    }

    /// `nodeRoom(n)`: `users < (big ? 8 : NODE_USERS[kind])` (sheep 1).
    pub fn node_room(&self, g: GatherTarget) -> bool {
        match g {
            GatherTarget::Node(n) => self.nodes.get(n.0 as usize).is_some_and(|n| n.room()),
            GatherTarget::Sheep(s) => self.sheep.get(s).is_some_and(|s| s.users < SHEEP_USERS),
        }
    }

    /// `nodeLeft(n)`: tree wood, sheep 15, vein amount.
    pub fn node_left(&self, g: GatherTarget) -> f32 {
        match g {
            GatherTarget::Node(n) => self
                .nodes
                .get(n.0 as usize)
                .map(|n| n.amount)
                .unwrap_or(0.0),
            GatherTarget::Sheep(_) => SHEEP_MEAT,
        }
    }

    /// `dropSiteAt(f, x, y, kind)` (§4.1): nearest (rectDist) own, done, alive building accepting `res`
    /// (first minimum in slab order).
    pub fn drop_site_at(&self, f: FactionIdx, p: V2, res: Res) -> Option<BuildingId> {
        let mut best = None;
        let mut bd = f32::INFINITY;
        for (id, b) in self.buildings.iter() {
            if b.faction != f || !b.done || b.dead || !b.def().accepts(res) {
                continue;
            }
            let d = b.dist(p);
            if d < bd {
                bd = d;
                best = Some(id);
            }
        }
        best
    }

    /// `threatNear(f, x, y, r)` (§6.1): an enemy soldier (`dmg && !onlyBuildings`, not sheltered) within `r`.
    /// Uses the spatial grid (unit phase); see [`World::threat_near_scan`] for the exact scan.
    pub fn threat_near(&self, f: FactionIdx, p: V2, r: f32) -> bool {
        let mut out = Vec::new();
        self.grid.query(p, r + GRID_MARGIN, &mut out);
        out.iter()
            .any(|id| self.units.get(*id).is_some_and(|e| is_threat(e, f, p, r)))
    }

    /// Exact `threatNear` over every unit (used outside the unit phase, where the grid may be stale).
    pub fn threat_near_scan(&self, f: FactionIdx, p: V2, r: f32) -> bool {
        self.units.values().any(|e| is_threat(e, f, p, r))
    }

    /// `shelterFor(u)` (§6.2): nearest own done house with < 5 inside, else the castle.
    pub fn shelter_for(&self, id: UnitId) -> Option<BuildingId> {
        let u = self.units.get(id)?;
        let mut best = None;
        let mut bd = f32::INFINITY;
        for (bid, b) in self.buildings.iter() {
            if b.faction != u.faction
                || b.dead
                || !b.done
                || b.kind != BuildingKind::House
                || b.inside.len() >= SHELTER_CAP
            {
                continue;
            }
            let d = b.dist(u.pos);
            if d < bd {
                bd = d;
                best = Some(bid);
            }
        }
        best.or_else(|| {
            self.factions[u.faction as usize]
                .castle
                .filter(|c| self.building_alive(*c))
        })
    }

    /// `enterShelter(u, b)` (§6.3): dust FX, inside, placed at the rect centre, idle state.
    pub fn enter_shelter(&mut self, id: UnitId, b: BuildingId) {
        let Some(c) = self.buildings.get(b).map(|b| b.rect.center()) else {
            return;
        };
        let Some(u) = self.units.get_mut(id) else {
            return;
        };
        let p = u.pos;
        u.inside = Some(b);
        u.pos = c;
        u.moving = false;
        u.state = UnitState::Idle;
        if let Some(bl) = self.buildings.get_mut(b)
            && !bl.inside.contains(&id)
        {
            bl.inside.push(id);
        }
        self.emit(GameEvent::Fx {
            fx: FxKind::Dust1,
            x: p.x,
            y: p.y + 4.0,
            scale: 1.0,
            dy: -6.0,
            follow: None,
            color: None,
        });
    }

    /// `leaveShelter(u)` (§6.3): exit at `nearest_passable(b.x + rand(-0.3,0.3)*w*64, b.y1 + 24)` [RNG], dust FX.
    pub fn leave_shelter(&mut self, id: UnitId) {
        let Some(b) = self.units.get(id).and_then(|u| u.inside) else {
            return;
        };
        let exit = if let Some(bl) = self.buildings.get_mut(b) {
            bl.inside.retain(|u| *u != id);
            Some((bl.pos.x, bl.rect.y1, bl.def().w))
        } else {
            None
        };
        let p = match exit {
            Some((x, y1, w)) => {
                let dx = self.rng.range(-0.3, 0.3) * w as f32 * crate::TILE;
                self.map.nearest_passable(v2(x + dx, y1 + DEPLOY_DY))
            }
            None => self.units.get(id).map(|u| u.pos).unwrap_or_default(),
        };
        if let Some(u) = self.units.get_mut(id) {
            u.inside = None;
            u.pos = p;
        }
        self.emit(GameEvent::Fx {
            fx: FxKind::Dust1,
            x: p.x,
            y: p.y + 4.0,
            scale: 1.0,
            dy: -6.0,
            follow: None,
            color: None,
        });
    }

    /// `shelterStep(u, o, dt)` (§6.4).
    pub fn shelter_step(&mut self, id: UnitId) {
        let Some(u) = self.units.get(id) else { return };
        let Order::Shelter { building, calm, .. } = &u.order else {
            return;
        };
        if *calm >= CALM_TIME {
            self.resume_work(id);
            return;
        }
        if u.inside.is_some() {
            return;
        }
        let mut b = *building;
        let usable = b.is_some_and(|b| {
            self.buildings.get(b).is_some_and(|bl| !bl.dead && bl.done) && !self.shelter_full(b)
        });
        if !usable {
            b = self.shelter_for(id);
            if let Some(u) = self.units.get_mut(id)
                && let Order::Shelter { building, .. } = &mut u.order
            {
                *building = b;
            }
        }
        let r = match b.and_then(|b| self.field_of(id, FieldTarget::Building(b))) {
            Some(fr) => self.nav_to(id, fr),
            None => NavResult::Blocked,
        };
        match (r, b) {
            (NavResult::Arrived, Some(b)) => self.enter_shelter(id, b),
            (NavResult::Moving, _) => {}
            _ => {
                if let Some(u) = self.units.get_mut(id) {
                    u.brave = true;
                    u.calm_t = 0.0;
                }
                self.resume_work(id);
            }
        }
    }

    /// `resumeWork(u)` (§6.4): back to the order the pawn had before fleeing (gather keeps its node only if still
    /// valid; idle → idle where it came out; build/repair/drop/move as they were).
    pub fn resume_work(&mut self, id: UnitId) {
        let Some(u) = self.units.get(id) else { return };
        let Order::Shelter { back, .. } = &u.order else {
            self.idle(id);
            return;
        };
        match (**back).clone() {
            Order::Gather(mut g) => {
                g.first = g.first.filter(|n| self.node_valid(*n));
                self.set_order(id, Order::Gather(g));
            }
            // JS `setOrder(u, back); idle(u)`: leave the shelter first, then idle where it came out
            Order::Idle { .. } | Order::Shelter { .. } => {
                self.set_order(id, Order::idle_at(V2::ZERO));
                self.idle(id);
            }
            o => self.set_order(id, o),
        }
    }

    /// `embolden(u)` (§6.5): a human direct order given while enemies are in sight makes a pawn brave (it won't
    /// flee until it has seen no threat for 4 s). Peacetime orders change nothing.
    pub fn embolden(&mut self, id: UnitId) {
        let Some(u) = self.units.get(id) else { return };
        if u.kind != UnitKind::Pawn {
            return;
        }
        let centre = u
            .inside
            .and_then(|b| self.buildings.get(b))
            .map(|b| b.pos)
            .unwrap_or(u.pos);
        let r = self.factions[u.faction as usize].mods.unit_sight_px(u.kind) + 2.0 * crate::TILE;
        if !self.threat_near_scan(u.faction, centre, r) {
            return;
        }
        if let Some(u) = self.units.get_mut(id) {
            u.brave = true;
            u.calm_t = 0.0;
        }
    }

    /// `nodeNear(f, res, p)` (§4.7 / 04a §11.2): nearest valid, roomy, explored node of `res` within
    /// `WORK_RADIUS` of the castle → `(node, distance)`.
    pub fn node_near(&self, f: FactionIdx, res: Res, p: V2) -> Option<(GatherTarget, f32)> {
        let castle = self.factions.get(f as usize)?.castle?;
        let c = self.buildings.get(castle).filter(|b| !b.dead)?.pos;
        let mut best = None;
        let mut bd = f32::INFINITY;
        for (g, np) in self.nodes_of(res) {
            if np.dist(c) > WORK_RADIUS
                || !self.node_valid(g)
                || !self.node_room(g)
                || !self.explored(f, np)
            {
                continue;
            }
            let d = np.dist(p);
            if d < bd {
                bd = d;
                best = Some(g);
            }
        }
        best.map(|g| (g, bd))
    }

    /// `updateResources` part for trees and veins (§2.3): tree regrow after 110 s; veins +10 every 25 s while
    /// below max (also when depleted). Gold `twinkle` is client-side (addenda G8).
    pub fn update_nodes(&mut self) {
        let dt = TICK_DT;
        let mut regrown = Vec::new();
        for n in self.nodes.iter_mut() {
            match n.kind {
                NodeKind::Tree => {
                    if n.amount <= 0.0 {
                        n.regrow -= dt;
                        if n.regrow <= 0.0 {
                            n.amount = TREE_WOOD;
                            regrown.push(n.pos);
                        }
                    }
                }
                NodeKind::Gold | NodeKind::Stone => {
                    if n.amount < n.max {
                        n.regen += dt;
                        if n.regen > VEIN_REGEN_TIME {
                            n.regen = 0.0;
                            n.amount = (n.amount + VEIN_REGEN_AMOUNT).min(n.max);
                        }
                    }
                }
            }
        }
        for p in regrown {
            self.emit(GameEvent::fx(FxKind::Dust1, p.x, p.y));
        }
    }

    /// `dropPickup(kind, x, y, amount, life, chest)` (§2.4): hop to a random spot (angle [RNG], radius 24-54,
    /// chests do not hop).
    pub fn drop_pickup(
        &mut self,
        res: Res,
        at: V2,
        amount: f32,
        life: f32,
        chest: bool,
    ) -> PickupId {
        let a = self.rng.range(0.0, core::f32::consts::TAU);
        let r = if chest {
            0.0
        } else {
            self.rng.range(24.0, 54.0)
        };
        let to = v2(
            at.x + a.cos() * r,
            at.y + a.sin() * r * 0.6 + if chest { 0.0 } else { 10.0 },
        );
        self.pickups.insert(Pickup {
            res,
            amount,
            chest,
            pos: at,
            from: at,
            to,
            t: if chest { 1.0 } else { 0.0 },
            life,
            hop: 0.0,
        })
    }

    /// `updatePickups(dt)` (§2.4): hop for 0.4 s, scan every 0.2 s: first unit (slab order) within 32x28 takes it
    /// (any faction, any type), res += amount (not `gathered`), chest → prop `chest_open` + toast, float + coin
    /// for humans; expire at life 0.
    pub fn update_pickups(&mut self) {
        let dt = TICK_DT;
        self.pick_t -= dt;
        let scan = self.pick_t <= 0.0;
        if scan {
            self.pick_t = PICKUP_SCAN;
        }
        for pid in self.pickups.ids() {
            let Some(p) = self.pickups.get_mut(pid) else {
                continue;
            };
            p.t += dt;
            p.life -= dt;
            if p.t < PICKUP_HOP {
                let k = p.t / PICKUP_HOP;
                p.pos = p.from.lerp(p.to, k);
                p.hop = (k * core::f32::consts::PI).sin() * 30.0;
                continue;
            }
            p.pos = p.to;
            p.hop = 0.0;
            if !scan {
                continue;
            }
            let (pp, res, amount, chest) = (p.pos, p.res, p.amount, p.chest);
            let taker = self.units.iter().find_map(|(uid, u)| {
                (!u.dead
                    && u.inside.is_none()
                    && (u.pos.x - pp.x).abs() <= PICKUP_DX
                    && (u.pos.y - pp.y).abs() <= PICKUP_DY)
                    .then_some((uid, u.faction, u.pos))
            });
            let Some((_, f, upos)) = taker else { continue };
            if let Some(p) = self.pickups.get_mut(pid) {
                p.life = 0.0;
            }
            self.factions[f as usize].res[res] += amount as f64;
            if chest {
                self.emit(GameEvent::Prop {
                    prop: PropKind::ChestOpen,
                    x: pp.x.round(),
                    y: pp.y.round(),
                });
            }
            if self.factions[f as usize].human {
                self.emit(GameEvent::Float {
                    to: f,
                    x: upos.x.round(),
                    y: (upos.y - 80.0).round(),
                    amount: amount.round() as i32,
                    res,
                });
                self.emit(GameEvent::Play {
                    to: f,
                    sound: Sound::Coin,
                });
                if chest {
                    self.emit(GameEvent::Toast {
                        to: f,
                        msg: Msg::ChestFound {
                            res,
                            amount: amount.round() as u32,
                        },
                    });
                }
            }
        }
        self.pickups.retain(|_, p| p.life > 0.0);
    }
}

/// Enemy soldier able to scare pawns (`dmg && !onlyBuildings`, not sheltered) within `r` of `p`.
#[inline]
fn is_threat(e: &crate::world::Unit, f: FactionIdx, p: V2, r: f32) -> bool {
    e.active() && e.faction != f && e.def().is_soldier() && {
        let d = e.pos - p;
        d.x.abs() < r && d.y.abs() < r && d.len() < r
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::map::{GenDeposit, GenPoint, GenTree, GeneratedWorld, Map};
    use crate::world::{MatchSetup, SetupPlayer};

    /// Flat synthetic world: land x 1..140, y 1..100; bases (20,20) and (110,70) (both human, fog fully explored,
    /// no starting units); trees at (30..34, 26); small gold (26,30); big gold (40,30); mine point (60,20);
    /// meadow at cell (40,50); a water moat x 45..58, y 58..66 with an island x 49..53, y 60..63.
    pub(crate) fn test_world() -> World {
        let mut map = Map::new();
        let set = |m: &mut Map, x: i32, y: i32, v: u8| {
            let c = m.idx(x, y) as usize;
            m.land[c] = v;
        };
        for y in 1..100 {
            for x in 1..140 {
                set(&mut map, x, y, 1);
            }
        }
        for y in 58..=66 {
            for x in 45..=58 {
                set(&mut map, x, y, 0);
            }
        }
        for y in 60..=63 {
            for x in 49..=53 {
                set(&mut map, x, y, 1);
            }
        }
        let trees: Vec<GenTree> = (30..35)
            .map(|x| GenTree {
                cell: map.idx(x, 26),
                variant: 0,
            })
            .collect();
        let golds = vec![GenDeposit {
            cell: map.idx(26, 30),
            amount: GOLD_MAX,
            big: false,
        }];
        let big = GenDeposit {
            cell: map.idx(40, 30),
            amount: BIG_GOLD,
            big: true,
        };
        let point = GenPoint {
            cell: map.idx(60, 20),
            kind: PointKind::Mine,
        };
        let mut blocked: Vec<i32> = trees.iter().map(|t| t.cell).collect();
        blocked.push(golds[0].cell);
        blocked.extend(map.quad(big.cell));
        blocked.extend(map.quad(point.cell));
        for c in blocked {
            map.block[c as usize] = 1;
            map.static_block[c as usize] = 1;
        }
        let meadow = map.center(map.idx(40, 50));
        let g = GeneratedWorld {
            seed: 7,
            map,
            bases: vec![(20, 20), (110, 70)],
            plateaus: Vec::new(),
            trees,
            golds,
            stones: Vec::new(),
            bigs: vec![(NodeKind::Gold, big)],
            meadows: vec![meadow],
            points: vec![point],
            chests: Vec::new(),
            ponds: Vec::new(),
            open_water: Vec::new(),
        };
        let setup = MatchSetup {
            players: KingdomColor::ALL
                .iter()
                .map(|c| SetupPlayer {
                    color: *c,
                    name: c.name_it().to_string(),
                })
                .collect(),
            level: AiLevel::Normal,
            solo: true,
        };
        let mut w = World::from_generated(g, &setup);
        w.units.clear();
        w.sheep.clear();
        for f in 0..w.factions.len() as FactionIdx {
            w.factions[f as usize].fog.explored.fill();
            w.census(f);
        }
        w.events.clear();
        w
    }

    pub(crate) fn cell_pos(w: &World, x: i32, y: i32) -> V2 {
        w.map.center(w.map.idx(x, y))
    }

    pub(crate) fn spawn(w: &mut World, f: FactionIdx, kind: UnitKind, x: i32, y: i32) -> UnitId {
        let p = cell_pos(w, x, y);
        let u = w.spawn_unit(f, kind, p);
        w.idle(u);
        w.census(f);
        u
    }

    pub(crate) fn run(w: &mut World, secs: f32) {
        let n = (secs / TICK_DT).round() as usize;
        for _ in 0..n {
            w.step(&[]);
        }
    }

    fn tree(w: &World, x: i32, y: i32) -> GatherTarget {
        let c = w.map.idx(x, y);
        let i = w
            .nodes
            .iter()
            .position(|n| n.cell == c)
            .expect("tree exists");
        GatherTarget::Node(NodeId(i as u32))
    }

    #[test]
    fn one_load_takes_job_time() {
        let mut w = test_world();
        let t = tree(&w, 30, 26);
        let u = spawn(&mut w, 0, UnitKind::Pawn, 30, 27); // standing on the tree's ring
        let o = w.gather_order(t).unwrap();
        w.set_order(u, o);
        let mut ticks = 0;
        while w.units.get(u).unwrap().carry.is_none() && ticks < 200 {
            w.step(&[]);
            ticks += 1;
        }
        let secs = ticks as f32 * TICK_DT;
        assert!(
            (secs - Res::Wood.job_time()).abs() < 0.11,
            "load after {secs} s"
        );
        let c = w.units.get(u).unwrap().carry.unwrap();
        assert_eq!(c.res, Res::Wood);
        assert_eq!(c.amount, CARRY);
        let GatherTarget::Node(n) = t else {
            unreachable!()
        };
        assert_eq!(w.nodes[n.0 as usize].amount, TREE_WOOD - CARRY);
        assert_eq!(
            w.nodes[n.0 as usize].users, 1,
            "node stays booked while delivering"
        );
        // axes: gather × 1.3
        let mut w2 = test_world();
        w2.factions[0].mods.apply_tech(TechId::Axes);
        let u2 = spawn(&mut w2, 0, UnitKind::Pawn, 30, 27);
        let o = w2.gather_order(t).unwrap();
        w2.set_order(u2, o);
        let mut ticks2 = 0;
        while w2.units.get(u2).unwrap().carry.is_none() && ticks2 < 200 {
            w2.step(&[]);
            ticks2 += 1;
        }
        assert!(ticks2 < ticks && (ticks2 as f32 * TICK_DT - 3.0 / 1.3).abs() < 0.11);
    }

    #[test]
    fn gathering_rate_and_delivery() {
        let mut w = test_world();
        // a depot next to the trees: short trips
        let depot = w.add_building(0, BuildingKind::Depot, 31, 28, true);
        w.census(0);
        assert_eq!(
            w.drop_site_at(0, cell_pos(&w, 31, 27), Res::Wood),
            Some(depot)
        );
        let t = tree(&w, 31, 26);
        let u = spawn(&mut w, 0, UnitKind::Pawn, 33, 30);
        let o = w.gather_order(t).unwrap();
        w.set_order(u, o);
        let wood0 = w.factions[0].res[Res::Wood];
        run(&mut w, 120.0);
        let got = w.factions[0].res[Res::Wood] - wood0;
        // ~ 10 / (3 + walk + 0.9) * 60 per minute: 2 min → 230..300 with a short walk
        assert!(
            (180.0..=320.0).contains(&got),
            "gathered {got} wood in 2 min"
        );
        assert_eq!(w.factions[0].gathered, got);
        assert!(matches!(w.units.get(u).unwrap().order, Order::Gather(_)));
    }

    #[test]
    fn node_user_caps_and_search() {
        let mut w = test_world();
        let t = tree(&w, 32, 26);
        let pawns: Vec<UnitId> = (0..3)
            .map(|i| spawn(&mut w, 0, UnitKind::Pawn, 31 + i, 28))
            .collect();
        for &u in &pawns {
            let o = w.gather_order(t).unwrap();
            w.set_order(u, o);
        }
        run(&mut w, 1.0);
        let GatherTarget::Node(n) = t else {
            unreachable!()
        };
        assert_eq!(w.nodes[n.0 as usize].users, 2, "a tree holds 2 pawns");
        let others: Vec<_> = pawns
            .iter()
            .filter(|u| w.units.get(**u).unwrap().node != Some(t))
            .collect();
        assert_eq!(others.len(), 1);
        let other = w
            .units
            .get(*others[0])
            .unwrap()
            .node
            .expect("found another tree");
        assert!(matches!(other, GatherTarget::Node(_)) && other != t);
        // big deposit: 8 users
        let big = GatherTarget::Node(NodeId(w.nodes.iter().position(|n| n.big).unwrap() as u32));
        let miners: Vec<UnitId> = (0..9)
            .map(|i| spawn(&mut w, 0, UnitKind::Pawn, 36 + i % 3, 33 + i / 3))
            .collect();
        for &u in &miners {
            let o = w.gather_order(big).unwrap();
            w.set_order(u, o);
        }
        run(&mut w, 0.5);
        let GatherTarget::Node(bn) = big else {
            unreachable!()
        };
        assert_eq!(w.nodes[bn.0 as usize].users, BIG_USERS);
    }

    #[test]
    fn depleted_tree_regrows_and_pawn_moves_on() {
        let mut w = test_world();
        let t = tree(&w, 30, 26);
        let GatherTarget::Node(n) = t else {
            unreachable!()
        };
        w.nodes[n.0 as usize].amount = 5.0;
        let u = spawn(&mut w, 0, UnitKind::Pawn, 30, 27);
        let o = w.gather_order(t).unwrap();
        w.set_order(u, o);
        run(&mut w, 3.2);
        assert_eq!(w.units.get(u).unwrap().carry.map(|c| c.amount), Some(5.0));
        assert_eq!(w.nodes[n.0 as usize].amount, 0.0);
        assert_eq!(w.nodes[n.0 as usize].users, 0);
        assert!(!w.node_valid(t));
        // after delivering it looks for another tree nearby (explored territory)
        run(&mut w, 40.0);
        let node = w.units.get(u).unwrap().node;
        assert!(node.is_some() && node != Some(t));
        // stump regrows after 110 s
        run(&mut w, TREE_REGROW - 40.0);
        assert_eq!(w.nodes[n.0 as usize].amount, TREE_WOOD);
    }

    #[test]
    fn vein_regains_slowly() {
        let mut w = test_world();
        let i = w
            .nodes
            .iter()
            .position(|n| n.kind == NodeKind::Gold && !n.big)
            .unwrap();
        w.nodes[i].amount = 0.0;
        run(&mut w, VEIN_REGEN_TIME + 0.2);
        assert_eq!(w.nodes[i].amount, VEIN_REGEN_AMOUNT);
    }

    /// House (48,56)-(49,56) next to the moat; the enemy warrior stands on the island at (51,60): 4 cells from the
    /// pawns (sight 5 cells), within 7 cells of the house (sheltered pawns watch sight + 2 from the building).
    #[test]
    fn pawns_shelter_and_return() {
        let mut w = test_world();
        let house = w.add_building(0, BuildingKind::House, 48, 56, true);
        w.census(0);
        let spots = [
            (50, 56),
            (51, 56),
            (52, 56),
            (50, 57),
            (51, 57),
            (52, 57),
            (51, 56),
        ];
        let pawns: Vec<UnitId> = spots
            .iter()
            .map(|&(x, y)| spawn(&mut w, 0, UnitKind::Pawn, x, y))
            .collect();
        let t = tree(&w, 34, 26);
        let o = w.gather_order(t).unwrap();
        w.set_order(pawns[0], o);
        let foe = spawn(&mut w, 1, UnitKind::Warrior, 51, 60);
        run(&mut w, 0.5);
        for &u in &pawns {
            assert!(
                matches!(w.units.get(u).unwrap().order, Order::Shelter { .. }),
                "pawn flees"
            );
        }
        run(&mut w, 2.5);
        let hb = w.buildings.get(house).unwrap();
        assert_eq!(hb.inside.len(), SHELTER_CAP, "a house hides 5 pawns");
        let castle = w.factions[0].castle;
        let to_castle = pawns
            .iter()
            .filter(|u| {
                matches!(w.units.get(**u).unwrap().order,
                    Order::Shelter { building, .. } if building == castle)
            })
            .count();
        assert_eq!(to_castle, 2, "the others run to the castle");
        // sheltered pawns stay inside while the enemy is around
        run(&mut w, 6.0);
        assert_eq!(w.buildings.get(house).unwrap().inside.len(), SHELTER_CAP);
        // a direct order while threatened makes a pawn brave
        let inside = w.buildings.get(house).unwrap().inside[0];
        w.embolden(inside);
        assert!(w.units.get(inside).unwrap().brave);
        // the threat goes away: back to work after 4 calm seconds
        w.kill_unit(foe, crate::world::DamageSource::NONE);
        run(&mut w, CALM_TIME + 1.0);
        assert!(w.buildings.get(house).unwrap().inside.is_empty());
        let u0 = w.units.get(pawns[0]).unwrap();
        assert!(u0.inside.is_none());
        assert!(matches!(u0.order, Order::Gather(g) if g.res == Res::Wood));
        assert!(matches!(
            w.units.get(pawns[1]).unwrap().order,
            Order::Idle { .. }
        ));
    }

    #[test]
    fn brave_pawn_keeps_working() {
        let mut w = test_world();
        w.add_building(0, BuildingKind::House, 48, 56, true);
        w.census(0);
        let u = spawn(&mut w, 0, UnitKind::Pawn, 51, 57);
        spawn(&mut w, 1, UnitKind::Warrior, 51, 60);
        run(&mut w, 0.1);
        // peacetime check is done on the real threat: here a threat exists → brave
        w.embolden(u);
        assert!(w.units.get(u).unwrap().brave);
        let t = tree(&w, 34, 26);
        let o = w.gather_order(t).unwrap();
        w.set_order(u, o);
        run(&mut w, 2.0);
        assert!(matches!(w.units.get(u).unwrap().order, Order::Gather(_)));
        // a peacetime order changes nothing
        let mut w2 = test_world();
        let p = spawn(&mut w2, 0, UnitKind::Pawn, 51, 57);
        w2.embolden(p);
        assert!(!w2.units.get(p).unwrap().brave);
    }

    #[test]
    fn ai_pawns_flee_too_and_destroyed_house_ejects() {
        let mut w = test_world();
        w.factions[0].human = false;
        w.add_building(0, BuildingKind::House, 48, 56, true);
        w.census(0);
        let u = spawn(&mut w, 0, UnitKind::Pawn, 51, 57);
        spawn(&mut w, 1, UnitKind::Warrior, 51, 60);
        run(&mut w, 3.0);
        let house = w.units.get(u).unwrap().inside.expect("AI pawn hides too");
        w.destroy_building(house, crate::world::DamageSource::NONE);
        let un = w.units.get(u).unwrap();
        assert!(un.inside.is_none() && matches!(un.order, Order::Shelter { .. }));
        run(&mut w, 0.1);
        let castle = w.factions[0].castle;
        assert!(matches!(w.units.get(u).unwrap().order,
            Order::Shelter { building, .. } if building == castle));
    }

    #[test]
    fn pickups_go_to_first_unit() {
        let mut w = test_world();
        let p = cell_pos(&w, 70, 40);
        w.drop_pickup(Res::Gold, p, TREASURE, f32::INFINITY, true);
        let gold0 = w.factions[1].res[Res::Gold];
        let u = spawn(&mut w, 1, UnitKind::Pawn, 70, 40);
        w.units.get_mut(u).unwrap().pos = p;
        run(&mut w, 0.3);
        assert!(w.pickups.is_empty());
        assert_eq!(w.factions[1].res[Res::Gold], gold0 + TREASURE as f64);
        assert_eq!(
            w.factions[1].gathered, 0.0,
            "pickups do not count as gathered"
        );
        assert!(w.events.iter().any(|e| matches!(
            e,
            GameEvent::Toast {
                to: 1,
                msg: Msg::ChestFound { .. }
            }
        )));
    }

    /// First valid placement (spiral by distance) for `kind` around cell `(x, y)` within `r` cells.
    fn find_spot(w: &World, f: FactionIdx, kind: BuildingKind, x: i32, y: i32, r: i32) -> Option<(i32, i32)> {
        let mut best = None;
        let mut bd = i32::MAX;
        for cy in y - r..=y + r {
            for cx in x - r..=x + r {
                let d = (cx - x).pow(2) + (cy - y).pow(2);
                if d < bd && w.place_block(f, kind, cx, cy).is_none() {
                    bd = d;
                    best = Some((cx, cy));
                }
            }
        }
        best
    }

    /// Real map, one human kingdom driven by scripted commands + 4 AI kingdoms, 10 game minutes: the human's
    /// pawns deliver all four resources, the sites it places get built, the castle trains pawns, and the
    /// booking counters stay consistent. `cargo test -p ts-sim --release ten_minute_economy -- --ignored`
    #[test]
    #[ignore]
    fn ten_minute_economy() {
        use crate::cmd::Command;
        let setup = MatchSetup {
            players: vec![SetupPlayer {
                color: KingdomColor::ALL[0],
                name: "Prova".to_string(),
            }],
            level: AiLevel::Normal,
            solo: true,
        };
        let mut w = World::new(11, &setup);
        let f = w.factions.iter().position(|f| f.human).unwrap() as FactionIdx;
        let castle = w.factions[f as usize].castle.unwrap();
        let (ccx, ccy) = w.buildings.get(castle).unwrap().center_cells();
        let (ccx, ccy) = (ccx as i32, ccy as i32);
        w.step(&[]);
        let pawns = w.factions[f as usize].pawns.clone();
        assert_eq!(pawns.len(), 4);
        // one pawn per resource through the Workers sliders (meat may wait for the farm)
        let mut cmds: Vec<(FactionIdx, Command)> = Res::ALL
            .iter()
            .map(|r| (f, Command::Jobs { res: *r, n: 1 }))
            .collect();
        cmds.push((f, Command::DebugGift { res: Some(Res::Wood) }));
        for _ in 0..3 {
            cmds.push((
                f,
                Command::Train {
                    building: castle,
                    unit: UnitKind::Pawn,
                },
            ));
        }
        w.step(&cmds);
        let mut events = Vec::new();
        // farm, house and a depot next to the nearest tree, built by the first pawn (it then resumes its job)
        let cp = w.buildings.get(castle).unwrap().pos;
        let tree = w
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Tree)
            .min_by(|a, b| a.pos.dist(cp).total_cmp(&b.pos.dist(cp)))
            .map(|n| w.map.xy(n.cell))
            .unwrap();
        let mut sites = Vec::new();
        for (kind, at) in [
            (BuildingKind::Farm, (ccx, ccy + 5)),
            (BuildingKind::House, (ccx - 6, ccy + 3)),
            (BuildingKind::Depot, (tree.0, tree.1 + 2)),
        ] {
            let (x, y) = find_spot(&w, f, kind, at.0, at.1, 10).expect("room for a building");
            let r = w.step(&[(
                f,
                Command::Place {
                    kind,
                    cx: x,
                    cy: y,
                    builders: vec![pawns[0]],
                },
            )]);
            assert_eq!(r, vec![Ok(())], "{kind:?} at {x},{y}");
            sites.push(
                w.buildings
                    .iter()
                    .find(|(_, b)| b.kind == kind && b.faction == f)
                    .map(|(id, _)| id)
                    .unwrap(),
            );
            events.extend(w.drain_events());
        }
        let mut delivered = [0f32; 4];
        let check_users = |w: &World| {
            // every node's booking counter equals the number of pawns booked on it
            for (i, n) in w.nodes.iter().enumerate() {
                let g = GatherTarget::Node(NodeId(i as u32));
                let booked = w.units.values().filter(|u| !u.dead && u.node == Some(g)).count();
                assert_eq!(n.users as usize, booked, "users of node {i}");
            }
            for (sid, s) in w.sheep.iter() {
                let g = GatherTarget::Sheep(sid);
                let booked = w.units.values().filter(|u| !u.dead && u.node == Some(g)).count();
                assert_eq!(s.users as usize, booked, "users of a sheep");
            }
        };
        for tick in 0..(600.0 / TICK_DT) as usize {
            w.step(&[]);
            for e in w.drain_events() {
                if let GameEvent::Float { to, res, amount, .. } = e
                    && to == f
                {
                    delivered[res as usize] += amount as f32;
                }
            }
            if tick % 400 == 0 {
                check_users(&w);
            }
            if tick == (120.0 / TICK_DT) as usize {
                // sites built by one pawn chaining through them (afterBuild), pawns trained (later the AI may raid)
                for s in &sites {
                    assert!(w.buildings.get(*s).is_some_and(|b| b.done), "site finished");
                }
                assert!(w.factions[f as usize].pawn_n >= 7, "castle trained its pawns");
            }
        }
        check_users(&w);
        eprintln!("delivered {delivered:?}, gathered {:.0}", w.factions[f as usize].gathered);
        for r in Res::ALL {
            assert!(delivered[*r as usize] >= 40.0, "{r:?} delivered {delivered:?}");
        }
        for fac in &w.factions {
            assert!(fac.gathered > 0.0);
        }
    }

    /// Real generated map, 5 AI kingdoms, 4 minutes: every kingdom gathers and nothing panics. Prints the sim
    /// speed. `cargo test -p ts-sim --release real_map_economy -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_map_economy_smoke() {
        let setup = MatchSetup::default();
        let mut w = World::new(3, &setup);
        let t0 = std::time::Instant::now();
        let ticks = (240.0 / TICK_DT) as usize;
        for _ in 0..ticks {
            w.step(&[]);
            w.events.clear();
        }
        let el = t0.elapsed().as_secs_f64();
        eprintln!(
            "{} ticks in {el:.2}s ({:.3} ms/tick), units {}, sheep {}",
            ticks,
            el * 1000.0 / ticks as f64,
            w.units.len(),
            w.sheep.len()
        );
        for f in &w.factions {
            eprintln!("faction {} gathered {:.0} res {:?}", f.idx, f.gathered, f.res.floored());
            assert!(f.gathered > 0.0);
        }
    }
}
