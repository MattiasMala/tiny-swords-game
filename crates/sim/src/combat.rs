// OWNER: S2 (units) — see docs/spec/03a-units-combat.md §8 (soldier AI: state machine, acquireTarget, fight,
// startAttack/startBlock/tickAction, doHit, breach, raidOn), §9 (monks), §10.1-10.3 (hurt, alarms), §2.1 attack
// timing table (defs::UnitDef.attack), addenda U1-U3; 01 §2.3 damage pipeline; 04b §10 entity-side AI hooks.
//
// Implemented: update_soldier, update_monk, acquire_target, fight, start_attack, start_block, tick_action, do_hit,
// breach, raid_on, alarm_for, answer_alarm, hurt (full §10.1 pipeline), damage_to.
// Neighbour scans use `World.grid` (rebuilt at the start of the unit phase; positions drift ≤ ~10 px during the
// phase, so every query is widened by `SCAN_SLACK` and re-checked with the exact distance). Candidates are visited in
// slab-index order, which reproduces the JS "first minimum in `G.units` order" tie-break.

use crate::World;
use crate::defs::*;
use crate::events::GameEvent;
use crate::math::{V2, clamp, v2};
use crate::path::FieldTarget;
use crate::slab::{BuildingId, UnitId};
use crate::unit::NavResult;
use crate::world::{Alarm, DamageSource, MoveOrder, Order, Shooter, Target, UnitState};
use crate::TICK_DT;

/// Extra radius added to grid queries: units move a little after the grid is rebuilt.
pub const SCAN_SLACK: f32 = 24.0;

/// Kind of the damage dealer for `damageTo` (buildings have no bonus/vsBuilding → ×1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attacker {
    Unit(UnitKind),
    Building(BuildingKind),
}

impl From<Shooter> for Attacker {
    fn from(s: Shooter) -> Attacker {
        match s {
            Shooter::Unit { kind, .. } => Attacker::Unit(kind),
            Shooter::Building { kind, .. } => Attacker::Building(kind),
        }
    }
}

/// `damageTo(src, base, t)` = `base * (t is building ? vsBuilding||1 : bonus[t.type]||1)`.
pub fn damage_to(src: Attacker, base: f32, target_kind: Option<UnitKind>) -> f32 {
    match (src, target_kind) {
        (Attacker::Unit(k), None) => base * k.def().vs_building,
        (Attacker::Unit(k), Some(t)) => base * k.def().bonus_vs(t),
        (Attacker::Building(_), _) => base,
    }
}

/// Pure unit-damage arithmetic of `hurt` before the block roll (01 §2.3, addenda A2):
/// arrow multiplier (ram ×0.2; blockers/knights × `arrow_res`) → `max(1, amount - armor)`.
pub fn unit_damage(amount: f32, target: UnitKind, target_mods: &Mods, arrow: bool) -> f32 {
    let mut a = amount;
    if arrow {
        if target == UnitKind::Ram {
            a *= ARROW_VS_RAM;
        } else if target.def().block > 0.0 || target == UnitKind::Knight {
            a *= target_mods.arrow_res;
        }
    }
    (a - target_mods.unit_armor(target)).max(1.0)
}

/// Kind of order for the soldier state machine (avoids cloning the order every tick).
#[derive(Clone, Copy, Debug, PartialEq)]
enum SoldierOrder {
    Idle,
    Move,
    AMove(MoveOrder),
    Attack(Target),
    Other,
}

impl World {
    /// `hurt(t, amount, by)` (§10.1). Threat/threatT; human alarm (building, pawn, or within 1500 of castle);
    /// buildings ×0.4; units: arrow → ram ×0.2 / blocker|knight × arrowRes; `max(1, amount - armor)`; block roll
    /// [RNG] ×0.35 + `start_block` + sound; `hurt_t = 0.12`; instant retaliation; alert event every ≥12 s for
    /// humans; death → `kill_unit` / `destroy_building`.
    pub fn hurt(&mut self, t: Target, amount: f32, by: DamageSource) {
        let from = by.unit.and_then(|u| self.units.get(u)).map(|u| u.pos);
        self.hurt_from(t, amount, by, from);
    }

