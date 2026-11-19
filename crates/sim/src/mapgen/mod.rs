// OWNER: S1 (map) — see docs/spec/02-world.md §5 (pipeline), §6 (generation invariants), addenda W2-W6;
// docs/MAPGEN.md (design); 09-asset-inventory.md §3.6, §3.7, §11 (HD-2D aesthetics of the reference map).
//
// Aesthetic, deterministic generator. Pipeline (`build`):
//   1. land.rs    continent on a coarse 4x4 grid → run-jittered rectilinear coast; 5 kingdom sites ≥ 64 apart;
//                 forced base land; mainland = 4-connected to base 0; inland ponds.
//   2. relief.rs  20-38 plateaus covering ~30 % of the mainland (massifs first; rectangle unions, morphologically
//                 cleaned) with south cliffs (some into the sea), stairs at cliff-run ends, level-2 terraces; the
//                 lowland never split; unreachable pockets → noBuild; hills ordered by distance to the bases'
//                 centroid.  Then islets off the coast.
//   3. stock.rs   fair start stock (`near()`), mineral sites on plateaus (W4), capture points + their stock, big
//                 deposits; roads.rs curving dirt tracks between neighbouring castles and spurs to nearby points
//                 (+ invisible protected corridors from every stair); then scattered groups grown into a few big
//                 compact woods (one species each).
//   4. scenery.rs camps + chests, prop vignettes (ruins, farms, hamlets, graveyards, standing stones, wrecks...).
//   5. prune unreachable objects, final static obstacles, `sound()` acceptance test (02 §5.2).
//   6. decor.rs   bushes/rocks as transition glue, water rocks, ducks, clouds, ground details; surface.rs per-cell
//                 `Surface` + water depth.
//
// DETERMINISM (the client regenerates from the seed and compares `Map::hash`): only + - * / and sqrt on floats —
// NO sin/cos/atan2/powf/exp/ln (they may differ between native and wasm); rotations use literal cos/sin pairs.
// No HashMap iteration. All randomness from one Mulberry32 stream per attempt.

pub mod check;
mod decor;
pub mod grid;
mod land;
mod relief;
mod roads;
mod scenery;
mod stock;
mod surface;

use crate::defs::*;
use crate::map::{
    GenChest, GenDeposit, GenPoint, GenTree, GeneratedWorld, Map, PlateauInfo,
};
use crate::math::V2;
use crate::path::compute_field;
use crate::rng::Mulberry32;
use crate::world::MatchSetup;
use grid::Noise;
use relief::Hill;

pub(crate) const W: i32 = crate::map::W;
pub(crate) const H: i32 = crate::map::H;

/// Cell index `y*W + x` (no bounds check).
#[inline]
pub(crate) fn idx(x: i32, y: i32) -> i32 {
    y * W + x
}

/// `'Generazione della mappa fallita (seme ' + seed + ')'` (spec 02 §5.1) — only for logs: the driver never
/// fails (the last attempt is accepted unchecked).
pub fn failure_text(seed: u32) -> String {
    format!("Generazione della mappa fallita (seme {seed})")
}

/// Deterministic map generation from the match seed (spec 02 §5.1 driver: retries with `seed + attempt*7919`).
pub fn generate(seed: u32, setup: &MatchSetup) -> GeneratedWorld {
    generate_report(seed, setup).0
}

/// Max checked attempts before the unchecked fallback.
const ATTEMPTS: u32 = 60;

/// [`generate`] plus the number of attempts used and whether the `sound()` acceptance test passed (false only if
/// every attempt failed and the last map was accepted unchecked). For tests and `examples/mapgen_png.rs`.
pub fn generate_report(seed: u32, _setup: &MatchSetup) -> (GeneratedWorld, u32, bool) {
    for attempt in 0..ATTEMPTS {
        if let Ok(g) = build(seed, seed.wrapping_add(attempt.wrapping_mul(7919)), true) {
            return (g, attempt + 1, true);
        }
    }
    // unchecked fallback: any attempt whose shape succeeded
    for k in 0..400u32 {
        if let Ok(g) = build(seed, seed.wrapping_add((ATTEMPTS + k).wrapping_mul(7919)), false) {
            return (g, ATTEMPTS + k + 1, false);
        }
    }
    (fallback_world(seed), u32::MAX, false)
}

