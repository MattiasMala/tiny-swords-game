//! Continent shape, kingdom sites, ponds and islets (02 §5.4-§5.7, §5.10 redesigned per MAPGEN.md §1, §5).
//!
//! The coastline is designed on a coarse 4x4-cell grid (rounded superellipse + low-frequency value noise, carved by
//! gulfs and inner seas), then enlarged with *run-jittered* edges: every vertical edge is shifted by -1/0/+1 cells in
//! runs of 3-8 rows (and horizontal edges likewise), so outlines are long straight runs with occasional one-cell
//! steps — the rectilinear tiers of the reference. A 3x3 closing + opening removes one-cell spurs, notches and
//! holes; tiny land/water components are dropped.

use super::grid::{Mask, Sat, cheb_dist, components4, flood4};
use super::{Gen, H, W, idx};
use crate::defs::{BASE_CLEAR, BASE_COUNT, BASE_GAP, BASE_LAND};

/// Coarse cell size (fine cells).
pub const COARSE: i32 = 4;

/// `inMargin(x, y)`: land never touches the outer 2 rows/columns (3 at the bottom) — 02 addenda W6.
#[inline]
pub fn in_margin(x: i32, y: i32) -> bool {
    x >= 2 && y >= 2 && x < W - 2 && y < H - 3
}

