// OWNER: S3 (economy) — see docs/spec/03b-gather-buildings.md §7 (construction: buildStep, completeBuilding,
// afterBuild), §8 (addBuilding, blockBuilding, census hooks, updateQueue, deploy, rally, relocateBuilding,
// destroyBuilding, farm breeding), §9 (tower/castle shooting), §10 (palisades); 04a §5 (queues, trainSpeed),
// §9.2 (zoneBlock), §10; addenda G1, G6; 04a addenda E3.
//
// Complete port. Palisade passability lives in `map` (own side passes, everyone else is blocked; enemies breach
// them in combat.rs); the anti-trap "cross" fallback lives in `path`. [`wall_shape`] is the render-only
// auto-orientation of a palisade cell (§10.3) shared with the client.
#![allow(dead_code)]

use crate::World;
use crate::cmd::{QueueWhat, zone_refusal};
use crate::defs::*;
use crate::events::{GameEvent, Msg, dur};
use crate::math::{V2, v2};
use crate::path::FieldTarget;
use crate::slab::{BuildingId, UnitId};
use crate::unit::NavResult;
use crate::world::{
    Building, DamageSource, GatherOrder, GatherTarget, MoveOrder, Order, RallyTarget, SheepHome,
    Shooter,
};
use crate::{TICK_DT, TILE};

/// Render topology of a palisade cell (§10.3): which neighbours (palisades of ANY faction) it connects to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WallShape {
    /// palisade west / east
    pub left: bool,
    pub right: bool,
    /// palisade north / south
    pub up: bool,
    pub down: bool,
}

impl WallShape {
    /// Draw the horizontal fence (`front = side || !(up || down)`: horizontal or isolated).
    pub fn front(&self) -> bool {
        self.left || self.right || !(self.up || self.down)
    }
    /// Draw the vertical stake column (`!front || up`); `front` too → corner (4 stakes behind the fence).
    pub fn stakes(&self) -> bool {
        !self.front() || self.up
    }
    /// Number of stakes in the column (4 when combined with a fence, else 5).
    pub fn stake_count(&self) -> u8 {
        if self.front() { STAKE_N - 1 } else { STAKE_N }
    }
}

/// `drawWall` neighbour test (§10.3) for cell `(x, y)` on a `w × h` grid; `is_wall(x, y)` = a palisade of any
/// faction stands there. Unlike the JS, x neighbours never wrap across rows.
pub fn wall_shape(x: i32, y: i32, w: i32, h: i32, is_wall: impl Fn(i32, i32) -> bool) -> WallShape {
    let at = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && is_wall(x, y);
    WallShape {
        left: at(x - 1, y),
        right: at(x + 1, y),
        up: at(x, y - 1),
        down: at(x, y + 1),
    }
}

impl World {
    /// `addBuilding(f, kind, cx, cy, done)` (§8.3): geometry, hp, house variant `houses++ % 3`, `cd = rand(0,1)`
    /// [RNG], rally/pasture defaults, block cells. Does NOT pay or touch `sites` (the `place` command does).
    pub fn add_building(
        &mut self,
        f: FactionIdx,
        kind: BuildingKind,
        cx: i32,
        cy: i32,
        done: bool,
    ) -> BuildingId {
        let mods = self.factions[f as usize].mods;
        let mut b = Building::new(f, kind, cx, cy, done, &mods);
        if kind.def().variants > 1 {
            let fac = &mut self.factions[f as usize];
            b.variant = (fac.houses % kind.def().variants as u32) as u8;
            fac.houses += 1;
        }
        b.cd = self.rng.f32();
        let id = self.buildings.insert(b);
        self.block_building(id, true);
        id
    }

    /// `blockBuilding(b, on)` (§8.4): `set_block(cells, ±1)` + `set_owner(cells, side|0, wall)`.
    pub fn block_building(&mut self, id: BuildingId, on: bool) {
        let Some(b) = self.buildings.get(id) else {
            return;
        };
        let cells = b.cells();
        let wall = b.kind == BuildingKind::Wall;
        let side = if on { side_of(b.faction) } else { 0 };
        self.map.set_block(&cells, if on { 1 } else { -1 });
        self.map.set_owner(&cells, side, wall);
    }

