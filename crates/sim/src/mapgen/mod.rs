// OWNER: S1 (map) — see docs/spec/02-world.md §5 (pipeline), §6 (generation invariants the new generator MUST keep),
// addenda W2-W6; 09-asset-inventory.md §3.6, §3.7, §11 (HD-2D aesthetics: tiers, colour by tier, cliffs on south
// edges, ramps at cliff ends, clustered trees on edges, bushes/rocks as transition glue, calm open spaces).
// Also sim/examples/mapgen_png.rs.
//
// The foundation wrote a SIMPLE but valid placeholder so client and sim can run before S1 delivers the real
// aesthetic generator: rounded island from value noise, 5 bases on a jittered pentagon (≥ 64 cells apart), a few
// level-1 plateaus with stairs and resources, the per-base start stock of `BASE_STOCK`, capture points, big
// deposits, scattered groups, chests/camps/landmarks, decor and surfaces, pruning and the `sound()` acceptance test.
// S1 replaces `generate` entirely (keep its signature and `GeneratedWorld`).
//
// DETERMINISM (client regenerates from the seed and compares `Map::hash`): only + - * / and sqrt on floats here —
// NO sin/cos/atan2/powf/exp/ln (they may differ between native and wasm); rotations use literal cos/sin pairs.
#![allow(dead_code, unused)]

use crate::defs::*;
use crate::map::{
    Decor, DecorKind, GenChest, GenDeposit, GenPoint, GenTree, GeneratedWorld, H, Map, PlateauInfo,
    Surface, W,
};
use crate::math::{V2, v2};
use crate::path::compute_field;
use crate::rng::Mulberry32;
use crate::world::MatchSetup;

/// `'Generazione della mappa fallita (seme ' + seed + ')'` (spec 02 §5.1) — only for logs: the placeholder never
/// fails (the last attempt is accepted unchecked).
pub fn failure_text(seed: u32) -> String {
    format!("Generazione della mappa fallita (seme {seed})")
}

/// Deterministic map generation from the match seed (spec 02 §5.1 driver: retries with `seed + attempt*7919`).
pub fn generate(seed: u32, setup: &MatchSetup) -> GeneratedWorld {
    generate_report(seed, setup).0
}

/// [`generate`] plus the number of attempts used and whether the `sound()` acceptance test passed (false only if
/// all 40 attempts failed and the last map was accepted unchecked). For tests and `examples/mapgen_png.rs`.
pub fn generate_report(seed: u32, setup: &MatchSetup) -> (GeneratedWorld, u32, bool) {
    for attempt in 0..40u32 {
        if let Some(g) = build(seed, seed.wrapping_add(attempt.wrapping_mul(7919)), true) {
            return (g, attempt + 1, true);
        }
    }
    let g =
        build(seed, seed.wrapping_add(40 * 7919), false).expect("unchecked build always succeeds");
    (g, 41, false)
}

/// `(cos, sin)` of the angles used by the base-stock directions (literal values: no transcendental calls).
const ROT: [(f64, f64); 6] = [
    (0.980_066_6, 0.198_669_3),   // 0.2   meadow
    (-0.323_289_6, 0.946_300_1),  // 1.9   forest
    (-0.666_276_0, -0.745_705_2), // -2.3  forest
    (0.497_571_2, -0.867_423_2),  // -1.05 gold
    (0.659_983_1, 0.751_280_4),   // 0.85  gold (far)
    (0.968_912_4, -0.247_404_0),  // -0.25 stone
];
/// fallback turns `0, ±0.4, ±0.8, ±1.3` as (cos, sin)
const TURNS: [(f64, f64); 7] = [
    (1.0, 0.0),
    (0.921_061_0, 0.389_418_3),
    (0.921_061_0, -0.389_418_3),
    (0.696_706_7, 0.717_356_1),
    (0.696_706_7, -0.717_356_1),
    (0.267_498_8, 0.963_558_2),
    (0.267_498_8, -0.963_558_2),
];
/// pentagon template (cells from the map centre): angles 90°, 18°, 306°, 234°, 162° on an ellipse 95..100 x 60
const PENTAGON: [(f64, f64); 5] = [
    (0.0, 60.0),
    (95.1, 18.5),
    (58.8, -48.5),
    (-58.8, -48.5),
    (-95.1, 18.5),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Forest,
    Grove,
    Gold,
    Stone,
    Meadow,
}

struct Gen {
    rng: Mulberry32,
    map: Map,
    used: Vec<u8>,
    reserved: Vec<u8>,
    bases: Vec<(i32, i32)>,
    trees: Vec<GenTree>,
    golds: Vec<GenDeposit>,
    stones: Vec<GenDeposit>,
    bigs: Vec<(NodeKind, GenDeposit)>,
    meadows: Vec<V2>,
    points: Vec<GenPoint>,
    chests: Vec<GenChest>,
    plateaus: Vec<PlateauInfo>,
    lattice: Vec<f64>,
}

