// OWNER: S4 (faction) — see docs/spec/05-control-loop.md §12 (fog: explored + visible, what is gated), 04a §11.1
// (`explore(f)`: START_SIGHT disc, unit/building/point discs `(i-cx)²+(j-cy)² <= r²+r`, `seen_key` change cache,
// building sight centre `(b.x, b.y-30)`, sites 2 cells, point centre `(p.x, p.y-64)`, + mods.sight),
// 04b addenda A6 (explore keeps running for human-OWNED factions while AI-driven; `explored()` tests `human`),
// 05 addenda C3 (enemy building ghosts are client-side), ARCHITECTURE.md (visible sets at 5 Hz).
//
// Discs are stamped row by row as word-masked bit spans (no per-cell loop). `explore` is incremental (an entity
// re-stamps only when its cell or sight radius changed, JS `seenKey`); the visible set is rebuilt from scratch every
// `VISIBLE_PERIOD` for human-owned kingdoms only (nobody looks through an AI's eyes; AI ignores fog), and is OR-ed
// into `explored` so that visible ⊆ explored always holds. Eliminated kingdoms, test fog-off and match over see
// everything. Snapshot deltas of `explored` are computed by the snapshot builder.
#![allow(dead_code, unused)]

use crate::World;
use crate::defs::*;
use crate::math::{BitGrid, V2};
use crate::{TICK_DT, TILE};

/// Integer square root (floor).
#[inline]
fn isqrt(n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    let mut r = (n as f64).sqrt() as i32;
    while r * r > n {
        r -= 1;
    }
    while (r + 1) * (r + 1) <= n {
        r += 1;
    }
    r
}

/// Sets bits `a..=b` (`a <= b`, both in range).
#[inline]
fn set_span(g: &mut BitGrid, a: usize, b: usize) {
    let (wa, wb) = (a >> 6, b >> 6);
    let lo = !0u64 << (a & 63);
    let hi = !0u64 >> (63 - (b & 63));
    if wa == wb {
        g.words[wa] |= lo & hi;
        return;
    }
    g.words[wa] |= lo;
    for w in &mut g.words[wa + 1..wb] {
        *w = !0;
    }
    g.words[wb] |= hi;
}

/// `mark(x, y, r)` / `Fog.reveal`: every map cell `(i, j)` with `(i-cx)² + (j-cy)² <= r² + r`, where
/// `(cx, cy)` is the cell of `p` (cells outside the map are skipped; the centre itself may be off-map).
pub fn stamp_disc(g: &mut BitGrid, map_w: i32, map_h: i32, p: V2, r: i32) {
    if r < 0 || !p.is_finite() || g.words.is_empty() {
        return;
    }
    let cx = (p.x / TILE).floor() as i32;
    let cy = (p.y / TILE).floor() as i32;
    let rr = r * r + r;
    let j0 = (cy - r).max(0);
    let j1 = (cy + r).min(map_h - 1);
    for j in j0..=j1 {
        let dy = j - cy;
        let rem = rr - dy * dy;
        if rem < 0 {
            continue;
        }
        let dx = isqrt(rem).min(r);
        let i0 = (cx - dx).max(0);
        let i1 = (cx + dx).min(map_w - 1);
        if i0 > i1 {
            continue;
        }
        let row = (j * map_w) as usize;
        let (a, b) = (row + i0 as usize, row + i1 as usize);
        if b >= g.len as usize {
            continue;
        }
        set_span(g, a, b);
    }
}

/// `seenKey = (floor(y/TILE)*W + floor(x/TILE))*32 + r` (wrapping; only equality matters).
#[inline]
fn seen_key(map_w: i32, p: V2, r: i32) -> u32 {
    let cx = (p.x / TILE).floor() as i64;
    let cy = (p.y / TILE).floor() as i64;
    ((cy * map_w as i64 + cx) * 32 + r as i64) as u32
}

/// Sight radius in cells as an integer (all sights are whole cells).
#[inline]
fn cells(r: f32) -> i32 {
    r.round() as i32
}

