// OWNER: C1 (terrain) — CPU side of the voxel terrain: cell heights (levels, low beaches, stair ramps), the exact
// ground height used by picking / sprites, and the chunk mesher (visible faces only, Minecraft-style corner AO,
// per-block colour jitter, texture-array layer per face in UV1).
//
// Geometry rules (rendered from the `Map` data, no generator assumptions):
// * every `land > 0` cell is a column whose flat top is at `level_y(level)` (Sand on level 0 sits lower, at
//   `SAND_Y`, so beaches slope gently into the sea); water cells have no column (the water plane covers them);
// * a stair LOWER half (`stair` odd) that touches its UPPER half (`stair` even, one level higher) through a side is a
//   ramp: its edge against the upper half is at the upper level, the opposite edge at its own level;
// * a side face is emitted wherever the neighbour's surface is lower along the shared edge, down to the neighbour
//   (or the water plane); faces are split in bands of one level: the bottom band carries the foot texture (grass
//   tufts on land, foam lip in water), the bands above plain rock; left/right end caps where the wall run ends.
//   Cliff cells (`cliff = 1`) need no special case: they are simply the lower ground in front of a wall.

use crate::render::consts::{GROUND_Y, LEVEL_HEIGHT, WATER_Y, level_y};
use super::atlas::{
    WallCap, WallKind, flat_layer, overlay_priority, ramp_layer, surface_index, top_layer, wall_layer,
};
use bevy::prelude::*;
use ts_sim::map::{Map, Surface};

/// Cells per chunk side.
pub const CHUNK: i32 = 32;
/// Top of level-0 sand (beaches): a little above the water plane.
pub const SAND_Y: f32 = WATER_Y + 0.1;
/// Highest terrain point (levels 0..3) — ray marching starts here.
pub const MAX_TERRAIN_Y: f32 = GROUND_Y + 3.0 * LEVEL_HEIGHT;
const EPS: f32 = 1e-3;

/// Flat top height of a land cell (`None` = water / outside).
#[inline]
pub fn flat_top(map: &Map, cx: i32, cy: i32) -> Option<f32> {
    if !map.inside(cx, cy) {
        return None;
    }
    let i = map.idx(cx, cy) as usize;
    if map.land[i] == 0 {
        return None;
    }
    let lvl = map.level[i];
    if lvl == 0 && map.surface[i] == Surface::Sand {
        return Some(SAND_Y);
    }
    Some(level_y(lvl))
}

/// Direction from a stair lower half to its upper half (`(dx, dy)`), if the cell is a ramp.
#[inline]
pub fn ramp_dir(map: &Map, cx: i32, cy: i32) -> Option<(i32, i32)> {
    if !map.inside(cx, cy) {
        return None;
    }
    let i = map.idx(cx, cy) as usize;
    let st = map.stair[i];
    if st == 0 || st % 2 == 0 || map.land[i] == 0 {
        return None;
    }
    let lvl = map.level[i];
    // the upper half is normally north of the lower half (02 §5.8); accept any side
    for (dx, dy) in [(0, -1), (1, 0), (-1, 0), (0, 1)] {
        let (nx, ny) = (cx + dx, cy + dy);
        if !map.inside(nx, ny) {
            continue;
        }
        let j = map.idx(nx, ny) as usize;
        if map.stair[j] != 0 && map.stair[j] % 2 == 0 && map.land[j] != 0 && map.level[j] > lvl {
            return Some((dx, dy));
        }
    }
    None
}

