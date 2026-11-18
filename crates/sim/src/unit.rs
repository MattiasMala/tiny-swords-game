// OWNER: S2 (units) — see docs/spec/03a-units-combat.md §4 (unit object), §5.1 (setOrder/idle), §7 (movement &
// navigation: moveUnit, approach, travel, navTo, holdPost, eject rule), §7.5 (separation), §10.4 (killUnit),
// §12.1 (scouts / frontier), §13 (updateUnit, unitAct) + addenda U2, U5, U6; 03b §4.8 (jobOf).
//
// Stub fns S2 must fill (the foundation left minimal working versions where noted "MINIMAL"):
//   spawn_unit (MINIMAL), update_unit (skeleton of §13; verify order), separate_units (§7.5, no-op now),
//   kill_unit (MINIMAL), set_order / idle (done per §5.1), move_unit / approach / travel / nav_to (done per §7.1-7.3,
//   verify), hold_post (§7.3), update_scout + frontier (§12.1), unit_act / job_of (done), remove_dead_units (done).
// RNG: only `World.rng` (thinkT, fearT staggering); `animT` randomness is client-side (addenda U6).
#![allow(dead_code, unused)]

use crate::World;
use crate::defs::*;
use crate::events::GameEvent;
use crate::math::{V2, v2};
use crate::path::{FieldId, FieldRef, FieldTarget};
use crate::slab::UnitId;
use crate::world::{DamageSource, GatherTarget, Order, Target, Unit, UnitState};
use crate::{TICK_DT, TILE};

/// Result of `navTo` (entities.js:308-315).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavResult {
    /// standing on a cell with field value 0 (ring of the target)
    Arrived,
    /// stepped toward the next cell
    Moving,
    /// no next cell (unreachable)
    Blocked,
}

impl World {
    /// `spawnUnit(f, type, x, y)` (entities.js:250-268): new unit with faction stats; `think_t = rng*0.4`,
    /// `fear_t = rng*0.4` [RNG]. MINIMAL: complete; S2 may add bookkeeping.
    pub fn spawn_unit(&mut self, f: FactionIdx, kind: UnitKind, pos: V2) -> UnitId {
        let mut u = Unit::new(f, kind, pos, &self.factions[f as usize].mods);
        u.think_t = self.rng.f32() * 0.4;
        u.fear_t = self.rng.f32() * 0.4;
        self.units.insert(u)
    }

    /// `updateUnit(u, dt)` (03a §13, entities.js:999-1027). Skeleton of the exact order; behaviour fns are stubs:
    /// sheltered → `update_pawn` only; timers; faith regen; `work` → idle; `tick_action` busy check; eject rule
    /// (§7.4); dispatch pawn / monk / scout / soldier; locomotion anim; `act`, `job`.
    /// Running-dust FX timers are client-side (addenda U6).
    pub fn update_unit(&mut self, id: UnitId) {
        let dt = TICK_DT;
        let Some(u) = self.units.get(id) else { return };
        let kind = u.kind;
        if u.inside.is_some() {
            self.update_pawn(id);
            self.refresh_act(id);
            return;
        }
        let heal_mod = self.factions[u.faction as usize].mods.heal;
        {
            let u = self.units.get_mut(id).expect("checked");
            u.cd -= dt;
            u.hurt_t -= dt;
            u.anim_t += dt;
            u.moving = false;
            u.cross = false;
            let d = kind.def();
            if d.heal > 0.0 {
                u.faith = (u.faith + d.faith_regen * heal_mod * dt).min(d.faith);
            }
            if u.state == UnitState::Work {
                u.state = UnitState::Idle;
            }
        }
        if !self.tick_action(id) && !self.eject_from_building(id) {
            let d = kind.def();
            if kind == UnitKind::Pawn {
                self.update_pawn(id);
            } else if d.heal > 0.0 {
                self.update_monk(id);
            } else if d.scout {
                self.update_scout(id);
            } else {
                self.update_soldier(id);
            }
        }
        if let Some(u) = self.units.get_mut(id)
            && matches!(u.state, UnitState::Idle | UnitState::Move)
        {
            u.state = if u.moving {
                UnitState::Move
            } else {
                UnitState::Idle
            };
            let a = if u.moving {
                AnimKind::Run
            } else {
                AnimKind::Idle
            };
            if u.anim != a {
                u.anim = a;
                u.anim_t = 0.0;
            }
        }
        self.refresh_act(id);
    }

    /// Recomputes `act` and `job` (end of `updateUnit`).
    pub fn refresh_act(&mut self, id: UnitId) {
        let act = self.unit_act(id);
        let job = self.units.get(id).and_then(|u| {
            if u.kind == UnitKind::Pawn {
                job_of(&u.order)
            } else {
                None
            }
        });
        if let Some(u) = self.units.get_mut(id) {
            u.act = act;
            u.job = job;
        }
    }