impl World {
    /// `explore(f)` (04a §11.1): marks explored cells in `fog.explored`. First call: START_SIGHT disc around the
    /// castle. Then every own unit (sheltered pawns too), building (sites see 2 cells, from `(x, y-30)`) and owned
    /// point (from `(x, y-64)`), each re-stamped only when its cell or radius changed (`seen_key`).
    pub fn explore(&mut self, f: FactionIdx) {
        let fi = f as usize;
        if fi >= self.factions.len() {
            return;
        }
        let (mw, mh) = (self.map.w, self.map.h);
        let castle = self.factions[fi]
            .castle
            .and_then(|c| self.buildings.get(c))
            .map(|c| c.pos);
        let World {
            factions,
            units,
            buildings,
            points,
            ..
        } = self;
        let fac = &mut factions[fi];
        let sight = fac.mods.sight;
        let g = &mut fac.fog.explored;
        if !fac.fog.started {
            fac.fog.started = true;
            if let Some(c) = castle {
                stamp_disc(g, mw, mh, c, cells(START_SIGHT));
            }
        }
        for (_, u) in units.iter_mut() {
            if u.faction != f || u.dead {
                continue;
            }
            let r = cells(u.def().sight + sight);
            let key = seen_key(mw, u.pos, r);
            if u.seen_key != key {
                u.seen_key = key;
                stamp_disc(g, mw, mh, u.pos, r);
            }
        }
        for (_, b) in buildings.iter_mut() {
            if b.faction != f || b.dead {
                continue;
            }
            let r = cells(if b.done { b.def().sight } else { SITE_SIGHT } + sight);
            let at = V2::new(b.pos.x, b.pos.y - BUILDING_SIGHT_DY);
            let key = seen_key(mw, at, r);
            if b.seen_key != key {
                b.seen_key = key;
                stamp_disc(g, mw, mh, at, r);
            }
        }
        for p in points.iter_mut() {
            if p.owner != Some(f) {
                continue;
            }
            let r = cells(POINT_SIGHT + sight);
            let at = V2::new(p.pos.x, p.pos.y - TILE);
            // the owner is part of the key: a point captured from another kingdom must be stamped for the new owner
            let key = seen_key(mw, at, r).wrapping_mul(8).wrapping_add(f as u32 + 1);
            if p.seen_key != key {
                p.seen_key = key;
                stamp_disc(g, mw, mh, at, r);
            }
        }
    }

    /// Recomputes `fog.visible` for every alive human-owned faction every `VISIBLE_PERIOD` (0.2 s, `Fog.update`)
    /// from own units (sheltered pawns too), buildings and owned points (same discs as `explore`), then ORs it into
    /// `explored`. Called once per tick at the end of `World::step`.
    pub fn update_fog(&mut self) {
        let dt = TICK_DT;
        let (mw, mh) = (self.map.w, self.map.h);
        for fi in 0..self.factions.len() {
            let f = fi as FactionIdx;
            {
                let fac = &mut self.factions[fi];
                if !fac.alive || !(fac.owned || fac.human) {
                    continue;
                }
                fac.fog.vis_t -= dt;
                if fac.fog.vis_t > 0.0 {
                    continue;
                }
                fac.fog.vis_t = VISIBLE_PERIOD;
            }
            let World {
                factions,
                units,
                buildings,
                points,
                ..
            } = self;
            let fac = &mut factions[fi];
            let sight = fac.mods.sight;
            let fog = &mut fac.fog;
            fog.visible.clear();
            let g = &mut fog.visible;
            for u in units.values() {
                if u.faction == f && !u.dead {
                    stamp_disc(g, mw, mh, u.pos, cells(u.def().sight + sight));
                }
            }
            for b in buildings.values() {
                if b.faction == f && !b.dead {
                    let r = cells(if b.done { b.def().sight } else { SITE_SIGHT } + sight);
                    stamp_disc(
                        g,
                        mw,
                        mh,
                        V2::new(b.pos.x, b.pos.y - BUILDING_SIGHT_DY),
                        r,
                    );
                }
            }
            for p in points.iter() {
                if p.owner == Some(f) {
                    stamp_disc(
                        g,
                        mw,
                        mh,
                        V2::new(p.pos.x, p.pos.y - TILE),
                        cells(POINT_SIGHT + sight),
                    );
                }
            }
            if fog.explored.words.len() == fog.visible.words.len() {
                for (e, v) in fog.explored.words.iter_mut().zip(fog.visible.words.iter()) {
                    *e |= *v;
                }
            }
        }
    }

    /// Does faction `f` see everything? (test fog-off, match over, eliminated → spectator, AI-only kingdoms).
    #[inline]
    pub fn sees_all(&self, f: FactionIdx) -> bool {
        // an unknown faction index (bad input) is treated like an AI kingdom: no fog
        let Some(fac) = self.factions.get(f as usize) else {
            return true;
        };
        fac.fog_off || self.outcome.is_some() || !fac.alive || !(fac.owned || fac.human)
    }

    /// Is the point currently visible to faction `f`? (`Fog.sees`; see [`World::sees_all`]).
    pub fn is_visible(&self, f: FactionIdx, p: V2) -> bool {
        if self.sees_all(f) {
            return true;
        }
        let c = self.map.cell_of(p);
        c >= 0 && self.factions[f as usize].fog.visible.get(c as usize)
    }