/// Corner heights `[NW, NE, SE, SW]` of a land cell (`None` = water / outside).
pub fn corners(map: &Map, cx: i32, cy: i32) -> Option<[f32; 4]> {
    let base = flat_top(map, cx, cy)?;
    let Some((dx, dy)) = ramp_dir(map, cx, cy) else {
        return Some([base; 4]);
    };
    let hi = flat_top(map, cx + dx, cy + dy).unwrap_or(base).max(base);
    let lo = level_y(map.level[map.idx(cx, cy) as usize]);
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
#[derive(Default, Debug)]
pub struct ChunkMesh {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    /// tile UV (0..1 per face)
    pub uv: Vec<[f32; 2]>,
    /// `x` = texture-array layer, `y` = layer shown through transparent texels (-1 = none)
    pub uv1: Vec<[f32; 2]>,
    /// linear vertex colour (AO × jitter)
    pub col: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
}

impl ChunkMesh {
    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    fn vert(&mut self, p: Vec3, n: Vec3, uv: Vec2, layer: u32, under: f32, shade: f32) {
        self.pos.push(p.to_array());
        self.nrm.push(n.to_array());
        self.uv.push(uv.to_array());
        self.uv1.push([layer as f32, under]);
        self.col.push([shade, shade, shade, 1.0]);
    }

    /// Convex polygon (counter-clockwise seen from the side `n` points to) as a triangle fan.
    fn polygon(&mut self, pts: &[(Vec3, Vec2, f32)], n: Vec3, layer: u32) {
        if pts.len() < 3 {
            return;
        }
        let base = self.pos.len() as u32;
        for (p, uv, s) in pts {
            self.vert(*p, n, *uv, layer, -1.0, *s);
        }
        for k in 1..pts.len() as u32 - 1 {
            self.idx.extend([base, base + k, base + k + 1]);
        }
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

/// Side directions: (dx, dy, outward normal). Order N, E, S, W (bit 0..3 of the top edge mask).
const SIDES: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

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

/// Corner `k` of `self` coincides with corner `match_corner(s, k)` of the neighbour across side `s`.
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
            let i = map.idx(cx, cy) as usize;
            let surf = surface_index(map.surface[i]);
            let hsh = cell_hash(cx, cy);
            let jitter = 0.95 + (hsh & 0xff) as f32 / 255.0 * 0.1;
            top_face(&mut m, map, cx, cy, c, surf, hsh, jitter);
            for s in 0..4 {
                side_face(&mut m, map, cx, cy, c, s, jitter);
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
) {
    let ramp = ramp_dir(map, cx, cy);
    // edge masks (N=1, E=2, S=4, W=8): `lower` = neighbour surface lower along the shared edge (rock rim);
    // `ground` = same height but a lower-priority surface (this cell draws its outline over it)
    let mut mask = 0u32;
    let mut ground = 0u32;
    let mut under: Option<usize> = None;
    if ramp.is_none() {
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
                    } else if nh < own + EPS && ramp_dir(map, nx, ny).is_none() {
                        let ns = surface_index(map.surface[map.idx(nx, ny) as usize]);
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
    }
    let (layer, under) = match (ramp, under.and_then(|u| flat_layer(surf, mask | ground).map(|l| (l, u)))) {
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
    // UVs: texture up = north; interior tiles get random flips (no visible repetition)
    let mut uvs = [
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    match ramp {
        Some((dx, dy)) => {
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
            if hsh & 0x200 != 0 {
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
        m.vert(p[k], n, uvs[k], layer, under, shade[k]);
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
    clip(&p, |v| y1 - v.y)
}

fn side_face(
    m: &mut ChunkMesh,
    map: &Map,
    cx: i32,
    cy: i32,
    c: [f32; 4],
    s: usize,
    jitter: f32,
) {
    let (dx, dy) = SIDES[s];
    let (ka, kb) = SIDE_CORNERS[s];
    let (ta, tb) = (c[ka], c[kb]);
    let nb = corners(map, cx + dx, cy + dy);
    let wet = nb.is_none();
    let (ba, bb) = match nb {
        Some(n) => (n[across(s, ka)].max(WATER_Y), n[across(s, kb)].max(WATER_Y)),
        None => (WATER_Y, WATER_Y),
    };
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
            // v: bottom band anchored at its bottom (foot on the ground), upper bands at their top
            let pts: Vec<(Vec3, Vec2, f32)> = part
                .iter()
                .map(|q| {
                    let xz = pa + (pb - pa) * q.x;
                    let v = if first {
                        1.0 - (q.y - y) / LEVEL_HEIGHT
                    } else {
                        (y1 - q.y) / LEVEL_HEIGHT
                    };
                    // contact darkening near the foot, lighter at the lip
                    let k = ((q.y - bottom) / LEVEL_HEIGHT).clamp(0.0, 1.0);
                    let shade = (0.62 + 0.38 * k) * jitter;
                    (Vec3::new(xz.x, q.y, xz.y), Vec2::new(q.x, v), shade)
                })
                .collect();
            // polygon is CCW in (u, y) seen from outside (u to the right, y up) → CCW in world from outside
            m.polygon(&pts, normal, layer);
        }
        y = y1;
        band += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn island() -> Map {
        let mut m = Map::with_size(8, 8);
        for y in 1..7 {
            for x in 1..7 {
                let i = m.idx(x, y) as usize;
                m.land[i] = 1;
                m.surface[i] = Surface::GrassLow;
            }
        }
        // plateau rows 2..3, x 2..5
        for y in 2..4 {
            for x in 2..6 {
                let i = m.idx(x, y) as usize;
                m.level[i] = 1;
            }
        }
        // stair left of (2,3): hi (1,3) level 1 code 2, lo (1,4) code 1
        let hi = m.idx(1, 3) as usize;
        let lo = m.idx(1, 4) as usize;
        m.stair[hi] = 2;
        m.level[hi] = 1;
        m.stair[lo] = 1;
        m
    }

    #[test]
    fn heights_and_ramps() {
        let m = island();
        assert_eq!(ground_height(&m, 0.5, 0.5), WATER_Y);
        assert_eq!(ground_height(&m, 1.5, 1.5), level_y(0));
        assert_eq!(ground_height(&m, 3.5, 2.5), level_y(1));
        assert_eq!(ramp_dir(&m, 1, 4), Some((0, -1)));
        // ramp: north edge at level 1, south edge at level 0
        assert!((ground_height(&m, 1.5, 4.0) - level_y(1)).abs() < 1e-4);
        assert!((ground_height(&m, 1.5, 5.0 - 1e-4) - level_y(0)).abs() < 1e-3);
        let mid = ground_height(&m, 1.5, 4.5);
        assert!(mid > level_y(0) && mid < level_y(1));
        assert_eq!(ground_height(&m, -5.0, 3.0), WATER_Y);
    }

    #[test]
    fn chunk_has_faces_and_valid_indices() {
        let m = island();
        let c = build_chunk(&m, 0, 0);
        assert!(!c.is_empty());
        assert_eq!(c.pos.len(), c.uv.len());
        assert_eq!(c.pos.len(), c.uv1.len());
        assert!(c.idx.iter().all(|&i| (i as usize) < c.pos.len()));
        assert!(c.idx.len() % 3 == 0);
        // the plateau south wall must face +Z
        let has_south_wall = c
            .nrm
            .iter()
            .zip(&c.pos)
            .any(|(n, p)| n[2] > 0.9 && (p[2] - 4.0).abs() < 1e-4 && p[1] > level_y(0) + 0.5);
        assert!(has_south_wall);
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
