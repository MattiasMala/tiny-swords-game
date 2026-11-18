// OWNER: S1 (map) — see docs/spec/02-world.md §4 (movement graph and distance fields) + addenda W5, W6.
//
// Distance fields in tenths of a cell (10 orthogonal, 14 diagonal, +CROSS_COST per own-building cell in cross
// mode), `FIELD_INF` = unreachable. The foundation wrote a working (but unoptimised) version so units can move
// before S1 delivers:
//   * `compute_field`: Dial's algorithm with the moveMask rule inline (no precomputed masks yet).
//   * `FieldCache`: key -> field, insertion-order (FIFO) eviction at FIELD_CACHE, stale-field policy of §4.6
//     with the per-tick `refresh` budget, `fresh` budget for `march` (formation.rs).
//   * `World::get_field` / `field_to_cell` / `field_for` (§4.5-§4.6 incl. the `crossed` fallback), `step_from`
//     (§4.7), `neighbor_ok` (§4.1).
// S1 must: verify against the spec, add the cached `moveMask(side, cross)` (§4.2) keyed by side*2+cross and
// invalidated on `map.version`, recycle evicted arrays, and keep these signatures.
//
// Usage pattern (no borrow trouble): `let f = world.field_for(FieldTarget::Building(b), side, here);`
// then `world.fields.dist(f.id, cell)` / `world.step_from(f.id, cell, side, f.crossed)`.
// A `FieldId` is valid only until the next `get_field`/`field_*` call (eviction may reuse the slot).
#![allow(dead_code)]

use crate::World;
use crate::defs::{CROSS_COST, DIRS8, FIELD_CACHE, FIELD_INF, FIELD_NEW, FIELD_REFRESH};
use crate::map::Map;
use crate::slab::{BuildingId, NodeId, PointId};
use serde::{Deserialize, Serialize};

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

/// Distance-field cache (`World.fields` + budgets `refresh`/`fresh`).
#[derive(Clone, Debug, Default)]
pub struct FieldCache {
    entries: Vec<Entry>,
    stamp: u64,
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
            entries: Vec::new(),
            stamp: 0,
            refresh: FIELD_REFRESH,
            fresh: FIELD_NEW,
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

    pub fn clear(&mut self) {
        self.entries.clear();
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
        let Some(i) = self.entries.iter().position(|e| e.key == key) else {
            return Lookup::Miss;
        };
        let e = &self.entries[i];
        if e.version == map.version {
            return Lookup::Hit(i);
        }
        let reachable = from >= 0 && e.dist.get(from as usize).is_some_and(|d| *d < FIELD_INF);
        if e.version >= map.opened || reachable {
            if self.refresh <= 0 {
                return Lookup::Hit(i); // budget exhausted: keep using the stale field
            }
            self.refresh -= 1;
        }
        Lookup::Recompute(i)
    }

    /// Stores a freshly computed field (re-inserted at the end of the FIFO order).
    fn store(
        &mut self,
        slot: Option<usize>,
        key: FieldKey,
        version: u32,
        dist: Vec<u32>,
    ) -> FieldId {
        self.stamp += 1;
        let entry = Entry {
            key,
            version,
            stamp: self.stamp,
            dist,
        };
        if let Some(i) = slot {
            self.entries[i] = entry;
            return FieldId(i as u32);
        }
        if self.entries.len() >= FIELD_CACHE {
            // evict the oldest-inserted entry
            let (i, _) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.stamp)
                .expect("non-empty cache");
            self.entries[i] = entry;
            return FieldId(i as u32);
        }
        self.entries.push(entry);
        FieldId(self.entries.len() as u32 - 1)
    }
}