    /// `updateBuilding(b, dt)` (§8.11, §9, 04a §5.2): only done buildings: `update_queue`, farm breeding,
    /// tower/castle shooting via `fire_arrow` (range 420/380 + mods.range, dmg 15/14 + atk[archer], reload
    /// 1.1/1.2, rescan 0.3).
    pub fn update_building(&mut self, id: BuildingId) {
        let dt = TICK_DT;
        let Some(b) = self.buildings.get(id) else {
            return;
        };
        if !b.done || b.dead {
            return;
        }
        let f = b.faction;
        self.update_queue(id);
        let mods = self.factions[f as usize].mods;
        let Some(b) = self.buildings.get_mut(id) else {
            return;
        };
        if b.dead {
            return;
        }
        let kind = b.kind;
        if kind == BuildingKind::Farm {
            b.breed_t += dt * mods.breed;
            if b.breed_t >= FARM_BREED {
                b.breed_t = 0.0;
                // JS counts every sheep homed here still in `G.sheep` (dead ones are filtered later in the tick)
                let n = self
                    .sheep
                    .values()
                    .filter(|s| s.home == SheepHome::Farm(id))
                    .count();
                if n < FARM_SHEEP {
                    self.make_sheep(SheepHome::Farm(id));
                }
            }
            return;
        }
        let tower = kind == BuildingKind::Tower;
        if !tower && kind != BuildingKind::Castle {
            return;
        }
        b.cd -= dt;
        if b.cd > 0.0 {
            return;
        }
        let range = if tower { TOWER_RANGE } else { CASTLE_RANGE } + mods.range;
        let (bp, muzzle) = (b.pos, b.def().muzzle());
        let mut best = None;
        let mut bd = range;
        for (uid, o) in self.units.iter() {
            if o.dead || o.inside.is_some() || o.faction == f {
                continue;
            }
            let dx = o.pos.x - bp.x;
            if dx.abs() >= bd {
                continue;
            }
            let d = o.pos.dist(bp);
            if d < bd {
                bd = d;
                best = Some((uid, o.pos));
            }
        }
        let Some((_, tp)) = best else {
            if let Some(b) = self.buildings.get_mut(id) {
                b.cd = SHOOT_RESCAN;
            }
            return;
        };
        let dmg = if tower { TOWER_DMG } else { CASTLE_DMG } + mods.atk[UnitKind::Archer as usize];
        self.fire_arrow(
            f,
            Shooter::Building { id, kind },
            v2(bp.x, bp.y - muzzle),
            v2(tp.x, tp.y - ARROW_HIT_YOFF),
            dmg,
        );
        if let Some(b) = self.buildings.get_mut(id) {
            b.cd = if tower { TOWER_RELOAD } else { CASTLE_RELOAD };
        }
    }

    /// `updateQueue(b, dt)` (04a §5.2): advance head (units × `train_speed(has[kind])`, techs ×1), complete next
    /// tick (addenda E3): tech → busy off + `apply_tech`; unit → `room_for` ? deploy : wait + toast every 15 s
    /// (humans).
    pub fn update_queue(&mut self, id: BuildingId) {
        let dt = TICK_DT;
        let Some(b) = self.buildings.get(id) else {
            return;
        };
        let Some(q) = b.queue.first().copied() else {
            return;
        };
        let (f, kind) = (b.faction, b.kind);
        if q.t < q.total {
            let speed = match q.what {
                QueueWhat::Unit(_) => self.train_speed_of(f, kind),
                QueueWhat::Tech(_) => 1.0,
            };
            if let Some(b) = self.buildings.get_mut(id) {
                b.queue[0].t = q.total.min(q.t + dt * speed);
            }
            return;
        }
        match q.what {
            QueueWhat::Tech(t) => {
                if let Some(b) = self.buildings.get_mut(id) {
                    b.queue.remove(0);
                }
                self.factions[f as usize].busy.remove(t);
                self.apply_tech(f, t);
            }
            QueueWhat::Unit(u) => {
                if self.room_for(f, u) {
                    if let Some(b) = self.buildings.get_mut(id) {
                        b.queue.remove(0);
                    }
                    self.deploy(id, u);
                } else if self.factions[f as usize].human {
                    let time = self.time;
                    let Some(b) = self.buildings.get_mut(id) else {
                        return;
                    };
                    if time - b.housed_t > HOUSED_TOAST_EVERY {
                        b.housed_t = time;
                        let fac = &self.factions[f as usize];
                        let msg = if u == UnitKind::Pawn {
                            if fac.pawn_cap >= PAWN_MAX {
                                Msg::PawnsMax
                            } else {
                                Msg::NeedHouses
                            }
                        } else if fac.troop_cap >= TROOP_MAX {
                            Msg::TroopsMax
                        } else {
                            Msg::NeedCamps
                        };
                        self.emit(GameEvent::Toast { to: f, msg });
                    }
                }
            }
        }
    }