    /// [`hurt`](Self::hurt) with the position of the damage source (`src.x, src.y` in JS, used to face a block):
    /// projectiles pass their shooter's position (also for tower/castle arrows, whose source is a building).
    pub fn hurt_from(&mut self, t: Target, amount: f32, by: DamageSource, from: Option<V2>) {
        let (tf, tpos, tkind) = match t {
            Target::Unit(u) => match self.units.get(u) {
                Some(un) if !un.dead => (un.faction, un.pos, Some(un.kind)),
                _ => return,
            },
            Target::Building(b) => match self.buildings.get(b) {
                Some(bl) if !bl.dead => (bl.faction, bl.pos, None),
                _ => return,
            },
        };
        let time = self.time;
        // src = attacking unit (or the shooter of the projectile), alive
        let src = by
            .unit
            .and_then(|s| self.units.get(s).filter(|su| !su.dead).map(|su| (s, su.pos)));
        if let Some((sid, spos)) = src {
            let castle = self.factions[tf as usize]
                .castle
                .and_then(|c| self.buildings.get(c))
                .map(|c| c.pos);
            let fac = &mut self.factions[tf as usize];
            fac.threat = Some(sid);
            fac.threat_t = time;
            if fac.human
                && (tkind.is_none()
                    || tkind == Some(UnitKind::Pawn)
                    || castle.is_some_and(|c| c.dist(tpos) < HOME_RADIUS))
            {
                fac.alarm = Some(Alarm { pos: spos, t: time });
            }
        }
        let mut amount = amount;
        match (t, tkind) {
            (Target::Unit(uid), Some(kind)) => {
                let mods = self.factions[tf as usize].mods;
                amount = unit_damage(amount, kind, &mods, by.arrow);
                let d = kind.def();
                let state = self.units.get(uid).map(|u| u.state);
                if d.block > 0.0
                    && state != Some(UnitState::Attack)
                    && self.rng.f32() < d.block
                {
                    amount *= BLOCK_MULT;
                    self.start_block(uid, from);
                    self.emit(GameEvent::Sound {
                        sound: Sound::Block,
                        x: tpos.x,
                        y: tpos.y,
                    });
                }
                if let Some(u) = self.units.get_mut(uid) {
                    u.hurt_t = HURT_FLASH;
                    u.last_hurt = time;
                    let retaliate = matches!(u.order, Order::Idle { .. } | Order::AMove(_));
                    if u.target.is_none()
                        && d.is_soldier()
                        && retaliate
                        && let Some((sid, _)) = src
                    {
                        u.target = Some(Target::Unit(sid));
                    }
                }
            }
            _ => amount *= BUILDING_ARMOR,
        }
        let dead = match t {
            Target::Unit(u) => self.units.get_mut(u).map(|u| {
                u.hp -= amount;
                u.hp <= 0.0
            }),
            Target::Building(b) => self.buildings.get_mut(b).map(|b| {
                b.hp -= amount;
                b.hp <= 0.0
            }),
        }
        .unwrap_or(false);
        let fac = &mut self.factions[tf as usize];
        if fac.human && time - fac.alert_t > ALERT_EVERY {
            fac.alert_t = time;
            self.emit(GameEvent::Alert {
                to: tf,
                x: tpos.x.round(),
                y: tpos.y.round(),
            });
        }
        if dead {
            match t {
                Target::Unit(u) => self.kill_unit(u, by),
                Target::Building(b) => self.destroy_building(b, by),
            }
        }
    }

    fn soldier_order(&self, id: UnitId) -> SoldierOrder {
        match self.units.get(id).map(|u| &u.order) {
            Some(Order::Idle { .. }) => SoldierOrder::Idle,
            Some(Order::Move(_)) => SoldierOrder::Move,
            Some(Order::AMove(m)) => SoldierOrder::AMove(*m),
            Some(Order::Attack { target }) => SoldierOrder::Attack(*target),
            _ => SoldierOrder::Other,
        }
    }

    /// Guard post of an idle unit (JS `o.x, o.y`), `None` for other orders.
    pub fn post_of(&self, id: UnitId) -> Option<V2> {
        match self.units.get(id).map(|u| &u.order) {
            Some(Order::Idle { post, .. }) => Some(*post),
            _ => None,
        }
    }