    /// Eject rule (§7.4, entities.js:1005-1009): if the cell is not passable for the side and (not crossable, or
    /// idle without target) walk to `nearest_passable` and skip behaviour this tick. Returns true if ejecting.
    pub fn eject_from_building(&mut self, id: UnitId) -> bool {
        let Some(u) = self.units.get(id) else {
            return false;
        };
        let side = side_of(u.faction);
        let c = self.map.cell_of(u.pos);
        let idle_no_target = u.order.is_idle() && u.target.is_none();
        if !self.map.passable(c, side) && (!self.map.crossable(c, side) || idle_no_target) {
            let to = self.map.nearest_passable(u.pos);
            self.move_unit(id, to);
            return true;
        }
        false
    }

    /// `separateUnits()` (§7.5): pairwise push (R 32, strength 0.12, busy weight 0.2) via `map.try_move`, pairs
    /// `i < j` in slab order (addenda U2). STUB: no-op.
    pub fn separate_units(&mut self) {}

    /// Drops dead units from the slab (main.js:297).
    pub fn remove_dead_units(&mut self) {
        self.units.retain(|_, u| !u.dead);
    }

    /// `killUnit(u, by)` (§10.4): leave shelter, dead, release node, FX `boom1` (x, y-22, dy 60) + sound `death`,
    /// kill credit to another faction. MINIMAL: complete except S3 shelter details.
    pub fn kill_unit(&mut self, id: UnitId, by: DamageSource) {
        let Some(u) = self.units.get(id) else { return };
        if u.dead {
            return;
        }
        if u.inside.is_some() {
            self.leave_shelter(id);
        }
        self.claim(id, None);
        let Some(u) = self.units.get_mut(id) else {
            return;
        };
        u.dead = true;
        let (pos, f) = (u.pos, u.faction);
        self.emit(GameEvent::Fx {
            fx: FxKind::Boom1,
            x: pos.x,
            y: pos.y - 22.0,
            scale: 1.0,
            dy: 60.0,
            follow: None,
            color: None,
        });
        self.emit(GameEvent::Sound {
            sound: Sound::Death,
            x: pos.x,
            y: pos.y,
        });
        if let Some(bf) = by.faction
            && bf != f
        {
            self.factions[bf as usize].kills += 1;
        }
    }

    /// `setOrder(u, o)` (§5.1): leave shelter, release node, reset target/skip/workT/bestD/stallT/breachT.
    pub fn set_order(&mut self, id: UnitId, order: Order) {
        if self.units.get(id).is_some_and(|u| u.inside.is_some()) {
            self.leave_shelter(id);
        }
        self.claim(id, None);
        if let Some(u) = self.units.get_mut(id) {
            u.order = order;
            u.target = None;
            u.skip.clear();
            u.work_t = 0.0;
            u.best_d = f32::INFINITY;
            u.stall_t = 0.0;
            u.breach_t = -99.0;
        }
    }

    /// `idle(u)` = `setOrder(u, {k:'idle', x:u.x, y:u.y})`.
    pub fn idle(&mut self, id: UnitId) {
        if let Some(p) = self.units.get(id).map(|u| u.pos) {
            self.set_order(id, Order::idle_at(p));
        }
    }

    /// `moveUnit(u, tx, ty, dt)` (§7.1): step `min(d, speed*dt)` through `map.try_move`, flip if `|dx| > 4`,
    /// `moving = true` even if blocked.
    pub fn move_unit(&mut self, id: UnitId, to: V2) {
        let Some(u) = self.units.get(id) else { return };
        let d = u.pos.dist(to);
        if d < 0.5 {
            return;
        }
        let step = d.min(u.speed * TICK_DT);
        let delta = (to - u.pos) * (step / d);
        let np = self
            .map
            .try_move(u.pos, delta.x, delta.y, side_of(u.faction), u.cross);
        let u = self.units.get_mut(id).expect("checked");
        u.pos = np;
        if (to.x - u.pos.x).abs() > 4.0 {
            u.flip = to.x < u.pos.x;
        }
        u.moving = true;
    }

    /// `navTo(u, field, dt)` (§7.3): 0 → Arrived; `step_from` < 0 → Blocked; else move toward the next cell.
    pub fn nav_to(&mut self, id: UnitId, f: FieldRef) -> NavResult {
        let Some(u) = self.units.get(id) else {
            return NavResult::Blocked;
        };
        let side = side_of(u.faction);
        let c = self.map.cell_of(u.pos);
        if self.fields.dist(f.id, c) == 0 {
            return NavResult::Arrived;
        }
        let n = self.step_from(f.id, c, side, u.cross);
        if n < 0 {
            return NavResult::Blocked;
        }
        let to = self.map.center(n);
        self.move_unit(id, to);
        NavResult::Moving
    }

    /// `fieldOf(u, e)`: field to an entity's ring from the unit's cell; sets `u.cross = crossed`.
    pub fn field_of(&mut self, id: UnitId, target: FieldTarget) -> Option<FieldRef> {
        let u = self.units.get(id)?;
        let (side, c) = (side_of(u.faction), self.map.cell_of(u.pos));
        let f = self.field_for(target, side, c);
        if let Some(u) = self.units.get_mut(id) {
            u.cross = f.crossed;
        }
        Some(f)
    }

