//! Resources (02 §5.12 + addenda W4): fair per-base start stock, mineral sites on the plateaus, capture points with
//! their stock, big deposits, scattered forests / groves / gold / stone / meadows.
//!
//! Economy rules are the JS ones (same group sizes, radii, counts, strict/non-strict placement, `near()` directions).
//! Only *where* scattered groups go is aesthetic: forests prefer the edges of the lowland (coasts, cliff feet,
//! plateau rims), gold and stone the cliff feet, sheep meadows the calm open grass; every group is one species
//! chosen by region (pines with pines, birches with birches).

use super::{Gen, Group, H, W, idx};
use crate::defs::*;
use crate::map::{GenDeposit, GenPoint, GenTree};

/// `(cos, sin)` of the base-stock angles 0.2, 1.9, -2.3, -1.05, 0.85, -0.25 (no transcendental calls).
const ROT: [(f64, f64); 6] = [
    (0.980_066_577_841_241_6, 0.198_669_330_795_061_2),
    (-0.323_289_566_863_503_6, 0.946_300_087_687_414_4),
    (-0.666_276_021_279_824_3, -0.745_705_212_176_720_2),
    (0.497_571_047_891_727_1, -0.867_423_225_594_017),
    (0.659_983_145_884_982_1, 0.751_280_405_140_292_9),
    (0.968_912_421_710_644_8, -0.247_403_959_254_522_6),
];
/// `near()` turns `0, ±0.4, ±0.8, ±1.3, ±1.9, ±2.5, 3.14` as `(cos, sin)`.
const TURNS: [(f64, f64); 12] = [
    (1.0, 0.0),
    (0.921_060_994_002_885_1, 0.389_418_342_308_650_5),
    (0.921_060_994_002_885_1, -0.389_418_342_308_650_5),
    (0.696_706_709_347_165_4, 0.717_356_090_899_522_8),
    (0.696_706_709_347_165_4, -0.717_356_090_899_522_8),
    (0.267_498_828_624_587_2, 0.963_558_185_417_192_9),
    (0.267_498_828_624_587_2, -0.963_558_185_417_192_9),
    (-0.323_289_566_863_503_6, 0.946_300_087_687_414_4),
    (-0.323_289_566_863_503_6, -0.946_300_087_687_414_4),
    (-0.801_143_615_546_933_7, 0.598_472_144_103_956_5),
    (-0.801_143_615_546_933_7, -0.598_472_144_103_956_5),
    (-0.999_998_731_727_539_8, 0.001_592_652_916_486_828),
];

/// Tree sprite variants: 0 dark pine, 1 bright pine, 2 yellow-green birch, 3 autumn birch.
pub const PINE: [u8; 2] = [0, 1];
pub const BIRCH: [u8; 2] = [2, 3];

impl Gen {
    /// Species pair `(dominant, other)` for a group centred at `(x, y)` (region-coherent).
    pub fn species(&mut self, x: f64, y: f64) -> (u8, u8) {
        let pine = self.pine_at(x, y);
        let pair = if pine { PINE } else { BIRCH };
        if self.chance(0.5) { (pair[0], pair[1]) } else { (pair[1], pair[0]) }
    }

    /// Pine region: low-frequency noise, stronger toward the map border (framing forests, ref. §11.6/11).
    pub fn pine_at(&self, x: f64, y: f64) -> bool {
        let nx = (x - W as f64 / 2.0) / (W as f64 / 2.0);
        let ny = (y - H as f64 / 2.0) / (H as f64 / 2.0);
        let border = nx.abs().max(ny.abs());
        self.noise.at(x * 0.035 + 41.0, y * 0.035 + 13.0) + border * 0.3 > 0.78
    }