    /// Moves the guard post of an idle unit (no-op for other orders).
    pub fn set_post(&mut self, id: UnitId, p: V2) {
        if let Some(u) = self.units.get_mut(id)
            && let Order::Idle { post, .. } = &mut u.order
        {
            *post = p;
        }
    }

    /// `updateSoldier(u)` (§8.1): attack/move/idle/amove state machine with breach lock, alarm, raid chain.
    pub fn update_soldier(&mut self, id: UnitId) {
        let dt = TICK_DT;
        let time = self.time;
        let Some(u) = self.units.get(id) else { return };
        let cur_target = u.target;
        let breaching = time < u.breach_t && cur_target.is_some_and(|t| self.target_alive(t));
        let alarm = self.alarm_for(id);
        let order = self.soldier_order(id);
        match order {
            SoldierOrder::Other => {
                // soldiers never get pawn/scout/monk orders; anything else just stops
                self.idle(id);
                return;
            }
            SoldierOrder::Attack(ot) => {
                if !self.target_alive(ot) {
                    self.raid_on(id);
                    return;
                }
                if !breaching && let Some(u) = self.units.get_mut(id) {
                    u.target = Some(ot);
                }
            }
            SoldierOrder::Move => {
                if let Some(u) = self.units.get_mut(id) {
                    u.target = None;
                }
            }
            SoldierOrder::Idle | SoldierOrder::AMove(_) => {
                if !breaching {
                    let think = {
                        let u = self.units.get_mut(id).expect("checked");
                        u.think_t -= dt;
                        u.think_t <= 0.0
                    };
                    if think {
                        let next = self.rng.range(SOLDIER_THINK.0, SOLDIER_THINK.1);
                        let t = self.acquire_target(id, alarm.is_some());
                        if let Some(u) = self.units.get_mut(id) {
                            u.think_t = next;
                            if t != u.target {
                                u.target = t;
                                u.best_d = f32::INFINITY;
                                u.stall_t = 0.0;
                            }
                        }
                    }
                }
            }
        }
        let t = self.units.get(id).and_then(|u| u.target);
        if let Some(t) = t
            && self.target_alive(t)
        {
            if self.fight(id, t) {
                if let Some(u) = self.units.get_mut(id)
                    && let Order::AMove(m) | Order::Move(m) = &mut u.order
                {
                    m.fought = true;
                }
                return;
            }
            // target unreachable: an assault opens a gap, a guard gives up
            let gate = if order == SoldierOrder::Idle {
                None
            } else {
                let tp = self.target_pos(t).unwrap_or_default();
                self.breach(id, tp)
            };
            if let Some(u) = self.units.get_mut(id) {
                u.target = gate.map(Target::Building);
                if gate.is_some() {
                    u.breach_t = time + BREACH_LOCK;
                }
            }
            if gate.is_none() && matches!(order, SoldierOrder::Attack(_)) {
                self.idle(id);
            }
            return;
        }
        if let Some(u) = self.units.get_mut(id) {
            u.target = None;
        }
        if let Some(a) = alarm {
            self.answer_alarm(id, a);
        } else if order == SoldierOrder::Idle {
            self.hold_post(id);
        } else if self.march(id, TRAVEL_NEAR) {
            // move / amove reached its slot (or stalled); re-read the order (march updates `own`)
            let (amove, slot, fought, auto, pos) = match self.units.get(id) {
                Some(u) => match &u.order {
                    Order::AMove(m) => (true, m.to, m.fought, m.auto, u.pos),
                    Order::Move(m) => (false, m.to, m.fought, m.auto, u.pos),
                    _ => return,
                },
                None => return,
            };
            let gate = if amove && slot.dist(pos) > AMOVE_BREACH_DIST {
                self.breach(id, slot)
            } else {
                None
            };
            if let Some(g) = gate {
                if let Some(u) = self.units.get_mut(id) {
                    u.target = Some(Target::Building(g));
                    u.breach_t = time + BREACH_LOCK;
                    u.best_d = f32::INFINITY;
                    u.stall_t = 0.0;
                }
            } else if amove && (fought || !auto) {
                self.raid_on(id);
            } else {
                self.idle(id);
            }
        }
    }