    /// Visible bit of a cell index (same rules as [`World::is_visible`]; false off-map).
    pub fn is_visible_cell(&self, f: FactionIdx, c: i32) -> bool {
        if !self.map.valid(c) {
            return false;
        }
        self.sees_all(f) || self.factions[f as usize].fog.visible.get(c as usize)
    }

    /// `explored(f, x, y)`: always true for AI-driven factions (no fog) and before the first `explore`; else the
    /// explored bit (false off-map).
    pub fn explored(&self, f: FactionIdx, p: V2) -> bool {
        let Some(fac) = self.factions.get(f as usize) else {
            return true;
        };
        if !fac.human || !fac.fog.started {
            return true;
        }
        let c = self.map.cell_of(p);
        c >= 0 && fac.fog.explored.get(c as usize)
    }

    /// Explored bit of a cell `(cx, cy)` (ignores the AI shortcut; used by snapshots/UI).
    pub fn explored_cell(&self, f: FactionIdx, cx: i32, cy: i32) -> bool {
        self.map.inside(cx, cy)
            && self
                .factions
                .get(f as usize)
                .is_some_and(|fac| fac.fog.explored.get(self.map.idx(cx, cy) as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v2;

    /// Reference per-cell implementation of the JS `mark`.
    fn reference(w: i32, h: i32, p: V2, r: i32) -> BitGrid {
        let mut g = BitGrid::new((w * h) as usize);
        let (cx, cy) = ((p.x / TILE).floor() as i32, (p.y / TILE).floor() as i32);
        for j in (cy - r).max(0)..=(cy + r).min(h - 1) {
            for i in (cx - r).max(0)..=(cx + r).min(w - 1) {
                if (i - cx).pow(2) + (j - cy).pow(2) <= r * r + r {
                    g.set((j * w + i) as usize);
                }
            }
        }
        g
    }

    #[test]
    fn disc_matches_js_mark() {
        let (w, h) = (150, 70);
        for (x, y, r) in [
            (10.0, 10.0, 5),
            (64.0 * 75.5, 64.0 * 30.2, 15),
            (64.0 * 149.0, 64.0 * 69.0, 9),
            (64.0 * 63.0, 64.0 * 2.0, 7),
            (64.0 * 3.0, 64.0 * 40.0, 0),
            (-100.0, 64.0 * 10.0, 4),
        ] {
            let mut g = BitGrid::new((w * h) as usize);
            stamp_disc(&mut g, w, h, v2(x, y), r);
            assert_eq!(g, reference(w, h, v2(x, y), r), "disc at {x},{y} r={r}");
        }
    }

    #[test]
    fn explore_and_visible() {
        let mut w = crate::faction::tests::base_world();
        let f = crate::faction::tests::human(&w);
        assert!(w.explored(f, v2(1.0, 1.0)), "before the first explore all is known");
        w.step(&[]);
        let castle = w.buildings.get(w.factions[f as usize].castle.unwrap()).unwrap().pos;
        assert!(w.explored(f, castle));
        assert!(w.is_visible(f, castle));
        let far = v2(castle.x + 64.0 * 30.0, castle.y);
        let far = if w.map.cell_of(far) >= 0 { far } else { v2(castle.x - 64.0 * 30.0, castle.y) };
        assert!(!w.is_visible(f, far));
        // start disc (15 cells) is explored but not visible beyond unit/building sight
        let edge = v2(castle.x, castle.y - 64.0 * 14.0);
        if w.map.cell_of(edge) >= 0 {
            assert!(w.explored(f, edge));
        }
        // visible ⊆ explored
        let fog = &w.factions[f as usize].fog;
        for (e, v) in fog.explored.words.iter().zip(fog.visible.words.iter()) {
            assert_eq!(*v & !*e, 0);
        }
        // AI kingdoms: no fog
        let ai = (f + 1) % 5;
        assert!(w.explored(ai, far) && w.is_visible(ai, far));
        // test mode fog off
        w.exec_command(f, &crate::cmd::Command::DebugFogOff { on: true }).unwrap();
        assert!(w.is_visible(f, far));
        // a unit walking away re-stamps exploration
        w.exec_command(f, &crate::cmd::Command::DebugFogOff { on: false }).unwrap();
        let pawn = w.factions[f as usize].pawns[0];
        let before = w.factions[f as usize].fog.explored.count();
        w.units.get_mut(pawn).unwrap().pos = far;
        for _ in 0..6 {
            w.step(&[]);
        }
        assert!(w.factions[f as usize].fog.explored.count() > before);
        assert!(w.explored(f, far));
        assert!(w.is_visible(f, far));
    }
}