impl Gen {
    /// Piecewise-constant offsets in {-1, 0, +1} with runs of 3..=8 cells (`len` values).
    fn jitter_runs(&mut self, len: usize, amp: f64) -> Vec<i8> {
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            let run = 3 + self.below(6) as usize;
            let r = self.rnd();
            let v: i8 = if r < amp * 0.5 {
                -1
            } else if r < amp {
                1
            } else {
                0
            };
            for _ in 0..run {
                if out.len() < len {
                    out.push(v);
                }
            }
        }
        out
    }

    /// Enlarges a coarse mask (`cw x ch`, cell = `COARSE` fine cells) to the full map with run-jittered edges.
    /// `amp` = share of runs shifted by one cell.
    pub fn upscale(&mut self, coarse: &[u8], cw: i32, ch: i32, amp: f64) -> Mask {
        // region partitions offset by 2 so a region border never coincides with a coarse edge (multiple of 4)
        let rx = ((W + 2) / 16 + 1) as usize;
        let ry = ((H + 2) / 16 + 1) as usize;
        let hx: Vec<Vec<i8>> = (0..rx).map(|_| self.jitter_runs(H as usize, amp)).collect();
        let vy: Vec<Vec<i8>> = (0..ry).map(|_| self.jitter_runs(W as usize, amp)).collect();
        let mut m = Mask::new(W, H);
        for y in 0..H {
            for x in 0..W {
                let ox = hx[((x + 2) / 16) as usize][y as usize] as i32;
                let oy = vy[((y + 2) / 16) as usize][x as usize] as i32;
                let cx = (x + ox).div_euclid(COARSE).clamp(0, cw - 1);
                let cy = (y + oy).div_euclid(COARSE).clamp(0, ch - 1);
                m.v[(y * W + x) as usize] = coarse[(cy * cw + cx) as usize];
            }
        }
        m
    }

    /// Coarse continent (02 §5.4 redesigned, ref. §11.1/§11.11): a union of rectangles in 4-cell blocks — a core
    /// plus lobes and thin peninsulas grown outward from the coast until 58-68 % of the map is land — minus bays cut
    /// into the outline and inner seas; boundary blocks are then toggled at random (notches and bumps of 4 cells).
    fn coarse_continent(&mut self) -> (Vec<u8>, i32, i32) {
        let cw = W / COARSE;
        let ch = H / COARSE;
        let (bx0, bx1, by0, by1) = (1, cw - 2, 1, ch - 2);
        let mut cm = vec![0u8; (cw * ch) as usize];
        let fill = |cm: &mut Vec<u8>, x0: i32, y0: i32, w: i32, h: i32, v: u8| {
            for y in y0.max(by0)..(y0 + h).min(by1 + 1) {
                for x in x0.max(bx0)..(x0 + w).min(bx1 + 1) {
                    cm[(y * cw + x) as usize] = v;
                }
            }
        };
        // core
        let w = 28 + self.below(14);
        let h = 16 + self.below(10);
        let x0 = cw / 2 - w / 2 + self.below(9) - 4;
        let y0 = ch / 2 - h / 2 + self.below(7) - 3;
        fill(&mut cm, x0, y0, w, h, 1);
        let (mx, my) = (cw as f64 / 2.0, ch as f64 / 2.0);
        let boundary = |cm: &Vec<u8>, land: bool| -> Vec<(i32, i32)> {
            let mut out = Vec::new();
            for y in by0..=by1 {
                for x in bx0..=bx1 {
                    let v = cm[(y * cw + x) as usize] != 0;
                    if v != land {
                        continue;
                    }
                    let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                        let (nx, ny) = (x + dx, y + dy);
                        nx < 0 || ny < 0 || nx >= cw || ny >= ch || (cm[(ny * cw + nx) as usize] != 0) != v
                    });
                    if edge {
                        out.push((x, y));
                    }
                }
            }
            out
        };
        let target = 0.58 + self.rnd() * 0.10;
        let total = (cw * ch) as f64;
        for _ in 0..80 {
            let frac = cm.iter().filter(|v| **v != 0).count() as f64 / total;
            if frac >= target {
                break;
            }
            let edge = boundary(&cm, true);
            if edge.is_empty() {
                break;
            }
            let (ax, ay) = edge[self.below(edge.len() as u32) as usize];
            let thin = self.chance(0.3);
            let (w, h) = if thin {
                if self.chance(0.5) { (2 + self.below(2), 5 + self.below(6)) } else { (5 + self.below(7), 2 + self.below(2)) }
            } else {
                (4 + self.below(10), 3 + self.below(7))
            };
            // outward: the anchor sits near the inner side of the new rectangle
            let (dx, dy) = (ax as f64 - mx, ay as f64 - my);
            let x0 = if dx >= 0.0 { ax - self.below((w / 3).max(1) as u32) } else { ax - w + 1 + self.below((w / 3).max(1) as u32) };
            let y0 = if dy >= 0.0 { ay - self.below((h / 3).max(1) as u32) } else { ay - h + 1 + self.below((h / 3).max(1) as u32) };
            fill(&mut cm, x0, y0, w, h, 1);
        }
        // bays cut into the outline
        let bays = 3 + self.below(5);
        for _ in 0..bays {
            let edge = boundary(&cm, true);
            if edge.is_empty() {
                break;
            }
            let (ax, ay) = edge[self.below(edge.len() as u32) as usize];
            let w = 2 + self.below(5);
            let h = 2 + self.below(4);
            let (dx, dy) = (ax as f64 - mx, ay as f64 - my);
            let x0 = if dx >= 0.0 { ax - w / 2 } else { ax - w / 2 - 1 } + if dx.abs() > dy.abs() { (dx.signum() as i32) * (w / 3) } else { 0 };
            let y0 = ay - h / 2 + if dy.abs() >= dx.abs() { (dy.signum() as i32) * (h / 3) } else { 0 };
            fill(&mut cm, x0, y0, w, h, 0);
        }
        // inner seas
        let seas = self.below(3);
        for _ in 0..seas {
            for _ in 0..40 {
                let x = bx0 + 4 + self.below((bx1 - bx0 - 8).max(1) as u32);
                let y = by0 + 4 + self.below((by1 - by0 - 8).max(1) as u32);
                let all = (-4..=4).all(|dy| (-4..=4).all(|dx| cm[((y + dy) * cw + x + dx) as usize] != 0));
                if all {
                    let w = 3 + self.below(4);
                    let h = 2 + self.below(3);
                    fill(&mut cm, x - w / 2, y - h / 2, w, h, 0);
                    break;
                }
            }
        }
        // notches and bumps along the outline
        for land in [true, false] {
            let edge = boundary(&cm, land);
            for (x, y) in edge {
                if self.chance(0.1) {
                    cm[(y * cw + x) as usize] = (!land) as u8;
                }
            }
        }
        // isolated single blocks out
        for y in by0..=by1 {
            for x in bx0..=bx1 {
                let i = (y * cw + x) as usize;
                let v = cm[i];
                let same = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .filter(|(dx, dy)| cm[((y + dy) * cw + x + dx) as usize] == v)
                    .count();
                if same == 0 {
                    cm[i] = 1 - v;
                }
            }
        }
        (cm, cw, ch)
    }

    /// Margin + 3x3 closing/opening + removal of tiny components (land < `min_land`, water holes < 14).
    fn clean_land(&self, m: &mut Mask, min_land: u32) {
        for y in 0..H {
            for x in 0..W {
                if !in_margin(x, y) {
                    m.v[(y * W + x) as usize] = 0;
                }
            }
        }
        *m = m.close(3).open(3);
        // drop tiny land bits
        let (lab, sizes) = m.components();
        for (v, l) in m.v.iter_mut().zip(&lab) {
            if *l >= 0 && sizes[*l as usize] < min_land {
                *v = 0;
            }
        }
        // fill tiny water holes (not connected to the border sea)
        let water: Vec<u8> = m.v.iter().map(|v| (*v == 0) as u8).collect();
        let (lab, sizes) = components4(&water, W, H);
        for (i, l) in lab.iter().enumerate() {
            if *l >= 0 && sizes[*l as usize] < 40 {
                let (x, y) = (i as i32 % W, i as i32 / W);
                if in_margin(x, y) {
                    m.v[i] = 1;
                }
            }
        }
    }

    /// Steps 2-7: continent, bases (≥ `BASE_GAP` apart, land share > 0.92 on 23x19), forced base land,
    /// mainland = land 4-connected to base 0. Returns false if the sites cannot be chosen.
    pub fn shape_land(&mut self) -> bool {
        let (cm, cw, ch) = self.coarse_continent();
        let amp = 0.3 + self.rnd() * 0.2;
        let mut m = self.upscale(&cm, cw, ch, amp);
        self.clean_land(&mut m, 120);

        // ---- kingdom sites (02 §5.6): farthest of a handful of spots with land share > 0.92 ----
        let sat = Sat::new(&m.v, W, H);
        let mut spots: Vec<(i32, i32)> = Vec::new();
        let mut y = BASE_CLEAR.1 + 7;
        while y <= H - BASE_CLEAR.1 - 9 {
            let mut x = BASE_CLEAR.0 + 7;
            while x <= W - BASE_CLEAR.0 - 8 {
                let share = sat.sum(x - 11, y - 9, x + 11, y + 9) as f64 / (23.0 * 19.0);
                if share > 0.92 {
                    spots.push((x, y));
                }
                x += 2;
            }
            y += 2;
        }
        if spots.is_empty() {
            return false;
        }
        let first = spots[self.below(spots.len() as u32) as usize];
        self.bases.push(first);
        while self.bases.len() < BASE_COUNT {
            let mut best = None;
            let mut bd = -1.0;
            let mut t = 0;
            while t < 400 && (t < 14 || bd < BASE_GAP as f64) {
                t += 1;
                let s = spots[self.below(spots.len() as u32) as usize];
                let d = self
                    .bases
                    .iter()
                    .map(|b| (((b.0 - s.0).pow(2) + (b.1 - s.1).pow(2)) as f64).sqrt())
                    .fold(f64::INFINITY, f64::min);
                if d > bd {
                    bd = d;
                    best = Some(s);
                }
            }
            match best {
                Some(s) if bd >= BASE_GAP as f64 => self.bases.push(s),
                _ => return false,
            }
        }

        // ---- force land on BASE_LAND, reserve BASE_CLEAR ----
        let bases = self.bases.clone();
        for &(bx, by) in &bases {
            for y in by - BASE_LAND.1..=by + BASE_LAND.1 {
                for x in bx - BASE_LAND.0..=bx + BASE_LAND.0 {
                    if in_margin(x, y) {
                        m.v[(y * W + x) as usize] = 1;
                    }
                }
            }
        }
        // re-clean the junctions, keeping the forced rectangles
        self.clean_land(&mut m, 120);
        for &(bx, by) in &bases {
            for y in by - BASE_LAND.1..=by + BASE_LAND.1 {
                for x in bx - BASE_LAND.0..=bx + BASE_LAND.0 {
                    if in_margin(x, y) {
                        m.v[(y * W + x) as usize] = 1;
                    }
                }
            }
            for y in by - BASE_CLEAR.1..=by + BASE_CLEAR.1 {
                for x in bx - BASE_CLEAR.0..=bx + BASE_CLEAR.0 {
                    if in_margin(x, y) {
                        self.reserved[(y * W + x) as usize] = 1;
                    }
                }
            }
        }

        // ---- mainland: 4-connected to base 0; detached pieces become small islets or sea ----
        let start = idx(bases[0].0, bases[0].1);
        let main = flood4(W, H, &[start], |k| m.v[k] != 0);
        if bases.iter().any(|b| main[idx(b.0, b.1) as usize] == 0) {
            return false;
        }
        let (lab, sizes) = m.components();
        for i in 0..m.v.len() {
            self.map.land[i] = if main[i] != 0 {
                1
            } else if m.v[i] != 0 && lab[i] >= 0 && sizes[lab[i] as usize] <= 60 {
                2
            } else {
                0
            };
        }
        // detached islets too close to the mainland would read as part of it: drop those within 1 cell
        let mainland: Vec<u8> = self.map.land.iter().map(|v| (*v == 1) as u8).collect();
        let dm = cheb_dist(&mainland, W, H, 8);
        for i in 0..self.map.land.len() {
            if self.map.land[i] == 2 && dm[i] < 2 {
                self.map.land[i] = 0;
            }
        }
        true
    }

    /// Inland ponds (02 §5.7 redesigned): small rectilinear pools (3-6 x 2-4 with clipped corners) well inside the
    /// mainland, away from bases and each other. Ponds are impassable water (`land 0`).
    pub fn ponds(&mut self) {
        let mainland: Vec<u8> = self.map.land.iter().map(|v| (*v != 1) as u8).collect();
        let coast = cheb_dist(&mainland, W, H, 20);
        let want = 7 + self.below(7) as usize;
        let mut tries = 0;
        while self.ponds.len() < want && tries < 1500 {
            tries += 1;
            let w = 3 + self.below(4);
            let h = 2 + self.below(3);
            let x0 = 6 + self.below((W - 12 - w) as u32);
            let y0 = 6 + self.below((H - 12 - h) as u32);
            let ok = (y0 - 3..y0 + h + 3).all(|y| {
                (x0 - 3..x0 + w + 3).all(|x| {
                    let c = idx(x, y) as usize;
                    self.map.land[c] == 1 && self.reserved[c] == 0 && coast[c] >= 4
                })
            }) && self.bases.iter().all(|b| {
                let (dx, dy) = (b.0 - (x0 + w / 2), b.1 - (y0 + h / 2));
                dx.abs() > BASE_CLEAR.0 + 6 || dy.abs() > BASE_CLEAR.1 + 6
            });
            let px = x0 as f64 + w as f64 / 2.0;
            let py = y0 as f64 + h as f64 / 2.0;
            if !ok
                || self
                    .ponds
                    .iter()
                    .any(|p| (p.0 as f64 - px) * (p.0 as f64 - px) + (p.1 as f64 - py) * (p.1 as f64 - py) < 22.0 * 22.0)
            {
                continue;
            }
            // clipped corners (rounded look by the tile art), a one-cell bump on a long side
            let cut = [self.chance(0.7), self.chance(0.7), self.chance(0.7), self.chance(0.7)];
            for y in y0..y0 + h {
                for x in x0..x0 + w {
                    let corner = (x == x0 || x == x0 + w - 1) && (y == y0 || y == y0 + h - 1);
                    if corner && w >= 4 && h >= 3 {
                        let k = (x != x0) as usize + 2 * (y != y0) as usize;
                        if cut[k] {
                            continue;
                        }
                    }
                    let c = idx(x, y) as usize;
                    self.map.land[c] = 0;
                }
            }
            if w >= 5 && self.chance(0.5) {
                let bx = x0 + 1 + self.below((w - 2) as u32);
                let by = if self.chance(0.5) { y0 - 1 } else { y0 + h };
                let c = idx(bx, by) as usize;
                self.map.land[c] = 0;
                let c2 = idx(bx + 1, by) as usize;
                self.map.land[c2] = 0;
            }
            self.ponds.push((px as f32, py as f32));
        }
    }

    /// Islets (02 §5.10 redesigned, MAPGEN §5): 1x1, 2x1, 3x1, 3x2, 2x2, 4x2, 3x3 rocks of grass 2-4 cells off the
    /// coast (a water channel always separates them). Taller ones are raised: top rows level 1, bottom row a cliff.
    /// `land = 2` (scenery, never walkable). Returns the islet rectangles for decoration.
    pub fn islets(&mut self) -> Vec<(i32, i32, i32, i32)> {
        let ground: Vec<u8> = self.map.land.iter().map(|v| (*v != 0) as u8).collect();
        let dist = cheb_dist(&ground, W, H, 12);
        let mainland: Vec<u8> = self.map.land.iter().map(|v| (*v == 1) as u8).collect();
        let dmain = cheb_dist(&mainland, W, H, 12);
        const SHAPES: [(i32, i32); 8] = [(1, 1), (2, 1), (3, 1), (3, 2), (2, 2), (4, 2), (3, 3), (1, 1)];
        let want = 16 + self.below(14) as usize;
        let mut out: Vec<(i32, i32, i32, i32)> = Vec::new();
        let mut tries = 0;
        while out.len() < want && tries < 4000 {
            tries += 1;
            let (w, h) = SHAPES[self.below(SHAPES.len() as u32) as usize];
            let x0 = 4 + self.below((W - 8 - w) as u32);
            let y0 = 4 + self.below((H - 9 - h) as u32);
            let mut ok = true;
            let mut near = false;
            for y in y0 - 1..y0 + h + 1 {
                for x in x0 - 1..x0 + w + 1 {
                    let c = idx(x, y) as usize;
                    if dist[c] < 2 && (x >= x0 && x < x0 + w && y >= y0 && y < y0 + h) {
                        ok = false;
                    }
                    if ground[c] != 0 {
                        ok = false;
                    }
                }
            }
            for y in y0..y0 + h {
                for x in x0..x0 + w {
                    let c = idx(x, y) as usize;
                    if dmain[c] <= 4 {
                        near = true;
                    }
                }
            }
            if !ok || !near {
                continue;
            }
            let (cx, cy) = (x0 * 2 + w, y0 * 2 + h);
            if out
                .iter()
                .any(|o| ((o.0 * 2 + o.2) - cx).abs() < 20 && ((o.1 * 2 + o.3) - cy).abs() < 20)
            {
                continue;
            }
            let raised = h >= 2 && w >= 2 && self.chance(0.45);
            for y in y0..y0 + h {
                for x in x0..x0 + w {
                    let c = idx(x, y) as usize;
                    self.map.land[c] = 2;
                    if raised {
                        if y < y0 + h - 1 {
                            self.map.level[c] = 1;
                        } else {
                            self.map.cliff[c] = 1;
                        }
                    }
                }
            }
            out.push((x0, y0, w, h));
        }
        out
    }
}