/// Diagnostics: the checked result of attempt `attempt` of `seed` (`Err` = rejection reason).
pub fn attempt(seed: u32, attempt: u32) -> Result<GeneratedWorld, String> {
    build(seed, seed.wrapping_add(attempt.wrapping_mul(7919)), true)
}

/// Diagnostics: attempt `attempt` of `seed` without the acceptance test (`None` if its shape failed).
pub fn attempt_unchecked(seed: u32, attempt: u32) -> Option<GeneratedWorld> {
    build(seed, seed.wrapping_add(attempt.wrapping_mul(7919)), false).ok()
}

/// Last-resort world (never reached in practice): a rectangle island with the 5 bases on a pentagon.
fn fallback_world(seed: u32) -> GeneratedWorld {
    let mut map = Map::new();
    for y in 6..H - 7 {
        for x in 6..W - 6 {
            let c = map.idx(x, y) as usize;
            map.land[c] = 1;
            map.surface[c] = crate::map::Surface::GrassLow;
        }
    }
    let bases = vec![(144, 156), (239, 114), (203, 40), (85, 40), (49, 114)];
    GeneratedWorld {
        seed,
        map,
        bases,
        plateaus: Vec::new(),
        trees: Vec::new(),
        golds: Vec::new(),
        stones: Vec::new(),
        bigs: Vec::new(),
        meadows: Vec::new(),
        points: Vec::new(),
        chests: Vec::new(),
        ponds: Vec::new(),
        open_water: Vec::new(),
    }
}

/// Resource group kinds of `place()`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Forest,
    Grove,
    Gold,
    Stone,
    Meadow,
}

/// Reusable buffers of the road search (`touched` = states to reset).
#[derive(Default)]
pub struct Search {
    dist: Vec<u32>,
    prev: Vec<u32>,
    touched: Vec<u32>,
    /// per-cell road cost without the existing-road discount (0 = impassable), computed once per `roads()`
    base: Vec<u16>,
    /// per-cell meander term
    wobble: Vec<u8>,
}

/// Generator state of one attempt.
pub struct Gen {
    search: std::cell::RefCell<Search>,
    rng: Mulberry32,
    noise: Noise,
    pub map: Map,
    /// one object per cell (resources, solid props, points, bigs)
    used: Vec<u8>,
    /// cleared base zones + unreachable pockets
    reserved: Vec<u8>,
    /// no plateau here (kingdom surroundings)
    keep: Vec<u8>,
    /// spacing halo around plateaus
    near: Vec<u8>,
    /// hill index + 1 of level ≥ 1 cells
    hill_of: Vec<u8>,
    /// stair access zones (3x4 around each stair top)
    stairzone: Vec<u8>,
    /// stair aprons: no objects in front of a stair foot (buildable)
    apron: Vec<u8>,
    /// road cells (`roads::ROAD_*`)
    road: Vec<u8>,
    /// non-solid decoration occupancy (no stacking)
    deco: Vec<u8>,
    /// surface overrides painted by scenery (dirt plots, camp ground, dry corners)
    paint: Vec<u8>,
    /// Chebyshev distance from walkable lowland cells to the nearest non-lowland cell (water, plateau, cliff)
    edge0: Vec<u8>,
    bases: Vec<(i32, i32)>,
    hills: Vec<Hill>,
    trees: Vec<GenTree>,
    golds: Vec<GenDeposit>,
    stones: Vec<GenDeposit>,
    bigs: Vec<(NodeKind, GenDeposit)>,
    meadows: Vec<V2>,
    points: Vec<GenPoint>,
    chests: Vec<GenChest>,
    plateaus: Vec<PlateauInfo>,
    ponds: Vec<(f32, f32)>,
    islets: Vec<(i32, i32, i32, i32)>,
    /// forest/grove centres (cells) and radii
    woods: Vec<(f32, f32, f32)>,
    /// species pair of each wood (parallel to `woods`)
    wood_sp: Vec<(u8, u8)>,
    /// species imposed on the next forest group (a group extending a wood keeps its species)
    force_sp: Option<(u8, u8)>,
    /// cells blocked by solid props (cell, prop)
    solid: Vec<(i32, PropKind)>,
    /// road cells where spurs join the network (signposts)
    junctions: Vec<i32>,
    /// land share relative to the old island (`solid / (N*0.71)`)
    rich: f64,
}