    /// `updateMonk(u)` (§9.1): heal scan (thinkT [RNG] 0.4-0.7), keep 150 px from enemy soldiers, escort.
    pub fn update_monk(&mut self, id: UnitId) {
        let dt = TICK_DT;
        let Some(u) = self.units.get(id) else { return };
        let f = u.faction;
        match u.order.clone() {
            Order::Move(m) => {
                if self.march(id, TRAVEL_NEAR) {
                    let p = self.units.get(id).map(|u| u.pos).unwrap_or(m.to);
                    self.set_order(
                        id,
                        Order::Idle {
                            post: p,
                            escort: m.escort,
                        },
                    );
                }
                return;
            }
            Order::Heal { target } => {
                let done = self
                    .units
                    .get(target)
                    .is_none_or(|t| t.dead || t.hp >= t.max_hp);
                if done {
                    self.idle(id);
                    return;
                }
            }
            _ => {}
        }
        let think = {
            let u = self.units.get_mut(id).expect("checked");
            u.think_t -= dt;
            u.think_t <= 0.0
        };
        if think {
            let next = self.rng.range(MONK_THINK.0, MONK_THINK.1);
            let me = self.units.get(id).expect("checked").pos;
            let mut heal = None;
            let mut hr = MONK_HEAL_BELOW;
            let mut foe = None;
            let mut fd = MONK_FOE_SCAN;
            let mut lead = None;
            let mut ld = MONK_LEAD_SCAN;
            let mut near = Vec::new();
            self.grid
                .query_sorted(me, MONK_LEAD_SCAN + SCAN_SLACK, &mut near);
            for &eid in &near {
                if eid == id {
                    continue;
                }
                let Some(e) = self.units.get(eid) else {
                    continue;
                };
                if !e.active() {
                    continue;
                }
                let d = e.pos.dist(me);
                let soldier = e.def().is_soldier();
                if e.faction != f {
                    if soldier && d < fd {
                        fd = d;
                        foe = Some(eid);
                    }
                    continue;
                }
                let ratio = e.hp / e.max_hp;
                if !e.def().siege && d < MONK_HEAL_SCAN && ratio < hr {
                    hr = ratio;
                    heal = Some(eid);
                }
                if soldier && d < ld {
                    ld = d;
                    lead = Some(eid);
                }
            }
            let u = self.units.get_mut(id).expect("checked");
            u.think_t = next;
            u.foe = foe;
            u.lead = lead;
            u.target = heal.map(Target::Unit);
        }
        let (order_heal, escort, is_idle) = match &self.units.get(id).expect("checked").order {
            Order::Heal { target } => (Some(*target), false, false),
            Order::Idle { escort, .. } => (None, *escort, true),
            _ => (None, false, false),
        };
        if let Some(ht) = order_heal
            && let Some(u) = self.units.get_mut(id)
        {
            u.target = Some(Target::Unit(ht));
        }
        let u = self.units.get(id).expect("checked");
        let (me, cd, faith, foe_id, lead_id) = (u.pos, u.cd, u.faith, u.foe, u.lead);
        let t = match u.target {
            Some(Target::Unit(t)) => self
                .units
                .get(t)
                .filter(|e| !e.dead && e.hp < e.max_hp)
                .map(|e| (t, e.pos)),
            _ => None,
        };
        let foe = foe_id.and_then(|fid| self.units.get(fid).filter(|e| !e.dead).map(|e| e.pos));
        let range = self.factions[f as usize].mods.unit_range(UnitKind::Monk);
        let reach = t.is_some_and(|(_, tp)| tp.dist(me) <= range);
        if reach && cd <= 0.0 && faith >= 1.0 {
            let tp = t.expect("reach implies target").1;
            self.start_attack(id, tp);
            return;
        }
        if let Some(fp) = foe
            && fp.dist(me) < MONK_SPACE
        {
            self.move_unit(id, me * 2.0 - fp);
            if is_idle {
                let p = self.units.get(id).expect("checked").pos;
                self.set_post(id, p);
            }
            return;
        }
        if let Some((_, tp)) = t {
            if !reach {
                self.approach(id, tp);
            }
            return; // with an empty pool it waits in place
        }
        if let Some(u) = self.units.get_mut(id) {
            u.target = None;
        }
        if !is_idle {
            self.idle(id);
            return;
        }
        let lead = if escort {
            lead_id.and_then(|l| self.units.get(l).filter(|e| !e.dead).map(|e| e.pos))
        } else {
            None
        };
        match lead {
            None => self.hold_post(id),
            Some(lp) if lp.dist(me) > ESCORT_GAP => {
                self.approach(id, lp);
                let p = self.units.get(id).expect("checked").pos;
                self.set_post(id, p);
            }
            Some(_) => {}
        }
    }

