// OWNER: S1 (map) — see docs/spec/02-world.md §1-§4, §6, §8 and 09-asset-inventory.md §3.6, §11.
//
// The foundation wrote the data model AND faithful ports of the simple queries of spec 02 §3 (passable, crossable,
// linkOK, stepOK, tryMove, clearLine, nearestPassable, setBlock/setOwner/resetBlock, canPlace). S1 owns the file:
// verify them against the spec, extend the rendering layers (Surface/Decor) for the HD-2D generator, and keep every
// signature stable (other modules call them). Cells are `i32` so the JS `-1` "outside" sentinel survives; every
// accessor bounds-checks (02 addenda W6).
//
// S1 TODO list for this file:
//   * spec 02 §2: keep the per-cell layers; add any rendering layer the voxel terrain needs (e.g. height offsets,
//     biome ids) — client C1 reads `Map` directly.
//   * spec 09 §3.6/§11: `Surface` assignment is done by mapgen; extend `Surface`/`DecorKind` if the art needs it.
//   * `hash()`: the client regenerates the map from the seed and compares this hash (08-net R5).
#![allow(dead_code)]

use crate::defs::{BuildingKind, DIRS8, MOVE_PROBE, PropKind};
use crate::math::{V2, v2};
use crate::{MAP_H, MAP_W, TILE};
use serde::{Deserialize, Serialize};

/// Map width / height in cells as `i32`.
pub const W: i32 = MAP_W as i32;
pub const H: i32 = MAP_H as i32;
/// Number of cells.
pub const N: usize = MAP_W * MAP_H;

/// Ground material of a cell for the HD-2D renderer (spec 09 §3.6 / §11; colours of `Tilemap_colorN`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Surface {
    /// sea / pond (`#47aba9`)
    #[default]
    Water,
    /// beach (derived texture, spec 09 §3.7)
    Sand,
    /// low / coastal tier — `Tilemap_color3` (emerald)
    GrassLow,
    /// main tier — `Tilemap_color1` (yellow-green)
    GrassMid,
    /// top terrace — `Tilemap_color2` (fresh green)
    GrassTop,
    /// dry / olive — `Tilemap_color4`
    GrassDry,
    /// pine-forest floor — `Tilemap_color5` (teal)
    ForestFloor,
    /// bare dirt (farm plots, under buildings; derived texture)
    Dirt,
    /// trodden dirt path between castles
    Path,
}

impl Surface {
    /// `N` of the `Tilemap_colorN.png` tileset used for grass surfaces.
    pub const fn tileset(self) -> Option<u8> {
        match self {
            Surface::GrassMid => Some(1),
            Surface::GrassTop => Some(2),
            Surface::GrassLow => Some(3),
            Surface::GrassDry => Some(4),
            Surface::ForestFloor => Some(5),
            _ => None,
        }
    }
}

/// Kind of a purely visual decoration (never blocks unless `Decor.solid`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecorKind {
    /// `Terrain/Decorations/Bushes/Bushe{v+1}.png` (fw 128, anchor 64,80, 8 fps, 8 frames)
    Bush,
    /// `Terrain/Decorations/Rocks/Rock{v+1}.png` (drawn at x-32, y-50)
    Rock,
    /// `Rocks in the Water/Water Rocks_0{v+1}.png` (16 frames of 64)
    WaterRock,
    /// `Rubber Duck/Rubber duck.png` (3 frames of 32)
    Duck,
    /// `Clouds/Clouds_0{v+1}.png` (drifts east at `scale` px/s on the client)
    Cloud,
    /// unreachable scenery tree (islets, map border forests): `Trees/Tree{v+1}.png`
    Tree,
    /// `Extra/Props/{name}.png`, anchored bottom-centre
    Prop(PropKind),
}

/// One decoration (position in sim px = foot point).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decor {
    pub kind: DecorKind,
    pub x: f32,
    pub y: f32,
    /// sprite variant (0..3 for bushes/rocks/trees/water rocks, 0..7 clouds)
    pub variant: u8,
    /// animation phase offset (frames)
    pub phase: f32,
    /// draw scale (clouds: drift speed px/s)
    pub scale: f32,
    /// blocks its cell (solid props: camps, landmarks) — already counted in `Map.static_block`
    pub solid: bool,
}