impl Gen {
    fn new(s: u32) -> Gen {
        let mut rng = Mulberry32::new(s);
        let noise = Noise::new((0..1024).map(|_| rng.next_f64()));
        let n = (W * H) as usize;
        Gen {
            search: Default::default(),
            rng,
            noise,
            map: Map::new(),
            used: vec![0; n],
            reserved: vec![0; n],
            keep: vec![0; n],
            near: vec![0; n],
            hill_of: vec![0; n],
            stairzone: vec![0; n],
            apron: vec![0; n],
            road: vec![0; n],
            deco: vec![0; n],
            paint: vec![0; n],
            edge0: vec![0; n],
            bases: Vec::new(),
            hills: Vec::new(),
            trees: Vec::new(),
            golds: Vec::new(),
            stones: Vec::new(),
            bigs: Vec::new(),
            meadows: Vec::new(),
            points: Vec::new(),
            chests: Vec::new(),
            plateaus: Vec::new(),
            ponds: Vec::new(),
            islets: Vec::new(),
            woods: Vec::new(),
            wood_sp: Vec::new(),
            force_sp: None,
            junctions: Vec::new(),
            solid: Vec::new(),
            rich: 1.0,
        }
    }

    #[inline]
    fn rnd(&mut self) -> f64 {
        self.rng.next_f64()
    }
    #[inline]
    fn below(&mut self, n: u32) -> i32 {
        if n == 0 {
            return 0;
        }
        self.rng.below(n) as i32
    }
    #[inline]
    fn chance(&mut self, p: f64) -> bool {
        self.rnd() < p
    }

    /// `freeLow(c)` (+ roads, stair aprons).
    fn free_low(&self, c: i32) -> bool {
        if !self.map.valid(c) {
            return false;
        }
        let i = c as usize;
        self.map.land[i] == 1
            && self.map.level[i] == 0
            && self.map.cliff[i] == 0
            && self.map.stair[i] == 0
            && self.reserved[i] == 0
            && self.map.no_build[i] == 0
            && self.used[i] == 0
            && self.road[i] == 0
            && self.apron[i] == 0
    }

    /// `freeHigh(c)`.
    fn free_high(&self, c: i32) -> bool {
        if !self.map.valid(c) {
            return false;
        }
        let i = c as usize;
        self.map.land[i] == 1
            && self.map.level[i] != 0
            && self.map.cliff[i] == 0
            && self.map.stair[i] == 0
            && self.map.no_build[i] == 0
            && self.used[i] == 0
            && self.road[i] == 0
            && self.apron[i] == 0
    }

    /// `farFromBases(x, y)`: every base farther than 14 cells.
    fn far_from_bases(&self, x: f64, y: f64) -> bool {
        self.bases.iter().all(|b| {
            let (dx, dy) = (b.0 as f64 - x, b.1 as f64 - y);
            dx * dx + dy * dy > FAR_FROM_BASES as f64 * FAR_FROM_BASES as f64
        })
    }

    /// Lowland edge distance (aesthetic weights).
    fn compute_edge0(&mut self) {
        let src: Vec<u8> = (0..self.map.len())
            .map(|i| !(self.map.land[i] == 1 && self.map.level[i] == 0 && self.map.cliff[i] == 0) as u8)
            .collect();
        self.edge0 = grid::cheb_dist(&src, W, H, 40);
    }

