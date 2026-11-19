// OWNER: C1 (terrain) — CPU side of the voxel terrain: cell heights (levels, low beaches, cliff columns, stair
// ramps), the exact ground height used by picking / sprites, and the chunk mesher (visible faces only,
// Minecraft-style corner AO, per-block colour jitter, texture-array layer per face in UV1).
//
// Geometry rules — rendered from the `Map` data following the renderer contract in `ts_sim::map` (module docs):
// * every `land > 0` cell is a column whose flat top is at `level_y(level)` (Sand on level 0 sits lower, at
//   `SAND_Y`, so beaches slope gently into the sea); water cells have no column (the water plane covers them);
// * a cliff cell (`cliff = 1`, the cell just south of a raised cell, on land or in the sea) is a rock column rising
//   to `Map::cliff_top` (the rim of the tier above): its top continues the plateau, its south face is the wall —
//   exactly where the 2D art paints the rock;
// * stairs: the lower half (`stair` odd) and the upper half (`stair` even, one level higher) form ONE slope, from the
//   lower level at the lower half's far edge to the upper level at the upper half's far edge (each half rises by
//   half a level);
// * a side face is emitted wherever the neighbour's surface is lower along the shared edge, down to the neighbour
//   (or the water plane); faces are split in bands of one level: the bottom band carries the foot texture (grass
//   tufts on land, foam lip in water), the bands above plain rock; left/right end caps where the wall run ends.
//   South walls of real cliffs (≥ half a level) also get the grass lip of the tier above (spec 09 §3.7: strip
//   y 40..63 of the elevated bottom-edge cell, sampled by the shader from the top-face layer of that edge).

use super::atlas::{
    WallCap, WallKind, flat_layer, overlay_priority, ramp_layer, surface_index, top_layer, wall_layer,
};
use crate::render::consts::{GROUND_Y, LEVEL_HEIGHT, WATER_Y, level_y};
use bevy::prelude::*;
use ts_sim::map::{Map, Surface};

/// Cells per chunk side.
pub const CHUNK: i32 = 32;
/// Top of level-0 sand (beaches): a little above the water plane.
pub const SAND_Y: f32 = WATER_Y + 0.1;
/// Highest terrain point (levels 0..3) — ray marching starts here.
pub const MAX_TERRAIN_Y: f32 = GROUND_Y + 3.0 * LEVEL_HEIGHT;
/// Height of the grass lip drawn at the top of south cliff walls (24 of 64 px, in tile units).
pub const LIP_H: f32 = 24.0 / 64.0;
/// Minimum drop of a south wall to carry a grass lip (lower steps keep the rim on the top face).
const LIP_MIN_DROP: f32 = 0.5 * LEVEL_HEIGHT;
/// `color.g` of vertices without a lip (any value ≥ 1).
const NO_LIP: f32 = 9.0;
const EPS: f32 = 1e-3;

/// Flat top height of a cell's column (`None` = water without cliff / outside).
#[inline]
pub fn flat_top(map: &Map, cx: i32, cy: i32) -> Option<f32> {
    if !map.inside(cx, cy) {
        return None;
    }
    let c = map.idx(cx, cy);
    let i = c as usize;
    if map.cliff[i] != 0 {
        return map.cliff_top(c).map(level_y);
    }
    if map.land[i] == 0 {
        return None;
    }
    let lvl = map.level[i];
    if lvl == 0 && map.surface[i] == Surface::Sand {
        return Some(SAND_Y);
    }
    Some(level_y(lvl))
}

/// Ground material shown on a column: cliff cells in the sea (surface `Water`) take the tier above them.
pub fn cell_surface(map: &Map, cx: i32, cy: i32) -> Surface {
    if !map.inside(cx, cy) {
        return Surface::Water;
    }
    let i = map.idx(cx, cy) as usize;
    let s = map.surface[i];
    if s != Surface::Water {
        return s;
    }
    if map.cliff[i] != 0 && map.inside(cx, cy - 1) {
        let n = map.surface[map.idx(cx, cy - 1) as usize];
        if n != Surface::Water {
            return n;
        }
    }
    Surface::GrassMid
}