/// A raised area (spec 02 §2.1 `plateaus`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlateauInfo {
    /// tileset / biome id 1..4 (render only)
    pub id: u8,
    /// landing cell at the top of its first stair (reachability goal)
    pub landing: (i32, i32),
    /// cell count
    pub cells: u32,
    /// landing of the terrace's first stair, if any
    pub top: Option<(i32, i32)>,
}

/// A generated tree (`trees[]`): cell + sprite variant 0..3. Wood = `TREE_WOOD`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenTree {
    pub cell: i32,
    pub variant: u8,
}

/// A generated gold/stone deposit; `big` = 2x2 deposit with top-left `cell` (gold 1500, stone 1800).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenDeposit {
    pub cell: i32,
    pub amount: f32,
    pub big: bool,
}

/// A capture point (2x2, top-left `cell`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenPoint {
    pub cell: i32,
    pub kind: crate::defs::PointKind,
}

/// A treasure chest (becomes a never-expiring pickup of `TREASURE` at `(cx, cy + 6)`, main.js:77).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenChest {
    pub cell: i32,
    pub res: crate::defs::Res,
}

/// Output of `mapgen::generate(seed, setup)`, consumed by `World::new` (and by the client, which regenerates the
/// same world from the seed; spec 02 §8). Entity creation order on both sides: trees, golds (small then big),
/// stones (small then big), points — that order defines `NodeId`/`PointId`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeneratedWorld {
    pub seed: u32,
    pub map: Map,
    /// 5 base centres (cells); castle 5x2 at `(x-2, y-1)`; `(x, y+2)` = base reachability cell
    pub bases: Vec<(i32, i32)>,
    pub plateaus: Vec<PlateauInfo>,
    pub trees: Vec<GenTree>,
    /// small gold veins (240) — big ones are in `bigs`
    pub golds: Vec<GenDeposit>,
    /// small stone veins (300) — big ones are in `bigs`
    pub stones: Vec<GenDeposit>,
    /// big 2x2 deposits, alternating gold 1500 / stone 1800
    pub bigs: Vec<(crate::defs::NodeKind, GenDeposit)>,
    /// sheep meadows (px, cell centre): 3 sheep each at start, wild respawn
    pub meadows: Vec<V2>,
    pub points: Vec<GenPoint>,
    pub chests: Vec<GenChest>,
    /// pond centres (cells, float) — render (ducks/lilypads already in `map.decor`)
    pub ponds: Vec<(f32, f32)>,
    /// water cells used for random splash FX on the client
    pub open_water: Vec<i32>,
}

/// The grid. Gameplay layers per spec 02 §2 + rendering layers for the HD-2D look.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Map {
    pub w: i32,
    pub h: i32,
    /// 0 water, 1 mainland (walkable), 2 islet (scenery)
    pub land: Vec<u8>,
    /// 0 ground, 1 plateau, 2 terrace
    pub level: Vec<u8>,
    /// 1 = rock wall cell (never walkable; may be on water)
    pub cliff: Vec<u8>,
    /// 0 none; 1 left-lower, 2 left-upper, 3 right-lower, 4 right-upper (odd = lower half)
    pub stair: Vec<u8>,
    /// plateau tileset id 1..4 (render)
    pub plateau: Vec<u8>,
    /// 1 = buildings cannot be placed
    pub no_build: Vec<u8>,
    /// static obstacles (trees, ores, solid props, points, bigs) — generation output, never changes
    pub static_block: Vec<u8>,
    /// obstacle COUNTER: static + buildings (`setBlock` adds/subtracts)
    pub block: Vec<u8>,
    /// side (faction idx + 1) of the palisade on the cell, 0 none
    pub wall_of: Vec<u8>,
    /// side of the building on the cell (palisades included), 0 none
    pub own_of: Vec<u8>,
    /// standing palisade cells per side (index 0 unused)
    pub walls: [u16; 6],
    /// bumped on every map change (field cache, slow snapshots)
    pub version: u32,
    /// `version` when a cell was last freed
    pub opened: u32,
    /// ground material per cell (render)
    pub surface: Vec<Surface>,
    /// decorations (render; solid ones also in `static_block`)
    pub decor: Vec<Decor>,
}

