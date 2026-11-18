// OWNER: S3 (economy) — see docs/spec/03b-gather-buildings.md §2 (nodes: nodeValid/Room, chopTree, updateResources
// for trees/veins, pickups §2.4), §3 (claim, gatherOrder, setOrder side effects), §4 (dropSite, findNode,
// gatherStep, deposit, workOn), §5 (updatePawn), §6 (threatNear, shelterFor, enter/leaveShelter, shelterStep,
// resumeWork, embolden), §4.7 nodeNear; addenda G2-G4, G6-G8. Sheep live in sheep.rs.
//
// Stub fns S3 must fill: update_pawn (MINIMAL: plain move only), gather_step, find_node, deposit, work_on,
// shelter_step, resume_work, embolden, update_pickups (MINIMAL: hop only, no pick-up), node_near.
// Done by the foundation (verify): claim, gather_order, node_valid, node_room, drop_site_at, threat_near,
// shelter_for, enter_shelter, leave_shelter, update_nodes, drop_pickup.
#![allow(dead_code, unused)]

use crate::TICK_DT;
use crate::World;
use crate::defs::*;
use crate::events::GameEvent;
use crate::math::{V2, v2};
use crate::slab::{BuildingId, PickupId, UnitId};
use crate::world::{GatherOrder, GatherTarget, Order, Pickup};

impl World {
    /// `updatePawn(u, dt)` (§5): fear check every 0.4 s → shelter; tool; pause; gather / build|repair / drop /
    /// idle / move. MINIMAL: travels `Move` orders then idles.
    pub fn update_pawn(&mut self, id: UnitId) {
        let Some(u) = self.units.get(id) else { return };
        if u.inside.is_some() {
            return;
        }
        if let Order::Move(m) = u.order.clone()
            && self.travel(id, m.to, TRAVEL_NEAR)
        {
            self.idle(id);
        }
    }

    /// `gatherStep(u, o, dt)` (§4.3). STUB.
    pub fn gather_step(&mut self, id: UnitId) {}

    /// `findNode(u, o)` (§4.2): player's pick, else nearest free explored node within 900 px of the origin
    /// (+0.3 × distance to the pawn), humans widen to `WORK_RADIUS` around the castle (always for meat). STUB.
    pub fn find_node(&mut self, id: UnitId) -> Option<GatherTarget> {
        None
    }

    /// `deposit(u)` (§4.4): res += load, gathered, float `+N` + coin sound for humans, pause 0.4. STUB.
    pub fn deposit(&mut self, id: UnitId) {}

    /// `workOn(u, target, dt)` (§4.5): face target, `chop` every 0.6 s of work, state Work, anim Work + tool. STUB.
    pub fn work_on(&mut self, id: UnitId, at: V2) {}

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
                    n.users = (n.users as i32 + d).max(0) as u8;
                }
            }
            GatherTarget::Sheep(s) => {
                if let Some(s) = self.sheep.get_mut(s) {
                    s.users = (s.users as i32 + d).max(0) as u8;
                }
            }
        }
    }

    /// `gatherOrder(n)` = `{k:'gather', res, first: n, px: n.x, py: n.y}`.
    pub fn gather_order(&self, g: GatherTarget) -> Option<Order> {
        let pos = self.gather_pos(g)?;
        let res = match g {
            GatherTarget::Node(n) => self.nodes[n.0 as usize].res(),
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
    pub fn threat_near(&self, f: FactionIdx, p: V2, r: f32) -> bool {
        self.units.values().any(|e| {
            e.active() && e.faction != f && e.def().is_soldier() && {
                let d = e.pos - p;
                d.x.abs() < r && d.y.abs() < r && d.len() < r
            }
        })
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
        if let Some(bl) = self.buildings.get_mut(b) {
            bl.inside.push(id);
        }
        if let Some(u) = self.units.get_mut(id) {
            let p = u.pos;
            u.inside = Some(b);
            u.pos = c;
            u.moving = false;
            u.state = crate::world::UnitState::Idle;
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

    /// `shelterStep(u, o, dt)` (§6.4). STUB.
    pub fn shelter_step(&mut self, id: UnitId) {}

    /// `resumeWork(u)` (§6.4). STUB: idle.
    pub fn resume_work(&mut self, id: UnitId) {
        self.idle(id);
    }

    /// `embolden(u)` (§6.5): human direct orders make a threatened pawn brave. STUB.
    pub fn embolden(&mut self, id: UnitId) {}

    /// `nodeNear(f, res, p)` (§4.7 / 04a §11.2): nearest valid, roomy, explored node of `res` within
    /// `WORK_RADIUS` of the castle → `(node, distance)`. STUB: `None`.
    pub fn node_near(&self, f: FactionIdx, res: Res, p: V2) -> Option<(GatherTarget, f32)> {
        None
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
    /// (any faction), res += amount (not `gathered`), chest → prop `chest_open` + toast, float + coin for humans;
    /// expire at life 0. MINIMAL: hop + expiry only.
    pub fn update_pickups(&mut self) {
        let dt = TICK_DT;
        for (_, p) in self.pickups.iter_mut() {
            p.t += dt;
            p.life -= dt;
            if p.t < PICKUP_HOP {
                let k = p.t / PICKUP_HOP;
                p.pos = p.from.lerp(p.to, k);
                p.hop = (k * core::f32::consts::PI).sin() * 30.0;
            } else {
                p.pos = if p.chest { p.pos } else { p.to };
                p.hop = 0.0;
            }
        }
        self.pickups.retain(|_, p| p.life > 0.0);
    }
}
