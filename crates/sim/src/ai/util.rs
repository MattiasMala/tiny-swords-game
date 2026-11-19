//! Helpers of the AI: queued-unit counts, castle position, `nearestEnemyFaction`, `aiWorth`, `findSpot`
//! (04a §9.4, §14; 04b §5). `freeBuilding` / `queued` live in faction.rs (S4).

use crate::World;
use crate::cmd::QueueWhat;
use crate::defs::*;
use crate::math::V2;
use crate::slab::UnitId;

impl World {
    /// `queued(f, t => t == 'pawn')` (`pawns = true`) or `queued(f, t => t != 'pawn')`: queued units across all
    /// own buildings.
    pub fn ai_queued(&self, f: FactionIdx, pawns: bool) -> u32 {
        self.queued(f, |q| matches!(q, QueueWhat::Unit(k) if (k == UnitKind::Pawn) == pawns)) as u32
    }

    /// Castle foot point of a faction (`f.castle.x, f.castle.y`), `None` without a live castle.
    pub fn ai_castle(&self, f: FactionIdx) -> Option<V2> {
        let c = self.factions.get(f as usize)?.castle?;
        self.buildings.get(c).filter(|b| !b.dead).map(|b| b.pos)
    }

    /// `nearestEnemyFaction(f)` (factions.js:530-538): alive faction ≠ f with the closest castle (strict `<`).
    pub fn ai_nearest_enemy(&self, f: FactionIdx) -> Option<FactionIdx> {
        let c = self.ai_castle(f)?;
        let mut best = None;
        let mut bd = f32::INFINITY;
        for o in &self.factions {
            if o.idx == f || !o.alive {
                continue;
            }
            let Some(oc) = self.ai_castle(o.idx) else {
                continue;
            };
            let d = oc.dist(c);
            if d < bd {
                bd = d;
                best = Some(o.idx);
            }
        }
        best
    }

    /// `aiWorth(u) = aiCost(type) * hp / maxHp` (0 for a missing unit).
    pub fn ai_worth(&self, id: UnitId) -> f32 {
        self.units
            .get(id)
            .map(|u| {
                if u.max_hp > 0.0 {
                    ai_cost(u.kind) * u.hp / u.max_hp
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0)
    }

    /// `findSpot(kind, x0, y0, rmin, rmax)` (factions.js:177-199, 04a §9.4 + addenda E8): rings `r = rmin..=rmax`
    /// around cell `(x0, y0)`, ring offsets shuffled with the sim RNG (Fisher–Yates); the first top-left cell
    /// where `canPlace` holds and the 1-cell border around the footprint is inside and passable. Does NOT check
    /// the build zone (the `place` command does).
    pub fn ai_find_spot(
        &mut self,
        kind: BuildingKind,
        x0: i32,
        y0: i32,
        rmin: i32,
        rmax: i32,
    ) -> Option<(i32, i32)> {
        let d = kind.def();
        let mut ring: Vec<(i32, i32)> = Vec::with_capacity(8 * rmax.max(1) as usize + 4);
        for r in rmin.max(0)..=rmax {
            ring.clear();
            for i in -r..=r {
                ring.push((i, -r));
                ring.push((i, r));
                ring.push((-r, i));
                ring.push((r, i));
            }
            self.rng.shuffle(&mut ring);
            for &(dx, dy) in &ring {
                // JS Math.round: halves round up (toward +inf)
                let cx = js_round((x0 + dx) as f32 - d.w as f32 / 2.0);
                let cy = js_round((y0 + dy) as f32 - d.h as f32 / 2.0);
                if self.ai_roomy(kind, cx, cy) {
                    return Some((cx, cy));
                }
            }
        }
        None
    }

    /// `roomy(cx, cy)` of `findSpot`: `canPlace` + passable 1-cell corridor all around.
    fn ai_roomy(&self, kind: BuildingKind, cx: i32, cy: i32) -> bool {
        let d = kind.def();
        if !self.map.can_place(kind, cx, cy, &[]) {
            return false;
        }
        for y in cy - 1..=cy + d.h {
            for x in cx - 1..=cx + d.w {
                if !self.map.inside(x, y) || !self.map.passable(self.map.idx(x, y), 0) {
                    return false;
                }
            }
        }
        true
    }
}

/// JS `Math.round` (round half up).
#[inline]
pub fn js_round(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}
