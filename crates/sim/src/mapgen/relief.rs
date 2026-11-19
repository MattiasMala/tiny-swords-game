//! Plateaus (level 1), terraces (level 2), cliffs and stairs — 02 §5.8 semantics, MAPGEN.md §1, §2, §4 shapes.
//!
//! Area-driven: plateaus are added until about 27-33 % of the mainland is raised (20-38 of them): first 5-8
//! massifs (a wide core + 3-5 lobes), then big and medium ones. Each plateau is a union of axis-aligned rectangles
//! (edges often aligned so outlines have long runs), clipped to the allowed area, then closed + opened with a 3x3
//! square (no one-cell spurs, notches or holes) and reduced to its largest component. Plateaus keep ≥ 4 cells from
//! each other (lowland corridors between them) and stay out of the kingdoms' keep zones. Some are anchored on a
//! straight stretch of coast so their cliff drops straight into the sea. A candidate that would split the lowland
//! (a cheap local ring test, else a global flood) is rejected, and a stair whose raised top would cut a one-cell
//! neck of the lower ground is skipped (`bridge_ok`), so access never depends on luck.
//!
//! Terraces (level 2): on plateaus ≥ 110 cells, 1-2 rectangles (2-3 larger ones on plateaus ≥ 400 cells) inside
//! the plateau's 2-cell inner margin (3 below), with 1-2 stairs from level-1 ground reachable from the plateau's own
//! stairs.
//!
//! Gameplay semantics (02 §3.1, §5.8) are kept exactly: the cell south of every bottom-edge cell is a `cliff`;
//! a stair is the 2-cell column `hi`/`lo` beside a bottom corner (`stairSpots`), `hi` gets the upper level; the
//! 3x4 access zone around `hi` is `noBuild`; stairs of one plateau are ≥ 6 cells apart.

use super::grid::{Mask, flood4};
use super::{Gen, H, W, idx};
use crate::defs::{BASE_CLEAR, BASE_COUNT};

/// A 2-cell stair (`stair[lo] = 1|3`, `stair[hi] = 2|4`) and its approach cell `ap` below `lo`.
#[derive(Clone, Copy, Debug)]
pub struct Stair {
    pub hi: i32,
    pub lo: i32,
    pub ap: i32,
    /// -1 = left of the plateau, +1 = right
    pub dir: i32,
}

impl Stair {
    /// `landing(s)`: the plateau cell beside the stair top (`hi - dir`).
    pub fn landing(&self) -> i32 {
        self.hi - self.dir
    }
}

/// A raised area being generated.
#[derive(Clone, Debug)]
pub struct Hill {
    pub cells: Vec<i32>,
    pub stairs: Vec<Stair>,
    /// terrace (level 2) cells and stairs
    pub top: Option<(Vec<i32>, Vec<Stair>)>,
    /// resource anchor: free level-1 cell nearest to the centroid
    pub anchor: (i32, i32),
}

impl Hill {
    pub fn bbox(&self) -> (i32, i32, i32, i32) {
        let mut b = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for &c in &self.cells {
            let (x, y) = (c % W, c / W);
            b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
        }
        b
    }
}

impl Gen {
    /// Keep zones around the kingdoms: no plateau there (the start stock ring stays on flat ground).
    fn keep_zones(&mut self) {
        let bases = self.bases.clone();
        for &(bx, by) in &bases {
            for y in by - BASE_CLEAR.1 - 8..=by + BASE_CLEAR.1 + 9 {
                for x in bx - BASE_CLEAR.0 - 9..=bx + BASE_CLEAR.0 + 9 {
                    if self.map.inside(x, y) {
                        let c = idx(x, y) as usize;
                        self.keep[c] = 1;
                    }
                }
            }
        }
    }