    /// `approach(u, tx, ty, dt)` (§7.3): straight line if `d < 280` and `clear_line`, else field to the cell;
    /// sets `path_left`. Returns "can still make progress".
    pub fn approach(&mut self, id: UnitId, to: V2) -> bool {
        let Some(u) = self.units.get(id) else {
            return false;
        };
        let side = side_of(u.faction);
        let d = u.pos.dist(to);
        let from = u.pos;
        {
            let u = self.units.get_mut(id).expect("checked");
            u.path_left = d;
            u.cross = false;
        }
        if d < CLEARLINE_DIST && self.map.clear_line(from, to, side) {
            self.move_unit(id, to);
            return true;
        }
        let b = self.map.cell_of(to);
        if b < 0 {
            return false;
        }
        let a = self.map.cell_of(from);
        let f = self.field_to_cell(b, side, a);
        if let Some(u) = self.units.get_mut(id) {
            u.cross = f.crossed;
        }
        let fa = self.fields.dist(f.id, a);
        if fa == 0 {
            return false;
        }
        let n = self.step_from(f.id, a, side, f.crossed);
        if n < 0 {
            return false;
        }
        if let Some(u) = self.units.get_mut(id) {
            u.path_left = d.max(fa as f32 * TILE / 10.0);
        }
        let to = self.map.center(n);
        self.move_unit(id, to);
        true
    }

    /// `travel(u, x, y, dt, near=10)` (§7.3): true when arrived OR can't get closer (stall 1.2 s near / 4 s).
    pub fn travel(&mut self, id: UnitId, to: V2, near: f32) -> bool {
        let Some(u) = self.units.get(id) else {
            return true;
        };
        let d = u.pos.dist(to);
        if d <= near {
            return true;
        }
        let moving = self.approach(id, to);
        let Some(u) = self.units.get_mut(id) else {
            return true;
        };
        if u.path_left < u.best_d - STALL_PROGRESS {
            u.best_d = u.path_left;
            u.stall_t = 0.0;
        } else {
            u.stall_t += TICK_DT;
            if u.stall_t
                > if d < STALL_NEAR {
                    STALL_SHORT
                } else {
                    STALL_LONG
                }
            {
                return true;
            }
        }
        !moving
    }

    /// `holdPost(u, dt)` (§7.3): if > 48 px from the post walk back (`travel` near 24); if it cannot arrive the
    /// post becomes where it stands. STUB: no-op.
    pub fn hold_post(&mut self, id: UnitId) {}

    /// `updateScout(u)` (§12.1): idle → hold post; other orders → travel then idle; explore → frontier goals,
    /// `bad` cells, toast `Msg::ExploreDone` for humans when nothing is left. STUB: plain move only.
    pub fn update_scout(&mut self, id: UnitId) {
        let Some(u) = self.units.get(id) else { return };
        if let Order::Move(m) = u.order.clone()
            && self.travel(id, m.to, TRAVEL_NEAR)
        {
            self.idle(id);
        }
    }

    /// `frontier(u, bad)` (§12.1): nearest (walking distance) unexplored cell on the 3-cell lattice, not in `bad`.
    /// `None` for AI (no fog) or when nothing is left. STUB.
    pub fn frontier(&mut self, id: UnitId, bad: &[i32]) -> Option<V2> {
        None
    }

    /// `unitAct(u)` (§13 / 03b §4.8).
    pub fn unit_act(&self, id: UnitId) -> Act {
        let Some(u) = self.units.get(id) else {
            return Act::Idle;
        };
        let d = u.def();
        if u.state == UnitState::Attack || u.target.is_some() {
            return if d.heal > 0.0 {
                Act::Heal
            } else {
                Act::Fighting
            };
        }
        match &u.order {
            Order::Gather(g) => g.res.act(),
            Order::Build { .. } => Act::Build,
            Order::Repair { .. } => Act::Repair,
            Order::Drop { .. } => Act::Carry,
            Order::Shelter { .. } => {
                if u.inside.is_some() {
                    Act::Sheltered
                } else {
                    Act::Fleeing
                }
            }
            Order::Explore { .. } => Act::Exploring,
            Order::Idle { .. } => {
                if u.kind == UnitKind::Pawn {
                    Act::Idle
                } else {
                    Act::Guard
                }
            }
            _ => Act::Marching,
        }
    }

    /// `setAnim(u, a)`: change anim, reset `anim_t` only if different.
    pub fn set_anim(&mut self, id: UnitId, a: AnimKind, dir: Dir5) {
        if let Some(u) = self.units.get_mut(id)
            && (u.anim != a || u.anim_dir != dir)
        {
            u.anim = a;
            u.anim_dir = dir;
            u.anim_t = 0.0;
        }
    }
}

/// `jobOf(u)`: gather order (or shelter's `back`) resource.
pub fn job_of(o: &Order) -> Option<Res> {
    match o.job_order() {
        Order::Gather(g) => Some(g.res),
        _ => None,
    }
}