    /// `scatter(cx, cy, n, r, ok)`: up to n cells in n*14 tries. `soft` = triangular offsets (denser centre,
    /// rounded clump) instead of the uniform square.
    pub fn scatter(&mut self, cx: f64, cy: f64, n: usize, r: f64, soft: bool, ok: &dyn Fn(&Gen, i32) -> bool) -> Vec<i32> {
        let mut out = Vec::new();
        let mut t = 0;
        while t < n * 14 && out.len() < n {
            t += 1;
            let (ox, oy) = if soft {
                (
                    (self.rnd() + self.rnd() - 1.0) * r * 1.35,
                    (self.rnd() + self.rnd() - 1.0) * r * 1.2,
                )
            } else {
                ((self.rnd() * 2.0 - 1.0) * r, (self.rnd() * 2.0 - 1.0) * r)
            };
            let x = (cx + ox).round() as i32;
            let y = (cy + oy).round() as i32;
            if x >= 2 && y >= 2 && x < W - 2 && y < H - 2 {
                let c = idx(x, y);
                if ok(self, c) {
                    self.used[c as usize] = 1;
                    out.push(c);
                }
            }
        }
        out
    }

    /// Compact organic clump for forests (same budget as `scatter`: up to n cells in n*14 tries, within radius r
    /// of the centre): starts at the free cell nearest to `(cx, cy)` and grows from random members, mostly to
    /// touching cells, sometimes leaving a one-cell gap — overlapping crowns, a rounded outline.
    pub fn grow(&mut self, cx: f64, cy: f64, n: usize, r: f64, ok: &dyn Fn(&Gen, i32) -> bool) -> Vec<i32> {
        let (x0, y0) = (cx.round() as i32, cy.round() as i32);
        let mut start = -1;
        'find: for rr in 0..=2i32 {
            for dy in -rr..=rr {
                for dx in -rr..=rr {
                    if dx.abs().max(dy.abs()) != rr {
                        continue;
                    }
                    let (x, y) = (x0 + dx, y0 + dy);
                    if x >= 2 && y >= 2 && x < W - 2 && y < H - 2 && ok(self, idx(x, y)) {
                        start = idx(x, y);
                        break 'find;
                    }
                }
            }
        }
        let mut out = Vec::new();
        if start < 0 {
            return out;
        }
        self.used[start as usize] = 1;
        out.push(start);
        let (sx, sy) = (start % W, start / W);
        let r2 = r * r + 0.5;
        let mut t = 0;
        while t < n * 14 && out.len() < n {
            t += 1;
            let base = out[self.below(out.len() as u32) as usize];
            let (bx, by) = (base % W, base / W);
            let step = if self.chance(0.08) { 2 } else { 1 };
            let (dx, dy) = crate::defs::DIRS8[self.below(8) as usize];
            let (x, y) = (bx + dx * step, by + dy * step);
            let (ox, oy) = ((x - sx) as f64, (y - sy) as f64);
            if ox * ox + oy * oy > r2 || x < 2 || y < 2 || x >= W - 2 || y >= H - 2 {
                continue;
            }
            let c = idx(x, y);
            if ok(self, c) {
                self.used[c as usize] = 1;
                out.push(c);
            }
        }
        out
    }

    fn push_trees(&mut self, cells: &[i32], sp: (u8, u8), mix: f64) {
        for &c in cells {
            let v = if self.chance(mix) { sp.1 } else { sp.0 };
            self.trees.push(GenTree { cell: c, variant: v });
        }
    }

    /// `place(kind, px, py, ok, strict)` (02 §5.12).
    pub fn place(&mut self, kind: Group, px: f64, py: f64, ok: &dyn Fn(&Gen, i32) -> bool, strict: bool) -> bool {
        match kind {
            Group::Forest | Group::Grove => {
                let (n, r) = if kind == Group::Grove {
                    (20 + self.below(8) as usize, 4.4)
                } else {
                    (8 + self.below(4) as usize, 2.8)
                };
                let cells = self.grow(px, py, n, r, ok);
                if strict && cells.len() < 5 {
                    for c in cells {
                        self.used[c as usize] = 0;
                    }
                    return false;
                }
                // groves are the dense pine woods on their teal floor (ref. §11.6); forests follow the region
                let sp = if let Some(sp) = self.force_sp.take() {
                    sp
                } else if kind == Group::Grove {
                    if self.chance(0.5) { (PINE[0], PINE[1]) } else { (PINE[1], PINE[0]) }
                } else {
                    self.species(px, py)
                };
                let mix = if sp.0 < 2 { 0.2 } else { 0.35 };
                self.push_trees(&cells, sp, mix);
                if !cells.is_empty() {
                    self.woods.push((px as f32, py as f32, r as f32));
                    self.wood_sp.push(sp);
                }
                cells.len() >= 5
            }
            Group::Gold | Group::Stone => {
                let n = if kind == Group::Gold { 3 } else { 2 };
                let cells = self.scatter(px, py, n, 1.6, false, ok);
                if strict && cells.len() < 2 {
                    for c in cells {
                        self.used[c as usize] = 0;
                    }
                    return false;
                }
                for &c in &cells {
                    if kind == Group::Gold {
                        self.golds.push(GenDeposit { cell: c, amount: GOLD_MAX, big: false });
                    } else {
                        self.stones.push(GenDeposit { cell: c, amount: STONE_MAX, big: false });
                    }
                }
                cells.len() >= 2
            }
            Group::Meadow => {
                let cells = self.scatter(px, py, 1, 1.5, false, ok);
                let Some(&c) = cells.first() else {
                    return false;
                };
                self.used[c as usize] = 0;
                let p = self.map.center(c);
                self.meadows.push(p);
                true
            }
        }
    }

    /// Per-base start stock, same relative directions toward the map centre (`near()`, 02 §5.12).
    pub fn base_stock(&mut self) {
        const PLAN: [(Group, usize, f64, f64); 6] = [
            (Group::Meadow, 0, 2.0, 0.0),
            (Group::Forest, 1, 2.5, 0.0),
            (Group::Forest, 2, 2.5, 0.0),
            (Group::Gold, 3, 1.5, 0.0),
            (Group::Gold, 4, 1.5, 5.0),
            (Group::Stone, 5, 1.5, 0.0),
        ];
        let bases = self.bases.clone();
        for &(bx, by) in &bases {
            let (dx, dy) = (W as f64 / 2.0 - bx as f64, H as f64 / 2.0 - by as f64);
            let l = (dx * dx + dy * dy).sqrt();
            let (ux, uy) = if l < 1e-6 { (1.0, 0.0) } else { (dx / l, dy / l) };
            for &(kind, rot, pad, far) in &PLAN {
                let (c0, s0) = ROT[rot];
                let (ax, ay) = (ux * c0 - uy * s0, ux * s0 + uy * c0);
                'turns: for &(ct, st) in &TURNS {
                    let (ca, sa) = (ax * ct - ay * st, ax * st + ay * ct);
                    let edge = (9.0 / ca.abs().max(1e-6)).min(7.0 / sa.abs().max(1e-6));
                    let d0 = edge + pad + far;
                    let mut d = d0;
                    while d <= d0.max(12.5 + far) {
                        // cells stay inside the radius `sound()` counts them in (BASE_STOCK: 14 near, 22 far;
                        // meadows 14), so a fair start never depends on luck
                        let r2 = match kind {
                            Group::Meadow => 13 * 13,
                            Group::Forest | Group::Grove => 16 * 16,
                            _ if far > 0.0 => 21 * 21,
                            _ => 13 * 13,
                        };
                        let ok = move |g: &Gen, c: i32| {
                            g.free_low(c) && {
                                let (x, y) = (c % W - bx, c / W - by);
                                x * x + y * y <= r2
                            }
                        };
                        if self.place(kind, bx as f64 + ca * d, by as f64 + sa * d, &ok, true) {
                            break 'turns;
                        }
                        d += 1.0;
                    }
                }
            }
        }
    }

    /// Mineral sites (addenda W4): site 0 on the plateau nearest the bases' centroid = 3 gold groups + 1 stone,
    /// sites 1..15 = 1 gold + 1 stone, one per plateau in distance order; extra sites on the largest plateaus
    /// (≥ 6 cells from other sites) when there are fewer than 16. Then 3-6 trees per ~90 elevated cells, as clumps
    /// on the plateau rims and corners.
    pub fn plateau_stock(&mut self) {
        let nh = self.hills.len();
        if nh == 0 {
            return;
        }
        let mut sites: Vec<(usize, i32, i32)> = Vec::new();
        for i in 0..16usize {
            if i < nh {
                let (x, y) = self.hills[i].anchor;
                sites.push((i, x, y));
                continue;
            }
            // extra site: the hill with the most cells per site, free cell farthest from its sites
            let mut best_h = 0;
            let mut best_r = -1.0;
            for (h, hill) in self.hills.iter().enumerate() {
                let k = sites.iter().filter(|s| s.0 == h).count() as f64 + 1.0;
                let r = hill.cells.len() as f64 / k;
                if r > best_r {
                    best_r = r;
                    best_h = h;
                }
            }
            let mut best = None;
            let mut bd = -1;
            for _ in 0..60 {
                let len = self.hills[best_h].cells.len() as u32;
                let pick = self.below(len) as usize;
                let c = self.hills[best_h].cells[pick];
                if !self.free_high(c) {
                    continue;
                }
                let (x, y) = (c % W, c / W);
                let d = sites
                    .iter()
                    .filter(|s| s.0 == best_h)
                    .map(|s| (s.1 - x).abs().max((s.2 - y).abs()))
                    .min()
                    .unwrap_or(99);
                if d > bd {
                    bd = d;
                    best = Some((x, y));
                }
            }
            match best {
                Some((x, y)) if bd >= 6 => sites.push((best_h, x, y)),
                _ => break,
            }
        }
        for (k, &(h, x, y)) in sites.iter().enumerate() {
            let (hx, hy) = (x as f64, y as f64);
            if k == 0 {
                for ox in [-3.0, 0.0, 3.0] {
                    self.site_group(Group::Gold, h, hx + ox, hy);
                }
                self.site_group(Group::Stone, h, hx, hy + 2.5);
            } else {
                self.site_group(Group::Gold, h, hx, hy);
                self.site_group(Group::Stone, h, hx + 3.0, hy + 1.0);
            }
        }
        // rim clumps
        for h in 0..nh {
            let id = h as u8 + 1;
            let cells = self.hills[h].cells.clone();
            let want = ((3 + self.below(4)) as f64 * (cells.len() as f64 / 90.0).max(1.0)).round() as usize;
            let rim: Vec<i32> = cells
                .iter()
                .copied()
                .filter(|&c| {
                    let (x, y) = (c % W, c / W);
                    let mut out = 0;
                    for (dx, dy) in DIRS8 {
                        if self.hill_of[idx(x + dx, y + dy) as usize] != id {
                            out += 1;
                        }
                    }
                    out >= 1
                })
                .collect();
            if rim.is_empty() {
                continue;
            }
            let mut placed = 0;
            let mut tries = 0;
            while placed < want && tries < 40 {
                tries += 1;
                let c = rim[self.below(rim.len() as u32) as usize];
                if !self.free_high(c) {
                    continue;
                }
                let (x, y) = (c % W, c / W);
                let n = (3 + self.below(3) as usize).min(want - placed);
                let ok = move |g: &Gen, k: i32| g.hill_of[k as usize] == id && g.free_high(k) && g.rim_dist(k, id) <= 2;
                let got = self.scatter(x as f64, y as f64, n, 1.6, true, &ok);
                if got.is_empty() {
                    continue;
                }
                let sp = self.species(x as f64, y as f64);
                self.push_trees(&got, sp, 0.35);
                placed += got.len();
            }
            // every plateau holds at least 3 trees (02 §6.6): a crowded rim falls back to any free cell of the hill,
            // rim cells first
            if placed < 3 {
                let mut cand: Vec<i32> = rim.iter().copied().filter(|&c| self.free_high(c)).collect();
                cand.extend(cells.iter().copied().filter(|&c| self.free_high(c) && !rim.contains(&c)));
                if !cand.is_empty() {
                    let (x, y) = (cand[0] % W, cand[0] / W);
                    let sp = self.species(x as f64, y as f64);
                    let mut got = Vec::new();
                    for c in cand {
                        if placed + got.len() >= 3 {
                            break;
                        }
                        if self.hill_of[c as usize] == id && self.free_high(c) {
                            self.used[c as usize] = 1;
                            got.push(c);
                        }
                    }
                    self.push_trees(&got, sp, 0.35);
                }
            }
        }
    }

    /// One gold / stone group of a mineral site on hill `h` at `(x, y)` (non-strict, as the JS); if not a single
    /// node fits there (crowded anchor), the group goes to the free cells of the hill nearest the site, so every
    /// site really holds its ore (W4).
    fn site_group(&mut self, kind: Group, h: usize, x: f64, y: f64) {
        let id = h as u8 + 1;
        let ok = move |g: &Gen, c: i32| g.hill_of[c as usize] == id && g.free_high(c);
        let count = |g: &Gen| if kind == Group::Gold { g.golds.len() } else { g.stones.len() };
        let before = count(self);
        self.place(kind, x, y, &ok, false);
        if count(self) > before {
            return;
        }
        let mut cand: Vec<(i64, i32)> = self.hills[h]
            .cells
            .iter()
            .copied()
            .filter(|&c| ok(self, c))
            .map(|c| {
                let (dx, dy) = ((c % W) as f64 - x, (c / W) as f64 - y);
                (((dx * dx + dy * dy) * 16.0) as i64, c)
            })
            .collect();
        cand.sort_unstable();
        for &(_, c) in cand.iter().take(12) {
            self.place(kind, (c % W) as f64, (c / W) as f64, &ok, false);
            if count(self) > before {
                return;
            }
        }
    }

    /// Chebyshev distance (≤ 3) from `c` to the outside of hill `id`.
    fn rim_dist(&self, c: i32, id: u8) -> i32 {
        let (x, y) = (c % W, c / W);
        for r in 1..=3 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if (dx as i32).abs().max((dy as i32).abs()) != r {
                        continue;
                    }
                    if !self.map.inside(x + dx, y + dy) || self.hill_of[idx(x + dx, y + dy) as usize] != id {
                        return r - 1;
                    }
                }
            }
        }
        3
    }

    /// `spread(fromBase, gap)` (02 §5.12): best of 80 roomy low spots (2x2 + 1-cell margin all free) by distance
    /// to the marks; marks the quad used.
    fn spread(&mut self, marks: &mut Vec<(i32, i32)>, from_base: f64, gap: f64) -> i32 {
        let roomy = |g: &Gen, x: i32, y: i32| -> bool {
            if !(3..=W - 5).contains(&x) || !(3..=H - 6).contains(&y) {
                return false;
            }
            (-1..=2).all(|dy| (-1..=2).all(|dx| g.free_low(idx(x + dx, y + dy))))
        };
        let mut best = -1;
        let mut bd = -1.0;
        for _ in 0..80 {
            let x = 4 + self.below((W - 8) as u32);
            let y = 4 + self.below((H - 9) as u32);
            if !roomy(self, x, y) {
                continue;
            }
            if self
                .bases
                .iter()
                .any(|b| (((b.0 - x).pow(2) + (b.1 - y).pow(2)) as f64) <= from_base * from_base)
            {
                continue;
            }
            let d = marks
                .iter()
                .map(|m| (((m.0 - x).pow(2) + (m.1 - y).pow(2)) as f64).sqrt())
                .fold(f64::INFINITY, f64::min);
            if d > bd {
                bd = d;
                best = idx(x, y);
            }
        }
        if best < 0 || bd < gap {
            return -1;
        }
        let (x, y) = self.map.xy(best);
        marks.push((x, y));
        for c in self.map.quad(best) {
            self.used[c as usize] = 1;
        }
        best
    }

    /// Capture points (≈ 36·rich, `POINT_MIX` by attempt index, 4x4 `noBuild`, `POINT_STOCK` groups beside them)
    /// and big deposits (≈ 20·rich, alternating gold 1500 / stone 1800).
    pub fn points_and_bigs(&mut self) {
        let mut marks = self.bases.clone();
        let np = (9.0 * 4.0 * self.rich).round() as usize;
        for i in 0..np {
            let c = self.spread(&mut marks, POINT_BASE_GAP as f64, POINT_MARK_GAP as f64);
            if c < 0 {
                continue;
            }
            let kind = POINT_MIX[i % 6];
            self.points.push(GenPoint { cell: c, kind });
            let (x, y) = self.map.xy(c);
            for yy in y - 1..=y + 2 {
                for xx in x - 1..=x + 2 {
                    let k = idx(xx, yy) as usize;
                    self.map.no_build[k] = 1;
                }
            }
            let group = match kind {
                PointKind::Mine => Some(Group::Gold),
                PointKind::Quarry => Some(Group::Stone),
                PointKind::Sawmill => Some(Group::Forest),
                PointKind::Ranch => Some(Group::Meadow),
                PointKind::Outpost => None,
            };
            if let Some(gr) = group {
                // the two JS spots (east, west; non-strict), then other sides of the point until two groups stand:
                // every mine / quarry / sawmill / ranch must carry its stock (02 §6.7)
                const SPOTS: [(f64, f64, bool); 8] = [
                    (4.5, 0.0, false),
                    (-4.5, 1.0, false),
                    (0.5, 4.5, true),
                    (0.5, -3.5, true),
                    (4.5, 4.0, true),
                    (-4.5, -3.0, true),
                    (4.5, -3.0, true),
                    (-4.5, 4.0, true),
                ];
                let (px, py) = (x as f64 + 1.0, y as f64 + 1.0);
                let mut done = 0;
                for (ox, oy, strict) in SPOTS {
                    if done >= 2 {
                        break;
                    }
                    if self.place(gr, px + ox, py + oy, &|g, c| g.free_low(c), strict) {
                        done += 1;
                    }
                }
            }
        }
        let nb = (5.0 * 4.0 * self.rich).round() as usize;
        for i in 0..nb {
            let c = self.spread(&mut marks, BIG_BASE_GAP as f64, BIG_MARK_GAP as f64);
            if c >= 0 {
                let (kind, amount) = if i % 2 == 1 {
                    (NodeKind::Stone, BIG_STONE)
                } else {
                    (NodeKind::Gold, BIG_GOLD)
                };
                self.bigs.push((kind, GenDeposit { cell: c, amount, big: true }));
            }
        }
    }

    /// Scattered groups ≥ 14 cells from every base (02 §5.12 counts × rich × 4), placed with an aesthetic
    /// acceptance weight on the distance to the lowland's edges.
    pub fn scattered(&mut self) {
        for (kind, count) in [
            (Group::Grove, 3.0),
            (Group::Forest, 26.0),
            (Group::Gold, 9.0),
            (Group::Stone, 10.0),
            (Group::Meadow, 10.0),
        ] {
            let target = (count * self.rich * 4.0).round() as usize;
            let mut placed = 0;
            let mut t = 0;
            while t < 4000 && placed < target {
                t += 1;
                // most groups extend an existing wood (touching it): a few big compact woods of dozens of trees
                // instead of many small dots, and calm open meadows between them (MAPGEN §6)
                let woody = matches!(kind, Group::Forest | Group::Grove) && self.woods.len() >= 3 && self.chance(0.8);
                let mut sp = None;
                let (x, y) = if woody {
                    let wi = self.below(self.woods.len() as u32) as usize;
                    sp = self.wood_sp.get(wi).copied();
                    let (wx, wy, wr) = self.woods[wi];
                    let (dx, dy) = DIRS8[self.below(8) as usize];
                    let d = wr as f64 * 0.9 + 1.0 + self.rnd() * 1.5;
                    let (x, y) = (wx as f64 + dx as f64 * d, wy as f64 + dy as f64 * d);
                    if x < 4.0 || y < 4.0 || x >= (W - 4) as f64 || y >= (H - 4) as f64 {
                        continue;
                    }
                    (x, y)
                } else {
                    (4.0 + self.rnd() * (W - 8) as f64, 4.0 + self.rnd() * (H - 8) as f64)
                };
                let c = idx(x as i32, y as i32) as usize;
                let e = self.edge0[c];
                let w = match kind {
                    Group::Forest | Group::Grove => match e {
                        0 => 0.0,
                        1..=2 => 0.8,
                        3..=7 => 1.0,
                        8..=11 => 0.45,
                        _ => 0.15,
                    },
                    Group::Gold | Group::Stone => match e {
                        0 => 0.0,
                        1..=4 => 1.0,
                        _ => 0.3,
                    },
                    Group::Meadow => {
                        if e >= 5 {
                            1.0
                        } else {
                            0.25
                        }
                    }
                };
                if self.rnd() >= w {
                    continue;
                }
                if self.far_from_bases(x, y) {
                    // a group extending a wood keeps its species (pines with pines, birches with birches)
                    self.force_sp = sp;
                    let ok = self.place(kind, x, y, &|g, c| g.free_low(c), false);
                    self.force_sp = None;
                    if ok {
                        placed += 1;
                    }
                }
            }
        }
    }
}
