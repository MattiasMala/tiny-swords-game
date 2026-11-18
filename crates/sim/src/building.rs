// OWNER: S3 (economy) — see docs/spec/03b-gather-buildings.md §7 (construction: buildStep, completeBuilding,
// afterBuild), §8 (addBuilding, blockBuilding, census hooks, updateQueue, deploy, rally, relocateBuilding,
// destroyBuilding, farm breeding), §9 (tower/castle shooting), §10 (palisades); 04a §5 (queues, trainSpeed),
// §9.2 (zoneBlock), §10; addenda G1, G6; 04a addenda E3.
//
// Stub fns S3 must fill: update_building (queue, farm breeding, tower/castle shots — STUB no-op),
// update_queue, build_step, after_build (STUB: idle), relocate_building, complete_building (MINIMAL),
// destroy_building (MINIMAL: no loot, no FX scatter), deploy (MINIMAL: no rally jobs).
// Done by the foundation (verify): add_building, block_building, remove_dead_buildings, zone_block.
#![allow(dead_code, unused)]

use crate::World;
use crate::cmd::zone_refusal;
use crate::defs::*;
use crate::events::{GameEvent, Msg, dur};
use crate::math::{V2, v2};
use crate::slab::{BuildingId, UnitId};
use crate::world::{Building, DamageSource, MoveOrder, Order};

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
    /// 1.1/1.2, rescan 0.3). STUB: no-op.
    pub fn update_building(&mut self, id: BuildingId) {}

    /// `updateQueue(b, dt)` (04a §5.2): advance head (units × `train_speed(has[kind])`, techs ×1), complete next
    /// tick: tech → busy off + `apply_tech`; unit → `room_for` ? deploy : wait + toast every 15 s (humans). STUB.
    pub fn update_queue(&mut self, id: BuildingId) {}

    /// `deploy(b, type)` (§8.6): spawn at `nearest_passable(b.x + rand(-0.3,0.3)*w*64, b.y1 + 24)` [RNG], dust2,
    /// rally job for pawns (node → gather, unfinished own site → build) else move to the rally point ± jitter
    /// [RNG]; `pawn_n++`/`troop_n++`. MINIMAL: spawn + move to the rally point.
    pub fn deploy(&mut self, b: BuildingId, kind: UnitKind) -> Option<UnitId> {
        let bl = self.buildings.get(b)?;
        let (f, x, y1, w, rally) = (bl.faction, bl.pos.x, bl.rect.y1, bl.def().w, bl.rally);
        let dx = self.rng.range(-0.3, 0.3) * w as f32 * crate::TILE;
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
        let jx = self.rng.range(-34.0, 34.0);
        let jy = self.rng.range(-22.0, 22.0);
        let q = self
            .map
            .nearest_passable(v2(rally.pos.x + jx, rally.pos.y + jy));
        self.set_order(u, Order::Move(MoveOrder::to(q)));
        let fac = &mut self.factions[f as usize];
        if kind == UnitKind::Pawn {
            fac.pawn_n += 1;
        } else {
            fac.troop_n += 1;
        }
        Some(u)
    }

    /// `buildStep(u, o, dt)` (§7.2): navigate to the ring, work: build progress / repair (3.5%/s/pawn).
    /// Addenda G6: on 'blocked' resume the gather order if any. STUB.
    pub fn build_step(&mut self, unit: UnitId) {}

    /// `afterBuild(u, b)` (§7.4): next site within 700 px, else auto-gather next to a new depot/farm, else resume.
    /// STUB: idle.
    pub fn after_build(&mut self, unit: UnitId, b: BuildingId) {
        self.idle(unit);
    }

    /// `completeBuilding(b)` (§7.3): done, dust2 per column, farm → 2 sheep, census, humans (not walls): play
    /// `build` + banner `Costruzione finita: label`. MINIMAL: done + census + events.
    pub fn complete_building(&mut self, id: BuildingId) {
        let Some(b) = self.buildings.get_mut(id) else {
            return;
        };
        b.done = true;
        b.progress = b.def().work;
        let (f, kind) = (b.faction, b.kind);
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

    /// `relocateBuilding(b, cx, cy)` (§8.8): unblock, move geometry/rally/occupants, re-block, dust2, sound build.
    /// STUB.
    pub fn relocate_building(&mut self, id: BuildingId, cx: i32, cy: i32) {}

    /// `destroyBuilding(b, by)` (§8.10): dead, unblock, eject sheltered pawns, free queued techs from `busy`
    /// (no refund), boom2 ×(w+1) [RNG], sound boom, raze credit + loot (3 × 15, life 60, not walls) when killed by
    /// another faction, castle → `eliminate_faction`. MINIMAL: no loot/FX scatter.
    pub fn destroy_building(&mut self, id: BuildingId, by: DamageSource) {
        let Some(b) = self.buildings.get(id) else {
            return;
        };
        if b.dead {
            return;
        }
        let (f, kind, pos, inside) = (b.faction, b.kind, b.pos, b.inside.clone());
        self.block_building(id, false);
        for u in inside {
            self.leave_shelter(u);
        }
        let queued: Vec<_> = self
            .buildings
            .get(id)
            .map(|b| b.queue.clone())
            .unwrap_or_default();
        for q in queued {
            if let crate::cmd::QueueWhat::Tech(t) = q.what {
                self.factions[f as usize].busy.remove(t);
            }
        }
        if let Some(b) = self.buildings.get_mut(id) {
            b.dead = true;
        }
        self.emit(GameEvent::Fx {
            fx: FxKind::Boom2,
            x: pos.x,
            y: pos.y - 10.0,
            scale: 1.0,
            dy: 80.0,
            follow: None,
            color: None,
        });
        self.emit(GameEvent::Sound {
            sound: Sound::Boom,
            x: pos.x,
            y: pos.y,
        });
        if let Some(bf) = by.faction.filter(|bf| *bf != f) {
            self.factions[bf as usize].razed += 1;
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
}
