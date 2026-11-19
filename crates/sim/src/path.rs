// OWNER: S1 (map) — see docs/spec/02-world.md §4 (movement graph and distance fields) + addenda W5, W6.
//
// Distance fields in tenths of a cell (10 orthogonal, 14 diagonal, +CROSS_COST per own-building cell in cross
// mode), `FIELD_INF` = unreachable.
//   * `move_mask` (§4.2): per-cell 8-bit neighbour mask, cached per `side*2 + cross` and invalidated when
//     `map.version` changes (`FieldCache::mask`).
//   * `compute_field` / `FieldCache::compute` (§4.3): Dial's algorithm with 64 circular buckets over the mask. The
//     cache keeps the buckets and recycles evicted distance arrays, so a recompute allocates nothing in steady state.
//   * `FieldCache` (§4.6): key -> field (hash index), insertion-order (FIFO) eviction at `FIELD_CACHE`, stale-field
//     policy with the per-tick `refresh` budget, `fresh` budget for `march` (formation.rs).
//   * `World::get_field` / `field_to_cell` / `field_for` (§4.5-§4.6 incl. the `crossed` fallback), `step_from`
//     (§4.7), `neighbor_ok` (§4.1).
//
// Usage pattern (no borrow trouble): `let f = world.field_for(FieldTarget::Building(b), side, here);`
// then `world.fields.dist(f.id, cell)` / `world.step_from(f.id, cell, side, f.crossed)`.
// A `FieldId` is valid only until the next `get_field`/`field_*` call (eviction may reuse the slot).

use crate::World;
use crate::defs::{CROSS_COST, DIRS8, FIELD_CACHE, FIELD_INF, FIELD_NEW, FIELD_REFRESH};
use crate::map::Map;
use crate::slab::{BuildingId, NodeId, PointId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What a distance field leads to (its seeds are the target's ring).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FieldTarget {
    /// `fieldToCell(c)`: seeds `[c]` if passable else `ring([c])`
    Cell(i32),
    /// `fieldFor(building)`: ring of the footprint
    Building(BuildingId),
    /// `fieldFor(node)`: ring of the node cells (tree/ore 1 cell, big deposit 2x2)
    Node(NodeId),
    /// ring of a capture point's 2x2
    Point(PointId),
}

/// Cache key: target + effective side + cross flag (02 addenda §10 Q6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldKey {
    pub target: FieldTarget,
    pub side: u8,
    pub cross: bool,
}

/// Handle of a cached field (slot index). Valid until the next field request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldId(pub u32);

/// Result of a field request: the field and whether the cross fallback was taken (`World.crossed` → `u.cross`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldRef {
    pub id: FieldId,
    pub crossed: bool,
}

#[derive(Clone, Debug)]
struct Entry {
    key: FieldKey,
    version: u32,
    stamp: u64,
    dist: Vec<u32>,
}

/// A cached `moveMask(side, cross)`.
#[derive(Clone, Debug)]
struct MaskEntry {
    /// `side*2 + cross`
    key: u16,
    version: u32,
    /// map size the mask was computed for (guards against a replaced map with the same version)
    len: usize,
    bits: Vec<u8>,
}

/// Bit `k` of a move mask ↔ neighbour offset `STEP[k]` (W, E, N, S, NW, NE, SW, SE), cost 10 or 14.
const fn step_offset(k: usize, w: i32) -> i32 {
    match k {
        0 => -1,
        1 => 1,
        2 => -w,
        3 => w,
        4 => -1 - w,
        5 => 1 - w,
        6 => -1 + w,
        _ => 1 + w,
    }
}

/// Reusable Dial buckets (64 circular queues).
#[derive(Clone, Debug)]
pub struct Buckets {
    q: Vec<Vec<u32>>,
}

impl Default for Buckets {
    fn default() -> Self {
        Buckets {
            q: (0..64).map(|_| Vec::new()).collect(),
        }
    }
}

/// Distance-field cache (`World.fields` + budgets `refresh`/`fresh`).
#[derive(Clone, Debug, Default)]
pub struct FieldCache {
    entries: Vec<Entry>,
    index: HashMap<FieldKey, u32>,
    stamp: u64,
    masks: Vec<MaskEntry>,
    buckets: Buckets,
    /// recycled distance arrays (from `clear`)
    spare: Vec<Vec<u32>>,
    /// scratch for seeds / target cells (no allocation per recompute)
    seeds: Vec<i32>,
    cells: Vec<i32>,
    /// stale-but-usable recomputes left this tick
    pub refresh: i32,
    /// group members that may still switch to their own path this tick (post-decrement, may go negative)
    pub fresh: i32,
}