    /// Random rectangle union around an anchor, rasterised into a local window `(x0, y0, mask)`. `tier`: 0 = massif
    /// (a wide core + 2-4 lobes), 1 = big plateau, 2 = medium plateau.
    fn rect_union(&mut self, ax: i32, ay: i32, cape: bool, tier: u8) -> (i32, i32, Mask) {
        let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
        let (w0, h0) = match tier {
            0 => (26 + self.below(16), 12 + self.below(8)),
            1 => (17 + self.below(13), 9 + self.below(6)),
            _ => (9 + self.below(10), 5 + self.below(6)),
        };
        let x0 = ax - self.below(w0 as u32);
        let y0 = if cape { ay - h0 + 1 } else { ay - self.below(h0 as u32) };
        rects.push((x0, y0, w0, h0));
        let extra = match tier {
            0 => 3 + self.below(3),
            1 => [1, 2, 2, 3][self.below(4) as usize],
            _ => [0, 1, 1, 2][self.below(4) as usize],
        };
        let (lw, lh) = if tier == 0 { (8, 5) } else { (5, 4) };
        for _ in 0..extra {
            let r = rects[self.below(rects.len() as u32) as usize];
            let w = lw + self.below(11);
            let h = lh + self.below(6);
            // attach on one side with an overlap of ≥ 2 cells; often align an edge with the parent
            let (x, y) = match self.below(4) {
                0 => {
                    // east
                    let x = r.0 + r.2 - 2 - self.below(3);
                    let y = if self.chance(0.5) { r.1 } else { r.1 + r.3 - h + self.below(3) - 1 };
                    (x, y)
                }
                1 => {
                    let x = r.0 - w + 2 + self.below(3);
                    let y = if self.chance(0.5) { r.1 + r.3 - h } else { r.1 - 1 + self.below(3) };
                    (x, y)
                }
                2 => {
                    // north
                    let y = r.1 - h + 2 + self.below(2);
                    let x = if self.chance(0.5) { r.0 } else { r.0 + r.2 - w };
                    (x, y)
                }
                _ => {
                    // south (never for capes: the coast row stays the bottom)
                    if cape {
                        let y = r.1 - h + 2;
                        (r.0 + self.below(r.2.max(1) as u32) - w / 2, y)
                    } else {
                        let y = r.1 + r.3 - 2 - self.below(2);
                        let x = if self.chance(0.5) { r.0 } else { r.0 + r.2 - w };
                        (x, y)
                    }
                }
            };
            rects.push((x, y, w, h));
        }
        let bx0 = rects.iter().map(|r| r.0).min().unwrap_or(ax) - 2;
        let by0 = rects.iter().map(|r| r.1).min().unwrap_or(ay) - 2;
        let bx1 = rects.iter().map(|r| r.0 + r.2).max().unwrap_or(ax) + 2;
        let by1 = rects.iter().map(|r| r.1 + r.3).max().unwrap_or(ay) + 2;
        let mut m = Mask::new(bx1 - bx0, by1 - by0);
        for r in &rects {
            for y in r.1..r.1 + r.3 {
                for x in r.0..r.0 + r.2 {
                    m.set(x - bx0, y - by0, true);
                }
            }
        }
        (bx0, by0, m)
    }

    /// `hillOK` + spacing: a cell a new plateau may cover.
    fn plateau_ok(&self, x: i32, y: i32) -> bool {
        if x < 3 || y < 3 || x > W - 4 || y > H - 6 {
            return false;
        }
        let c = idx(x, y) as usize;
        self.map.land[c] == 1 && self.keep[c] == 0 && self.near[c] == 0
    }

    /// `stairSpots(M, ground)` (02 §5.8).
    fn stair_spots(&self, inm: &dyn Fn(i32) -> bool, cells: &[i32], ground: &dyn Fn(i32) -> bool) -> Vec<Stair> {
        let mut out = Vec::new();
        for &c in cells {
            if inm(c + W) {
                continue;
            }
            for dir in [-1, 1] {
                let hi = c + dir;
                let lo = hi + W;
                let ap = lo + W;
                if [hi, lo, ap, hi - W, hi + dir, lo + dir].iter().any(|k| inm(*k)) {
                    continue;
                }
                let (hx, hy) = (hi % W, hi / W);
                if hx < 2 || hx > W - 3 || hy < 2 || hy + 2 > H - 3 {
                    continue;
                }
                if ground(hi) && ground(lo) && ground(ap) {
                    out.push(Stair { hi, lo, ap, dir });
                }
            }
        }
        out
    }