    /// Head of the queue is a finished unit waiting for room (UI: `In attesa di case` / `di accampamenti`).
    pub fn queue_waiting(&self, id: BuildingId) -> bool {
        self.buildings.get(id).is_some_and(|b| {
            b.queue
                .first()
                .is_some_and(|q| matches!(q.what, QueueWhat::Unit(_)) && q.t >= q.total)
        })
    }

    /// `deploy(b, type)` (§8.6): spawn at `nearest_passable(b.x + rand(-0.3,0.3)*w*64, b.y1 + 24)` [RNG], dust2,
    /// rally job for pawns (valid node → gather, unfinished own site → build) else move to the rally point ±
    /// jitter [RNG]; `pawn_n++`/`troop_n++`.
    pub fn deploy(&mut self, b: BuildingId, kind: UnitKind) -> Option<UnitId> {
        let bl = self.buildings.get(b)?;
        let (f, x, y1, w, rally) = (bl.faction, bl.pos.x, bl.rect.y1, bl.def().w, bl.rally);
        let dx = self.rng.range(-0.3, 0.3) * w as f32 * TILE;
        let p = self.map.nearest_passable(v2(x + dx, y1 + DEPLOY_DY));
        let u = self.spawn_unit(f, kind, p);
        self.emit(GameEvent::Fx {
            fx: FxKind::Dust2,
            x: p.x,
            y: p.y + 2.0,
            scale: 1.0,
            dy: 0.0,
            follow: None,
            color: None,
        });
        let pawn = kind == UnitKind::Pawn;
        let job = match rally.target {
            Some(RallyTarget::Gather(g)) if pawn && self.node_valid(g) => self.gather_order(g),
            Some(RallyTarget::Building(t))
                if pawn
                    && self
                        .buildings
                        .get(t)
                        .is_some_and(|t| !t.dead && !t.done && t.faction == f) =>
            {
                Some(Order::Build {
                    building: t,
                    resume: None,
                })
            }
            _ => None,
        };
        match job {
            Some(o) => self.set_order(u, o),
            None => {
                let jx = self.rng.range(-34.0, 34.0);
                let jy = self.rng.range(-22.0, 22.0);
                let q = self
                    .map
                    .nearest_passable(v2(rally.pos.x + jx, rally.pos.y + jy));
                self.set_order(u, Order::Move(MoveOrder::to(q)));
            }
        }
        let fac = &mut self.factions[f as usize];
        if pawn {
            fac.pawn_n += 1;
        } else {
            fac.troop_n += 1;
        }
        Some(u)
    }

    /// `buildStep(u, o, dt)` (§7.2): navigate to the ring, work: build progress (stacking per pawn) / repair
    /// (3.5%/s/pawn, free). Addenda G6: on 'blocked' resume the gather order if any.
    pub fn build_step(&mut self, unit: UnitId) {
        let dt = TICK_DT;
        let Some(u) = self.units.get(unit) else {
            return;
        };
        let (b, repair, resume) = match u.order {
            Order::Build { building, resume } => (building, false, resume),
            Order::Repair { building, resume } => (building, true, resume),
            _ => return,
        };
        let finished = match self.buildings.get(b) {
            None => true,
            Some(bl) => bl.dead || if repair { bl.hp >= bl.max_hp } else { bl.done },
        };
        if finished {
            self.after_build(unit, b);
            return;
        }
        let r = match self.field_of(unit, FieldTarget::Building(b)) {
            Some(fr) => self.nav_to(unit, fr),
            None => NavResult::Blocked,
        };
        if r == NavResult::Blocked {
            match resume {
                Some(g) => self.set_order(unit, Order::Gather(GatherOrder { first: None, ..g })),
                None => self.idle(unit),
            }
            return;
        }
        if r != NavResult::Arrived {
            return;
        }
        let Some(bp) = self.buildings.get(b).map(|b| b.pos) else {
            return;
        };
        self.work_on(unit, bp);
        if let Some(u) = self.units.get_mut(unit) {
            u.work_t += dt;
        }
        let Some(bl) = self.buildings.get_mut(b) else {
            return;
        };
        if repair {
            bl.hp = bl.max_hp.min(bl.hp + bl.max_hp * REPAIR_RATE * dt);
            return;
        }
        let work = bl.def().work.max(TICK_DT);
        bl.progress += dt;
        bl.hp = bl.max_hp.min(bl.hp + bl.max_hp * 0.9 * dt / work);
        if bl.progress >= bl.def().work {
            self.complete_building(b);
        }
    }