enum Lookup {
    Hit(usize),
    /// recompute into this slot (existing stale entry)
    Recompute(usize),
    Miss,
}

impl FieldCache {
    pub fn new() -> FieldCache {
        FieldCache {
            refresh: FIELD_REFRESH,
            fresh: FIELD_NEW,
            ..Default::default()
        }
    }

    /// Start of a sim tick: `refresh = FIELD_REFRESH; fresh = FIELD_NEW` (main.js:292-293, W5).
    pub fn begin_tick(&mut self) {
        self.refresh = FIELD_REFRESH;
        self.fresh = FIELD_NEW;
    }

    /// `World.fresh-- > 0` (march, entities.js:596): post-decrement test.
    pub fn take_fresh(&mut self) -> bool {
        let ok = self.fresh > 0;
        self.fresh -= 1;
        ok
    }

    /// Drops every field (arrays are kept for reuse) and every move mask.
    pub fn clear(&mut self) {
        for e in self.entries.drain(..) {
            self.spare.push(e.dist);
        }
        self.index.clear();
        self.masks.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The whole field (index = cell).
    #[inline]
    pub fn field(&self, id: FieldId) -> &[u32] {
        self.entries
            .get(id.0 as usize)
            .map(|e| e.dist.as_slice())
            .unwrap_or(&[])
    }

    /// Field value at a cell (`FIELD_INF` if out of range).
    #[inline]
    pub fn dist(&self, id: FieldId, c: i32) -> u32 {
        if c < 0 {
            return FIELD_INF;
        }
        self.field(id).get(c as usize).copied().unwrap_or(FIELD_INF)
    }

    /// `cached()` policy (02 §4.6) — decides between hit / stale use / recompute.
    fn lookup(&mut self, key: FieldKey, map: &Map, from: i32) -> Lookup {
        let Some(&i) = self.index.get(&key) else {
            return Lookup::Miss;
        };
        let i = i as usize;
        let e = &self.entries[i];
        if e.version == map.version && e.dist.len() == map.len() {
            return Lookup::Hit(i);
        }
        let reachable = from >= 0 && e.dist.get(from as usize).is_some_and(|d| *d < FIELD_INF);
        if e.dist.len() == map.len() && (e.version >= map.opened || reachable) {
            if self.refresh <= 0 {
                return Lookup::Hit(i); // budget exhausted: keep using the stale field
            }
            self.refresh -= 1;
        }
        Lookup::Recompute(i)
    }

    /// Slot for a new field: a free slot, or the oldest-inserted entry evicted (its array reused).
    fn slot_for(&mut self, key: FieldKey, map_len: usize) -> usize {
        if self.entries.len() < FIELD_CACHE {
            let dist = self.spare.pop().unwrap_or_default();
            self.entries.push(Entry {
                key,
                version: 0,
                stamp: 0,
                dist,
            });
            let _ = map_len;
            return self.entries.len() - 1;
        }
        let (i, _) = self
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.stamp)
            .unwrap_or((0, &self.entries[0]));
        let old = self.entries[i].key;
        self.index.remove(&old);
        self.entries[i].key = key;
        i
    }

    /// `moveMask(side, cross)` cached by `side*2 + cross`, valid while `map.version` is unchanged (§4.2).
    fn mask(&mut self, map: &Map, side: u8, cross: bool) -> usize {
        let key = side as u16 * 2 + cross as u16;
        if let Some(i) = self.masks.iter().position(|m| m.key == key) {
            let m = &mut self.masks[i];
            if m.version != map.version || m.len != map.len() {
                move_mask_into(map, side, cross, &mut m.bits);
                m.version = map.version;
                m.len = map.len();
            }
            return i;
        }
        let mut bits = Vec::new();
        move_mask_into(map, side, cross, &mut bits);
        self.masks.push(MaskEntry {
            key,
            version: map.version,
            len: map.len(),
            bits,
        });
        self.masks.len() - 1
    }