    /// `raise(M, L, spots, count)`: first stair random, each next the farthest (Manhattan between `hi`) from those
    /// placed, stop when the farthest is < 6 apart.
    fn raise_stairs(&mut self, level: u8, spots: &mut Vec<Stair>, count: usize) -> Vec<Stair> {
        let mut put: Vec<Stair> = Vec::new();
        while put.len() < count && !spots.is_empty() {
            let mut pick = self.below(spots.len() as u32) as usize;
            if !put.is_empty() {
                let mut far = -1;
                for (i, s) in spots.iter().enumerate() {
                    let d = put
                        .iter()
                        .map(|q| ((q.hi % W) - (s.hi % W)).abs() + ((q.hi / W) - (s.hi / W)).abs())
                        .min()
                        .unwrap_or(0);
                    if d > far {
                        far = d;
                        pick = i;
                    }
                }
                if far < 6 {
                    break;
                }
            }
            let s = spots.remove(pick);
            if !self.bridge_ok(s.hi, s.lo, level - 1) {
                // raising `hi` would cut a one-cell neck of the lower ground
                continue;
            }
            let i_lo = s.lo as usize;
            let i_hi = s.hi as usize;
            self.map.stair[i_lo] = if s.dir < 0 { 1 } else { 3 };
            self.map.stair[i_hi] = if s.dir < 0 { 2 } else { 4 };
            self.map.level[i_hi] = level;
            let (hx, hy) = (s.hi % W, s.hi / W);
            for y in hy - 1..=hy + 2 {
                for x in hx - 1..=hx + 1 {
                    if super::land::in_margin(x, y) {
                        let k = idx(x, y) as usize;
                        self.map.no_build[k] = 1;
                        self.stairzone[k] = 1;
                    }
                }
            }
            // apron in front of the stair foot: no objects (stays buildable)
            for y in hy + 2..=hy + 4 {
                for x in hx - 2..=hx + 2 {
                    if super::land::in_margin(x, y) {
                        let k = idx(x, y) as usize;
                        self.apron[k] = 1;
                    }
                }
            }
            put.push(s);
        }
        put
    }