    /// `afterBuild(u, b)` (§7.4): next own site within 700 px, else (only after building a depot/farm) auto-gather
    /// the nearest valid explored node within 520 px of it, else resume the interrupted gathering, else idle.
    pub fn after_build(&mut self, unit: UnitId, b: BuildingId) {
        let Some(u) = self.units.get(unit) else {
            return;
        };
        let (was_building, resume) = match u.order {
            Order::Build { resume, .. } => (true, resume),
            Order::Repair { resume, .. } => (false, resume),
            _ => (false, None),
        };
        let (f, upos) = (u.faction, u.pos);
        let mut next = None;
        let mut bd = NEXT_SITE_RADIUS;
        for (id, o) in self.buildings.iter() {
            if o.faction != f || o.done || o.dead {
                continue;
            }
            let d = o.dist(upos);
            if d < bd {
                bd = d;
                next = Some(id);
            }
        }
        if let Some(next) = next {
            self.set_order(
                unit,
                Order::Build {
                    building: next,
                    resume,
                },
            );
            return;
        }
        if was_building
            && let Some(bl) = self.buildings.get(b)
            && !bl.dead
            && bl.done
            && matches!(bl.kind, BuildingKind::Depot | BuildingKind::Farm)
        {
            let farm = bl.kind == BuildingKind::Farm;
            let rect = bl.rect;
            let mut node = None;
            let mut nd = AUTO_NODE_RADIUS;
            let mut consider = |g: GatherTarget, p: V2, w: &World| {
                if !w.node_valid(g) || !w.explored(f, p) {
                    return;
                }
                let d = rect.dist(p);
                if d < nd {
                    nd = d;
                    node = Some(g);
                }
            };
            if farm {
                for (sid, s) in self.sheep.iter() {
                    consider(GatherTarget::Sheep(sid), s.pos, self);
                }
            } else {
                for (i, n) in self.nodes.iter().enumerate() {
                    consider(
                        GatherTarget::Node(crate::slab::NodeId(i as u32)),
                        n.pos,
                        self,
                    );
                }
            }
            if let Some(o) = node.and_then(|g| self.gather_order(g)) {
                self.set_order(unit, o);
                return;
            }
        }
        match resume {
            Some(g) => self.set_order(unit, Order::Gather(GatherOrder { first: None, ..g })),
            None => self.idle(unit),
        }
    }

    /// `completeBuilding(b)` (§7.3): done, dust2 per column, farm → 2 sheep, census, humans (not walls): play
    /// `build` + banner `Costruzione finita: label`.
    pub fn complete_building(&mut self, id: BuildingId) {
        let Some(b) = self.buildings.get_mut(id) else {
            return;
        };
        if b.done || b.dead {
            return;
        }
        b.done = true;
        b.progress = b.progress.max(b.def().work);
        let (f, kind, x0, by, w) = (b.faction, b.kind, b.rect.x0, b.pos.y, b.def().w);
        for i in 0..w {
            self.emit(GameEvent::Fx {
                fx: FxKind::Dust2,
                x: x0 + (i as f32 + 0.5) * TILE,
                y: by + 4.0,
                scale: if w > 1 { 1.5 } else { 1.0 },
                dy: 0.0,
                follow: None,
                color: None,
            });
        }
        if kind == BuildingKind::Farm {
            for _ in 0..FARM_START_SHEEP {
                self.make_sheep(SheepHome::Farm(id));
            }
        }
        self.census(f);
        if self.factions[f as usize].human && kind != BuildingKind::Wall {
            self.emit(GameEvent::Play {
                to: f,
                sound: Sound::Build,
            });
            self.emit(GameEvent::Say {
                to: Some(f),
                msg: Msg::BuildingDone(kind),
                color: Some(f),
                dur: dur::BUILDING_DONE,
            });
        }
    }