impl Gen {
    fn rnd(&mut self) -> f64 {
        self.rng.next_f64()
    }
    fn below(&mut self, n: u32) -> i32 {
        self.rng.below(n) as i32
    }
    fn idx(&self, x: i32, y: i32) -> i32 {
        y * W + x
    }
    fn in_margin(x: i32, y: i32) -> bool {
        x >= 2 && y >= 2 && x < W - 2 && y < H - 3
    }
    /// value noise on a 32x32 wrapping lattice, smoothstep bilinear (pure arithmetic)
    fn noise(&self, x: f64, y: f64) -> f64 {
        let (xf, yf) = (x.floor(), y.floor());
        let (fx, fy) = (x - xf, y - yf);
        let (xi, yi) = (xf as i64 & 31, yf as i64 & 31);
        let l = |a: i64, b: i64| self.lattice[((b & 31) * 32 + (a & 31)) as usize];
        let (a, b, c, d) = (l(xi, yi), l(xi + 1, yi), l(xi, yi + 1), l(xi + 1, yi + 1));
        let u = fx * fx * (3.0 - 2.0 * fx);
        let w = fy * fy * (3.0 - 2.0 * fy);
        a + (b - a) * u + (c - a) * w + (a - b - c + d) * u * w
    }
    fn free_low(&self, c: i32) -> bool {
        let i = c as usize;
        self.map.land[i] == 1
            && self.map.level[i] == 0
            && self.map.cliff[i] == 0
            && self.map.stair[i] == 0
            && self.reserved[i] == 0
            && self.map.no_build[i] == 0
            && self.used[i] == 0
    }
    fn free_high(&self, c: i32) -> bool {
        let i = c as usize;
        self.map.land[i] == 1
            && self.map.level[i] != 0
            && self.map.cliff[i] == 0
            && self.map.stair[i] == 0
            && self.map.no_build[i] == 0
            && self.used[i] == 0
    }
    fn far_from_bases(&self, x: f64, y: f64) -> bool {
        self.bases.iter().all(|b| {
            let (dx, dy) = (b.0 as f64 - x, b.1 as f64 - y);
            dx * dx + dy * dy > FAR_FROM_BASES as f64 * FAR_FROM_BASES as f64
        })
    }

    /// `scatter(cx, cy, n, r, ok)`: up to n cells in n*14 tries.
    fn scatter(
        &mut self,
        cx: f64,
        cy: f64,
        n: usize,
        r: f64,
        ok: &dyn Fn(&Gen, i32) -> bool,
    ) -> Vec<i32> {
        let mut out = Vec::new();
        let mut t = 0;
        while t < n * 14 && out.len() < n {
            t += 1;
            let x = (cx + (self.rnd() * 2.0 - 1.0) * r).round() as i32;
            let y = (cy + (self.rnd() * 2.0 - 1.0) * r).round() as i32;
            if x >= 2 && y >= 2 && x < W - 2 && y < H - 2 {
                let c = self.idx(x, y);
                if ok(self, c) {
                    self.used[c as usize] = 1;
                    out.push(c);
                }
            }
        }
        out
    }