impl Map {
    /// All-water map of the standard size.
    pub fn new() -> Map {
        Map::with_size(W, H)
    }

    pub fn with_size(w: i32, h: i32) -> Map {
        let n = (w * h) as usize;
        Map {
            w,
            h,
            land: vec![0; n],
            level: vec![0; n],
            cliff: vec![0; n],
            stair: vec![0; n],
            plateau: vec![0; n],
            no_build: vec![0; n],
            static_block: vec![0; n],
            block: vec![0; n],
            wall_of: vec![0; n],
            own_of: vec![0; n],
            walls: [0; 6],
            version: 1,
            opened: 1,
            surface: vec![Surface::Water; n],
            decor: Vec::new(),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        (self.w * self.h) as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `idx(x, y) = y*W + x` (no bounds check).
    #[inline]
    pub fn idx(&self, x: i32, y: i32) -> i32 {
        y * self.w + x
    }

    /// `inside(x, y)`.
    #[inline]
    pub fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    /// Valid cell index.
    #[inline]
    pub fn valid(&self, c: i32) -> bool {
        c >= 0 && (c as usize) < self.len()
    }

    /// `(x, y)` of a cell index.
    #[inline]
    pub fn xy(&self, c: i32) -> (i32, i32) {
        (c % self.w, c / self.w)
    }

    /// `cellAt(px, py)`: -1 outside.
    #[inline]
    pub fn cell_at(&self, px: f32, py: f32) -> i32 {
        let x = (px / TILE).floor() as i32;
        let y = (py / TILE).floor() as i32;
        if self.inside(x, y) {
            self.idx(x, y)
        } else {
            -1
        }
    }

    #[inline]
    pub fn cell_of(&self, p: V2) -> i32 {
        self.cell_at(p.x, p.y)
    }

    /// Cell centre in px (`cx(c), cy(c)`).
    #[inline]
    pub fn center(&self, c: i32) -> V2 {
        let (x, y) = self.xy(c);
        v2(x as f32 * TILE + 32.0, y as f32 * TILE + 32.0)
    }

    /// 2x2 footprint whose top-left is `c` (capture points, big deposits).
    #[inline]
    pub fn quad(&self, c: i32) -> [i32; 4] {
        [c, c + 1, c + self.w, c + self.w + 1]
    }

    #[inline]
    fn at(v: &[u8], c: i32) -> u8 {
        if c >= 0 {
            v.get(c as usize).copied().unwrap_or(0)
        } else {
            0
        }
    }

    /// Mainland (`land == 1`), cell index form.
    #[inline]
    pub fn is_land(&self, c: i32) -> bool {
        Map::at(&self.land, c) == 1
    }

    /// Elevation 0..2 of a cell (0 outside).
    #[inline]
    pub fn level_at(&self, c: i32) -> u8 {
        Map::at(&self.level, c)
    }

    #[inline]
    pub fn is_stair(&self, c: i32) -> bool {
        Map::at(&self.stair, c) != 0
    }

    /// `passable(c, side)`: may stand here (free, or own palisade).
    #[inline]
    pub fn passable(&self, c: i32, side: u8) -> bool {
        self.valid(c) && {
            let i = c as usize;
            self.land[i] == 1
                && self.cliff[i] == 0
                && (self.block[i] == 0 || (side != 0 && self.wall_of[i] == side))
        }
    }

    /// `crossable(c, side)`: may pass through (free, or any own building).
    #[inline]
    pub fn crossable(&self, c: i32, side: u8) -> bool {
        self.valid(c) && {
            let i = c as usize;
            self.land[i] == 1
                && self.cliff[i] == 0
                && (self.block[i] == 0 || (side != 0 && self.own_of[i] == side))
        }
    }

    /// `linkOK(a, b)`: same level, or vertically adjacent stair halves.
    #[inline]
    pub fn link_ok(&self, a: i32, b: i32) -> bool {
        self.level_at(a) == self.level_at(b)
            || (self.is_stair(a) && self.is_stair(b) && (a - b).abs() == self.w)
    }

    /// `stepOK(x0, y0, x1, y1, side, cross)` (spec 02 §3.2).
    pub fn step_ok(&self, x0: f32, y0: f32, x1: f32, y1: f32, side: u8, cross: bool) -> bool {
        let a = self.cell_at(x0, y0);
        let b = self.cell_at(x1, y1);
        if a == b {
            return true;
        }
        let target_ok = if cross || !self.passable(a, side) {
            self.crossable(b, side)
        } else {
            self.passable(b, side)
        };
        if !target_ok {
            return false;
        }
        if !self.crossable(a, side) {
            return true; // stuck inside a blocked cell: can always get out
        }
        self.link_ok(a, b)
    }

    /// `tryMove(e, dx, dy)`: axis-separated sliding with a 9 px probe; returns the new position.
    pub fn try_move(&self, p: V2, dx: f32, dy: f32, side: u8, cross: bool) -> V2 {
        let mut q = p;
        if dx != 0.0
            && self.step_ok(
                q.x,
                q.y,
                q.x + dx + dx.signum() * MOVE_PROBE,
                q.y,
                side,
                cross,
            )
        {
            q.x += dx;
        }
        if dy != 0.0
            && self.step_ok(
                q.x,
                q.y,
                q.x,
                q.y + dy + dy.signum() * MOVE_PROBE,
                side,
                cross,
            )
        {
            q.y += dy;
        }
        q
    }

    /// `clearLine(x0, y0, x1, y1, side)` (02 §3.2 + addenda W1: cross = true, own buildings crossable).
    pub fn clear_line(&self, a: V2, b: V2, side: u8) -> bool {
        let d = a.dist(b);
        let n = (d / 24.0).ceil() as i32;
        let (mut px, mut py) = (a.x, a.y);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let x = a.x + (b.x - a.x) * t;
            let y = a.y + (b.y - a.y) * t;
            if !self.step_ok(px, py, x, py, side, true) || !self.step_ok(x, py, x, y, side, true) {
                return false;
            }
            px = x;
            py = y;
        }
        true
    }

    /// `nearestPassable(px, py)` (02 §3.4): neutral passability, levels ignored, rings r = 1..10.
    pub fn nearest_passable(&self, p: V2) -> V2 {
        let x0 = ((p.x / TILE).floor() as i32).clamp(0, self.w - 1);
        let y0 = ((p.y / TILE).floor() as i32).clamp(0, self.h - 1);
        if self.passable(self.idx(x0, y0), 0) {
            return p;
        }
        for r in 1..=10 {
            let mut best = -1;
            let mut bd = f32::INFINITY;
            for y in y0 - r..=y0 + r {
                for x in x0 - r..=x0 + r {
                    if (x - x0).abs().max((y - y0).abs()) != r || !self.inside(x, y) {
                        continue;
                    }
                    let c = self.idx(x, y);
                    if !self.passable(c, 0) {
                        continue;
                    }
                    let d = self.center(c).dist(p);
                    if d < bd {
                        bd = d;
                        best = c;
                    }
                }
            }
            if best >= 0 {
                return self.center(best);
            }
        }
        p
    }

    /// `setBlock(cells, delta)`: +1 building placed, -1 removed.
    pub fn set_block(&mut self, cells: &[i32], delta: i8) {
        for &c in cells {
            if self.valid(c) {
                let b = &mut self.block[c as usize];
                *b = (*b as i16 + delta as i16).clamp(0, 255) as u8;
            }
        }
        self.version += 1;
        if delta < 0 {
            self.opened = self.version;
        }
    }

    /// `setOwner(cells, side, wall)`: side = faction idx + 1, 0 clears.
    pub fn set_owner(&mut self, cells: &[i32], side: u8, wall: bool) {
        for &c in cells {
            if !self.valid(c) {
                continue;
            }
            let i = c as usize;
            let old = self.wall_of[i] as usize;
            if old != 0 && old < self.walls.len() {
                self.walls[old] = self.walls[old].saturating_sub(1);
            }
            self.own_of[i] = side;
            self.wall_of[i] = if wall { side } else { 0 };
            if wall && side != 0 && (side as usize) < self.walls.len() {
                self.walls[side as usize] += 1;
            }
        }
        self.version += 1;
        if side == 0 {
            self.opened = self.version;
        }
    }

    /// `resetBlock()`: only static obstacles remain (buildings are re-added with set_block/set_owner).
    pub fn reset_block(&mut self) {
        self.block.copy_from_slice(&self.static_block);
        self.wall_of.iter_mut().for_each(|v| *v = 0);
        self.own_of.iter_mut().for_each(|v| *v = 0);
        self.walls = [0; 6];
        self.version += 1;
        self.opened = self.version;
    }

    /// Cells of a `w x h` footprint with top-left `(cx, cy)`; `None` if any cell is outside the map.
    pub fn footprint(&self, cx: i32, cy: i32, w: i32, h: i32) -> Option<Vec<i32>> {
        let mut out = Vec::with_capacity((w * h).max(0) as usize);
        for y in cy..cy + h {
            for x in cx..cx + w {
                if !self.inside(x, y) {
                    return None;
                }
                out.push(self.idx(x, y));
            }
        }
        Some(out)
    }

    /// `canPlace(kind, cx, cy, own)` (spec 04a §9.1): inside, passable (side 0) unless the cell belongs only to
    /// the building being moved (`own` cells with `block == 1`), no stair, no `noBuild`, one single level.
    pub fn can_place(&self, kind: BuildingKind, cx: i32, cy: i32, own: &[i32]) -> bool {
        let d = kind.def();
        let Some(cells) = self.footprint(cx, cy, d.w, d.h) else {
            return false;
        };
        let lvl = self.level_at(cells[0]);
        cells.iter().all(|&c| {
            let i = c as usize;
            let mine = own.contains(&c) && self.block[i] == 1;
            (mine || self.passable(c, 0))
                && self.stair[i] == 0
                && self.no_build[i] == 0
                && self.level[i] == lvl
        })
    }

    /// `ring(cells, side)` (02 §4.4): passable 8-neighbours of a footprint on the level of `cells[0]`,
    /// insertion-ordered, without duplicates.
    pub fn ring(&self, cells: &[i32], side: u8) -> Vec<i32> {
        let mut out: Vec<i32> = Vec::new();
        let Some(&first) = cells.first() else {
            return out;
        };
        let lvl = self.level_at(first);
        for &c in cells {
            let (x, y) = self.xy(c);
            for (dx, dy) in DIRS8 {
                if !self.inside(x + dx, y + dy) {
                    continue;
                }
                let n = self.idx(x + dx, y + dy);
                if !cells.contains(&n)
                    && self.passable(n, side)
                    && self.level_at(n) == lvl
                    && !out.contains(&n)
                {
                    out.push(n);
                }
            }
        }
        out
    }

    /// FNV-1a hash of the generated gameplay layers (client verifies its regenerated map against the server's).
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |v: &[u8]| {
            for b in v {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        };
        eat(&self.land);
        eat(&self.level);
        eat(&self.cliff);
        eat(&self.stair);
        eat(&self.no_build);
        eat(&self.static_block);
        h
    }
}

impl Default for Map {
    fn default() -> Self {
        Map::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat() -> Map {
        let mut m = Map::with_size(10, 10);
        m.land.iter_mut().for_each(|v| *v = 1);
        m
    }

    #[test]
    fn passability_and_owner() {
        let mut m = flat();
        let c = m.idx(3, 3);
        assert!(m.passable(c, 1));
        m.set_block(&[c], 1);
        m.set_owner(&[c], 1, true);
        assert!(!m.passable(c, 0));
        assert!(m.passable(c, 1));
        assert!(!m.passable(c, 2));
        assert!(m.crossable(c, 1));
        assert_eq!(m.walls[1], 1);
        m.set_owner(&[c], 0, false);
        m.set_block(&[c], -1);
        assert_eq!(m.walls[1], 0);
        assert!(m.passable(c, 0));
        assert!(!m.passable(-1, 0));
    }

    #[test]
    fn try_move_slides() {
        let mut m = flat();
        let c = m.idx(2, 1);
        m.block[c as usize] = 1;
        // moving east from (1,1) into the blocked cell is refused, the y move still happens
        let p = m.try_move(v2(100.0, 96.0), 20.0, 5.0, 0, false);
        assert_eq!(p, v2(100.0, 101.0));
        // ring 1 around (2,1): first strict minimum in y-then-x scan order is (2,0)
        assert_eq!(m.nearest_passable(v2(160.0, 96.0)), v2(160.0, 32.0));
    }
}