    /// `acquireTarget(u, roam)` (§8.2): weighted nearest enemy unit within aggro (pawn ×2.2, monk ×0.6), guard
    /// leash 430, AI garrison threat clause, else enemy non-wall building.
    pub fn acquire_target(&mut self, id: UnitId, roam: bool) -> Option<Target> {
        let u = self.units.get(id)?;
        let (f, me, kind) = (u.faction, u.pos, u.kind);
        let d = kind.def();
        let post = match &u.order {
            Order::Idle { post, .. } => Some(*post),
            _ => None,
        };
        let guard = post.is_some() && !roam;
        let post = post.unwrap_or(me);
        if !d.only_buildings {
            let mut best = None;
            let mut bd = d.aggro;
            let mut near = Vec::new();
            self.grid.query_sorted(me, d.aggro + SCAN_SLACK, &mut near);
            for &eid in &near {
                let Some(e) = self.units.get(eid) else {
                    continue;
                };
                if !e.active() || e.faction == f {
                    continue;
                }
                let far = e.pos.dist(me);
                if far >= d.aggro {
                    continue;
                }
                let w = if e.kind == UnitKind::Pawn {
                    PAWN_TARGET_WEIGHT
                } else if e.def().is_healer() {
                    HEALER_FOCUS
                } else {
                    1.0
                };
                let dd = far * w;
                if dd < bd && (!guard || e.pos.dist(post) < GUARD_LEASH) {
                    bd = dd;
                    best = Some(eid);
                }
            }
            if let Some(b) = best {
                return Some(Target::Unit(b));
            }
            // AI only: the garrison goes after whoever hits the kingdom from out of range
            let fac = &self.factions[f as usize];
            if !fac.human
                && guard
                && self.time - fac.threat_t < AI_THREAT_MEMORY
                && let Some(th) = fac.threat
                && let Some(the) = self.units.get(th).filter(|e| !e.dead)
                && let Some(c) = fac.castle.and_then(|c| self.buildings.get(c))
                && the.pos.dist(c.pos) < AI_THREAT_RADIUS
                && me.dist(c.pos) < AI_THREAT_RADIUS
            {
                return Some(Target::Unit(th));
            }
        }
        let mut bd = if d.only_buildings {
            RAM_BUILDING_AGGRO
        } else {
            BUILDING_AGGRO_MIN.max(self.factions[f as usize].mods.unit_range(kind))
        };
        let mut best = None;
        for (bid, b) in self.buildings.iter() {
            if b.dead || b.faction == f || b.kind == BuildingKind::Wall {
                continue;
            }
            let dd = b.dist(me);
            if dd < bd && (!guard || b.dist(post) < GUARD_LEASH) {
                bd = dd;
                best = Some(bid);
            }
        }
        best.map(Target::Building)
    }