    /// Computes the field of `self.seeds` into slot `i` and stamps it as the newest entry.
    fn compute(&mut self, i: usize, map: &Map, side: u8, cross: bool) -> FieldId {
        let mi = self.mask(map, side, cross);
        let FieldCache {
            entries,
            masks,
            buckets,
            seeds,
            stamp,
            index,
            ..
        } = self;
        let e = &mut entries[i];
        dial(map, &masks[mi].bits, seeds, side, cross, &mut e.dist, buckets);
        *stamp += 1;
        e.stamp = *stamp;
        e.version = map.version;
        index.insert(e.key, i as u32);
        FieldId(i as u32)
    }
}

/// `moveMask(side, cross)` (02 §4.2) into `out` (resized to the map). Bit k ↔ `step_offset(k)`.
pub fn move_mask_into(map: &Map, side: u8, cross: bool, out: &mut Vec<u8>) {
    let n = map.len();
    let w = map.w as usize;
    let h = map.h as usize;
    out.clear();
    out.resize(n, 0);
    let own = if cross { &map.own_of } else { &map.wall_of };
    let open = |c: usize| {
        map.land[c] == 1 && map.cliff[c] == 0 && (map.block[c] == 0 || (side != 0 && own[c] == side))
    };
    let link = |a: usize, b: usize| {
        map.level[a] == map.level[b] || (map.stair[a] != 0 && map.stair[b] != 0 && a.abs_diff(b) == w)
    };
    for y in 0..h {
        for x in 0..w {
            let c = y * w + x;
            let wo = x > 0 && open(c - 1) && link(c, c - 1);
            let eo = x + 1 < w && open(c + 1) && link(c, c + 1);
            let no = y > 0 && open(c - w) && link(c, c - w);
            let so = y + 1 < h && open(c + w) && link(c, c + w);
            let mut bits = wo as u8 | (eo as u8) << 1 | (no as u8) << 2 | (so as u8) << 3;
            // diagonals k = 0 NW, 1 NE, 2 SW, 3 SE
            if wo && no {
                let t = c - 1 - w;
                if open(t) && link(c - 1, t) && link(c - w, t) {
                    bits |= 16;
                }
            }
            if eo && no {
                let t = c + 1 - w;
                if open(t) && link(c + 1, t) && link(c - w, t) {
                    bits |= 32;
                }
            }
            if wo && so {
                let t = c - 1 + w;
                if open(t) && link(c - 1, t) && link(c + w, t) {
                    bits |= 64;
                }
            }
            if eo && so {
                let t = c + 1 + w;
                if open(t) && link(c + 1, t) && link(c + w, t) {
                    bits |= 128;
                }
            }
            out[c] = bits;
        }
    }
}

/// Dial's algorithm (02 §4.3) over a precomputed move mask; `dist` is resized and overwritten.
fn dial(
    map: &Map,
    mask: &[u8],
    seeds: &[i32],
    side: u8,
    cross: bool,
    dist: &mut Vec<u32>,
    buckets: &mut Buckets,
) {
    let n = map.len();
    dist.clear();
    dist.resize(n, FIELD_INF);
    for q in buckets.q.iter_mut() {
        q.clear();
    }
    let mut pending = 0usize;
    for &s in seeds {
        if map.valid(s) && dist[s as usize] != 0 {
            dist[s as usize] = 0;
            buckets.q[0].push(s as u32);
            pending += 1;
        }
    }
    let w = map.w;
    let steps: [i32; 8] = std::array::from_fn(|k| step_offset(k, w));
    let extra = if cross { CROSS_COST } else { 0 };
    let mut d: u32 = 0;
    while pending > 0 {
        let bi = (d & 63) as usize;
        while let Some(c) = buckets.q[bi].pop() {
            pending -= 1;
            let c = c as usize;
            if dist[c] != d {
                continue;
            }
            let mut bits = mask[c];
            while bits != 0 {
                let k = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let t = (c as i32 + steps[k]) as usize;
                let mut nd = d + if k < 4 { 10 } else { 14 };
                if extra != 0 && map.block[t] != 0 && map.wall_of[t] != side {
                    nd += extra;
                }
                if nd < dist[t] {
                    dist[t] = nd;
                    buckets.q[(nd & 63) as usize].push(t as u32);
                    pending += 1;
                }
            }
        }
        d += 1;
    }
}