    /// `place(kind, px, py, ok, strict)` (spec 02 §5.12).
    fn place(
        &mut self,
        kind: Group,
        px: f64,
        py: f64,
        ok: &dyn Fn(&Gen, i32) -> bool,
        strict: bool,
    ) -> bool {
        match kind {
            Group::Forest | Group::Grove => {
                let (n, r) = if kind == Group::Grove {
                    (20 + self.below(8) as usize, 4.4)
                } else {
                    (8 + self.below(4) as usize, 2.8)
                };
                let cells = self.scatter(px, py, n, r, ok);
                if strict && cells.len() < 5 {
                    for c in cells {
                        self.used[c as usize] = 0;
                    }
                    return false;
                }
                let ok_n = cells.len() >= 5;
                for c in cells {
                    let v = self.below(4) as u8;
                    self.trees.push(GenTree {
                        cell: c,
                        variant: v,
                    });
                }
                ok_n
            }
            Group::Gold | Group::Stone => {
                let n = if kind == Group::Gold { 3 } else { 2 };
                let cells = self.scatter(px, py, n, 1.6, ok);
                if strict && cells.len() < 2 {
                    for c in cells {
                        self.used[c as usize] = 0;
                    }
                    return false;
                }
                let ok_n = cells.len() >= 2;
                for c in cells {
                    if kind == Group::Gold {
                        self.golds.push(GenDeposit {
                            cell: c,
                            amount: GOLD_MAX,
                            big: false,
                        });
                    } else {
                        self.stones.push(GenDeposit {
                            cell: c,
                            amount: STONE_MAX,
                            big: false,
                        });
                    }
                }
                ok_n
            }
            Group::Meadow => {
                let cells = self.scatter(px, py, 1, 1.5, ok);
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

    fn solid_prop(&mut self, prop: PropKind, c: i32) {
        self.used[c as usize] = 1;
        let p = self.map.center(c);
        self.map.decor.push(Decor {
            kind: DecorKind::Prop(prop),
            x: p.x,
            y: p.y + 20.0,
            variant: 0,
            phase: 0.0,
            scale: 1.0,
            solid: true,
        });
    }

    fn chest(&mut self, c: i32) {
        self.used[c as usize] = 1;
        let res = Res::ALL[self.chests.len() % 4];
        self.chests.push(GenChest { cell: c, res });
    }

    /// random free low cell far from bases (`spot()`), -1 if none in 300 tries
    fn spot(&mut self, test: &dyn Fn(&Gen, i32) -> bool) -> i32 {
        for _ in 0..300 {
            let x = 3 + self.below((W - 6) as u32);
            let y = 3 + self.below((H - 6) as u32);
            let c = self.idx(x, y);
            if self.far_from_bases(x as f64, y as f64) && test(self, c) {
                return c;
            }
        }
        -1
    }

    /// `spread(fromBase, gap)` for points and bigs; marks are bases + previous picks.
    fn spread(&mut self, marks: &mut Vec<(i32, i32)>, from_base: f64, gap: f64) -> i32 {
        let roomy = |g: &Gen, x: i32, y: i32| -> bool {
            if !(3..=W - 5).contains(&x) || !(3..=H - 6).contains(&y) {
                return false;
            }
            (-1..=2).all(|dy| (-1..=2).all(|dx| g.free_low(g.idx(x + dx, y + dy))))
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
                .any(|b| ((b.0 - x).pow(2) + (b.1 - y).pow(2)) as f64 <= from_base * from_base)
            {
                continue;
            }
            let d = marks
                .iter()
                .map(|m| (((m.0 - x).pow(2) + (m.1 - y).pow(2)) as f64).sqrt())
                .fold(f64::INFINITY, f64::min);
            if d > bd {
                bd = d;
                best = self.idx(x, y);
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
}

#[inline]
fn sq(v: f64) -> f64 {
    v * v
}

fn build(seed: u32, s: u32, check: bool) -> Option<GeneratedWorld> {
    let mut rng = Mulberry32::new(s);
    let lattice: Vec<f64> = (0..1024).map(|_| rng.next_f64()).collect();
    let n = (W * H) as usize;
    let mut g = Gen {
        rng,
        map: Map::new(),
        used: vec![0; n],
        reserved: vec![0; n],
        bases: Vec::new(),
        trees: Vec::new(),
        golds: Vec::new(),
        stones: Vec::new(),
        bigs: Vec::new(),
        meadows: Vec::new(),
        points: Vec::new(),
        chests: Vec::new(),
        plateaus: Vec::new(),
        lattice,
    };

    // ---- island: rounded rectangle (superellipse exponent 4) + value-noise wobble ----
    let grain = 0.07 + g.rnd() * 0.05;
    let rough = 0.25 + g.rnd() * 0.2;
    for y in 0..H {
        for x in 0..W {
            if !Gen::in_margin(x, y) {
                continue;
            }
            let nx = (x as f64 + 0.5 - W as f64 / 2.0) / (W as f64 / 2.0 - 2.5);
            let ny = (y as f64 + 0.5 - H as f64 / 2.0) / (H as f64 / 2.0 - 2.5);
            let d = nx * nx * nx * nx + ny * ny * ny * ny;
            if d + (g.noise(x as f64 * grain, y as f64 * grain) - 0.5) * rough < 0.72 {
                let c = g.idx(x, y) as usize;
                g.map.land[c] = 1;
            }
        }
    }
    for _ in 0..2 {
        for y in 1..H - 1 {
            for x in 1..W - 1 {
                let c = g.idx(x, y) as usize;
                let nb = [c - 1, c + 1, c - W as usize, c + W as usize]
                    .iter()
                    .filter(|k| g.map.land[**k] == 1)
                    .count();
                if g.map.land[c] == 1 && nb < 2 {
                    g.map.land[c] = 0;
                } else if g.map.land[c] == 0 && nb >= 3 && Gen::in_margin(x, y) {
                    g.map.land[c] = 1;
                }
            }
        }
    }

    // ---- bases: jittered pentagon, random mirror ----
    let fx = if g.rnd() < 0.5 { -1.0 } else { 1.0 };
    let fy = if g.rnd() < 0.5 { -1.0 } else { 1.0 };
    let order_shift = g.below(5) as usize;
    for i in 0..BASE_COUNT {
        let (px, py) = PENTAGON[(i + order_shift) % 5];
        let jx = g.below(9) as f64 - 4.0;
        let jy = g.below(9) as f64 - 4.0;
        let bx = (W as f64 / 2.0 + px * fx + jx).round() as i32;
        let by = (H as f64 / 2.0 + py * fy + jy).round() as i32;
        g.bases.push((bx, by));
    }
    for i in 0..g.bases.len() {
        for j in 0..i {
            let (a, b) = (g.bases[i], g.bases[j]);
            if ((((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f64).sqrt()) < BASE_GAP as f64 {
                return None;
            }
        }
    }
    let bases = g.bases.clone();
    for &(bx, by) in &bases {
        for y in by - BASE_LAND.1..=by + BASE_LAND.1 {
            for x in bx - BASE_LAND.0..=bx + BASE_LAND.0 {
                if Gen::in_margin(x, y) {
                    let c = g.idx(x, y) as usize;
                    g.map.land[c] = 1;
                }
            }
        }
        for y in by - BASE_CLEAR.1..=by + BASE_CLEAR.1 {
            for x in bx - BASE_CLEAR.0..=bx + BASE_CLEAR.0 {
                if Gen::in_margin(x, y) {
                    let c = g.idx(x, y) as usize;
                    g.reserved[c] = 1;
                }
            }
        }
    }
    // keep only land 4-connected to base 0
    {
        let start = g.idx(bases[0].0, bases[0].1);
        let mut seen = vec![false; n];
        let mut stack = vec![start];
        seen[start as usize] = true;
        while let Some(c) = stack.pop() {
            let (x, y) = g.map.xy(c);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                if !g.map.inside(x + dx, y + dy) {
                    continue;
                }
                let k = g.idx(x + dx, y + dy);
                if !seen[k as usize] && g.map.land[k as usize] == 1 {
                    seen[k as usize] = true;
                    stack.push(k);
                }
            }
        }
        for c in 0..n {
            if !seen[c] {
                g.map.land[c] = 0;
            }
        }
    }

    // ---- plateaus: rectangles raised to level 1, cliff on the south edge, stairs at the south corners ----
    let keep = |bases: &[(i32, i32)], x: i32, y: i32| {
        bases
            .iter()
            .any(|b| x >= b.0 - 13 && x <= b.0 + 13 && y >= b.1 - 11 && y <= b.1 + 12)
    };
    let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
    let want = 4 + g.below(3) as usize;
    let mut tries = 0;
    while rects.len() < want && tries < 400 {
        tries += 1;
        let w = 9 + g.below(7);
        let h = 6 + g.below(4);
        let x0 = 6 + g.below((W - 12 - w) as u32);
        let y0 = 6 + g.below((H - 14 - h) as u32);
        // footprint + 3-cell margin (south 4) must be mainland, away from bases and other plateaus
        let ok = (y0 - 3..y0 + h + 4).all(|y| {
            (x0 - 3..x0 + w + 3).all(|x| {
                g.map.inside(x, y) && g.map.land[g.idx(x, y) as usize] == 1 && !keep(&bases, x, y)
            })
        }) && rects.iter().all(|r| {
            x0 + w + 4 < r.0 || r.0 + r.2 + 4 < x0 || y0 + h + 5 < r.1 || r.1 + r.3 + 5 < y0
        });
        if !ok {
            continue;
        }
        rects.push((x0, y0, w, h));
    }
    let mid = {
        let (sx, sy) = bases
            .iter()
            .fold((0i32, 0i32), |a, b| (a.0 + b.0, a.1 + b.1));
        (
            sx as f64 / bases.len() as f64,
            sy as f64 / bases.len() as f64,
        )
    };
    rects.sort_by(|a, b| {
        let da =
            sq(a.0 as f64 + a.2 as f64 / 2.0 - mid.0) + sq(a.1 as f64 + a.3 as f64 / 2.0 - mid.1);
        let db =
            sq(b.0 as f64 + b.2 as f64 / 2.0 - mid.0) + sq(b.1 as f64 + b.3 as f64 / 2.0 - mid.1);
        da.total_cmp(&db)
    });
    for (i, &(x0, y0, w, h)) in rects.iter().enumerate() {
        let id = (i % 4 + 1) as u8;
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let c = g.idx(x, y) as usize;
                g.map.level[c] = 1;
                g.map.plateau[c] = id;
            }
        }
        let yb = y0 + h - 1;
        for x in x0..x0 + w {
            let c = g.idx(x, yb + 1) as usize;
            g.map.cliff[c] = 1;
            g.map.plateau[c] = id;
        }
        // stairs beside the south-west and south-east corners: hi (level 1) at row yb, lo below it
        let mut landing = None;
        for (sx, dir) in [(x0 - 1, -1), (x0 + w, 1)] {
            let hi = g.idx(sx, yb);
            let lo = g.idx(sx, yb + 1);
            g.map.stair[lo as usize] = if dir < 0 { 1 } else { 3 };
            g.map.stair[hi as usize] = if dir < 0 { 2 } else { 4 };
            g.map.level[hi as usize] = 1;
            g.map.plateau[hi as usize] = id;
            g.map.plateau[lo as usize] = id;
            for y in yb - 1..=yb + 2 {
                for x in sx - 1..=sx + 1 {
                    if Gen::in_margin(x, y) {
                        let c = g.idx(x, y) as usize;
                        g.map.no_build[c] = 1;
                    }
                }
            }
            if landing.is_none() {
                landing = Some((sx - dir, yb));
            }
        }
        g.plateaus.push(PlateauInfo {
            id,
            landing: landing.unwrap_or((x0, yb)),
            cells: (w * h) as u32,
            top: None,
        });
    }

    // ---- unreachable pockets → reserved + noBuild ----
    let goal0 = g.idx(bases[0].0, bases[0].1 + 2);
    let walk = compute_field(&g.map, &[goal0], 0, false);
    for c in 0..n {
        if g.map.land[c] == 1 && g.map.cliff[c] == 0 && walk[c] == FIELD_INF {
            g.reserved[c] = 1;
            g.map.no_build[c] = 1;
        }
    }

    // ---- per-base start stock, same relative directions toward the map centre (spec 02 §5.12 `near`) ----
    let plan: [(Group, usize, f64, f64); 6] = [
        (Group::Meadow, 0, 2.0, 0.0),
        (Group::Forest, 1, 2.5, 0.0),
        (Group::Forest, 2, 2.5, 0.0),
        (Group::Gold, 3, 1.5, 0.0),
        (Group::Gold, 4, 1.5, 5.0),
        (Group::Stone, 5, 1.5, 0.0),
    ];
    for &(bx, by) in &bases {
        let (dx, dy) = (W as f64 / 2.0 - bx as f64, H as f64 / 2.0 - by as f64);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (ux, uy) = (dx / l, dy / l);
        for &(kind, rot, pad, far) in &plan {
            let (c0, s0) = ROT[rot];
            let (ax, ay) = (ux * c0 - uy * s0, ux * s0 + uy * c0);
            'turns: for &(ct, st) in &TURNS {
                let (ca, sa) = (ax * ct - ay * st, ax * st + ay * ct);
                let edge = (9.0 / ca.abs().max(1e-6)).min(7.0 / sa.abs().max(1e-6));
                let d0 = edge + pad + far;
                let mut d = d0;
                while d <= d0.max(12.5 + far) {
                    if g.place(
                        kind,
                        bx as f64 + ca * d,
                        by as f64 + sa * d,
                        &|g, c| g.free_low(c),
                        true,
                    ) {
                        break 'turns;
                    }
                    d += 1.0;
                }
            }
        }
    }

    // ---- plateau resources: nearest to the bases' centroid = 3 gold groups; others 1 gold + 1 stone; trees ----
    for (i, &(x0, y0, w, h)) in rects.clone().iter().enumerate() {
        let (cx, cy) = (x0 as f64 + w as f64 / 2.0, y0 as f64 + h as f64 / 2.0);
        let inside = move |g: &Gen, c: i32| {
            let (x, y) = g.map.xy(c);
            x >= x0 && x < x0 + w && y >= y0 && y < y0 + h - 1 && g.free_high(c)
        };
        if i == 0 {
            for ox in [-3.0, 0.0, 3.0] {
                g.place(Group::Gold, cx + ox, cy - 1.0, &inside, false);
            }
            g.place(Group::Stone, cx, cy + 1.5, &inside, false);
        } else if i < 16 {
            g.place(Group::Gold, cx - 2.0, cy, &inside, false);
            g.place(Group::Stone, cx + 2.0, cy + 1.0, &inside, false);
        }
        let nt = 3 + g.below(4) as usize;
        let cells = g.scatter(cx, cy, nt, (w.min(h) as f64) / 2.0, &inside);
        for c in cells {
            let v = g.below(4) as u8;
            g.trees.push(GenTree {
                cell: c,
                variant: v,
            });
        }
    }

    // ---- capture points and big deposits ----
    let mut marks = bases.clone();
    for i in 0..12 {
        let c = g.spread(&mut marks, POINT_BASE_GAP as f64, POINT_MARK_GAP as f64);
        if c < 0 {
            continue;
        }
        let kind = POINT_MIX[i % 6];
        g.points.push(GenPoint { cell: c, kind });
        let (x, y) = g.map.xy(c);
        for yy in y - 1..=y + 2 {
            for xx in x - 1..=x + 2 {
                let k = g.idx(xx, yy) as usize;
                g.map.no_build[k] = 1;
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
            let (px, py) = (x as f64 + 1.0, y as f64 + 1.0);
            g.place(gr, px + 4.5, py, &|g, c| g.free_low(c), false);
            g.place(gr, px - 4.5, py + 1.0, &|g, c| g.free_low(c), false);
        }
    }
    for i in 0..6 {
        let c = g.spread(&mut marks, BIG_BASE_GAP as f64, BIG_MARK_GAP as f64);
        if c >= 0 {
            let (kind, amount) = if i % 2 == 1 {
                (NodeKind::Stone, BIG_STONE)
            } else {
                (NodeKind::Gold, BIG_GOLD)
            };
            g.bigs.push((
                kind,
                GenDeposit {
                    cell: c,
                    amount,
                    big: true,
                },
            ));
        }
    }

    // ---- scattered groups ----
    for (kind, count) in [
        (Group::Grove, 3),
        (Group::Forest, 26),
        (Group::Gold, 8),
        (Group::Stone, 8),
        (Group::Meadow, 8),
    ] {
        let mut placed = 0;
        let mut t = 0;
        while t < 600 && placed < count {
            t += 1;
            let x = 4.0 + g.rnd() * (W - 8) as f64;
            let y = 4.0 + g.rnd() * (H - 8) as f64;
            if g.far_from_bases(x, y) && g.place(kind, x, y, &|g, c| g.free_low(c), false) {
                placed += 1;
            }
        }
    }

    // ---- abandoned camps, loose chests, landmarks ----
    let room3 = |g: &Gen, c: i32| {
        let (x, y) = g.map.xy(c);
        x > 3
            && y > 3
            && x < W - 4
            && y < H - 4
            && (-1..=1).all(|dy| (-1..=1).all(|dx| g.free_low(g.idx(x + dx, y + dy))))
    };
    for i in 0..10 {
        let c = g.spot(&room3);
        if c < 0 {
            continue;
        }
        g.solid_prop(PropKind::Tent, c - W);
        g.solid_prop(PropKind::Campfire, c + 1);
        g.solid_prop(
            [PropKind::Barrel, PropKind::Crate, PropKind::Logs][i % 3],
            c - W + 1,
        );
        g.solid_prop(
            [PropKind::Hay, PropKind::Cart, PropKind::Crate][i % 3],
            c - 1,
        );
        g.chest(c);
    }
    for _ in 0..24 {
        let c = g.spot(&|g, c| g.free_low(c));
        if c >= 0 {
            g.chest(c);
        }
    }
    for i in 0..40 {
        let c = g.spot(&|g, c| g.free_low(c));
        if c >= 0 {
            g.solid_prop(LANDMARKS[i % LANDMARKS.len()], c);
        }
    }

    // ---- static blocks, prune unreachable gameplay objects (resources do not block for the prune) ----
    let mut sb = vec![0u8; n];
    for d in &g.map.decor {
        if d.solid {
            let c = g.map.cell_at(d.x, d.y - 20.0);
            if c >= 0 {
                sb[c as usize] = 1;
            }
        }
    }
    for p in &g.points {
        for c in g.map.quad(p.cell) {
            sb[c as usize] = 1;
        }
    }
    for (_, b) in &g.bigs {
        for c in g.map.quad(b.cell) {
            sb[c as usize] = 1;
        }
    }
    g.map.block.copy_from_slice(&sb);
    let way = compute_field(&g.map, &[goal0], 0, false);
    let lost = |c: i32| way[c as usize] == FIELD_INF;
    let shut = |map: &Map, c: i32| {
        [
            c - 1,
            c + 2,
            c - W,
            c + 2 * W,
            c + W - 1,
            c + W + 2,
            c - W + 1,
            c + 2 * W + 1,
        ]
        .iter()
        .all(|k| !map.valid(*k) || way[*k as usize] == FIELD_INF)
    };
    g.trees.retain(|t| !lost(t.cell));
    g.golds.retain(|t| !lost(t.cell));
    g.stones.retain(|t| !lost(t.cell));
    g.chests.retain(|t| !lost(t.cell));
    let meadow_cells: Vec<V2> = g
        .meadows
        .iter()
        .copied()
        .filter(|m| !lost(g.map.cell_of(*m)))
        .collect();
    g.meadows = meadow_cells;
    let map_ref = &g.map;
    let keep_points: Vec<GenPoint> = g
        .points
        .iter()
        .copied()
        .filter(|p| !shut(map_ref, p.cell))
        .collect();
    let keep_bigs: Vec<(NodeKind, GenDeposit)> = g
        .bigs
        .iter()
        .copied()
        .filter(|b| !shut(map_ref, b.1.cell))
        .collect();
    g.points = keep_points;
    g.bigs = keep_bigs;
    // final static layer: + trees, golds, stones
    for c in g
        .trees
        .iter()
        .map(|t| t.cell)
        .chain(g.golds.iter().map(|d| d.cell))
        .chain(g.stones.iter().map(|d| d.cell))
    {
        sb[c as usize] = 1;
    }
    for p in &g.points {
        for c in g.map.quad(p.cell) {
            sb[c as usize] = 1;
        }
    }
    g.map.static_block = sb.clone();
    g.map.block = sb;

    // ---- sound(): reachability + per-base stock + meadow ----
    if check {
        let mut goals: Vec<i32> = bases.iter().map(|b| g.idx(b.0, b.1 + 2)).collect();
        for p in &g.plateaus {
            goals.push(g.idx(p.landing.0, p.landing.1));
        }
        let f = compute_field(&g.map, &[goals[0]], 0, false);
        if goals.iter().any(|c| f[*c as usize] == FIELD_INF) {
            return None;
        }
        let open = |c: i32| {
            [c - 1, c + 1, c - W, c + W]
                .iter()
                .any(|k| g.map.valid(*k) && f[*k as usize] < FIELD_INF)
        };
        for &(bx, by) in &bases {
            for (kind, r, need) in BASE_STOCK {
                let list: Vec<i32> = match kind {
                    NodeKind::Tree => g.trees.iter().map(|t| t.cell).collect(),
                    NodeKind::Gold => g.golds.iter().map(|t| t.cell).collect(),
                    NodeKind::Stone => g.stones.iter().map(|t| t.cell).collect(),
                };
                let cnt = list
                    .iter()
                    .filter(|c| {
                        let (x, y) = g.map.xy(**c);
                        ((((x - bx).pow(2) + (y - by).pow(2)) as f64).sqrt() as f32) < r
                            && open(**c)
                    })
                    .count();
                if cnt < need {
                    return None;
                }
            }
            if !g.meadows.iter().any(|m| {
                let (dx, dy) = (m.x / 64.0 - bx as f32, m.y / 64.0 - by as f32);
                (dx * dx + dy * dy).sqrt() < BASE_MEADOW
            }) {
                return None;
            }
        }
    }

    // ---- decorations (visual) ----
    let mut bushes = 0;
    let mut t = 0;
    while t < 20_000 && bushes < 900 {
        t += 1;
        let x = 2 + g.below((W - 4) as u32);
        let y = 2 + g.below((H - 4) as u32);
        let c = g.idx(x, y);
        let free = g.map.land[c as usize] == 1
            && g.map.cliff[c as usize] == 0
            && g.map.stair[c as usize] == 0
            && g.used[c as usize] == 0
            && g.map.static_block[c as usize] == 0
            && g.reserved[c as usize] == 0;
        if !free {
            continue;
        }
        g.used[c as usize] = 1;
        let kind = if bushes % 3 == 0 {
            DecorKind::Bush
        } else {
            DecorKind::Rock
        };
        let (px, py) = (
            x as f32 * 64.0 + 16.0 + g.rnd() as f32 * 32.0,
            y as f32 * 64.0 + 30.0 + g.rnd() as f32 * 26.0,
        );
        let v = g.below(4) as u8;
        let ph = g.rnd() as f32 * 8.0;
        g.map.decor.push(Decor {
            kind,
            x: px,
            y: py,
            variant: v,
            phase: ph,
            scale: 1.0,
            solid: false,
        });
        bushes += 1;
    }
    let land_within = |map: &Map, x: i32, y: i32, r: i32| {
        (-r..=r).any(|dy| {
            (-r..=r).any(|dx| {
                map.inside(x + dx, y + dy) && map.land[map.idx(x + dx, y + dy) as usize] == 1
            })
        })
    };
    let mut open_water = Vec::new();
    let mut water_rocks = 0;
    let mut ducks: Vec<V2> = Vec::new();
    for y in 1..H - 1 {
        for x in 1..W - 1 {
            let c = g.idx(x, y);
            if g.map.land[c as usize] != 0 || land_within(&g.map, x, y, 1) {
                continue;
            }
            if land_within(&g.map, x, y, 5) {
                open_water.push(c);
            }
            if water_rocks < 300 && land_within(&g.map, x, y, 3) && g.rnd() < 0.05 {
                water_rocks += 1;
                let p = g.map.center(c);
                let v = g.below(4) as u8;
                let ph = g.rnd() as f32 * 16.0;
                g.map.decor.push(Decor {
                    kind: DecorKind::WaterRock,
                    x: p.x,
                    y: p.y,
                    variant: v,
                    phase: ph,
                    scale: 1.0,
                    solid: false,
                });
            } else if ducks.len() < 30 && land_within(&g.map, x, y, 4) && g.rnd() < 0.01 {
                let p = g.map.center(c);
                if ducks.iter().all(|d| d.dist(p) >= 1100.0) {
                    ducks.push(p);
                    let ph = g.rnd() as f32 * 6.0;
                    g.map.decor.push(Decor {
                        kind: DecorKind::Duck,
                        x: p.x,
                        y: p.y,
                        variant: 0,
                        phase: ph,
                        scale: 1.0,
                        solid: false,
                    });
                }
            }
        }
    }
    for i in 0..32 {
        let x = g.rnd() as f32 * (W as f32 * 64.0 + 300.0) - 300.0;
        let y = g.rnd() as f32 * (H as f32 * 64.0 - 50.0) - 100.0;
        let v = (i % 8) as u8;
        let speed = 7.0 + g.rnd() as f32 * 9.0;
        g.map.decor.push(Decor {
            kind: DecorKind::Cloud,
            x,
            y,
            variant: v,
            phase: 0.0,
            scale: speed,
            solid: false,
        });
    }

    // ---- surfaces (spec 09 §11: colour by tier, forest floor, sand rim, paths between castles) ----
    for y in 0..H {
        for x in 0..W {
            let c = g.idx(x, y) as usize;
            g.map.surface[c] = if g.map.land[c] == 0 && g.map.cliff[c] == 0 {
                Surface::Water
            } else if g.map.level[c] >= 1 || g.map.cliff[c] == 1 {
                Surface::GrassTop
            } else if [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                g.map.inside(x + dx, y + dy) && g.map.land[g.idx(x + dx, y + dy) as usize] == 0
            }) {
                Surface::Sand
            } else if land_within(&g.map, x, y, 0)
                && !(-3..=3).all(|dy| {
                    (-3..=3).all(|dx| {
                        g.map.inside(x + dx, y + dy)
                            && g.map.land[g.idx(x + dx, y + dy) as usize] != 0
                    })
                })
            {
                Surface::GrassLow
            } else {
                Surface::GrassMid
            };
        }
    }
    let tree_cells: Vec<i32> = g.trees.iter().map(|t| t.cell).collect();
    let mut tree_mask = vec![0u8; n];
    for c in &tree_cells {
        tree_mask[*c as usize] = 1;
    }
    for c in &tree_cells {
        let (x, y) = g.map.xy(*c);
        let around = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
            .filter(|(dx, dy)| {
                g.map.inside(x + dx, y + dy) && tree_mask[g.idx(x + dx, y + dy) as usize] == 1
            })
            .count();
        if around >= 3 {
            for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                if g.map.inside(x + dx, y + dy) {
                    let k = g.idx(x + dx, y + dy) as usize;
                    if g.map.level[k] == 0
                        && g.map.land[k] == 1
                        && g.map.surface[k] != Surface::Sand
                    {
                        g.map.surface[k] = Surface::ForestFloor;
                    }
                }
            }
        }
    }
    // dirt paths between consecutive bases (straight DDA, level-0 free cells only)
    for i in 0..bases.len() {
        let (a, b) = (bases[i], bases[(i + 1) % bases.len()]);
        let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
        for k in 0..=steps {
            let x = a.0 + ((b.0 - a.0) * k + steps / 2).div_euclid(steps);
            let y = a.1 + 2 + ((b.1 - a.1) * k + steps / 2).div_euclid(steps);
            if g.map.inside(x, y) {
                let c = g.idx(x, y) as usize;
                if g.map.land[c] == 1
                    && g.map.level[c] == 0
                    && g.map.cliff[c] == 0
                    && g.map.static_block[c] == 0
                    && g.map.stair[c] == 0
                {
                    g.map.surface[c] = Surface::Path;
                }
            }
        }
    }

    g.map.version += 1;
    g.map.opened = g.map.version;
    Some(GeneratedWorld {
        seed,
        map: g.map,
        bases,
        plateaus: g.plateaus,
        trees: g.trees,
        golds: g.golds,
        stones: g.stones,
        bigs: g.bigs,
        meadows: g.meadows,
        points: g.points,
        chests: g.chests,
        ponds: Vec::new(),
        open_water,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_map_is_valid_and_deterministic() {
        for seed in [1u32, 42, 123_456, 9_999_999, 77] {
            let (g, attempts, checked) = generate_report(seed, &MatchSetup::default());
            assert!(checked, "seed {seed}: sound() never passed");
            assert!(attempts <= 20, "seed {seed}: {attempts} attempts");
            eprintln!(
                "seed {seed}: attempts {attempts}, trees {}, golds {}, stones {}, bigs {}, points {}, chests {}, meadows {}, plateaus {}, decor {}",
                g.trees.len(),
                g.golds.len(),
                g.stones.len(),
                g.bigs.len(),
                g.points.len(),
                g.chests.len(),
                g.meadows.len(),
                g.plateaus.len(),
                g.map.decor.len()
            );
            assert_eq!(g.bases.len(), BASE_COUNT);
            for i in 0..5 {
                for j in 0..i {
                    let (a, b) = (g.bases[i], g.bases[j]);
                    let d = (((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f32).sqrt();
                    assert!(d >= BASE_GAP, "bases too close: {d}");
                }
                // castle placeable at (x-2, y-1)
                let (bx, by) = g.bases[i];
                assert!(
                    g.map.can_place(BuildingKind::Castle, bx - 2, by - 1, &[]),
                    "castle not placeable"
                );
            }
            assert!(!g.trees.is_empty() && !g.golds.is_empty() && !g.stones.is_empty());
            assert!(!g.plateaus.is_empty(), "no plateau");
            assert!(g.map.stair.iter().any(|s| *s != 0));
            let again = generate(seed, &MatchSetup::default());
            assert_eq!(g.map.hash(), again.map.hash());
            assert_eq!(g.trees.len(), again.trees.len());
        }
    }
}