    /// Solid props must never seal off ground that is walkable without them (a ruin across a narrow plateau, a
    /// fence closing a cove): props bordering such sealed cells are removed until every cell reachable on the
    /// empty map is reachable again with the static props (points and bigs keep their 1-cell margins, so they
    /// cannot seal anything alone). Resources do not count, as in the prune (a forest can be cut through).
    fn unseal(&mut self) {
        let n = self.map.len();
        let goal0 = idx(self.bases[0].0, self.bases[0].1 + 2);
        self.map.block.iter_mut().for_each(|v| *v = 0);
        let free = compute_field(&self.map, &[goal0], 0, false);
        for _ in 0..12 {
            let mut sb = vec![0u8; n];
            for (c, _) in &self.solid {
                sb[*c as usize] = 1;
            }
            for c in self.points.iter().map(|p| p.cell).chain(self.bigs.iter().map(|b| b.1.cell)) {
                for k in self.map.quad(c) {
                    sb[k as usize] = 2;
                }
            }
            self.map.block.copy_from_slice(&sb);
            let way = compute_field(&self.map, &[goal0], 0, false);
            let mut gone = vec![0u8; n];
            let mut any = false;
            for c in 0..n {
                if sb[c] != 0 || free[c] == FIELD_INF || way[c] != FIELD_INF {
                    continue;
                }
                let (x, y) = ((c as i32) % W, (c as i32) / W);
                for (dx, dy) in DIRS8 {
                    if self.map.inside(x + dx, y + dy) {
                        let k = idx(x + dx, y + dy) as usize;
                        if sb[k] == 1 {
                            gone[k] = 1;
                            any = true;
                        }
                    }
                }
            }
            if !any {
                break;
            }
            // a wide prop (anchored on the edge between its two cells: x on a multiple of 64) goes as a whole
            for d in &self.map.decor {
                if d.solid && (d.x as i32).rem_euclid(64) == 0 {
                    let c = d.cell(&self.map);
                    if c >= 0 && (c as usize) + 1 < n && (gone[c as usize] != 0 || gone[c as usize + 1] != 0) {
                        gone[c as usize] = 1;
                        gone[c as usize + 1] = 1;
                    }
                }
            }
            for (c, _) in &self.solid {
                if gone[*c as usize] != 0 {
                    self.used[*c as usize] = 0;
                }
            }
            self.solid.retain(|(c, _)| gone[*c as usize] == 0);
            let decor = std::mem::take(&mut self.map.decor);
            let map = &self.map;
            let kept: Vec<_> = decor
                .into_iter()
                .filter(|d| {
                    let c = d.cell(map);
                    !(d.solid && c >= 0 && gone[c as usize] != 0)
                })
                .collect();
            self.map.decor = kept;
        }
        self.map.block.iter_mut().for_each(|v| *v = 0);
    }