/// `computeField(seeds, side, cross)` (02 §4.3): exact multi-source shortest paths (Dial, 64 buckets).
/// Allocating convenience version (generation, tests); the sim goes through the cache.
pub fn compute_field(map: &Map, seeds: &[i32], side: u8, cross: bool) -> Vec<u32> {
    let mut mask = Vec::new();
    move_mask_into(map, side, cross, &mut mask);
    let mut dist = Vec::new();
    dial(map, &mask, seeds, side, cross, &mut dist, &mut Buckets::default());
    dist
}

/// Like [`compute_field`] but reusing caller buffers (`mask` must be the current `move_mask_into` result).
pub fn compute_field_with(
    map: &Map,
    mask: &[u8],
    seeds: &[i32],
    side: u8,
    cross: bool,
    dist: &mut Vec<u32>,
    buckets: &mut Buckets,
) {
    dial(map, mask, seeds, side, cross, dist, buckets);
}

/// `neighborOK(c, x, y, dx, dy, side, cross)` (02 §4.1): neighbour index or -1.
pub fn neighbor_ok(map: &Map, c: i32, dx: i32, dy: i32, side: u8, cross: bool) -> i32 {
    let (x, y) = map.xy(c);
    if !map.inside(x + dx, y + dy) {
        return -1;
    }
    let n = map.idx(x + dx, y + dy);
    let ok = |k: i32| {
        if cross {
            map.crossable(k, side)
        } else {
            map.passable(k, side)
        }
    };
    if !ok(n) {
        return -1;
    }
    if dx != 0 && dy != 0 {
        let o1 = c + dx;
        let o2 = c + dy * map.w;
        if !(ok(o1) && ok(o2)) {
            return -1;
        }
        if !map.link_ok(c, o1) || !map.link_ok(c, o2) || !map.link_ok(o1, n) || !map.link_ok(o2, n)
        {
            return -1;
        }
    } else if !map.link_ok(c, n) {
        return -1;
    }
    n
}

/// `stepFrom(field, c, side, cross)` (02 §4.7): next cell to walk to, -1 if none.
pub fn step_from(map: &Map, field: &[u32], c: i32, side: u8, cross: bool) -> i32 {
    if !map.valid(c) || field.len() != map.len() {
        return -1;
    }
    let (x, y) = map.xy(c);
    let get = |k: i32| field[k as usize];
    let standable = if cross {
        map.crossable(c, side)
    } else {
        map.passable(c, side)
    };
    if !standable {
        let mut bd = FIELD_INF;
        let mut best = -1;
        for (dx, dy) in DIRS8 {
            if !map.inside(x + dx, y + dy) {
                continue;
            }
            let n = c + dx + dy * map.w;
            if map.crossable(n, side) && get(n) < bd {
                bd = get(n);
                best = n;
            }
        }
        return best;
    }
    let mut best = -1;
    let mut bd = get(c);
    for (dx, dy) in DIRS8 {
        let n = neighbor_ok(map, c, dx, dy, side, cross);
        if n >= 0 && get(n) < bd {
            bd = get(n);
            best = n;
        }
    }
    best
}

impl World {
    /// Cells occupied by a field target (empty if the entity no longer exists).
    pub fn field_target_cells(&self, t: FieldTarget) -> Vec<i32> {
        match t {
            FieldTarget::Cell(c) => vec![c],
            FieldTarget::Building(b) => {
                self.buildings.get(b).map(|b| b.cells()).unwrap_or_default()
            }
            FieldTarget::Node(n) => self
                .nodes
                .get(n.0 as usize)
                .map(|n| n.cells(&self.map))
                .unwrap_or_default(),
            FieldTarget::Point(p) => self
                .points
                .get(p.0 as usize)
                .map(|p| self.map.quad(p.cell).to_vec())
                .unwrap_or_default(),
        }
    }

    /// Seeds of a field for the effective side `s` (02 §4.5).
    pub fn field_seeds(&self, t: FieldTarget, s: u8) -> Vec<i32> {
        match t {
            FieldTarget::Cell(c) => {
                if self.map.passable(c, s) {
                    vec![c]
                } else {
                    self.map.ring(&[c], s)
                }
            }
            _ => {
                let cells = self.field_target_cells(t);
                self.map.ring(&cells, s)
            }
        }
    }

