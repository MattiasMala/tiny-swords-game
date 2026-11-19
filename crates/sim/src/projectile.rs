// OWNER: S2 (units) — see docs/spec/03a-units-combat.md §14 (fireArrow, fireRock, updateArrows, rockImpact,
// tower/castle shooting is in building.rs and calls `fire_arrow`) and 03b §13; 01 §2.3 (arrow life, rock flight).
//
// Implemented: fire_arrow, fire_rock, update_projectiles (arrows hit the first enemy unit in slab order whose box
// they overlap, then enemy buildings; rocks land on a fixed point), rock_impact (splash).
// `pos` of a rock is its ground point (lerp start → landing); the 150 px arc is a pure visual
// (`sin(k*π)*150` with `k = t/dur`), computed by whoever renders it.

use crate::TICK_DT;
use crate::World;
use crate::combat::{Attacker, damage_to};
use crate::defs::*;
use crate::events::GameEvent;
use crate::math::{V2, v2};
use crate::slab::ProjId;
use crate::world::{DamageSource, ProjKind, Projectile, Shooter, Target};

impl World {
    /// `fireArrow(owner, x, y, tx, ty, dmg)`: speed 640, life `(d + 70)/640`, sound `shoot` at the origin.
    pub fn fire_arrow(
        &mut self,
        faction: FactionIdx,
        shooter: Shooter,
        from: V2,
        to: V2,
        dmg: f32,
    ) -> ProjId {
        // `hypot(...) || 1`
        let d = {
            let d = from.dist(to);
            if d > 0.0 { d } else { 1.0 }
        };
        let vel = (to - from) * (ARROW_SPEED / d);
        let p = Projectile {
            kind: ProjKind::Arrow,
            faction,
            shooter,
            dmg,
            pos: from,
            vel,
            ang: (to.y - from.y).atan2(to.x - from.x),
            life: (d + ARROW_OVERSHOOT) / ARROW_SPEED,
            from,
            to,
            t: 0.0,
            dur: 0.0,
            dead: false,
        };
        self.emit(GameEvent::Sound {
            sound: Sound::Shoot,
            x: from.x,
            y: from.y,
        });
        self.projectiles.insert(p)
    }

    /// `fireRock(owner, tx, ty, dmg)`: from `(x ± 20, y - 50)` (computed by the caller), duration `0.5 + d/520`,
    /// sound `shoot`.
    pub fn fire_rock(
        &mut self,
        faction: FactionIdx,
        shooter: Shooter,
        from: V2,
        to: V2,
        dmg: f32,
    ) -> ProjId {
        let d = from.dist(to);
        let p = Projectile {
            kind: ProjKind::Rock,
            faction,
            shooter,
            dmg,
            pos: from,
            vel: V2::ZERO,
            ang: 0.0,
            life: 0.0,
            from,
            to,
            t: 0.0,
            dur: ROCK_BASE_TIME + d / ROCK_SPEED,
            dead: false,
        };
        self.emit(GameEvent::Sound {
            sound: Sound::Shoot,
            x: from.x,
            y: from.y,
        });
        self.projectiles.insert(p)
    }

    /// Current position of a projectile's shooter (its last known spot = the launch point if it is gone).
    fn shooter_pos(&self, p: &Projectile) -> V2 {
        match p.shooter {
            Shooter::Unit { id, .. } => self.units.get(id).map(|u| u.pos),
            Shooter::Building { id, .. } => self.buildings.get(id).map(|b| b.pos),
        }
        .unwrap_or(p.from)
    }

    fn proj_source(p: &Projectile) -> DamageSource {
        DamageSource {
            unit: match p.shooter {
                Shooter::Unit { id, .. } => Some(id),
                Shooter::Building { .. } => None,
            },
            faction: Some(p.faction),
            arrow: p.kind == ProjKind::Arrow,
        }
    }