    /// `fight(u, t, dt)` (§8.3): in range → kite (archers) / face / `start_attack` when `cd <= 0`; else approach.
    /// Returns false if unreachable.
    pub fn fight(&mut self, id: UnitId, t: Target) -> bool {
        let Some(u) = self.units.get(id) else {
            return false;
        };
        let (me, kind, cd, f) = (u.pos, u.kind, u.cd, u.faction);
        let def = kind.def();
        let range = self.factions[f as usize].mods.unit_range(kind);
        let d = self.dist_to(me, t);
        let mut in_range = d
            <= range
                + match t {
                    Target::Building(_) => 0.0,
                    Target::Unit(_) => RANGE_SLACK,
                };
        if !in_range
            && let Target::Building(b) = t
            && def.range < 200.0
        {
            // melee standing in the ring around the building
            if let Some(fr) = self.field_of(id, FieldTarget::Building(b)) {
                in_range = self.fields.dist(fr.id, self.map.cell_of(me)) == 0;
            }
        }
        if in_range {
            let aim = match t {
                Target::Unit(tu) => {
                    let Some(te) = self.units.get(tu) else {
                        return false;
                    };
                    if d < KITE_RANGE
                        && cd > KITE_MIN_CD
                        && def.is_ranged()
                        && !def.siege
                        && !te.def().is_ranged()
                    {
                        // archer kiting: step straight away while reloading
                        let away = me * 2.0 - te.pos;
                        self.move_unit(id, away);
                        return true;
                    }
                    te.pos
                }
                Target::Building(b) => {
                    let Some(bl) = self.buildings.get(b) else {
                        return false;
                    };
                    let r = bl.rect;
                    v2(clamp(me.x, r.x0, r.x1), (r.y0 + r.y1) * 0.5)
                }
            };
            if let Some(u) = self.units.get_mut(id)
                && (aim.x - u.pos.x).abs() > 4.0
            {
                u.flip = aim.x < u.pos.x;
            }
            if cd <= 0.0 {
                self.start_attack(id, aim);
            }
            return true;
        }
        match t {
            Target::Building(b) => match self.field_of(id, FieldTarget::Building(b)) {
                Some(fr) => self.nav_to(id, fr) != NavResult::Blocked,
                None => false,
            },
            Target::Unit(tu) => match self.units.get(tu).map(|e| e.pos) {
                Some(tp) => self.approach(id, tp),
                None => false,
            },
        }
    }

    /// `startAttack(u, tx, ty)` (§8.4): state Attack, cd = def.cd, anim (warrior combo, lancer dir5), anim_t 0.
    pub fn start_attack(&mut self, id: UnitId, aim: V2) {
        let Some(u) = self.units.get_mut(id) else {
            return;
        };
        u.state = UnitState::Attack;
        u.hit_done = false;
        u.aim = aim;
        u.cd = u.def().cd;
        if (aim.x - u.pos.x).abs() > 2.0 {
            u.flip = aim.x < u.pos.x;
        }
        u.anim = AnimKind::Attack;
        match u.kind {
            UnitKind::Warrior => {
                // alternates attack2, attack1, ...
                u.combo ^= 1;
                u.anim = if u.combo == 1 {
                    AnimKind::Attack2
                } else {
                    AnimKind::Attack
                };
            }
            UnitKind::Lancer => u.anim_dir = dir5(aim.x - u.pos.x, aim.y - u.pos.y),
            _ => {}
        }
        u.anim_t = 0.0;
    }

    /// `startBlock(u, from)` (§8.4): state Block for 0.5 s, face the attacker.
    pub fn start_block(&mut self, id: UnitId, from: Option<V2>) {
        let Some(u) = self.units.get_mut(id) else {
            return;
        };
        u.state = UnitState::Block;
        u.block_t = BLOCK_TIME;
        if let Some(p) = from {
            u.flip = p.x < u.pos.x;
        }
        let dir = if u.kind == UnitKind::Lancer {
            from.map(|p| dir5(p.x - u.pos.x, p.y - u.pos.y))
                .unwrap_or(Dir5::Right)
        } else {
            u.anim_dir
        };
        self.set_anim(id, AnimKind::Block, dir);
    }