    /// Fills `self.fields.seeds` for a recompute (scratch buffers, no allocation in steady state).
    fn fill_seeds(&mut self, t: FieldTarget, s: u8) {
        let mut seeds = std::mem::take(&mut self.fields.seeds);
        let mut cells = std::mem::take(&mut self.fields.cells);
        seeds.clear();
        cells.clear();
        match t {
            FieldTarget::Cell(c) => {
                if self.map.passable(c, s) {
                    seeds.push(c);
                } else {
                    self.map.ring_into(&[c], s, &mut seeds);
                }
            }
            FieldTarget::Building(b) => {
                if let Some(b) = self.buildings.get(b) {
                    cells.extend(b.cells());
                }
                self.map.ring_into(&cells, s, &mut seeds);
            }
            FieldTarget::Node(n) => {
                if let Some(n) = self.nodes.get(n.0 as usize) {
                    if n.big {
                        cells.extend_from_slice(&self.map.quad(n.cell));
                    } else {
                        cells.push(n.cell);
                    }
                }
                self.map.ring_into(&cells, s, &mut seeds);
            }
            FieldTarget::Point(p) => {
                if let Some(p) = self.points.get(p.0 as usize) {
                    cells.extend_from_slice(&self.map.quad(p.cell));
                }
                self.map.ring_into(&cells, s, &mut seeds);
            }
        }
        self.fields.seeds = seeds;
        self.fields.cells = cells;
    }

    fn cached_field(&mut self, target: FieldTarget, side: u8, cross: bool, from: i32) -> FieldId {
        let key = FieldKey {
            target,
            side,
            cross,
        };
        let slot = match self.fields.lookup(key, &self.map, from) {
            Lookup::Hit(i) => return FieldId(i as u32),
            Lookup::Recompute(i) => i,
            Lookup::Miss => self.fields.slot_for(key, self.map.len()),
        };
        self.fill_seeds(target, side);
        self.fields.compute(slot, &self.map, side, cross)
    }

    /// `getField(key, seedFn, side, from)` (02 §4.6): separate fields only for sides owning palisades; falls back
    /// to the cross field (through own buildings) when `from` cannot reach the target.
    pub fn get_field(&mut self, target: FieldTarget, side: u8, from: i32) -> FieldRef {
        let walls = if self.map.walls.get(side as usize).copied().unwrap_or(0) > 0 {
            side
        } else {
            0
        };
        let id = self.cached_field(target, walls, false, from);
        let crossed = from >= 0 && side != 0 && self.fields.dist(id, from) == FIELD_INF;
        if crossed {
            let id = self.cached_field(target, side, true, from);
            return FieldRef { id, crossed: true };
        }
        FieldRef { id, crossed: false }
    }

    /// `fieldToCell(c, side, from)`.
    pub fn field_to_cell(&mut self, c: i32, side: u8, from: i32) -> FieldRef {
        self.get_field(FieldTarget::Cell(c), side, from)
    }

    /// `fieldFor(e, side, from)` for a building / node / point.
    pub fn field_for(&mut self, target: FieldTarget, side: u8, from: i32) -> FieldRef {
        self.get_field(target, side, from)
    }