    /// `relocateBuilding(b, cx, cy)` (§8.8): instant, free; keeps hp/queue/progress/occupants; rally and pasture
    /// shift with it (farm sheep follow: their home is the farm id, addenda G1); dust2 on both edges, sound build.
    /// Validation (`moveBlock`) is the command's job.
    pub fn relocate_building(&mut self, id: BuildingId, cx: i32, cy: i32) {
        let Some(b) = self.buildings.get(id) else {
            return;
        };
        if b.dead || (b.cx == cx && b.cy == cy) {
            return;
        }
        let d = v2((cx - b.cx) as f32 * TILE, (cy - b.cy) as f32 * TILE);
        self.edge_dust(id);
        self.block_building(id, false);
        let Some(b) = self.buildings.get_mut(id) else {
            return;
        };
        b.place_at(cx, cy);
        b.rally.pos += d;
        b.pasture += d;
        let (centre, pos, inside) = (b.rect.center(), b.pos, b.inside.clone());
        self.block_building(id, true);
        for u in inside {
            if let Some(u) = self.units.get_mut(u) {
                u.pos = centre;
            }
        }
        self.edge_dust(id);
        self.emit(GameEvent::Sound {
            sound: Sound::Build,
            x: pos.x,
            y: pos.y,
        });
    }

    fn edge_dust(&mut self, id: BuildingId) {
        let Some(b) = self.buildings.get(id) else {
            return;
        };
        let (x0, y, w) = (b.rect.x0, b.pos.y, b.def().w);
        for i in 0..w {
            self.emit(GameEvent::fx(
                FxKind::Dust2,
                x0 + (i as f32 + 0.5) * TILE,
                y + 4.0,
            ));
        }
    }

    /// `destroyBuilding(b, by)` (§8.10): dead, unblock, eject sheltered pawns, free queued techs from `busy`
    /// (no refund), boom2 ×(w+1) [RNG], sound boom, farm sheep keep grazing the spot (G1), raze credit + loot
    /// (3 × 15, life 60, not walls) when killed by another faction, castle → `eliminate_faction`.
    pub fn destroy_building(&mut self, id: BuildingId, by: DamageSource) {
        let Some(b) = self.buildings.get_mut(id) else {
            return;
        };
        if b.dead {
            return;
        }
        b.dead = true;
        let (f, kind, pos, y1, pasture, w, ay) = (
            b.faction,
            b.kind,
            b.pos,
            b.rect.y1,
            b.pasture,
            b.def().w,
            b.def().ay as f32,
        );
        let inside = b.inside.clone();
        let queued: Vec<_> = b.queue.iter().map(|q| q.what).collect();
        self.block_building(id, false);
        for u in inside {
            self.leave_shelter(u);
        }
        for q in queued {
            if let QueueWhat::Tech(t) = q {
                self.factions[f as usize].busy.remove(t);
            }
        }
        for _ in 0..=w {
            let x = pos.x + self.rng.range(-0.4, 0.4) * w as f32 * TILE;
            let y = pos.y - self.rng.range(10.0, (ay * 0.5).max(10.0));
            let scale = if w > 1 { self.rng.range(1.0, 1.5) } else { 0.7 };
            self.emit(GameEvent::Fx {
                fx: FxKind::Boom2,
                x,
                y,
                scale,
                dy: 80.0,
                follow: None,
                color: None,
            });
        }
        self.emit(GameEvent::Sound {
            sound: Sound::Boom,
            x: pos.x,
            y: pos.y,
        });
        if kind == BuildingKind::Farm {
            for (_, s) in self.sheep.iter_mut() {
                if s.home == SheepHome::Farm(id) {
                    s.home = SheepHome::Spot(pasture);
                }
            }
        }
        if let Some(bf) = by.faction.filter(|bf| *bf != f) {
            if let Some(fac) = self.factions.get_mut(bf as usize) {
                fac.razed += 1;
            }
            if kind != BuildingKind::Wall {
                let p = self.map.nearest_passable(v2(pos.x, y1 + 20.0));
                let loot = match kind {
                    BuildingKind::Farm => [Res::Meat, Res::Meat, Res::Wood],
                    BuildingKind::Tower => [Res::Stone, Res::Stone, Res::Wood],
                    _ => [Res::Wood, Res::Wood, Res::Gold],
                };
                for r in loot {
                    let jx = self.rng.range(-30.0, 30.0);
                    self.drop_pickup(r, v2(p.x + jx, p.y), LOOT_AMOUNT, LOOT_LIFE, false);
                }
            }
        }
        if kind == BuildingKind::Castle {
            self.eliminate_faction(f, by.faction);
        }
    }