    /// `tickAction(u, dt)` (§8.4): attack → hit at `hit_frame` (once, `do_hit`), end at `frames`; block timer.
    /// Returns busy.
    pub fn tick_action(&mut self, id: UnitId) -> bool {
        let Some(u) = self.units.get_mut(id) else {
            return false;
        };
        match u.state {
            UnitState::Attack => {
                let Some(a) = u.def().attack else {
                    u.state = UnitState::Idle;
                    return false;
                };
                let fr = (u.anim_t * a.fps).floor() as i32;
                if !u.hit_done && fr >= a.hit_frame as i32 {
                    u.hit_done = true;
                    self.do_hit(id);
                }
                if fr >= a.frames as i32 {
                    if let Some(u) = self.units.get_mut(id) {
                        u.state = UnitState::Idle;
                    }
                    return false;
                }
                true
            }
            UnitState::Block => {
                u.block_t -= TICK_DT;
                if u.block_t <= 0.0 {
                    u.state = UnitState::Idle;
                    return false;
                }
                true
            }
            _ => false,
        }
    }

    /// `doHit(u)` (§8.5): archer arrow / catapult rock / monk heal / melee hit with reach check.
    pub fn do_hit(&mut self, id: UnitId) {
        let Some(u) = self.units.get(id) else { return };
        let (me, kind, f, flip, aim) = (u.pos, u.kind, u.faction, u.flip, u.aim);
        let target = u.target.filter(|t| self.target_alive(*t));
        let mods = self.factions[f as usize].mods;
        let dmg = mods.unit_dmg(kind);
        let shooter = Shooter::Unit { id, kind };
        match kind {
            UnitKind::Archer => {
                let to = match target {
                    Some(Target::Building(b)) => {
                        let r = self.buildings.get(b).expect("alive").rect;
                        v2((r.x0 + r.x1) * 0.5, (r.y0 + r.y1) * 0.5 - 20.0)
                    }
                    Some(Target::Unit(t)) => {
                        let p = self.units.get(t).expect("alive").pos;
                        v2(p.x, p.y - ARROW_HIT_YOFF)
                    }
                    None => aim,
                };
                let from = v2(
                    me.x + if flip {
                        -ARCHER_MUZZLE.x
                    } else {
                        ARCHER_MUZZLE.x
                    },
                    me.y - ARCHER_MUZZLE.y,
                );
                self.fire_arrow(f, shooter, from, to, dmg);
            }
            UnitKind::Catapult => {
                let to = match target {
                    Some(Target::Unit(t)) => self.units.get(t).expect("alive").pos,
                    _ => aim,
                };
                let from = v2(
                    me.x + if flip { -ROCK_MUZZLE.x } else { ROCK_MUZZLE.x },
                    me.y - ROCK_MUZZLE.y,
                );
                self.fire_rock(f, shooter, from, to, dmg);
            }
            UnitKind::Monk => {
                let Some(Target::Unit(t)) = target else {
                    return;
                };
                let (heal, pool) = (kind.def().heal * mods.heal, u.faith);
                let color = self.factions[f as usize].color;
                let Some(tu) = self.units.get_mut(t) else {
                    return;
                };
                let amount = heal.min(tu.max_hp - tu.hp).min(pool);
                tu.hp += amount;
                let tp = tu.pos;
                if let Some(u) = self.units.get_mut(id) {
                    u.faith -= amount;
                }
                self.emit(GameEvent::Fx {
                    fx: FxKind::Heal,
                    x: tp.x,
                    y: tp.y,
                    scale: 1.0,
                    dy: 4.0,
                    follow: Some(t),
                    color: Some(color),
                });
                self.emit(GameEvent::Sound {
                    sound: Sound::Heal,
                    x: tp.x,
                    y: tp.y,
                });
            }
            _ => {
                // melee (ram included): the swing whiffs if the target moved away
                let Some(t) = target else { return };
                let reach = mods.unit_range(kind) * MELEE_CONFIRM
                    + match t {
                        Target::Building(_) => MELEE_BUILDING_EXTRA,
                        Target::Unit(_) => 0.0,
                    };
                if self.dist_to(me, t) <= reach {
                    let tk = match t {
                        Target::Unit(tu) => self.units.get(tu).map(|e| e.kind),
                        Target::Building(_) => None,
                    };
                    let amount = damage_to(Attacker::Unit(kind), dmg, tk);
                    self.hurt(
                        t,
                        amount,
                        DamageSource {
                            unit: Some(id),
                            faction: Some(f),
                            arrow: false,
                        },
                    );
                    self.emit(GameEvent::Sound {
                        sound: if kind == UnitKind::Ram {
                            Sound::Boom
                        } else {
                            Sound::Hit
                        },
                        x: me.x,
                        y: me.y,
                    });
                }
            }
        }
    }