    /// `updateArrows(dt)` (§14.1-14.2): arrows move, expire, hit the first enemy unit (box ±18 / y-28 ±26) then
    /// buildings (`x0<x<x1`, `y0-70<y<y1`) via `hurt` with `damage_to`; rocks follow their flight and call
    /// `rock_impact` at k >= 1.
    pub fn update_projectiles(&mut self) {
        if self.projectiles.is_empty() {
            return;
        }
        let dt = TICK_DT;
        // units moved (and some died) since the start of the unit phase: re-bucket for exact hit tests
        self.grid.rebuild(&self.units);
        let mut near = Vec::new();
        for pid in self.projectiles.ids() {
            let Some(p) = self.projectiles.get_mut(pid) else {
                continue;
            };
            if p.dead {
                continue;
            }
            match p.kind {
                ProjKind::Rock => {
                    p.t += dt;
                    let k = (p.t / p.dur.max(1e-3)).min(1.0);
                    p.pos = p.from.lerp(p.to, k);
                    p.ang += ROCK_SPIN * dt;
                    if k >= 1.0 {
                        p.dead = true;
                        let rock = p.clone();
                        self.rock_impact(&rock);
                    }
                }
                ProjKind::Arrow => {
                    p.pos += p.vel * dt;
                    p.life -= dt;
                    if p.life <= 0.0 {
                        p.dead = true;
                        continue;
                    }
                    let a = p.clone();
                    let at = a.pos;
                    // first enemy unit (slab order) whose hit box contains the arrow
                    self.grid.query_sorted(
                        v2(at.x, at.y + ARROW_HIT_YOFF),
                        ARROW_HIT_DY.max(ARROW_HIT_DX) + 2.0,
                        &mut near,
                    );
                    let hit = near.iter().copied().find(|&uid| {
                        self.units.get(uid).is_some_and(|u| {
                            u.active()
                                && u.faction != a.faction
                                && (u.pos.x - at.x).abs() < ARROW_HIT_DX
                                && (u.pos.y - ARROW_HIT_YOFF - at.y).abs() < ARROW_HIT_DY
                        })
                    });
                    if let Some(uid) = hit {
                        let tk = self.units.get(uid).map(|u| u.kind);
                        let amount = damage_to(Attacker::from(a.shooter), a.dmg, tk);
                        let from = self.shooter_pos(&a);
                        self.hurt_from(
                            Target::Unit(uid),
                            amount,
                            Self::proj_source(&a),
                            Some(from),
                        );
                        self.emit(GameEvent::Sound {
                            sound: Sound::Hit,
                            x: at.x,
                            y: at.y,
                        });
                        if let Some(p) = self.projectiles.get_mut(pid) {
                            p.dead = true;
                        }
                        continue;
                    }
                    let bhit = self.buildings.iter().find_map(|(bid, b)| {
                        (!b.dead
                            && b.faction != a.faction
                            && at.x > b.rect.x0
                            && at.x < b.rect.x1
                            && at.y > b.rect.y0 - ARROW_BUILDING_TOP
                            && at.y < b.rect.y1)
                            .then_some(bid)
                    });
                    if let Some(bid) = bhit {
                        let amount = damage_to(Attacker::from(a.shooter), a.dmg, None);
                        let from = self.shooter_pos(&a);
                        self.hurt_from(
                            Target::Building(bid),
                            amount,
                            Self::proj_source(&a),
                            Some(from),
                        );
                        if let Some(p) = self.projectiles.get_mut(pid) {
                            p.dead = true;
                        }
                    }
                }
            }
        }
        self.projectiles.retain(|_, p| !p.dead);
    }

    /// `rockImpact(a)` (§14.2): splash on enemy units (< 72 px) and buildings (rectDist < 36), FX boom1 + dust2,
    /// sound boom. No friendly fire, no arrow reductions.
    pub fn rock_impact(&mut self, p: &Projectile) {
        let r = match p.shooter {
            Shooter::Unit { kind, .. } => kind.def().splash,
            Shooter::Building { .. } => 0.0,
        };
        let at = p.to;
        let src = Self::proj_source(p);
        let from = self.shooter_pos(p);
        let attacker = Attacker::from(p.shooter);
        let mut near = Vec::new();
        self.grid.query_sorted(at, r + 2.0, &mut near);
        for uid in near {
            let Some(u) = self.units.get(uid) else {
                continue;
            };
            if !u.active() || u.faction == p.faction || u.pos.dist(at) >= r {
                continue;
            }
            let amount = damage_to(attacker, p.dmg, Some(u.kind));
            self.hurt_from(Target::Unit(uid), amount, src, Some(from));
        }
        let hits: Vec<_> = self
            .buildings
            .iter()
            .filter(|(_, b)| !b.dead && b.faction != p.faction && b.dist(at) < r * 0.5)
            .map(|(bid, _)| bid)
            .collect();
        for bid in hits {
            let amount = damage_to(attacker, p.dmg, None);
            self.hurt_from(Target::Building(bid), amount, src, Some(from));
        }
        self.emit(GameEvent::Fx {
            fx: FxKind::Boom1,
            x: at.x,
            y: at.y - 10.0,
            scale: 1.0,
            dy: 30.0,
            follow: None,
            color: None,
        });
        self.emit(GameEvent::Fx {
            fx: FxKind::Dust2,
            x: at.x,
            y: at.y + 6.0,
            scale: 1.6,
            dy: 0.0,
            follow: None,
            color: None,
        });
        self.emit(GameEvent::Sound {
            sound: Sound::Boom,
            x: at.x,
            y: at.y,
        });
    }
}
