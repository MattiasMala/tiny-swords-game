// OWNER: S2 (units) — see docs/spec/03a-units-combat.md §4 (unit object), §5.1 (setOrder/idle), §7 (movement &
// navigation: moveUnit, approach, travel, navTo, holdPost, eject rule), §7.5 (separation), §10.4 (killUnit),
// §12.1 (scouts / frontier), §13 (updateUnit, unitAct) + addenda U2, U5, U6; 03b §4.8 (jobOf).
//
// Implemented (complete): spawn_unit, update_unit (§13 order), separate_units (§7.5 + U2), kill_unit, set_order /
// idle (§5.1), move_unit / approach / travel / nav_to / field_of (§7.1-7.3), hold_post, eject rule (§7.4),
// update_scout + frontier (§12.1), unit_act / job_of, remove_dead_units.
// RNG: only `World.rng` (thinkT, fearT staggering); `animT` and running-dust randomness are client-side (U6).
// Deviation (perf): `approach` also walks straight to goals >= 280 px away when `open_corridor` holds (one free,
// level cell around the whole segment), instead of computing a distance field per goal cell — mass marches
// otherwise thrash the 128-entry field cache (measured: 500-unit battle 6.7 s → 0.67 s for 1200 ticks).

use crate::World;
use crate::defs::*;
use crate::events::{GameEvent, Msg};
use crate::math::V2;
use crate::path::{FieldRef, FieldTarget};
use crate::slab::UnitId;
use crate::world::{DamageSource, Order, Unit, UnitState};
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
    /// `fear_t = rng*0.4` [RNG] (in this order, entities.js:255-264).
    pub fn spawn_unit(&mut self, f: FactionIdx, kind: UnitKind, pos: V2) -> UnitId {
        let mut u = Unit::new(f, kind, pos, &self.factions[f as usize].mods);
        u.think_t = self.rng.f32() * 0.4;
        u.fear_t = self.rng.f32() * 0.4;
        self.units.insert(u)
    }

    /// `updateUnit(u, dt)` (03a §13, entities.js:999-1027), in the exact JS order:
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
    /// `i < j` in slab order (addenda U2: units killed this tick still take part; only sheltered ones are
    /// skipped). The 64 px bucket grid (key `floor(x/64)*4096 + floor(y/64)`) is built once, from the positions
    /// after the unit phase; pairs are visited `i` ascending then `j` ascending, positions updated in place.
    pub fn separate_units(&mut self) {
        let ids = self.units.ids();
        let n = ids.len();
        if n < 2 {
            return;
        }
        let key = |p: V2| -> i64 {
            (p.x / SEP_CELL).floor() as i64 * 4096 + (p.y / SEP_CELL).floor() as i64
        };
        // (bucket key, index) sorted → each bucket is a contiguous ascending run of indices
        let mut cells: Vec<(i64, u32)> = Vec::with_capacity(n);
        for (i, id) in ids.iter().enumerate() {
            if let Some(u) = self.units.get(*id)
                && u.inside.is_none()
            {
                cells.push((key(u.pos), i as u32));
            }
        }
        cells.sort_unstable();
        let mut near: Vec<u32> = Vec::new();
        for i in 0..n {
            let Some(a) = self.units.get(ids[i]) else {
                continue;
            };
            if a.inside.is_some() {
                continue;
            }
            let k0 = key(a.pos);
            near.clear();
            for gx in -1..=1i64 {
                for gy in -1..=1i64 {
                    let k = k0 + gx * 4096 + gy;
                    let start = cells.partition_point(|c| c.0 < k);
                    for c in &cells[start..] {
                        if c.0 != k {
                            break;
                        }
                        if c.1 as usize > i {
                            near.push(c.1);
                        }
                    }
                }
            }
            if near.len() > 1 {
                near.sort_unstable();
            }
            for &j in &near {
                let (Some(a), Some(b)) = (self.units.get(ids[i]), self.units.get(ids[j as usize]))
                else {
                    continue;
                };
                let dx = b.pos.x - a.pos.x;
                let dy = b.pos.y - a.pos.y;
                if dx.abs() > SEP_RADIUS || dy.abs() > SEP_RADIUS {
                    continue;
                }
                let d = dx.hypot(dy);
                if d >= SEP_RADIUS {
                    continue;
                }
                let push = (SEP_RADIUS - d) * SEP_STRENGTH;
                let (ux, uy) = if d > 0.01 {
                    (dx / d, dy / d)
                } else {
                    ((i as f32).cos(), (i as f32).sin())
                };
                let busy = |s: UnitState| {
                    if matches!(s, UnitState::Work | UnitState::Attack) {
                        SEP_BUSY_WEIGHT
                    } else {
                        1.0
                    }
                };
                let (wa, wb) = (busy(a.state), busy(b.state));
                let (pa, sa, ca) = (a.pos, side_of(a.faction), a.cross);
                let (pb, sb, cb) = (b.pos, side_of(b.faction), b.cross);
                let na = self
                    .map
                    .try_move(pa, -ux * push * wa, -uy * push * wa, sa, ca);
                if let Some(a) = self.units.get_mut(ids[i]) {
                    a.pos = na;
                }
                let nb = self.map.try_move(pb, ux * push * wb, uy * push * wb, sb, cb);
                if let Some(b) = self.units.get_mut(ids[j as usize]) {
                    b.pos = nb;
                }
            }
        }
    }

    /// Drops dead units from the slab (main.js:297).
    pub fn remove_dead_units(&mut self) {
        self.units.retain(|_, u| !u.dead);
    }

    /// `killUnit(u, by)` (§10.4): leave shelter, dead, release node, FX `boom1` (x, y-22, dy 60) + sound `death`,
    /// kill credit to another faction (no corpse: the unit vanishes in an explosion puff).
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
        // `!(d >= 0.5)` also rejects a NaN goal (would poison the position forever)
        if !(d >= 0.5) || !d.is_finite() {
            return;
        }
        let step = d.min(u.speed * TICK_DT);
        // facing comes from the offset BEFORE the step (JS `dx = tx - u.x`), not from what is left after it
        let dx = to.x - u.pos.x;
        let delta = (to - u.pos) * (step / d);
        let np = self
            .map
            .try_move(u.pos, delta.x, delta.y, side_of(u.faction), u.cross);
        let u = self.units.get_mut(id).expect("checked");
        u.pos = np;
        if dx.abs() > 4.0 {
            u.flip = dx < 0.0;
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
        // Rust addition (perf, see `open_corridor`): far goals across open, level ground are walked straight
        // instead of through a per-goal distance field (the field route there is the same straight run).
        if d >= CLEARLINE_DIST && self.open_corridor(from, to, side) {
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

    /// Straight run from `a` to `b` with one free cell on every side: every cell within one cell (3x3) of every
    /// cell the segment crosses is passable for `side`, on the level of `a`'s cell and not a stair. Then
    /// `try_move` (9 px probe, `cross = false`) can never be stopped by terrain or buildings along the segment,
    /// and the distance field toward `b` would lead the same way. Used by `approach` for goals ≥ 280 px away: in
    /// big battles hundreds of marching units each want a field to their own formation slot and the 128-entry
    /// field cache thrashes (≈80 full-map recomputes per tick with 500 units); this keeps open-ground marches
    /// field-free. Cost: one 3x3 check per crossed cell (samples every 32 px, early exit at the first miss).
    pub fn open_corridor(&self, a: V2, b: V2, side: u8) -> bool {
        let m = &self.map;
        let ca = m.cell_of(a);
        if ca < 0 || m.cell_of(b) < 0 {
            return false;
        }
        let lvl = m.level_at(ca);
        let d = a.dist(b);
        if !d.is_finite() {
            return false;
        }
        let n = (d / 32.0).ceil().max(1.0) as i32;
        let mut last = -1;
        for i in 0..=n {
            let c = m.cell_of(a.lerp(b, i as f32 / n as f32));
            if c == last {
                continue;
            }
            last = c;
            if c < 0 {
                return false;
            }
            let (cx, cy) = (c % m.w, c / m.w);
            for y in cy - 1..=cy + 1 {
                for x in cx - 1..=cx + 1 {
                    if x < 0 || y < 0 || x >= m.w || y >= m.h {
                        return false;
                    }
                    let k = y * m.w + x;
                    if !m.passable(k, side) || m.level_at(k) != lvl || m.is_stair(k) {
                        return false;
                    }
                }
            }
        }
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
    /// post becomes where it stands.
    pub fn hold_post(&mut self, id: UnitId) {
        let Some(u) = self.units.get(id) else { return };
        let Order::Idle { post, .. } = u.order else {
            return;
        };
        if post.dist(u.pos) > HOLD_POST_DIST
            && self.travel(id, post, HOLD_POST_NEAR)
            && let Some(u) = self.units.get_mut(id)
            && let Order::Idle { post, .. } = &mut u.order
        {
            *post = u.pos;
        }
    }

    /// `updateScout(u)` (§12.1): idle → hold post; other orders → travel then idle; explore → frontier goals,
    /// `bad` cells, toast `Msg::ExploreDone` for humans when nothing is left.
    pub fn update_scout(&mut self, id: UnitId) {
        let Some(u) = self.units.get(id) else { return };
        let f = u.faction;
        let goal = match &u.order {
            Order::Idle { .. } => {
                self.hold_post(id);
                return;
            }
            Order::Move(m) | Order::AMove(m) => {
                let to = m.to;
                if self.travel(id, to, TRAVEL_NEAR) {
                    self.idle(id);
                }
                return;
            }
            Order::Explore { goal, .. } => *goal,
            _ => {
                // scouts never fight / gather: anything else just stops
                self.idle(id);
                return;
            }
        };
        let mut next = goal.is_none_or(|g| self.explored(f, g));
        if !next
            && let Some(g) = goal
            && self.travel(id, g, EXPLORE_NEAR)
        {
            // reached, or cannot get there: try another one
            next = true;
            let c = self.map.cell_of(g);
            if let Some(u) = self.units.get_mut(id)
                && let Order::Explore { bad, .. } = &mut u.order
                && !bad.contains(&c)
            {
                bad.push(c);
            }
        }
        if !next {
            return;
        }
        let bad = match self.units.get(id).map(|u| &u.order) {
            Some(Order::Explore { bad, .. }) => bad.clone(),
            _ => Vec::new(),
        };
        let g = self.frontier(id, &bad);
        if let Some(u) = self.units.get_mut(id) {
            if let Order::Explore { goal, .. } = &mut u.order {
                *goal = g;
            }
            u.best_d = f32::INFINITY;
            u.stall_t = 0.0;
        }
        if g.is_none() {
            self.idle(id);
            if self.factions[f as usize].human {
                self.emit(GameEvent::Toast {
                    to: f,
                    msg: Msg::ExploreDone,
                });
            }
        }
    }

    /// `frontier(u, bad)` (§12.1): nearest (walking distance) unexplored cell on the 3-cell lattice
    /// (`x, y = 2, 5, 8, … < W-2 / H-2`), not in `bad`. `None` for AI-driven kingdoms (no fog) or when nothing is
    /// left.
    pub fn frontier(&mut self, id: UnitId, bad: &[i32]) -> Option<V2> {
        let u = self.units.get(id)?;
        let f = u.faction;
        if !self.factions[f as usize].human {
            return None;
        }
        let side = side_of(f);
        let here = self.map.cell_of(u.pos);
        if here < 0 {
            return None;
        }
        let fr = self.field_to_cell(here, side, here);
        let field = self.fields.field(fr.id);
        let seen = &self.factions[f as usize].fog.explored;
        let (w, h) = (self.map.w, self.map.h);
        let mut best = -1;
        let mut bd = FIELD_INF;
        let mut y = 2;
        while y < h - 2 {
            let mut c = y * w + 2;
            let end = y * w + w - 2;
            while c < end {
                let fc = field.get(c as usize).copied().unwrap_or(FIELD_INF);
                if fc < bd && !seen.get(c as usize) && !bad.contains(&c) {
                    bd = fc;
                    best = c;
                }
                c += 3;
            }
            y += 3;
        }
        (best >= 0).then(|| self.map.center(best))
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