    /// Step 14: prune unreachable objects (resources do not block for this test), then the final static layer:
    /// trees, golds, stones, solid props, points and bigs.
    fn prune_and_block(&mut self) {
        self.unseal();
        let n = self.map.len();
        let mut sb = vec![0u8; n];
        for (c, v) in self.solid.iter() {
            let _ = v;
            sb[*c as usize] = 1;
        }
        for p in &self.points {
            for c in self.map.quad(p.cell) {
                sb[c as usize] = 1;
            }
        }
        for (_, b) in &self.bigs {
            for c in self.map.quad(b.cell) {
                sb[c as usize] = 1;
            }
        }
        self.map.block.copy_from_slice(&sb);
        let goal0 = idx(self.bases[0].0, self.bases[0].1 + 2);
        let way = compute_field(&self.map, &[goal0], 0, false);
        let lost = |c: i32| !self.map.valid(c) || way[c as usize] == FIELD_INF;
        let w = W;
        let shut = |c: i32| {
            [c - 1, c + 2, c - w, c + 2 * w, c + w - 1, c + w + 2, c - w + 1, c + 2 * w + 1]
                .iter()
                .all(|k| !self.map.valid(*k) || way[*k as usize] == FIELD_INF)
        };
        let trees: Vec<GenTree> = self.trees.iter().copied().filter(|t| !lost(t.cell)).collect();
        let golds: Vec<GenDeposit> = self.golds.iter().copied().filter(|t| !lost(t.cell)).collect();
        let stones: Vec<GenDeposit> = self.stones.iter().copied().filter(|t| !lost(t.cell)).collect();
        let chests: Vec<GenChest> = self.chests.iter().copied().filter(|t| !lost(t.cell)).collect();
        let meadows: Vec<V2> = self
            .meadows
            .iter()
            .copied()
            .filter(|m| !lost(self.map.cell_of(*m)))
            .collect();
        let points: Vec<GenPoint> = self.points.iter().copied().filter(|p| !shut(p.cell)).collect();
        let bigs: Vec<(NodeKind, GenDeposit)> = self.bigs.iter().copied().filter(|b| !shut(b.1.cell)).collect();
        // freed cells of dropped objects
        for t in &self.trees {
            if lost(t.cell) {
                self.used[t.cell as usize] = 0;
            }
        }
        self.trees = trees;
        self.golds = golds;
        self.stones = stones;
        self.chests = chests;
        self.meadows = meadows;
        self.points = points;
        self.bigs = bigs;
        // final static layer (points/bigs recomputed after the prune)
        let mut sb = vec![0u8; n];
        for (c, _) in self.solid.iter() {
            sb[*c as usize] = 1;
        }
        for c in self
            .trees
            .iter()
            .map(|t| t.cell)
            .chain(self.golds.iter().map(|d| d.cell))
            .chain(self.stones.iter().map(|d| d.cell))
        {
            sb[c as usize] = 1;
        }
        for p in &self.points {
            for c in self.map.quad(p.cell) {
                sb[c as usize] = 1;
            }
        }
        for (_, b) in &self.bigs {
            for c in self.map.quad(b.cell) {
                sb[c as usize] = 1;
            }
        }
        self.map.static_block = sb.clone();
        self.map.block = sb;
    }

    /// `sound()` (02 §5.2): bases, plateau and terrace landings mutually reachable with obstacles; per-base stock
    /// (`BASE_STOCK`, adjacent to a reachable cell) and a meadow within 14 cells.
    fn sound(&self) -> Result<(), String> {
        let mut goals: Vec<i32> = self.bases.iter().map(|b| idx(b.0, b.1 + 2)).collect();
        for p in &self.plateaus {
            goals.push(idx(p.landing.0, p.landing.1));
            if let Some(t) = p.top {
                goals.push(idx(t.0, t.1));
            }
        }
        let f = compute_field(&self.map, &[goals[0]], 0, false);
        if goals.iter().any(|c| f[*c as usize] == FIELD_INF) {
            return Err(format!("goal {:?} unreachable", goals.iter().find(|c| f[**c as usize] == FIELD_INF).map(|c| self.map.xy(*c))));
        }
        let open = |c: i32| {
            [c - 1, c + 1, c - W, c + W]
                .iter()
                .any(|k| self.map.valid(*k) && f[*k as usize] < FIELD_INF)
        };
        for &(bx, by) in &self.bases {
            for (kind, r, need) in BASE_STOCK {
                let cnt = match kind {
                    NodeKind::Tree => self.count_near(self.trees.iter().map(|t| t.cell), bx, by, r, &open),
                    NodeKind::Gold => self.count_near(self.golds.iter().map(|t| t.cell), bx, by, r, &open),
                    NodeKind::Stone => self.count_near(self.stones.iter().map(|t| t.cell), bx, by, r, &open),
                };
                if cnt < need {
                    return Err(format!("base ({bx},{by}): {cnt} {kind:?} within {r} < {need}"));
                }
            }
            if !self.meadows.iter().any(|m| {
                let (dx, dy) = (m.x / 64.0 - bx as f32, m.y / 64.0 - by as f32);
                (dx * dx + dy * dy).sqrt() < BASE_MEADOW
            }) {
                return Err(format!("base ({bx},{by}): no meadow"));
            }
        }
        // every mine / quarry / sawmill / ranch kept its POINT_STOCK group after the prune (02 §6.7)
        for p in &self.points {
            let (x, y) = self.map.xy(p.cell);
            let near = |c: i32, r: i32| {
                let (cx, cy) = self.map.xy(c);
                (cx - x - 1).abs() <= r && (cy - y - 1).abs() <= r
            };
            let ok = match p.kind {
                PointKind::Mine => self.golds.iter().any(|d| near(d.cell, 8)),
                PointKind::Quarry => self.stones.iter().any(|d| near(d.cell, 8)),
                PointKind::Sawmill => self.trees.iter().any(|t| near(t.cell, 9)),
                PointKind::Ranch => self.meadows.iter().any(|q| near(self.map.cell_of(*q), 8)),
                PointKind::Outpost => true,
            };
            if !ok {
                return Err(format!("{:?} point ({x},{y}) without its resources", p.kind));
            }
        }
        Ok(())
    }