    /// `stepFrom` on a cached field.
    pub fn step_from(&self, f: FieldId, c: i32, side: u8, cross: bool) -> i32 {
        step_from(&self.map, self.fields.field(f), c, side, cross)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_on_flat_map() {
        let mut m = Map::with_size(8, 8);
        m.land.iter_mut().for_each(|v| *v = 1);
        let s = m.idx(0, 0);
        let f = compute_field(&m, &[s], 0, false);
        assert_eq!(f[m.idx(3, 0) as usize], 30);
        assert_eq!(f[m.idx(2, 2) as usize], 28);
        // a wall column with a gap
        for y in 0..7 {
            let c = m.idx(4, y);
            m.block[c as usize] = 1;
        }
        let f = compute_field(&m, &[s], 0, false);
        assert!(f[m.idx(5, 0) as usize] < FIELD_INF);
        assert_eq!(f[m.idx(4, 0) as usize], FIELD_INF);
        let n = step_from(&m, &f, m.idx(6, 0), 0, false);
        assert!(n >= 0 && f[n as usize] < f[m.idx(6, 0) as usize]);
        // around the wall: (5,0) is reached through the gap at (4,7)
        assert_eq!(f[m.idx(5, 0) as usize], 4 * 14 + 2 * 10 + 3 * 14);
    }

    #[test]
    fn levels_need_stairs() {
        let mut m = Map::with_size(6, 6);
        m.land.iter_mut().for_each(|v| *v = 1);
        for x in 0..6 {
            let c = m.idx(x, 0);
            m.level[c as usize] = 1;
        }
        let f = compute_field(&m, &[m.idx(0, 5)], 0, false);
        assert_eq!(f[m.idx(2, 0) as usize], FIELD_INF);
        // stair: upper half (level 1) at (0,0)... lower half at (0,1)
        let hi = m.idx(0, 0);
        let lo = m.idx(0, 1);
        m.stair[hi as usize] = 2;
        m.stair[lo as usize] = 1;
        let f = compute_field(&m, &[m.idx(0, 5)], 0, false);
        assert!(f[m.idx(2, 0) as usize] < FIELD_INF);
        // only orthogonal moves through the stair halves: (1,1)->(0,0) diagonal is refused
        let mask = {
            let mut v = Vec::new();
            move_mask_into(&m, 0, false, &mut v);
            v
        };
        assert_eq!(mask[lo as usize] & 4, 4, "lo -> hi (north) linked");
        assert_eq!(mask[m.idx(1, 1) as usize] & 16, 0, "no diagonal across levels");
    }

    /// The mask-based Dial must equal a reference Dijkstra built on `neighbor_ok` (all sides / cross modes).
    #[test]
    fn mask_matches_neighbor_ok() {
        let mut m = Map::with_size(12, 9);
        let mut s: u32 = 7;
        let mut rnd = |n: u32| {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (s >> 16) % n
        };
        for c in 0..m.len() {
            m.land[c] = if rnd(10) == 0 { 0 } else { 1 };
            m.level[c] = if c % 12 > 7 { 1 } else { 0 };
            if rnd(6) == 0 {
                m.block[c] = 1;
                m.own_of[c] = (rnd(3)) as u8;
                if rnd(2) == 0 {
                    m.wall_of[c] = m.own_of[c];
                }
            }
        }
        let (hi, lo) = (m.idx(8, 3), m.idx(8, 4));
        m.stair[hi as usize] = 2;
        m.stair[lo as usize] = 1;
        for side in 0..3u8 {
            for cross in [false, true] {
                let seed = m.idx(0, 0);
                m.land[seed as usize] = 1;
                let f = compute_field(&m, &[seed], side, cross);
                // reference: Bellman-Ford over neighbor_ok
                let mut r = vec![FIELD_INF; m.len()];
                r[seed as usize] = 0;
                loop {
                    let mut changed = false;
                    for c in 0..m.len() as i32 {
                        if r[c as usize] == FIELD_INF {
                            continue;
                        }
                        for (dx, dy) in DIRS8 {
                            let n = neighbor_ok(&m, c, dx, dy, side, cross);
                            if n < 0 {
                                continue;
                            }
                            let mut nd = r[c as usize] + if dx != 0 && dy != 0 { 14 } else { 10 };
                            if cross && m.block[n as usize] != 0 && m.wall_of[n as usize] != side {
                                nd += CROSS_COST;
                            }
                            if nd < r[n as usize] {
                                r[n as usize] = nd;
                                changed = true;
                            }
                        }
                    }
                    if !changed {
                        break;
                    }
                }
                assert_eq!(f, r, "side {side} cross {cross}");
            }
        }
    }

    #[test]
    fn cache_reuses_slots() {
        let mut fc = FieldCache::new();
        let mut m = Map::with_size(6, 6);
        m.land.iter_mut().for_each(|v| *v = 1);
        for i in 0..(FIELD_CACHE + 5) {
            let key = FieldKey {
                target: FieldTarget::Cell(i as i32 % 36),
                side: (i / 36) as u8,
                cross: false,
            };
            let slot = fc.slot_for(key, m.len());
            fc.seeds.clear();
            fc.seeds.push(i as i32 % 36);
            fc.compute(slot, &m, 0, false);
        }
        assert_eq!(fc.len(), FIELD_CACHE);
        assert_eq!(fc.index.len(), FIELD_CACHE);
        // the first five keys were evicted
        let k0 = FieldKey {
            target: FieldTarget::Cell(0),
            side: 0,
            cross: false,
        };
        assert!(!fc.index.contains_key(&k0));
    }
}