    /// Drops dead buildings from the slab (after the building phase).
    pub fn remove_dead_buildings(&mut self) {
        self.buildings.retain(|_, b| !b.dead);
    }

    /// `zoneBlock(f, kind, cx, cy)` (04a §9.2): footprint centre within 12 (defense) / 40 cells of the castle
    /// centre, or within 9 of an owned outpost. Returns the refusal string.
    pub fn zone_block(
        &self,
        f: FactionIdx,
        kind: BuildingKind,
        cx: i32,
        cy: i32,
    ) -> Option<String> {
        let d = kind.def();
        let (x, y) = (cx as f32 + d.w as f32 / 2.0, cy as f32 + d.h as f32 / 2.0);
        let r = if d.defense {
            DEFENSE_RADIUS
        } else {
            BUILD_RADIUS
        };
        let fac = &self.factions[f as usize];
        if let Some(c) = fac.castle.and_then(|c| self.buildings.get(c)) {
            let (ccx, ccy) = c.center_cells();
            if (x - ccx).hypot(y - ccy) <= r {
                return None;
            }
        }
        for p in &self.points {
            if p.owner == Some(f)
                && p.kind.def().zone
                && (x - p.gx).hypot(y - p.gy) <= OUTPOST_RADIUS
            {
                return None;
            }
        }
        Some(zone_refusal(d.defense))
    }

    /// `shelterFull(b)`: houses hold 5, the castle is unlimited.
    pub fn shelter_full(&self, id: BuildingId) -> bool {
        self.buildings
            .get(id)
            .is_none_or(|b| b.kind != BuildingKind::Castle && b.inside.len() >= SHELTER_CAP)
    }

