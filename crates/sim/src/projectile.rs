// OWNER: S2 (units) — see docs/spec/03a-units-combat.md §14 (fireArrow, fireRock, updateArrows, rockImpact,
// tower/castle shooting is in building.rs and calls `fire_arrow`) and 03b §13; 01 §2.3 (arrow life, rock flight).
//
// Stub fns S2 must fill: fire_arrow / fire_rock (done: create + `shoot` sound), update_projectiles (MINIMAL: flight
// and expiry, NO hits), rock_impact (stub).
#![allow(dead_code, unused)]

use crate::TICK_DT;
use crate::World;
use crate::defs::*;
use crate::events::GameEvent;
use crate::math::{V2, v2};
use crate::slab::ProjId;
use crate::world::{ProjKind, Projectile, Shooter};

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
        let d = from.dist(to).max(1.0);
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

    /// `fireRock(owner, tx, ty, dmg)`: from `(x ± 20, y - 50)`, duration `0.5 + d/520`, sound `shoot`.
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

    /// `updateArrows(dt)` (§14.1-14.2): arrows move, expire, hit the first enemy unit (box ±18 / y-28 ±26) then
    /// buildings (`x0<x<x1`, `y0-70<y<y1`) via `hurt` with `damage_to`; rocks follow the arc (visual) and call
    /// `rock_impact` at k >= 1. MINIMAL: movement + expiry only.
    pub fn update_projectiles(&mut self) {
        let dt = TICK_DT;
        for (_, p) in self.projectiles.iter_mut() {
            match p.kind {
                ProjKind::Arrow => {
                    p.pos += p.vel * dt;
                    p.life -= dt;
                    if p.life <= 0.0 {
                        p.dead = true;
                    }
                }
                ProjKind::Rock => {
                    p.t += dt;
                    let k = (p.t / p.dur.max(1e-3)).min(1.0);
                    p.pos = p.from.lerp(p.to, k);
                    p.ang += ROCK_SPIN * dt;
                    if k >= 1.0 {
                        p.dead = true;
                    }
                }
            }
        }
        self.projectiles.retain(|_, p| !p.dead);
    }

    /// `rockImpact(a)` (§14.2): splash on enemy units (< 72 px) and buildings (rectDist < 36), FX boom1 + dust2,
    /// sound boom. STUB.
    pub fn rock_impact(&mut self, p: &Projectile) {}
}
