// OWNER: S2 (units) — see docs/spec/03a-units-combat.md §8 (soldier AI: state machine, acquireTarget, fight,
// startAttack/startBlock/tickAction, doHit, breach, raidOn), §9 (monks), §10.1-10.3 (hurt, alarms), §2.1 attack
// timing table (defs::UnitDef.attack), addenda U1-U3; 01 §2.3 damage pipeline; 04b §10 entity-side AI hooks.
//
// Stub fns S2 must fill: update_soldier, update_monk, acquire_target, fight, start_attack, start_block,
// tick_action, do_hit, breach, raid_on, alarm_for, answer_alarm; `hurt` is MINIMAL (pipeline of §10.1 without
// threat/alarm/retaliation/block/alert bookkeeping — complete it). `damage_to` is done.
#![allow(dead_code, unused)]

use crate::World;
use crate::defs::*;
use crate::events::GameEvent;
use crate::math::V2;
use crate::slab::{BuildingId, UnitId};
use crate::world::{Alarm, DamageSource, Shooter, Target, UnitState};

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

impl World {
    /// `hurt(t, amount, by)` (§10.1). Full rules: threat/threatT; human alarm (building, pawn, or within 1500 of
    /// castle); buildings ×0.4; units: arrow → ram ×0.2 / blocker|knight × arrowRes; `max(1, amount - armor)`;
    /// block roll [RNG] ×0.35 + `start_block` + sound; `hurt_t = 0.12`; instant retaliation; alert event every
    /// ≥12 s for humans; death → `kill_unit` / `destroy_building`.
    /// MINIMAL: damage arithmetic + death only.
    pub fn hurt(&mut self, t: Target, amount: f32, by: DamageSource) {
        match t {
            Target::Building(b) => {
                let Some(bl) = self.buildings.get_mut(b) else {
                    return;
                };
                if bl.dead {
                    return;
                }
                bl.hp -= amount * BUILDING_ARMOR;
                if bl.hp <= 0.0 {
                    self.destroy_building(b, by);
                }
            }
            Target::Unit(u) => {
                let Some(un) = self.units.get(u) else { return };
                if un.dead {
                    return;
                }
                let mods = self.factions[un.faction as usize].mods;
                let mut a = amount;
                if by.arrow {
                    if un.kind == UnitKind::Ram {
                        a *= ARROW_VS_RAM;
                    } else if un.def().block > 0.0 || un.kind == UnitKind::Knight {
                        a *= mods.arrow_res;
                    }
                }
                a = (a - mods.unit_armor(un.kind)).max(1.0);
                let time = self.time;
                let un = self.units.get_mut(u).expect("checked");
                un.hurt_t = HURT_FLASH;
                un.last_hurt = time;
                un.hp -= a;
                if un.hp <= 0.0 {
                    self.kill_unit(u, by);
                }
            }
        }
    }

    /// `updateSoldier(u)` (§8.1): attack/move/idle/amove state machine with breach lock, alarm, raid chain.
    /// STUB: plain moves only (travel then idle).
    pub fn update_soldier(&mut self, id: UnitId) {
        let Some(u) = self.units.get(id) else { return };
        match u.order.clone() {
            crate::world::Order::Move(m) | crate::world::Order::AMove(m)
                if self.travel(id, m.to, TRAVEL_NEAR) =>
            {
                self.idle(id);
            }
            _ => {}
        }
    }

    /// `updateMonk(u)` (§9.1): heal scan (thinkT [RNG] 0.4-0.7), keep 150 px from enemy soldiers, escort.
    /// STUB: plain moves only.
    pub fn update_monk(&mut self, id: UnitId) {
        self.update_soldier(id);
    }

    /// `acquireTarget(u, roam)` (§8.2): weighted nearest enemy unit within aggro (pawn ×2.2, monk ×0.6), guard
    /// leash 430, AI garrison threat clause, else enemy non-wall building. STUB: `None`.
    pub fn acquire_target(&mut self, id: UnitId, roam: bool) -> Option<Target> {
        None
    }

    /// `fight(u, t, dt)` (§8.3): in range → kite (archers) / face / `start_attack` when `cd <= 0`; else approach.
    /// Returns false if unreachable. STUB: false.
    pub fn fight(&mut self, id: UnitId, t: Target) -> bool {
        false
    }

    /// `startAttack(u, tx, ty)` (§8.4): state Attack, cd = def.cd, anim (warrior combo, lancer dir5), anim_t 0.
    /// STUB.
    pub fn start_attack(&mut self, id: UnitId, aim: V2) {}

    /// `startBlock(u, from)` (§8.4): state Block for 0.5 s, face the attacker. STUB.
    pub fn start_block(&mut self, id: UnitId, from: Option<V2>) {}

    /// `tickAction(u, dt)` (§8.4): attack → hit at `hit_frame` (once, `do_hit`), end at `frames`; block timer.
    /// Returns busy. STUB: never busy.
    pub fn tick_action(&mut self, id: UnitId) -> bool {
        false
    }

    /// `doHit(u)` (§8.5): archer arrow / catapult rock / monk heal / melee hit with reach check. STUB.
    pub fn do_hit(&mut self, id: UnitId) {}

    /// `breach(u, gx, gy)` (§8.6): best of the 4 nearest enemy buildings (walls included) within 900 px whose
    /// field reaches the unit. STUB: `None`.
    pub fn breach(&mut self, id: UnitId, goal: V2) -> Option<BuildingId> {
        None
    }

    /// `raidOn(u)` (§8.7): humans auto-amove to the nearest enemy within 900 px; AI → idle. STUB: idle.
    pub fn raid_on(&mut self, id: UnitId) {
        self.idle(id);
    }

    /// `alarmFor(u)` (§10.2): the faction alarm if human, < 10 s old, unit idle soldier. STUB: `None`.
    pub fn alarm_for(&self, id: UnitId) -> Option<Alarm> {
        None
    }

    /// `answerAlarm(u, a, dt)` (§10.2, addenda U1). STUB.
    pub fn answer_alarm(&mut self, id: UnitId, a: Alarm) {}
}