/// Stair slope of a cell: `(direction towards the high end, height of the high edge, height of the low edge)`.
/// The two halves of a stair make one slope: lower half `lo → mid`, upper half `mid → hi`.
pub fn ramp(map: &Map, cx: i32, cy: i32) -> Option<((i32, i32), f32, f32)> {
    if !map.inside(cx, cy) {
        return None;
    }
    let i = map.idx(cx, cy) as usize;
    let st = map.stair[i];
    if st == 0 || map.land[i] == 0 || map.cliff[i] != 0 {
        return None;
    }
    let own = flat_top(map, cx, cy)?;
    let lvl = map.level[i];
    // the upper half is normally north of the lower half (02 §5.8, map docs); accept any side
    for (dx, dy) in [(0, -1), (0, 1), (1, 0), (-1, 0)] {
        let (nx, ny) = (cx + dx, cy + dy);
        if !map.inside(nx, ny) {
            continue;
        }
        let j = map.idx(nx, ny) as usize;
        let sj = map.stair[j];
        if sj == 0 || map.land[j] == 0 || map.cliff[j] != 0 {
            continue;
        }
        let other = flat_top(map, nx, ny).unwrap_or(own);
        if st % 2 == 1 {
            if sj % 2 == 0 && map.level[j] > lvl && other > own {
                return Some(((dx, dy), (own + other) * 0.5, own));
            }
        } else if sj % 2 == 1 && map.level[j] < lvl && other < own {
            return Some(((-dx, -dy), own, (own + other) * 0.5));
        }
    }
    None
}

/// Corner heights `[NW, NE, SE, SW]` of a column (`None` = water / outside).
pub fn corners(map: &Map, cx: i32, cy: i32) -> Option<[f32; 4]> {
    let base = flat_top(map, cx, cy)?;
    let Some(((dx, dy), hi, lo)) = ramp(map, cx, cy) else {
        return Some([base; 4]);
    };
    Some(match (dx, dy) {
        (0, -1) => [hi, hi, lo, lo],
        (0, 1) => [lo, lo, hi, hi],
        (1, 0) => [lo, hi, hi, lo],
        _ => [hi, lo, lo, hi],
    })
}

/// Highest corner of a cell (water → `WATER_Y`).
#[inline]
fn top_max(map: &Map, cx: i32, cy: i32) -> f32 {
    corners(map, cx, cy).map_or(WATER_Y, |c| c[0].max(c[1]).max(c[2]).max(c[3]))
}

/// Exact terrain surface height (world units) at world `(x, z)`; water / outside the map → `WATER_Y`.
pub fn ground_height(map: &Map, x: f32, z: f32) -> f32 {
    if !x.is_finite() || !z.is_finite() {
        return WATER_Y;
    }
    let (cx, cy) = (x.floor() as i32, z.floor() as i32);
    let Some(c) = corners(map, cx, cy) else {
        return WATER_Y;
    };
    if c[0] == c[1] && c[1] == c[2] && c[2] == c[3] {
        return c[0];
    }
    let (fx, fz) = (x - cx as f32, z - cy as f32);
    let n = c[0] + (c[1] - c[0]) * fx;
    let s = c[3] + (c[2] - c[3]) * fx;
    n + (s - n) * fz
}