/// `computeField(seeds, side, cross)` (02 §4.3): exact multi-source shortest paths (Dial, 64 buckets).
pub fn compute_field(map: &Map, seeds: &[i32], side: u8, cross: bool) -> Vec<u32> {
    let n = map.len();
    let w = map.w;
    let mut dist = vec![FIELD_INF; n];
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); 64];
    let mut pending = 0usize;
    for &s in seeds {
        if map.valid(s) && dist[s as usize] != 0 {
            dist[s as usize] = 0;
            buckets[0].push(s as u32);
            pending += 1;
        }
    }
    let own = |c: usize| if cross { map.own_of[c] } else { map.wall_of[c] };
    let open = |c: usize| {
        map.land[c] == 1
            && map.cliff[c] == 0
            && (map.block[c] == 0 || (side != 0 && own(c) == side))
    };
    let link = |a: usize, b: usize| map.link_ok(a as i32, b as i32);
    let extra = if cross { CROSS_COST } else { 0 };
    let mut d: u32 = 0;
    while pending > 0 {
        let bi = (d & 63) as usize;
        while let Some(c) = buckets[bi].pop() {
            pending -= 1;
            let c = c as usize;
            if dist[c] != d {
                continue;
            }
            let x = c as i32 % w;
            let y = c as i32 / w;
            let wo = x > 0 && open(c - 1) && link(c, c - 1);
            let eo = x < w - 1 && open(c + 1) && link(c, c + 1);
            let no = y > 0 && open(c - w as usize) && link(c, c - w as usize);
            let so = y < map.h - 1 && open(c + w as usize) && link(c, c + w as usize);
            let relax = |t: usize,
                         cost: u32,
                         dist: &mut Vec<u32>,
                         buckets: &mut Vec<Vec<u32>>,
                         pending: &mut usize| {
                let surcharge = if extra != 0 && map.block[t] != 0 && map.wall_of[t] != side {
                    extra
                } else {
                    0
                };
                let nd = d + cost + surcharge;
                if nd < dist[t] {
                    dist[t] = nd;
                    buckets[(nd & 63) as usize].push(t as u32);
                    *pending += 1;
                }
            };
            if wo {
                relax(c - 1, 10, &mut dist, &mut buckets, &mut pending);
            }
            if eo {
                relax(c + 1, 10, &mut dist, &mut buckets, &mut pending);
            }
            if no {
                relax(c - w as usize, 10, &mut dist, &mut buckets, &mut pending);
            }
            if so {
                relax(c + w as usize, 10, &mut dist, &mut buckets, &mut pending);
            }
            // diagonals k = 0 NW, 1 NE, 2 SW, 3 SE (step order -1-W, +1-W, -1+W, +1+W)
            for k in 0..4 {
                let east = k & 1 == 1;
                let south = k & 2 == 2;
                if !((if east { eo } else { wo }) && (if south { so } else { no })) {
                    continue;
                }
                let cx = if east { c + 1 } else { c - 1 };
                let cy = if south {
                    c + w as usize
                } else {
                    c - w as usize
                };
                let t = if south {
                    cx + w as usize
                } else {
                    cx - w as usize
                };
                if open(t) && link(cx, t) && link(cy, t) {
                    relax(t, 14, &mut dist, &mut buckets, &mut pending);
                }
            }
        }
        d += 1;
    }
    dist
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
    if !map.valid(c) || field.is_empty() {
        return -1;
    }
    let (x, y) = map.xy(c);
    let get = |k: i32| field.get(k as usize).copied().unwrap_or(FIELD_INF);
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
            let n = map.idx(x + dx, y + dy);
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

    fn cached_field(&mut self, target: FieldTarget, side: u8, cross: bool, from: i32) -> FieldId {
        let key = FieldKey {
            target,
            side,
            cross,
        };
        match self.fields.lookup(key, &self.map, from) {
            Lookup::Hit(i) => FieldId(i as u32),
            Lookup::Recompute(i) => {
                let seeds = self.field_seeds(target, side);
                let d = compute_field(&self.map, &seeds, side, cross);
                self.fields.store(Some(i), key, self.map.version, d)
            }
            Lookup::Miss => {
                let seeds = self.field_seeds(target, side);
                let d = compute_field(&self.map, &seeds, side, cross);
                self.fields.store(None, key, self.map.version, d)
            }
        }
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
    }
}