    /// `breach(u, gx, gy)` (§8.6): best of the 4 nearest enemy buildings (walls included) within 900 px whose
    /// field reaches the unit.
    pub fn breach(&mut self, id: UnitId, goal: V2) -> Option<BuildingId> {
        let u = self.units.get(id)?;
        let (me, f) = (u.pos, u.faction);
        let here = self.map.cell_of(me);
        let mut cands: Vec<(f32, BuildingId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| !b.dead && b.faction != f && b.dist(me) < BREACH_RADIUS)
            .map(|(bid, b)| (b.dist(me) + b.dist(goal), bid))
            .collect();
        // stable sort, like Array.prototype.sort
        cands.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, b) in cands.iter().take(BREACH_CANDIDATES) {
            if let Some(fr) = self.field_of(id, FieldTarget::Building(b))
                && self.fields.dist(fr.id, here) < FIELD_INF
            {
                return Some(b);
            }
        }
        None
    }

    /// `raidOn(u)` (§8.7): humans auto-amove to the nearest enemy within 900 px; AI → idle.
    pub fn raid_on(&mut self, id: UnitId) {
        let Some(u) = self.units.get(id) else { return };
        let (me, f, only_b) = (u.pos, u.faction, u.def().only_buildings);
        let mut goal: Option<V2> = None;
        if self.factions[f as usize].human {
            let mut bd = RAID_RADIUS;
            if !only_b {
                let mut near = Vec::new();
                self.grid
                    .query_sorted(me, RAID_RADIUS + SCAN_SLACK, &mut near);
                for &eid in &near {
                    let Some(e) = self.units.get(eid) else {
                        continue;
                    };
                    if !e.active() || e.faction == f {
                        continue;
                    }
                    let d = e.pos.dist(me);
                    if d < bd {
                        bd = d;
                        goal = Some(e.pos);
                    }
                }
            }
            for (_, b) in self.buildings.iter() {
                if b.dead || b.faction == f || b.kind == BuildingKind::Wall {
                    continue;
                }
                let d = b.dist(me);
                if d < bd {
                    bd = d;
                    goal = Some(v2(b.pos.x, b.rect.y1 + DEPLOY_DY));
                }
            }
        }
        match goal {
            Some(p) => {
                let mut m = MoveOrder::to(p);
                m.auto = true;
                self.set_order(id, Order::AMove(m));
            }
            None => self.idle(id),
        }
    }

    /// `alarmFor(u)` (§10.2): the faction alarm if human, < 10 s old, unit idle soldier.
    pub fn alarm_for(&self, id: UnitId) -> Option<Alarm> {
        let u = self.units.get(id)?;
        let fac = &self.factions[u.faction as usize];
        let a = fac.alarm?;
        (fac.human
            && self.time - a.t < ALARM_TIME
            && u.order.is_idle()
            && u.def().is_soldier())
        .then_some(a)
    }

    /// `answerAlarm(u, a, dt)` (§10.2, addenda U1): a unit idling far from home moves its post next to the castle
    /// [RNG], then runs to the alarm spot (stops 120 px short).
    pub fn answer_alarm(&mut self, id: UnitId, a: Alarm) {
        let Some(u) = self.units.get(id) else { return };
        let (me, f) = (u.pos, u.faction);
        let castle = self.factions[f as usize]
            .castle
            .and_then(|c| self.buildings.get(c))
            .map(|c| (c.pos, c.rect.y1));
        if let (Some((cp, cy1)), Some(post)) = (castle, self.post_of(id))
            && post.dist(cp) > HOME_RADIUS
        {
            let rx = self.rng.range(-140.0, 140.0);
            let ry = self.rng.range(70.0, 150.0);
            let p = self.map.nearest_passable(v2(cp.x + rx, cy1 + ry));
            self.set_post(id, p);
        }
        if a.pos.dist(me) > 120.0 {
            self.approach(id, a.pos);
        }
    }
}

#[cfg(test)]
#[path = "combat_tests.rs"]
pub(crate) mod tests;