/// Deterministic per-cell hash (colour jitter, UV flips).
#[inline]
pub fn cell_hash(cx: i32, cy: i32) -> u32 {
    let mut h = (cx as u32).wrapping_mul(0x8da6_b343) ^ (cy as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^ (h >> 16)
}

/// Mesh buffers of one chunk.
///
/// Vertex layout read by `shaders/terrain.wesl`:
/// * `uv`: tile UV (0..1 per face; walls: `v` per one-level band);
/// * `uv1.x`: texture-array layer; `uv1.y`: top faces → layer shown through transparent texels of an overlay tile,
///   walls → layer of the grass lip (top-face layer of the rim above); `-1` = none;
/// * `col.r`: brightness (AO × jitter × contact shade); `col.g`: walls → distance below the wall top in tiles
///   (lip where `< LIP_H`), others `NO_LIP`; `col.b`, `col.a` unused (1).
#[derive(Default, Debug)]
pub struct ChunkMesh {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub uv1: Vec<[f32; 2]>,
    pub col: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
}

impl ChunkMesh {
    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    #[allow(clippy::too_many_arguments)]
    fn vert(&mut self, p: Vec3, n: Vec3, uv: Vec2, layer: u32, aux: f32, shade: f32, lip_t: f32) {
        self.pos.push(p.to_array());
        self.nrm.push(n.to_array());
        self.uv.push(uv.to_array());
        self.uv1.push([layer as f32, aux]);
        self.col.push([shade, lip_t, 1.0, 1.0]);
    }
}

/// Minecraft corner AO → brightness.
const AO_LEVELS: [f32; 4] = [1.0, 0.8, 0.66, 0.54];

#[inline]
fn ao(side1: bool, side2: bool, corner: bool) -> usize {
    if side1 && side2 {
        3
    } else {
        side1 as usize + side2 as usize + corner as usize
    }
}

/// Side directions: (dx, dy). Order N, E, S, W (bit 0..3 of the top edge mask).
const SIDES: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
const SOUTH: usize = 2;

/// The two corners (indices into `[NW, NE, SE, SW]`) of side `s`, ordered LEFT → RIGHT as seen from outside.
const SIDE_CORNERS: [(usize, usize); 4] = [(1, 0), (2, 1), (3, 2), (0, 3)];

/// World XZ of corner `k` of cell (cx, cy).
#[inline]
fn corner_xz(cx: i32, cy: i32, k: usize) -> (f32, f32) {
    let (x0, z0) = (cx as f32, cy as f32);
    match k {
        0 => (x0, z0),
        1 => (x0 + 1.0, z0),
        2 => (x0 + 1.0, z0 + 1.0),
        _ => (x0, z0 + 1.0),
    }
}

/// Corner `k` of `self` coincides with corner `across(s, k)` of the neighbour across side `s`.
#[inline]
fn across(s: usize, k: usize) -> usize {
    match (s, k) {
        (0, 0) => 3,
        (0, 1) => 2,
        (2, 3) => 0,
        (2, 2) => 1,
        (1, 1) => 0,
        (1, 2) => 3,
        (3, 0) => 1,
        (3, 3) => 2,
        _ => k,
    }
}

/// Bottom heights `(left, right)` of side `s` (neighbour surface or the water plane) and whether it is in the sea.
#[inline]
fn side_bottom(map: &Map, cx: i32, cy: i32, s: usize) -> (f32, f32, bool) {
    let (dx, dy) = SIDES[s];
    let (ka, kb) = SIDE_CORNERS[s];
    match corners(map, cx + dx, cy + dy) {
        Some(n) => (
            n[across(s, ka)].max(WATER_Y),
            n[across(s, kb)].max(WATER_Y),
            false,
        ),
        None => (WATER_Y, WATER_Y, true),
    }
}

/// The south wall of this flat column is a real cliff (gets the grass lip; the top face drops its south rim).
#[inline]
fn south_lip(map: &Map, cx: i32, cy: i32, c: [f32; 4], is_ramp: bool) -> bool {
    if is_ramp {
        return false;
    }
    let (ka, kb) = SIDE_CORNERS[SOUTH];
    let (ba, bb, _) = side_bottom(map, cx, cy, SOUTH);
    c[ka] - ba >= LIP_MIN_DROP && c[kb] - bb >= LIP_MIN_DROP
}

/// Builds the mesh of chunk `(ccx, ccy)` (cells `ccx*CHUNK ..`, `ccy*CHUNK ..`).
pub fn build_chunk(map: &Map, ccx: i32, ccy: i32) -> ChunkMesh {
    let mut m = ChunkMesh::default();
    let x0 = ccx * CHUNK;
    let y0 = ccy * CHUNK;
    for cy in y0..(y0 + CHUNK).min(map.h) {
        for cx in x0..(x0 + CHUNK).min(map.w) {
            let Some(c) = corners(map, cx, cy) else {
                continue;
            };
            let surf = surface_index(cell_surface(map, cx, cy));
            let hsh = cell_hash(cx, cy);
            let jitter = 0.955 + (hsh & 0xff) as f32 / 255.0 * 0.09;
            let rmp = ramp(map, cx, cy);
            let lip = south_lip(map, cx, cy, c, rmp.is_some());
            top_face(&mut m, map, cx, cy, c, surf, hsh, jitter, rmp, lip);
            for s in 0..4 {
                side_face(&mut m, map, cx, cy, c, s, jitter, surf, lip && s == SOUTH);
            }
        }
    }
    m
}

#[allow(clippy::too_many_arguments)]
fn top_face(
    m: &mut ChunkMesh,
    map: &Map,
    cx: i32,
    cy: i32,
    c: [f32; 4],
    surf: usize,
    hsh: u32,
    jitter: f32,
    rmp: Option<((i32, i32), f32, f32)>,
    lip: bool,
) {
    // edge masks (N=1, E=2, S=4, W=8): `lower` = neighbour surface lower along the shared edge (rock rim);
    // `ground` = same height but a lower-priority surface (this cell draws its outline over it)
    let mut mask = 0u32;
    let mut ground = 0u32;
    let mut under: Option<usize> = None;
    if rmp.is_none() {
        for (s, (dx, dy)) in SIDES.iter().enumerate() {
            let (a, b) = SIDE_CORNERS[s];
            let own = c[a].min(c[b]);
            let (nx, ny) = (cx + dx, cy + dy);
            match corners(map, nx, ny) {
                None => mask |= 1 << s,
                Some(n) => {
                    let nh = n[across(s, a)].max(n[across(s, b)]);
                    if nh < own - EPS {
                        mask |= 1 << s;
                    } else if nh < own + EPS && ramp(map, nx, ny).is_none() {
                        let ns = surface_index(cell_surface(map, nx, ny));
                        if overlay_priority(ns) < overlay_priority(surf) {
                            ground |= 1 << s;
                            if under.is_none_or(|u| overlay_priority(ns) < overlay_priority(u)) {
                                under = Some(ns);
                            }
                        }
                    }
                }
            }
        }
        // the south rim of a real cliff is drawn on the wall (grass lip), not on the top face
        if lip {
            mask &= !(1 << SOUTH);
        }
    }
    let (layer, under) = match (
        rmp,
        under.and_then(|u| flat_layer(surf, mask | ground).map(|l| (l, u))),
    ) {
        (Some(_), _) => (ramp_layer(surf), -1.0),
        (None, Some((l, u))) if ground != 0 => (l, top_layer(u, 0) as f32),
        _ => (top_layer(surf, mask), -1.0),
    };
    // AO per corner: occluders are neighbour columns rising above this corner
    let occl = |dx: i32, dy: i32, h: f32| top_max(map, cx + dx, cy + dy) > h + 0.05;
    let corner_dirs = [(-1, -1), (1, -1), (1, 1), (-1, 1)];
    let mut shade = [1.0f32; 4];
    let mut aos = [0usize; 4];
    for k in 0..4 {
        let (dx, dy) = corner_dirs[k];
        let a = ao(occl(dx, 0, c[k]), occl(0, dy, c[k]), occl(dx, dy, c[k]));
        aos[k] = a;
        shade[k] = AO_LEVELS[a] * jitter;
    }
    // UVs: texture up = north; interior tiles get random flips (no visible repetition), except above a lip (the
    // lip continues the tile's bottom rows)
    let mut uvs = [
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    match rmp {
        Some(((dx, dy), _, _)) => {
            // texture top (v = 0) on the high edge
            uvs = match (dx, dy) {
                (0, -1) => uvs,
                (0, 1) => [uvs[2], uvs[3], uvs[0], uvs[1]],
                (1, 0) => [uvs[3], uvs[0], uvs[1], uvs[2]],
                _ => [uvs[1], uvs[2], uvs[3], uvs[0]],
            };
        }
        None if mask == 0 && ground == 0 => {
            if hsh & 0x100 != 0 {
                for u in &mut uvs {
                    u.x = 1.0 - u.x;
                }
            }
            if hsh & 0x200 != 0 && !lip {
                for u in &mut uvs {
                    u.y = 1.0 - u.y;
                }
            }
        }
        None => {}
    }
    let p: [Vec3; 4] = std::array::from_fn(|k| {
        let (x, z) = corner_xz(cx, cy, k);
        Vec3::new(x, c[k], z)
    });
    let n = (p[3] - p[0]).cross(p[1] - p[0]).normalize_or(Vec3::Y);
    let base = m.pos.len() as u32;
    for k in 0..4 {
        m.vert(p[k], n, uvs[k], layer, under, shade[k], NO_LIP);
    }
    // CCW seen from above: NW, SW, SE, NE; flip the diagonal against AO anisotropy
    if aos[0] + aos[2] > aos[1] + aos[3] {
        m.idx
            .extend([base + 1, base + 3, base + 2, base + 1, base, base + 3]);
    } else {
        m.idx
            .extend([base, base + 3, base + 2, base, base + 2, base + 1]);
    }
}

/// Sutherland–Hodgman clip of a convex polygon (in face-local (u, y) space) to `y0 <= y <= y1`.
fn clip_band(poly: &[Vec2], y0: f32, y1: f32) -> Vec<Vec2> {
    fn clip(poly: &[Vec2], keep: impl Fn(Vec2) -> f32) -> Vec<Vec2> {
        let mut out = Vec::with_capacity(poly.len() + 2);
        for i in 0..poly.len() {
            let a = poly[i];
            let b = poly[(i + 1) % poly.len()];
            let (da, db) = (keep(a), keep(b));
            if da >= 0.0 {
                out.push(a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                let t = da / (da - db);
                out.push(a + (b - a) * t);
            }
        }
        out
    }
    let p = clip(poly, |v| v.y - y0);
    if p.len() < 3 {
        return p;
    }
    let mut out = clip(&p, |v| y1 - v.y);
    // drop repeated vertices (a polygon vertex lying exactly on a clip line is emitted twice)
    out.dedup_by(|a, b| a.distance_squared(*b) < 1e-10);
    while out.len() > 1 && out[0].distance_squared(out[out.len() - 1]) < 1e-10 {
        out.pop();
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn side_face(
    m: &mut ChunkMesh,
    map: &Map,
    cx: i32,
    cy: i32,
    c: [f32; 4],
    s: usize,
    jitter: f32,
    surf: usize,
    lip: bool,
) {
    let (dx, dy) = SIDES[s];
    let (ka, kb) = SIDE_CORNERS[s];
    let (ta, tb) = (c[ka], c[kb]);
    let (ba, bb, wet) = side_bottom(map, cx, cy, s);
    if ta <= ba + EPS && tb <= bb + EPS {
        return;
    }
    let normal = Vec3::new(dx as f32, 0.0, dy as f32);
    let (ax, az) = corner_xz(cx, cy, ka);
    let (bx, bz) = corner_xz(cx, cy, kb);
    let pa = Vec2::new(ax, az);
    let pb = Vec2::new(bx, bz);
    // wall caps: the run ends where the cell beside this one (along the face) is lower than this wall's top
    let (lx, ly) = SIDES[(s + 1) % 4]; // left as seen from outside
    let (rx, ry) = SIDES[(s + 3) % 4];
    let top = ta.max(tb);
    let left_open = top_max(map, cx + lx, cy + ly) < top - EPS;
    let right_open = top_max(map, cx + rx, cy + ry) < top - EPS;
    let cap = match (left_open, right_open) {
        (true, true) => WallCap::Pillar,
        (true, false) => WallCap::Left,
        (false, true) => WallCap::Right,
        _ => WallCap::Mid,
    };
    // grass lip: the elevated bottom-edge cell of the rim (S + the open ends), rows 40..63
    let lip_layer = if lip {
        let ends = match cap {
            WallCap::Left => 8,
            WallCap::Right => 2,
            WallCap::Pillar => 10,
            WallCap::Mid => 0,
        };
        top_layer(surf, 4 | ends) as f32
    } else {
        -1.0
    };
    // face polygon in local (u, y): u = 0 at the left corner
    let mut poly: Vec<Vec2> = Vec::with_capacity(4);
    poly.push(Vec2::new(0.0, ba));
    poly.push(Vec2::new(1.0, bb));
    poly.push(Vec2::new(1.0, tb.max(bb)));
    poly.push(Vec2::new(0.0, ta.max(ba)));
    poly.dedup_by(|a, b| a.distance_squared(*b) < 1e-10);
    let bottom = ba.min(bb);
    let mut y = bottom;
    let mut band = 0;
    while y < top - EPS && band < 8 {
        let y1 = (y + LEVEL_HEIGHT).min(top);
        let first = band == 0;
        let kind = if first {
            if wet { WallKind::Wet } else { WallKind::Foot }
        } else {
            WallKind::Plain
        };
        let layer = wall_layer(kind, cap);
        let part = clip_band(&poly, y, y1);
        if part.len() >= 3 {
            let base = m.pos.len() as u32;
            for q in &part {
                let xz = pa + (pb - pa) * q.x;
                // v: bottom band anchored at its bottom (foot on the ground), upper bands at their top
                let v = if first {
                    1.0 - (q.y - y) / LEVEL_HEIGHT
                } else {
                    (y1 - q.y) / LEVEL_HEIGHT
                };
                // contact darkening near the foot, lighter at the lip
                let k = ((q.y - bottom) / LEVEL_HEIGHT).clamp(0.0, 1.0);
                let shade = (0.66 + 0.34 * k) * jitter;
                let wall_top = ta + (tb - ta) * q.x;
                let lip_t = if lip {
                    ((wall_top - q.y) / LEVEL_HEIGHT).max(0.0)
                } else {
                    NO_LIP
                };
                m.vert(
                    Vec3::new(xz.x, q.y, xz.y),
                    normal,
                    Vec2::new(q.x, v),
                    layer,
                    lip_layer,
                    shade,
                    lip_t,
                );
            }
            // polygon is CCW in (u, y) seen from outside (u to the right, y up) → CCW in world from outside
            for k in 1..part.len() as u32 - 1 {
                m.idx.extend([base, base + k, base + k + 1]);
            }
        }
        y = y1;
        band += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 8×8 island: level-0 grass ring, plateau rows 2..3 × x 2..5 (level 1) with its cliff row 4 below, a left stair
    /// (upper half (1,3), lower half (1,4)) and a sea cliff at (6,4)… (cliff cell in the water).
    fn island() -> Map {
        let mut m = Map::with_size(8, 8);
        for y in 1..7 {
            for x in 1..7 {
                let i = m.idx(x, y) as usize;
                m.land[i] = 1;
                m.surface[i] = Surface::GrassLow;
            }
        }
        for y in 2..4 {
            for x in 2..6 {
                let i = m.idx(x, y) as usize;
                m.level[i] = 1;
                m.surface[i] = Surface::GrassMid;
            }
        }
        for x in 2..6 {
            let i = m.idx(x, 4) as usize;
            m.cliff[i] = 1;
            m.surface[i] = Surface::GrassMid;
        }
        let hi = m.idx(1, 3) as usize;
        let lo = m.idx(1, 4) as usize;
        m.stair[hi] = 2;
        m.level[hi] = 1;
        m.stair[lo] = 1;
        m
    }

    #[test]
    fn heights_ramps_and_cliffs() {
        let m = island();
        assert_eq!(ground_height(&m, 0.5, 0.5), WATER_Y);
        assert_eq!(ground_height(&m, 1.5, 1.5), level_y(0));
        assert_eq!(ground_height(&m, 3.5, 2.5), level_y(1));
        // cliff cell: a rock column up to the plateau rim
        assert_eq!(ground_height(&m, 3.5, 4.5), level_y(1));
        assert_eq!(ground_height(&m, 3.5, 5.5), level_y(0));
        // stair: one slope over both halves
        let mid = (level_y(0) + level_y(1)) * 0.5;
        assert_eq!(ramp(&m, 1, 4).map(|r| r.0), Some((0, -1)));
        assert_eq!(ramp(&m, 1, 3).map(|r| r.0), Some((0, -1)));
        assert!((ground_height(&m, 1.5, 3.0) - level_y(1)).abs() < 1e-4);
        assert!((ground_height(&m, 1.5, 4.0) - mid).abs() < 1e-4);
        assert!((ground_height(&m, 1.5, 5.0 - 1e-4) - level_y(0)).abs() < 1e-3);
        assert_eq!(ground_height(&m, -5.0, 3.0), WATER_Y);
        assert_eq!(ground_height(&m, f32::NAN, 3.0), WATER_Y);
    }

    #[test]
    fn sea_cliff_is_a_column() {
        let mut m = island();
        // turn the cliff row into a sea cliff (cliff cells in the water, open sea south of them)
        for y in 4..7 {
            for x in 2..6 {
                let i = m.idx(x, y) as usize;
                m.land[i] = 0;
                m.surface[i] = Surface::Water;
            }
        }
        assert_eq!(ground_height(&m, 3.5, 4.5), level_y(1));
        assert_eq!(cell_surface(&m, 3, 4), Surface::GrassMid);
        let c = build_chunk(&m, 0, 0);
        // a wet wall facing south at z = 5 going down to the water plane
        assert!(
            c.nrm
                .iter()
                .zip(&c.pos)
                .any(|(n, p)| n[2] > 0.9 && (p[2] - 5.0).abs() < 1e-4 && p[1].abs() < 1e-4)
        );
    }

    #[test]
    fn chunk_has_faces_lips_and_valid_indices() {
        let m = island();
        let c = build_chunk(&m, 0, 0);
        assert!(!c.is_empty());
        assert_eq!(c.pos.len(), c.uv.len());
        assert_eq!(c.pos.len(), c.uv1.len());
        assert_eq!(c.pos.len(), c.col.len());
        assert!(c.idx.iter().all(|&i| (i as usize) < c.pos.len()));
        assert!(c.idx.len() % 3 == 0);
        // the cliff wall faces +Z at the cliff cells' south side, with a grass lip near its top
        let wall: Vec<usize> = (0..c.pos.len())
            .filter(|&i| c.nrm[i][2] > 0.9 && (c.pos[i][2] - 5.0).abs() < 1e-4 && (2.0..6.0).contains(&c.pos[i][0]))
            .collect();
        assert!(!wall.is_empty());
        assert!(wall.iter().any(|&i| c.pos[i][1] > level_y(1) - 1e-3 && c.col[i][1] < 1e-3));
        assert!(wall.iter().all(|&i| c.uv1[i][1] >= 0.0));
    }

    #[test]
    fn band_clip() {
        let poly = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 2.0),
        ];
        let b = clip_band(&poly, 1.0, 2.0);
        // triangle above y = 1
        assert_eq!(b.len(), 3);
    }
}
