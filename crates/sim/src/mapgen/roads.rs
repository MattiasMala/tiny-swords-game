//! Dirt roads (MAPGEN.md §9 + notes on the first version): neighbouring castles are joined along the edges of a
//! minimum spanning tree (edges longer than 100 cells dropped: local tracks, not a network), routed with a
//! turn-penalised 8-direction weighted A* on the walk graph (levels only through stairs, no corner cutting) whose
//! cost prefers the lowland, avoids hugging obstacles, merges into existing roads and carries a low-frequency
//! noise term, so tracks curve gently; spurs reach the capture points within ~30 cells along one shortest-path tree
//! toward the roads (nearby spurs merge and fork naturally). Every path is made 4-connected. Main roads are two
//! cells wide, spurs one; a dirt plaza lies in front of every castle. Road cells stay free of resources and props
//! (so the castles stay connected), but they are buildable. Hidden corridors (no surface) protect the way from
//! every stair to the network.

use super::{Gen, H, W, idx};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// `road[]` values.
pub const ROAD_CORE: u8 = 1;
pub const ROAD_WIDE: u8 = 2;
pub const ROAD_PLAZA: u8 = 3;
/// invisible protected corridor (stair access): no objects, normal surface
pub const ROAD_HIDDEN: u8 = 4;

const TURN: u32 = 14;

impl Gen {
    /// Walk cost of entering cell `c` for a road (0 = impassable).
    fn road_cost(&self, c: usize) -> u32 {
        let plain = self.plain_cost(c);
        if plain != 0 && self.road[c] != 0 { 5 } else { plain }
    }

    /// Fills the per-cell cost tables of the road search (`Search.base`, `Search.wobble`); `used` does not change
    /// while roads are traced, existing roads are discounted at query time.
    fn road_tables(&self) {
        let n = self.map.len();
        let mut base = vec![0u16; n];
        let mut wobble = vec![0u8; n];
        for (c, b) in base.iter_mut().enumerate() {
            *b = self.plain_cost(c) as u16;
            let (x, y) = ((c as i32) % W, (c as i32) / W);
            wobble[c] = (self.noise.at(x as f64 * 0.09 + 3.0, y as f64 * 0.09 + 17.0) * 10.0) as u8;
        }
        let mut sc = self.search.borrow_mut();
        sc.base = base;
        sc.wobble = wobble;
    }