    fn count_near(&self, cells: impl Iterator<Item = i32>, bx: i32, by: i32, r: f32, open: &dyn Fn(i32) -> bool) -> usize {
        cells
            .filter(|c| {
                let (x, y) = self.map.xy(*c);
                ((((x - bx).pow(2) + (y - by).pow(2)) as f64).sqrt() as f32) < r && open(*c)
            })
            .count()
    }
}

/// One generation attempt (`seed` = match seed reported in the result, `s` = attempt seed); `Err` = why it was
/// rejected.
fn build(seed: u32, s: u32, check: bool) -> Result<GeneratedWorld, String> {
    build_marked(seed, s, check, &mut |_| {})
}

/// Diagnostics: attempt `attempt` of `seed` (unchecked) calling `mark(stage)` after every pipeline stage (the
/// example uses it for timings; the generator itself never reads a clock).
pub fn profile(seed: u32, attempt: u32, mark: &mut dyn FnMut(&'static str)) -> Option<GeneratedWorld> {
    build_marked(seed, seed.wrapping_add(attempt.wrapping_mul(7919)), false, mark).ok()
}

fn build_marked(seed: u32, s: u32, check: bool, mark: &mut dyn FnMut(&'static str)) -> Result<GeneratedWorld, String> {
    let mut g = Gen::new(s);
    if !g.shape_land() {
        return Err("kingdom sites".into());
    }
    mark("land");
    g.ponds();
    if !g.relief() {
        return Err("too few plateaus".into());
    }
    mark("relief");
    g.islets = g.islets();
    let solid = g.map.land.iter().filter(|v| **v == 1).count();
    g.rich = solid as f64 / ((W * H) as f64 * 0.71);
    g.compute_edge0();

    g.base_stock();
    g.points_and_bigs();
    mark("islets+stock+points");
    g.roads();
    mark("roads");
    g.plateau_stock();
    g.scattered();
    mark("scattered");
    g.scenery();
    mark("scenery");
    g.prune_and_block();
    if check {
        g.sound()?;
    }
    mark("prune+sound");
    g.decorate();
    mark("decorate");
    g.surfaces();
    mark("surfaces");

    g.map.version += 1;
    g.map.opened = g.map.version;
    let open_water = g.open_water();
    Ok(GeneratedWorld {
        seed,
        map: g.map,
        bases: g.bases,
        plateaus: g.plateaus,
        trees: g.trees,
        golds: g.golds,
        stones: g.stones,
        bigs: g.bigs,
        meadows: g.meadows,
        points: g.points,
        chests: g.chests,
        ponds: g.ponds,
        open_water,
    })
}

#[cfg(test)]
mod tests;