    /// Raising the stair top `hi` keeps the lower ground around it connected: every walkable 4-neighbour of `hi` on
    /// level `low` is reached from `lo` inside a 9x9 window without `hi` (sufficient local test).
    fn bridge_ok(&self, hi: i32, lo: i32, low: u8) -> bool {
        let (hx, hy) = (hi % W, hi / W);
        const R: i32 = 4;
        const S: i32 = 2 * R + 1;
        let walk = |x: i32, y: i32| -> bool {
            if !self.map.inside(x, y) || (x == hx && y == hy) {
                return false;
            }
            let k = idx(x, y) as usize;
            self.map.land[k] == 1 && self.map.cliff[k] == 0 && self.map.level[k] == low
        };
        let mut seen = [false; (S * S) as usize];
        let mut stack: Vec<(i32, i32)> = Vec::with_capacity(32);
        let (lx, ly) = (lo % W, lo / W);
        if !walk(lx, ly) {
            return true;
        }
        seen[((ly - hy + R) * S + (lx - hx + R)) as usize] = true;
        stack.push((lx, ly));
        while let Some((x, y)) = stack.pop() {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if (nx - hx).abs() > R || (ny - hy).abs() > R || !walk(nx, ny) {
                    continue;
                }
                let i = ((ny - hy + R) * S + (nx - hx + R)) as usize;
                if !seen[i] {
                    seen[i] = true;
                    stack.push((nx, ny));
                }
            }
        }
        [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|(dx, dy)| {
            let (nx, ny) = (hx + dx, hy + dy);
            !walk(nx, ny) || seen[((ny - hy + R) * S + (nx - hx + R)) as usize]
        })
    }

    /// Straight coast cells (land with sea below, 3 in a row): anchors of capes.
    fn cape_cells(&self) -> Vec<i32> {
        let mut out = Vec::new();
        for y in 4..H - 6 {
            for x in 4..W - 4 {
                let c = idx(x, y) as usize;
                let row = |k: usize| self.map.land[k] == 1 && self.map.land[k + W as usize] == 0;
                if row(c) && row(c - 1) && row(c + 1) && self.keep[c] == 0 {
                    out.push(c as i32);
                }
            }
        }
        out
    }

    /// Step 8: plateaus, cliffs, stairs, terraces; then unreachable pockets (step 9) and the hill order by
    /// distance to the bases' centroid. Returns false if fewer than 6 plateaus survived.
    pub fn relief(&mut self) -> bool {
        self.keep_zones();
        let land_cells = self.map.land.iter().filter(|v| **v == 1).count();
        // area-driven: about a quarter of the mainland raised (JS: ~9 700 level-1 cells), in 20-38 plateaus,
        // a few massifs first, then big and medium ones
        let target = (land_cells as f64 * (0.27 + self.rnd() * 0.06)) as usize;
        let massifs = 5 + self.below(4) as usize;
        let capes = self.cape_cells();
        let b0 = idx(self.bases[0].0, self.bases[0].1 + 2);
        let lowland = |map: &crate::map::Map, k: usize| map.land[k] == 1 && map.level[k] == 0 && map.cliff[k] == 0;
        let mut reach = flood4(W, H, &[b0], |k| lowland(&self.map, k)).iter().filter(|v| **v != 0).count();
        let mut hills: Vec<Hill> = Vec::new();
        let mut area = 0usize;
        let mut t = 0;
        while (area < target || hills.len() < 20) && hills.len() < 38 && t < 4000 {
            t += 1;
            let tier = if hills.len() < massifs && t < 1500 {
                0
            } else if area < target * 3 / 4 {
                1
            } else {
                2
            };
            let cape = !capes.is_empty() && self.chance(0.3);
            let (ax, ay) = if cape {
                let c = capes[self.below(capes.len() as u32) as usize];
                (c % W, c / W)
            } else {
                (4 + self.below((W - 8) as u32), 4 + self.below((H - 10) as u32))
            };
            if !self.plateau_ok(ax, ay) {
                continue;
            }
            let (ox, oy, mut m) = self.rect_union(ax, ay, cape, tier);
            // clip to the allowed area, close, clip, open, largest component
            let mut allowed = Mask::new(m.w, m.h);
            for y in 0..m.h {
                for x in 0..m.w {
                    allowed.set(x, y, self.plateau_ok(x + ox, y + oy));
                }
            }
            m.and(&allowed);
            let mut m2 = m.close(3);
            m2.and(&allowed);
            let mut m3 = m2.open(3);
            let n = m3.keep_largest();
            if n < 40 {
                continue;
            }
            let cells: Vec<i32> = (0..m3.h)
                .flat_map(|y| (0..m3.w).map(move |x| (x, y)))
                .filter(|(x, y)| m3.get(*x, *y))
                .map(|(x, y)| (y + oy) * W + (x + ox))
                .collect();
            let (mut bx0, mut by0, mut bx1, mut by1) = (W, H, 0, 0);
            for &c in &cells {
                bx0 = bx0.min(c % W);
                bx1 = bx1.max(c % W);
                by0 = by0.min(c / W);
                by1 = by1.max(c / W);
            }
            if bx1 - bx0 < 5 || by1 - by0 < 3 {
                continue;
            }
            // the lowland must stay in one piece: a plateau may not seal off a neck of land
            {
                // plateau cells and the cliff row below them
                let covered = |k: usize| -> bool {
                    let (x, y) = (k as i32 % W - ox, k as i32 / W - oy);
                    m3.get(x, y) || m3.get(x, y - 1)
                };
                // cheap sufficient test first: the lowland cells around the candidate are connected to each other
                // inside a window 5 cells larger than its box (then any path through the area can go around it)
                let (wx0, wy0) = ((bx0 - 5).max(0), (by0 - 5).max(0));
                let (wx1, wy1) = ((bx1 + 5).min(W - 1), (by1 + 6).min(H - 1));
                let ww = wx1 - wx0 + 1;
                let wh = wy1 - wy0 + 1;
                let map = &self.map;
                let free = |k: usize| lowland(map, k) && !covered(k);
                let mut ring: Vec<i32> = Vec::new();
                for y in wy0..=wy1 {
                    for x in wx0..=wx1 {
                        let k = idx(x, y) as usize;
                        if !free(k) {
                            continue;
                        }
                        let touch = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                            .iter()
                            .any(|(dx, dy)| map.inside(x + dx, y + dy) && covered(idx(x + dx, y + dy) as usize));
                        if touch {
                            ring.push((y - wy0) * ww + (x - wx0));
                        }
                    }
                }
                let local_ok = ring.first().is_some_and(|&r0| {
                    let seen = flood4(ww, wh, &[r0], |k| {
                        let (x, y) = (k as i32 % ww + wx0, k as i32 / ww + wy0);
                        free(idx(x, y) as usize)
                    });
                    ring.iter().all(|r| seen[*r as usize] != 0)
                });
                let mut lost_land = 0usize;
                for &c in &cells {
                    lost_land += 1;
                    let s = (c + W) as usize;
                    if self.map.land[s] == 1 && !m3.get(s as i32 % W - ox, s as i32 / W - oy) {
                        lost_land += 1;
                    }
                }
                if local_ok {
                    reach = reach.saturating_sub(lost_land);
                } else {
                    let after = flood4(W, H, &[b0], free).iter().filter(|v| **v != 0).count();
                    if after + lost_land + 30 < reach {
                        continue;
                    }
                    reach = after;
                }
            }
            area += cells.len();
            // commit: level, cliffs, spacing halo
            for &c in &cells {
                self.map.level[c as usize] = 1;
                self.hill_of[c as usize] = hills.len() as u8 + 1;
            }
            for &c in &cells {
                let s = (c + W) as usize;
                if self.hill_of[s] != hills.len() as u8 + 1 {
                    // on water this is a cliff dropping straight into the sea (wet-foot rock face)
                    self.map.cliff[s] = 1;
                }
                let (x, y) = (c % W, c / W);
                for yy in y - 3..=y + 3 {
                    for xx in x - 3..=x + 3 {
                        if self.map.inside(xx, yy) {
                            let k = idx(xx, yy) as usize;
                            self.near[k] = 1;
                        }
                    }
                }
            }
            hills.push(Hill {
                cells,
                stairs: Vec::new(),
                top: None,
                anchor: (ax, ay),
            });
        }

        // ---- stairs from the main lowland ----
        let low = {
            let map = &self.map;
            flood4(W, H, &[b0], |k| map.land[k] == 1 && map.level[k] == 0 && map.cliff[k] == 0)
        };
        let mut kept: Vec<Hill> = Vec::new();
        for (hi, mut h) in hills.into_iter().enumerate() {
            let id = hi as u8 + 1;
            let hill_of = &self.hill_of;
            let inm = |k: i32| k >= 0 && (k as usize) < hill_of.len() && hill_of[k as usize] == id;
            let map = &self.map;
            let reserved = &self.reserved;
            let stairzone = &self.stairzone;
            let ground = |k: i32| {
                let k = k as usize;
                map.land[k] == 1
                    && map.level[k] == 0
                    && map.cliff[k] == 0
                    && map.stair[k] == 0
                    && reserved[k] == 0
                    && stairzone[k] == 0
                    && low[k] != 0
            };
            let mut spots = self.stair_spots(&inm, &h.cells, &ground);
            let n = h.cells.len();
            let count = if n < 70 { 1 } else if n < 200 { 2 } else if n < 420 { 3 } else { 4 };
            let put = self.raise_stairs(1, &mut spots, count);
            if put.is_empty() {
                // no access: flatten it back
                for &c in &h.cells {
                    self.map.level[c as usize] = 0;
                    self.hill_of[c as usize] = 0;
                    let s = (c + W) as usize;
                    self.map.cliff[s] = 0;
                }
                // the cliffs of this hill only (spacing guarantees no sharing)
                continue;
            }
            h.stairs = put;
            kept.push(h);
        }
        // renumber hill_of to the kept order
        self.hill_of.iter_mut().for_each(|v| *v = 0);
        for (i, h) in kept.iter().enumerate() {
            for &c in &h.cells {
                self.hill_of[c as usize] = i as u8 + 1;
            }
        }

        // ---- terraces (level 2) ----
        for i in 0..kept.len() {
            let n = kept[i].cells.len();
            let p = if n >= 220 { 0.85 } else if n >= 110 { 0.6 } else { 0.0 };
            if p == 0.0 || !self.chance(p) {
                continue;
            }
            if let Some(top) = self.terrace(&kept[i], i as u8 + 1) {
                kept[i].top = Some(top);
            }
        }

        // ---- unreachable pockets (step 9) ----
        let walk = crate::path::compute_field(&self.map, &[b0], 0, false);
        for c in 0..self.map.len() {
            if self.map.land[c] == 1 && self.map.cliff[c] == 0 && walk[c] == crate::defs::FIELD_INF {
                self.reserved[c] = 1;
                self.map.no_build[c] = 1;
            }
        }

        // ---- anchors and order by distance to the bases' centroid ----
        let (sx, sy) = self.bases.iter().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        let mid = (sx as f64 / BASE_COUNT as f64, sy as f64 / BASE_COUNT as f64);
        for h in kept.iter_mut() {
            let (mut cx, mut cy) = (0.0, 0.0);
            for &c in &h.cells {
                cx += (c % W) as f64;
                cy += (c / W) as f64;
            }
            cx /= h.cells.len() as f64;
            cy /= h.cells.len() as f64;
            let mut bd = f64::INFINITY;
            for &c in &h.cells {
                let k = c as usize;
                if self.map.cliff[k] == 0 && self.map.no_build[k] == 0 && self.map.level[k] == 1 {
                    let (x, y) = ((c % W) as f64, (c / W) as f64);
                    let d = (x - cx) * (x - cx) + (y - cy) * (y - cy);
                    if d < bd {
                        bd = d;
                        h.anchor = (c % W, c / W);
                    }
                }
            }
        }
        kept.sort_by(|a, b| {
            let d = |h: &Hill| {
                let (dx, dy) = (h.anchor.0 as f64 - mid.0, h.anchor.1 as f64 - mid.1);
                dx * dx + dy * dy
            };
            let (da, db) = (d(a), d(b));
            da.total_cmp(&db)
        });
        self.hill_of.iter_mut().for_each(|v| *v = 0);
        for (i, h) in kept.iter().enumerate() {
            let id = i as u8 + 1;
            for &c in &h.cells {
                self.hill_of[c as usize] = id;
                self.map.plateau[c as usize] = id;
                let s = (c + W) as usize;
                if self.map.cliff[s] != 0 {
                    self.map.plateau[s] = id;
                }
            }
            for s in h.stairs.iter().chain(h.top.iter().flat_map(|t| t.1.iter())) {
                self.map.plateau[s.hi as usize] = id;
                self.map.plateau[s.lo as usize] = id;
            }
            self.plateaus.push(crate::map::PlateauInfo {
                id,
                landing: self.map.xy(h.stairs[0].landing()),
                cells: h.cells.len() as u32,
                top: h.top.as_ref().map(|t| self.map.xy(t.1[0].landing())),
            });
        }
        self.hills = kept;
        self.hills.len() >= 6
    }

    /// A level-2 terrace inside hill `id` (02 §5.8 `inner` rule: 2 cells of plateau around it, 3 below), with
    /// stairs from level-1 ground reachable from the hill's own stairs. Reverted if it has no usable stair.
    fn terrace(&mut self, h: &Hill, id: u8) -> Option<(Vec<i32>, Vec<Stair>)> {
        let (x0, y0, x1, y1) = h.bbox();
        let (ox, oy) = (x0 - 1, y0 - 1);
        let mut pm = Mask::new(x1 - x0 + 3, y1 - y0 + 3);
        for &c in &h.cells {
            pm.set(c % W - ox, c / W - oy, true);
        }
        let mut inner = pm.erode(2, 2, 2, 3);
        for y in 0..inner.h {
            for x in 0..inner.w {
                if inner.get(x, y) {
                    let k = idx(x + ox, y + oy) as usize;
                    if self.stairzone[k] != 0 || self.map.no_build[k] != 0 {
                        inner.set(x, y, false);
                    }
                }
            }
        }
        let inner_cells: Vec<(i32, i32)> = (0..inner.h)
            .flat_map(|y| (0..inner.w).map(move |x| (x, y)))
            .filter(|(x, y)| inner.get(*x, *y))
            .collect();
        if inner_cells.len() < 12 {
            return None;
        }
        // terrace size grows with the plateau (a massif carries a real upper tier, a plateau a small knoll)
        let big = h.cells.len() >= 400;
        for _ in 0..30 {
            let (px, py) = inner_cells[self.below(inner_cells.len() as u32) as usize];
            let mut tm = Mask::new(inner.w, inner.h);
            let nr = if big { 2 + self.below(2) } else { 1 + self.below(2) };
            for k in 0..nr {
                let (w, hh) = if big {
                    (6 + self.below(9), 3 + self.below(4))
                } else {
                    (3 + self.below(7), 2 + self.below(4))
                };
                let (ax, ay) = if k == 0 {
                    (px, py)
                } else if big {
                    (px + self.below(9) - 4, py + self.below(5) - 2)
                } else {
                    (px + self.below(5) - 2, py + self.below(3) - 1)
                };
                let rx = ax - self.below(w as u32);
                let ry = ay - self.below(hh as u32);
                for y in ry..ry + hh {
                    for x in rx..rx + w {
                        tm.set(x, y, true);
                    }
                }
            }
            tm.and(&inner);
            let mut tm = tm.open(3);
            let n = tm.keep_largest();
            if n < 10 {
                continue;
            }
            let cells: Vec<i32> = (0..tm.h)
                .flat_map(|y| (0..tm.w).map(move |x| (x, y)))
                .filter(|(x, y)| tm.get(*x, *y))
                .map(|(x, y)| (y + oy) * W + (x + ox))
                .collect();
            // tentatively raise + cliffs
            let mark = 200u8;
            for &c in &cells {
                self.map.level[c as usize] = 2;
                self.hill_of[c as usize] = mark;
            }
            let mut cliffs = Vec::new();
            for &c in &cells {
                let s = c + W;
                if self.hill_of[s as usize] != mark {
                    self.map.cliff[s as usize] = 1;
                    cliffs.push(s);
                }
            }
            // level-1 ground of this hill reachable from its stair landings
            let starts: Vec<i32> = h.stairs.iter().map(|s| s.landing()).collect();
            let reach = {
                let map = &self.map;
                let hill_of = &self.hill_of;
                flood4(W, H, &starts, |k| hill_of[k] == id && map.level[k] == 1 && map.cliff[k] == 0)
            };
            let hill_of = &self.hill_of;
            let inm = |k: i32| k >= 0 && (k as usize) < hill_of.len() && hill_of[k as usize] == mark;
            let map = &self.map;
            let stairzone = &self.stairzone;
            let ground = |k: i32| {
                let k = k as usize;
                hill_of[k] == id
                    && map.level[k] == 1
                    && map.cliff[k] == 0
                    && map.stair[k] == 0
                    && stairzone[k] == 0
                    && reach[k] != 0
            };
            let mut spots = self.stair_spots(&inm, &cells, &ground);
            if spots.is_empty() {
                for &c in &cells {
                    self.map.level[c as usize] = 1;
                    self.hill_of[c as usize] = id;
                }
                for &s in &cliffs {
                    self.map.cliff[s as usize] = 0;
                }
                continue;
            }
            let count = if cells.len() > 30 { 2 } else { 1 };
            let put = self.raise_stairs(2, &mut spots, count);
            for &c in &cells {
                self.hill_of[c as usize] = id;
            }
            if put.is_empty() {
                // every spot was refused by `bridge_ok`: a terrace without a stair would be an unreachable level
                // (and `plateaus[].top` needs a first stair) — revert it
                for &c in &cells {
                    self.map.level[c as usize] = 1;
                }
                for &s in &cliffs {
                    self.map.cliff[s as usize] = 0;
                }
                continue;
            }
            // terrace stair `hi` cells belong to the hill too
            return Some((cells, put));
        }
        None
    }
}