    /// Walk cost of entering cell `c` for a road ignoring the existing-road discount (0 = impassable): plain 10,
    /// +9 on raised ground, +6 on `noBuild` surroundings, +3 per orthogonal obstacle / water / cliff neighbour.
    fn plain_cost(&self, c: usize) -> u32 {
        let m = &self.map;
        if m.land[c] != 1 || m.cliff[c] != 0 || self.used[c] != 0 {
            return 0;
        }
        let mut cost = 10;
        if m.level[c] > 0 && m.stair[c] == 0 {
            cost += 9;
        }
        if m.no_build[c] != 0 && m.stair[c] == 0 && self.stairzone[c] == 0 {
            cost += 6;
        }
        let (x, y) = ((c as i32) % W, (c as i32) / W);
        let mut near = 0;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if !self.map.inside(x + dx, y + dy) {
                continue;
            }
            let k = idx(x + dx, y + dy) as usize;
            if self.used[k] != 0 || m.land[k] != 1 || m.cliff[k] != 0 {
                near += 1;
            }
        }
        cost + near * 3
    }

    /// Turn-penalised search from `src` cells to the first cell accepted by `goal`; returns the cell path (source
    /// first) or empty. `limit` caps the cost. State = cell x incoming direction.
    ///
    /// `smooth` (visible roads): 8 directions (a diagonal step needs both orthogonal cells open and on the same
    /// level, like the walk graph), turns cost by angle (45° cheap, 90° dear) and a low-frequency noise term makes
    /// long roads meander gently instead of running in ruler-straight L shapes. Otherwise 4 directions, no noise
    /// (hidden corridors). `target` ≥ 0 enables an octile A* heuristic toward that cell.
    fn road_search(&self, src: &[i32], goal: &dyn Fn(i32) -> bool, limit: u32, target: i32, smooth: bool) -> Vec<i32> {
        let n = self.map.len();
        if self.search.borrow().base.len() != n {
            self.road_tables();
        }
        let mut sc = self.search.borrow_mut();
        let sc = &mut *sc;
        if sc.dist.len() != n * 8 {
            sc.dist = vec![u32::MAX; n * 8];
            sc.prev = vec![u32::MAX; n * 8];
            sc.touched.clear();
        }
        for &t in &sc.touched {
            sc.dist[t as usize] = u32::MAX;
            sc.prev[t as usize] = u32::MAX;
        }
        sc.touched.clear();
        let (dist, prev, touched, base, wob) = (&mut sc.dist, &mut sc.prev, &mut sc.touched, &sc.base, &sc.wobble);
        let road = &self.road;
        let cost_of = |k: usize| -> u32 {
            let b = base[k] as u32;
            if b == 0 {
                0
            } else if road[k] != 0 {
                5
            } else {
                b
            }
        };
        let (tx, ty) = if target >= 0 { (target % W, target / W) } else { (0, 0) };
        let heur = |c: i32| -> u32 {
            if target < 0 {
                return 0;
            }
            let (dx, dy) = (((c % W) - tx).unsigned_abs(), ((c / W) - ty).unsigned_abs());
            let (lo, hi) = (dx.min(dy), dx.max(dy));
            // weighted A*: octile distance at about the mean step cost (plain 10 + meander ~5); not admissible,
            // the road is merely near-optimal — fine for scenery, and 5-10x fewer states
            ((hi - lo) * 10 + lo * 14) * 3 / 2
        };
        let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
        for &s in src {
            if s < 0 || s as usize >= n {
                continue;
            }
            for d in 0..8u32 {
                let st = s as u32 * 8 + d;
                dist[st as usize] = 0;
                touched.push(st);
                heap.push(Reverse((heur(s), 0, st)));
            }
        }
        // directions in angular order: E, SE, S, SW, W, NW, N, NE
        const DX: [i32; 8] = [1, 1, 0, -1, -1, -1, 0, 1];
        const DY: [i32; 8] = [0, 1, 1, 1, 0, -1, -1, -1];
        const TURN_COST: [u32; 5] = [0, 4, TURN, 60, 90];
        while let Some(Reverse((_, d, st))) = heap.pop() {
            if d != dist[st as usize] {
                continue;
            }
            let c = (st / 8) as i32;
            let dir = st % 8;
            if goal(c) && d > 0 {
                // rebuild
                let mut out = vec![c];
                let mut s = st;
                while prev[s as usize] != u32::MAX {
                    s = prev[s as usize];
                    let cc = (s / 8) as i32;
                    if *out.last().unwrap_or(&-1) != cc {
                        out.push(cc);
                    }
                }
                out.reverse();
                return out;
            }
            if d > limit {
                break;
            }
            let (x, y) = (c % W, c / W);
            for nd in 0..8u32 {
                let (dx, dy) = (DX[nd as usize], DY[nd as usize]);
                let diag = dx != 0 && dy != 0;
                if diag && !smooth {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if nx < 1 || ny < 1 || nx >= W - 1 || ny >= H - 1 {
                    continue;
                }
                let k = ny * W + nx;
                if diag {
                    let (o1, o2) = (c + dx, c + dy * W);
                    if base[o1 as usize] == 0
                        || base[o2 as usize] == 0
                        || !self.map.link_ok(c, o1)
                        || !self.map.link_ok(c, o2)
                        || !self.map.link_ok(o1, k)
                        || !self.map.link_ok(o2, k)
                    {
                        continue;
                    }
                } else if !self.map.link_ok(c, k) {
                    continue;
                }
                let cost = cost_of(k as usize);
                if cost == 0 && !goal(k) {
                    continue;
                }
                let ang = (nd as i32 - dir as i32).rem_euclid(8);
                let ang = ang.min(8 - ang) as usize;
                let (t, step) = if smooth {
                    (
                        if d > 0 { TURN_COST[ang] } else { 0 },
                        cost.max(5) + wob[k as usize] as u32,
                    )
                } else {
                    (if d > 0 && ang != 0 { TURN } else { 0 }, cost.max(5))
                };
                let nd2 = d + if diag { step * 14 / 10 } else { step } + t;
                let ns = k as u32 * 8 + nd;
                if nd2 < dist[ns as usize] {
                    dist[ns as usize] = nd2;
                    touched.push(ns);
                    prev[ns as usize] = st;
                    heap.push(Reverse((nd2 + heur(k), nd2, ns)));
                }
            }
        }
        Vec::new()
    }

    fn can_widen(&self, c: i32, lvl: u8) -> bool {
        if !self.map.valid(c) {
            return false;
        }
        let k = c as usize;
        let m = &self.map;
        m.land[k] == 1
            && m.cliff[k] == 0
            && m.stair[k] == 0
            && m.level[k] == lvl
            && self.used[k] == 0
            && self.stairzone[k] == 0
    }

    /// Makes a path 4-connected: after every diagonal step the corner cell (both are open by construction) is
    /// inserted, so objects placed later can never close the way (no corner cutting in the walk graph).
    fn four_connect(&self, path: &[i32]) -> Vec<i32> {
        let mut out = Vec::with_capacity(path.len() * 3 / 2);
        for (i, &c) in path.iter().enumerate() {
            if i > 0 {
                let p = path[i - 1];
                let (px, py) = (p % W, p / W);
                let (cx, cy) = (c % W, c / W);
                if px != cx && py != cy {
                    // pick the corner with the lower noise (stable, varied) among the two open ones
                    let a = idx(cx, py);
                    let b = idx(px, cy);
                    let na = self.noise.at(cx as f64 * 0.5, py as f64 * 0.5);
                    let nb = self.noise.at(px as f64 * 0.5, cy as f64 * 0.5);
                    out.push(if na <= nb { a } else { b });
                }
            }
            out.push(c);
        }
        out
    }

    fn mark_road(&mut self, path: &[i32], wide: bool) {
        let path = self.four_connect(path);
        for (i, &c) in path.iter().enumerate() {
            let k = c as usize;
            if self.map.stair[k] != 0 {
                continue;
            }
            if self.road[k] == 0 || self.road[k] == ROAD_WIDE || self.road[k] == ROAD_HIDDEN {
                self.road[k] = ROAD_CORE;
            }
            if wide {
                let lvl = self.map.level[k];
                let prev = if i > 0 { path[i - 1] } else { c };
                let next = path.get(i + 1).copied().unwrap_or(c);
                let horiz = (prev - c).abs() == 1 || (next - c).abs() == 1;
                let vert = (prev - c).abs() == W || (next - c).abs() == W;
                let mut extra = [-1i32; 2];
                if horiz && !vert {
                    extra[0] = c + W;
                } else if vert && !horiz {
                    extra[0] = c + 1;
                } else if horiz && vert {
                    // a corner of the staircase: 2x2 brush keeps the band two cells wide
                    extra[0] = c + 1;
                    extra[1] = c + W;
                }
                for e in extra {
                    if e >= 0 && self.can_widen(e, lvl) && self.road[e as usize] == 0 {
                        self.road[e as usize] = ROAD_WIDE;
                    }
                }
            }
        }
    }

    /// Roads between neighbouring castles, plazas, spurs to nearby capture points, stair corridors.
    pub fn roads(&mut self) {
        let bases = self.bases.clone();
        let nb = bases.len();
        // plazas first (castle front), so roads start from them
        for &(bx, by) in &bases {
            for y in by + 1..=by + 3 {
                for x in bx - 3..=bx + 3 {
                    if y == by + 3 && (x == bx - 3 || x == bx + 3) {
                        continue;
                    }
                    let k = idx(x, y) as usize;
                    if self.map.land[k] == 1 && self.map.level[k] == 0 && self.map.cliff[k] == 0 {
                        self.road[k] = ROAD_PLAZA;
                    }
                }
            }
        }
        // Prim's MST on Euclidean distances
        let d2 = |a: (i32, i32), b: (i32, i32)| (a.0 - b.0).pow(2) + (a.1 - b.1).pow(2);
        let mut in_tree = vec![false; nb];
        in_tree[0] = true;
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for _ in 1..nb {
            let mut best = (usize::MAX, usize::MAX, i32::MAX);
            for i in 0..nb {
                if !in_tree[i] {
                    continue;
                }
                for j in 0..nb {
                    if in_tree[j] {
                        continue;
                    }
                    let d = d2(bases[i], bases[j]);
                    if d < best.2 {
                        best = (i, j, d);
                    }
                }
            }
            if best.0 == usize::MAX {
                break;
            }
            in_tree[best.1] = true;
            edges.push((best.0, best.1));
        }
        // only neighbouring castles are joined: the longest tree edges (across the whole island) are dropped, so
        // the roads read as local tracks, not a network (MAPGEN.md, notes on the first version)
        edges.retain(|e| d2(bases[e.0], bases[e.1]) <= 100 * 100);
        edges.sort_by_key(|e| d2(bases[e.0], bases[e.1]));
        for (a, b) in edges {
            let s = idx(bases[a].0, bases[a].1 + 2);
            let t = idx(bases[b].0, bases[b].1 + 2);
            let path = self.road_search(&[s], &|c| c == t, 400_000, t, true);
            if path.is_empty() {
                continue;
            }
            self.mark_road(&path, true);
        }
        // spurs to the capture points (≤ 30 cells of road): one shortest-path tree toward the roads, so nearby
        // spurs merge and fork naturally
        let (toward, dist) = self.road_tree(true);
        let pts: Vec<i32> = self.points.iter().map(|p| p.cell).collect();
        for p in pts {
            let ring: Vec<i32> = {
                let (x, y) = self.map.xy(p);
                let mut v = Vec::new();
                for (dx, dy) in [(0, -1), (1, -1), (-1, 0), (2, 0), (-1, 1), (2, 1), (0, 2), (1, 2)] {
                    let k = idx(x + dx, y + dy);
                    if self.road_cost(k as usize) > 0 && self.map.level[k as usize] == self.map.level[p as usize] {
                        v.push(k);
                    }
                }
                v
            };
            if ring.is_empty() || ring.iter().any(|k| self.road[*k as usize] != 0) {
                continue;
            }
            let Some(&start) = ring.iter().min_by_key(|k| dist[**k as usize]) else {
                continue;
            };
            if dist[start as usize] > 30 * 18 {
                continue;
            }
            let path = Self::trace(&toward, start);
            if path.len() >= 2 {
                self.junctions.push(*path.last().unwrap_or(&path[0]));
                self.mark_road(&path, false);
            }
        }
        self.stair_corridors();
    }

    /// Follows a [`Gen::road_tree`] from `c` to its root (a road cell); the root is included.
    fn trace(toward: &[u32], mut c: i32) -> Vec<i32> {
        let mut path = Vec::new();
        while c >= 0 && toward[c as usize] != u32::MAX && path.len() < 4000 {
            path.push(c);
            let p = toward[c as usize];
            if p == c as u32 {
                break;
            }
            c = p as i32;
        }
        path
    }

    /// Protected (invisible) corridors: from every plateau stair's approach to the road network, and from every
    /// terrace stair's approach to its plateau's landings, so no resource or prop can ever cut a plateau off.
    fn stair_corridors(&mut self) {
        let hills = self.hills.clone();
        // one multi-source search from every visible road cell gives each cell its cheapest way to the network
        let (toward, _) = self.road_tree(false);
        for h in &hills {
            for s in &h.stairs {
                let path = Self::trace(&toward, s.ap);
                self.mark_hidden(&path);
            }
            if let Some((_, tstairs)) = &h.top {
                let landings: Vec<i32> = h.stairs.iter().map(|s| s.landing()).collect();
                for s in tstairs {
                    let path = self.road_search(&[s.ap], &|c| landings.contains(&c), 2_000_000, -1, false);
                    self.mark_hidden(&path);
                }
            }
        }
    }

    /// For every cell, the next cell on its cheapest way (road costs, levels only through stairs) to the nearest
    /// visible road cell, and that cost; road cells point to themselves, unreachable cells hold `u32::MAX`.
    /// `smooth`: 8 directions (diagonals need both orthogonal cells open, like the walk graph) + the meander term,
    /// for visible spurs; otherwise 4 directions, plain costs (hidden corridors).
    fn road_tree(&self, smooth: bool) -> (Vec<u32>, Vec<u32>) {
        let n = self.map.len();
        if self.search.borrow().base.len() != n {
            self.road_tables();
        }
        let sc = self.search.borrow();
        let base = &sc.base;
        let wob = &sc.wobble;
        let mut dist = vec![u32::MAX; n];
        let mut next = vec![u32::MAX; n];
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        for c in 0..n {
            if matches!(self.road[c], ROAD_CORE | ROAD_WIDE | ROAD_PLAZA) {
                dist[c] = 0;
                next[c] = c as u32;
                heap.push(Reverse((0, c as u32)));
            }
        }
        while let Some(Reverse((d, c))) = heap.pop() {
            if d != dist[c as usize] {
                continue;
            }
            let c = c as i32;
            let (x, y) = (c % W, c / W);
            for (dx, dy) in crate::defs::DIRS8 {
                let diag = dx != 0 && dy != 0;
                if diag && !smooth {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if nx < 1 || ny < 1 || nx >= W - 1 || ny >= H - 1 {
                    continue;
                }
                let k = ny * W + nx;
                let cost = base[k as usize] as u32;
                if cost == 0 {
                    continue;
                }
                if diag {
                    let (o1, o2) = (c + dx, c + dy * W);
                    if base[o1 as usize] == 0
                        || base[o2 as usize] == 0
                        || !self.map.link_ok(c, o1)
                        || !self.map.link_ok(c, o2)
                        || !self.map.link_ok(o1, k)
                        || !self.map.link_ok(o2, k)
                    {
                        continue;
                    }
                } else if !self.map.link_ok(c, k) {
                    continue;
                }
                let step = if smooth { cost + wob[k as usize] as u32 } else { cost };
                let nd = d + if diag { step * 14 / 10 } else { step };
                if nd < dist[k as usize] {
                    dist[k as usize] = nd;
                    next[k as usize] = c as u32;
                    heap.push(Reverse((nd, k as u32)));
                }
            }
        }
        (next, dist)
    }

    fn mark_hidden(&mut self, path: &[i32]) {
        let path = self.four_connect(path);
        for &c in &path {
            let k = c as usize;
            if self.road[k] == 0 && self.map.stair[k] == 0 {
                self.road[k] = ROAD_HIDDEN;
            }
        }
    }
}