    /// Render shape of the palisade standing on `cell` (any faction's palisades connect).
    pub fn wall_shape_at(&self, cell: i32) -> WallShape {
        let (x, y) = self.map.xy(cell);
        let (w, h) = (self.map.w, self.map.h);
        wall_shape(x, y, w, h, |x, y| {
            self.map.wall_of[(y * w + x) as usize] != 0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_topology() {
        let walls = [(1, 1), (2, 1), (3, 1), (3, 2), (3, 3), (6, 6)];
        let is = |x: i32, y: i32| walls.contains(&(x, y));
        let s = wall_shape(1, 1, 10, 10, is);
        assert!(s.front() && !s.stakes());
        // corner: fence + stakes going down
        let s = wall_shape(3, 1, 10, 10, is);
        assert!(s.front() && !s.up && s.down);
        let s = wall_shape(3, 2, 10, 10, is);
        assert!(!s.front() && s.stakes() && s.stake_count() == 5);
        // isolated: a fence
        let s = wall_shape(6, 6, 10, 10, is);
        assert!(s.front() && !s.stakes());
        // no wrap across rows at the edge
        let s = wall_shape(0, 2, 4, 4, |x, y| (x, y) == (3, 1));
        assert!(!s.left);
    }

    use crate::gather::tests::{cell_pos, run, spawn, test_world};
    use crate::world::QueueItem;

    fn queue(w: &mut World, b: BuildingId, what: QueueWhat, total: f32) {
        w.buildings.get_mut(b).unwrap().queue.push(QueueItem {
            what,
            t: 0.0,
            total,
        });
    }

    #[test]
    fn training_speed_bonus() {
        let mut w = test_world();
        let a = w.add_building(0, BuildingKind::Barracks, 30, 40, true);
        w.census(0);
        queue(&mut w, a, QueueWhat::Unit(UnitKind::Warrior), 20.0);
        run(&mut w, 5.0);
        let t1 = w.buildings.get(a).unwrap().queue[0].t;
        assert!((t1 - 5.0).abs() < 0.06, "one barracks ×1: {t1}");
        // a second (finished) barracks: +20 % for both
        w.add_building(0, BuildingKind::Barracks, 36, 40, true);
        w.census(0);
        run(&mut w, 5.0);
        let t2 = w.buildings.get(a).unwrap().queue[0].t - t1;
        assert!((t2 - 6.0).abs() < 0.06, "two barracks ×1.2: {t2}");
        // research is never accelerated
        queue(&mut w, a, QueueWhat::Tech(TechId::Boots), 10.0);
        w.factions[0].busy.insert(TechId::Boots);
        run(&mut w, 10.0);
        // warrior out (20 s of work done at ×1.2) one tick after the bar fills (addenda E3)
        let b = w.buildings.get(a).unwrap();
        assert_eq!(b.queue.len(), 1);
        assert!(matches!(b.queue[0].what, QueueWhat::Tech(TechId::Boots)));
        assert_eq!(w.factions[0].troop_n, 1);
        run(&mut w, 10.5);
        assert!(w.buildings.get(a).unwrap().queue.is_empty());
        assert!(w.factions[0].has_tech(TechId::Boots));
        assert!(!w.factions[0].busy.has(TechId::Boots));
        assert_eq!(train_speed(1), 1.0);
        assert!((train_speed(9) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn no_room_waits_with_toast() {
        let mut w = test_world();
        let castle = w.factions[0].castle.unwrap();
        // 10 pawns = cap of the castle alone
        for i in 0..10 {
            spawn(&mut w, 0, UnitKind::Pawn, 30 + i, 45);
        }
        assert_eq!(w.factions[0].pawn_cap, 10);
        queue(&mut w, castle, QueueWhat::Unit(UnitKind::Pawn), 1.0);
        run(&mut w, 2.0);
        assert!(w.queue_waiting(castle));
        let toasts = |w: &World| {
            w.events
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        GameEvent::Toast {
                            to: 0,
                            msg: Msg::NeedHouses
                        }
                    )
                })
                .count()
        };
        assert_eq!(toasts(&w), 1);
        run(&mut w, 10.0);
        assert_eq!(toasts(&w), 1, "at most one toast every 15 s");
        // a house gives room: the pawn comes out
        w.add_building(0, BuildingKind::House, 30, 50, true);
        w.census(0);
        run(&mut w, 0.2);
        assert!(w.buildings.get(castle).unwrap().queue.is_empty());
        assert_eq!(w.factions[0].pawn_n, 11);
    }

    #[test]
    fn builders_stack_and_rally_jobs() {
        let mut w = test_world();
        let site = w.add_building(0, BuildingKind::House, 30, 40, false);
        w.census(0);
        assert_eq!(w.factions[0].sites[BuildingKind::House as usize], 1);
        let a = spawn(&mut w, 0, UnitKind::Pawn, 29, 40);
        let b = spawn(&mut w, 0, UnitKind::Pawn, 32, 40);
        for u in [a, b] {
            w.set_order(
                u,
                Order::Build {
                    building: site,
                    resume: None,
                },
            );
        }
        let work = BuildingKind::House.def().work;
        run(&mut w, work / 2.0 - 0.3);
        assert!(!w.buildings.get(site).unwrap().done);
        run(&mut w, 0.6);
        let bl = w.buildings.get(site).unwrap();
        assert!(bl.done, "two pawns build twice as fast");
        assert!((bl.hp - bl.max_hp).abs() < 1.0);
        assert_eq!(w.factions[0].pawn_cap, 15);
        // repair: 3.5 %/s per pawn
        w.buildings.get_mut(site).unwrap().hp = bl.max_hp * 0.5;
        w.set_order(
            a,
            Order::Repair {
                building: site,
                resume: None,
            },
        );
        run(&mut w, 2.0);
        let hp = w.buildings.get(site).unwrap().hp / w.buildings.get(site).unwrap().max_hp;
        assert!(hp > 0.55 && hp < 0.58, "repaired to {hp}");
        // rally on a tree: new pawns go gather there
        let castle = w.factions[0].castle.unwrap();
        let tree = w
            .nodes
            .iter()
            .position(|n| n.kind == NodeKind::Tree)
            .unwrap();
        let g = GatherTarget::Node(crate::slab::NodeId(tree as u32));
        w.buildings.get_mut(castle).unwrap().rally.target = Some(RallyTarget::Gather(g));
        let p = w.deploy(castle, UnitKind::Pawn).unwrap();
        assert!(matches!(w.units.get(p).unwrap().order, Order::Gather(o) if o.first == Some(g)));
        let s = w.deploy(castle, UnitKind::Scout).unwrap();
        assert!(matches!(w.units.get(s).unwrap().order, Order::Move(_)));
    }

    #[test]
    fn farm_breeds_sheep_and_moves_with_its_flock() {
        let mut w = test_world();
        let farm = w.add_building(0, BuildingKind::Farm, 60, 40, false);
        w.complete_building(farm);
        let flock = |w: &World| {
            w.sheep
                .values()
                .filter(|s| s.home == SheepHome::Farm(farm))
                .count()
        };
        assert_eq!(flock(&w), FARM_START_SHEEP);
        run(&mut w, FARM_BREED + 0.1);
        assert_eq!(flock(&w), 3);
        run(&mut w, 5.0 * FARM_BREED);
        assert_eq!(flock(&w), FARM_SHEEP);
        // move: hp/queue kept, rally and pasture follow, sheep still count
        let before = w.buildings.get(farm).unwrap().clone();
        w.relocate_building(farm, 70, 44);
        let after = w.buildings.get(farm).unwrap();
        assert_eq!((after.cx, after.cy), (70, 44));
        assert_eq!(after.hp, before.hp);
        assert_eq!(after.rally.pos - before.rally.pos, v2(640.0, 256.0));
        assert_eq!(after.pasture - before.pasture, v2(640.0, 256.0));
        assert!(w.map.passable(w.map.idx(61, 41), 0));
        assert!(!w.map.passable(w.map.idx(71, 45), 0));
        run(&mut w, FARM_BREED + 0.1);
        assert_eq!(flock(&w), FARM_SHEEP);
        // destroyed: the flock keeps grazing the last spot
        w.destroy_building(farm, DamageSource::NONE);
        assert_eq!(flock(&w), 0);
        assert_eq!(
            w.sheep
                .values()
                .filter(|s| matches!(s.home, SheepHome::Spot(_)))
                .count(),
            FARM_SHEEP
        );
    }

    #[test]
    fn destruction_loot_and_castle_loss() {
        let mut w = test_world();
        let depot = w.add_building(0, BuildingKind::Depot, 60, 40, true);
        let wall = w.add_building(0, BuildingKind::Wall, 64, 40, true);
        let by = DamageSource {
            unit: None,
            faction: Some(1),
            arrow: false,
        };
        w.destroy_building(wall, by);
        assert!(w.pickups.is_empty(), "palisades give no loot");
        w.destroy_building(depot, by);
        assert_eq!(w.pickups.len(), 3);
        assert_eq!(w.factions[1].razed, 2);
        let wood: f32 = w
            .pickups
            .values()
            .filter(|p| p.res == Res::Wood)
            .map(|p| p.amount)
            .sum();
        assert_eq!(wood, 2.0 * LOOT_AMOUNT);
        // loot expires after 60 s
        run(&mut w, LOOT_LIFE + 0.1);
        assert!(w.pickups.is_empty());
        let castle = w.factions[0].castle.unwrap();
        w.destroy_building(castle, by);
        assert!(!w.factions[0].alive);
    }

    #[test]
    fn palisade_passability() {
        let mut w = test_world();
        let wall = w.add_building(0, BuildingKind::Wall, 64, 40, true);
        let c = w.map.idx(64, 40);
        assert!(
            w.map.passable(c, side_of(0)),
            "own units pass and stand on own palisades"
        );
        assert!(!w.map.passable(c, side_of(1)), "enemies are blocked");
        assert!(!w.map.passable(c, 0), "sheep (side 0) are blocked");
        assert!(!w.map.can_place(BuildingKind::Wall, 64, 40, &[]));
        let s = w.wall_shape_at(c);
        assert!(s.front() && !s.stakes());
        w.add_building(0, BuildingKind::Wall, 64, 41, true);
        assert!(w.wall_shape_at(c).down);
        w.destroy_building(wall, DamageSource::NONE);
        assert!(w.map.passable(c, side_of(1)));
        let _ = cell_pos(&w, 0, 0);
    }
}
