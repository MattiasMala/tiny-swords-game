//! Dirt roads (MAPGEN.md §9): the castles are joined by a minimum spanning tree plus one extra edge (a loop),
//! routed with a turn-penalised 4-direction Dijkstra on the walk graph (levels only through stairs) that prefers
//! the lowland, avoids hugging obstacles and merges into existing roads; spurs reach the capture points within 30
//! cells. Main roads are two cells wide, spurs one; a dirt plaza lies in front of every castle. Road cells stay free
//! of resources and props (so the castles stay connected), but they are buildable.

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
        let m = &self.map;
        if m.land[c] != 1 || m.cliff[c] != 0 || self.used[c] != 0 {
            return 0;
        }
        if self.road[c] != 0 {
            return 5;
        }
        let mut cost = 10;
        if m.level[c] > 0 && m.stair[c] == 0 {
            cost += 9;
        }
        if m.no_build[c] != 0 && m.stair[c] == 0 && self.stairzone[c] == 0 {
            cost += 6; // capture-point surroundings, pockets
        }
        // keep a little away from obstacles and the water's edge
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

    /// Turn-penalised Dijkstra (state = cell x incoming direction) from `src` cells to the first cell accepted by
    /// `goal`; returns the cell path (source first) or empty. `limit` caps the cost.
    fn road_search(&self, src: &[i32], goal: &dyn Fn(i32) -> bool, limit: u32) -> Vec<i32> {
        let n = self.map.len();
        let mut sc = self.search.borrow_mut();
        let sc = &mut *sc;
        if sc.dist.len() != n * 4 {
            sc.dist = vec![u32::MAX; n * 4];
            sc.prev = vec![u32::MAX; n * 4];
        }
        for &t in &sc.touched {
            sc.dist[t as usize] = u32::MAX;
            sc.prev[t as usize] = u32::MAX;
        }
        sc.touched.clear();
        let (dist, prev, touched) = (&mut sc.dist, &mut sc.prev, &mut sc.touched);
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        for &s in src {
            if s < 0 || s as usize >= n {
                continue;
            }
            for d in 0..4u32 {
                let st = s as u32 * 4 + d;
                dist[st as usize] = 0;
                touched.push(st);
                heap.push(Reverse((0, st)));
            }
        }
        const DX: [i32; 4] = [1, -1, 0, 0];
        const DY: [i32; 4] = [0, 0, 1, -1];
        while let Some(Reverse((d, st))) = heap.pop() {
            if d != dist[st as usize] {
                continue;
            }
            let c = (st / 4) as i32;
            let dir = st % 4;
            if goal(c) && d > 0 {
                // rebuild
                let mut out = vec![c];
                let mut s = st;
                while prev[s as usize] != u32::MAX {
                    s = prev[s as usize];
                    let cc = (s / 4) as i32;
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
            for nd in 0..4u32 {
                let (nx, ny) = (x + DX[nd as usize], y + DY[nd as usize]);
                if nx < 1 || ny < 1 || nx >= W - 1 || ny >= H - 1 {
                    continue;
                }
                let k = ny * W + nx;
                if !self.map.link_ok(c, k) {
                    continue;
                }
                let cost = self.road_cost(k as usize);
                if cost == 0 && !goal(k) {
                    continue;
                }
                let t = if nd != dir && d > 0 { TURN } else { 0 };
                let nd2 = d + cost.max(5) + t;
                let ns = k as u32 * 4 + nd;
                if nd2 < dist[ns as usize] {
                    dist[ns as usize] = nd2;
                    touched.push(ns);
                    prev[ns as usize] = st;
                    heap.push(Reverse((nd2, ns)));
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

    fn mark_road(&mut self, path: &[i32], wide: bool) {
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
                let mut extra = Vec::with_capacity(3);
                if horiz {
                    extra.push(c + W);
                }
                if vert {
                    extra.push(c + 1);
                }
                if horiz && vert {
                    extra.push(c + W + 1);
                }
                for e in extra {
                    if self.can_widen(e, lvl) && self.road[e as usize] == 0 {
                        self.road[e as usize] = ROAD_WIDE;
                    }
                }
            }
        }
    }

    /// Roads between castles (MST + one loop edge), plazas, spurs to nearby capture points.
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
        // one extra edge: the shortest pair not yet joined
        let mut extra = (usize::MAX, usize::MAX, i32::MAX);
        for i in 0..nb {
            for j in i + 1..nb {
                if edges.iter().any(|e| (e.0 == i && e.1 == j) || (e.0 == j && e.1 == i)) {
                    continue;
                }
                let d = d2(bases[i], bases[j]);
                if d < extra.2 {
                    extra = (i, j, d);
                }
            }
        }
        if extra.0 != usize::MAX {
            edges.push((extra.0, extra.1));
        }
        edges.sort_by_key(|e| d2(bases[e.0], bases[e.1]));
        for (a, b) in edges {
            let s = idx(bases[a].0, bases[a].1 + 2);
            let t = idx(bases[b].0, bases[b].1 + 2);
            let path = self.road_search(&[s], &|c| c == t, 400_000);
            if path.is_empty() {
                continue;
            }
            self.mark_road(&path, true);
        }
        // spurs to the capture points (≤ 30 cells of road)
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
            let road = &self.road;
            let path = self.road_search(&ring, &|c| road[c as usize] != 0, 30 * 12);
            if !path.is_empty() {
                self.junctions.push(*path.last().unwrap_or(&path[0]));
                self.mark_road(&path, false);
            }
        }
        self.stair_corridors();
    }

    /// Protected (invisible) corridors: from every plateau stair's approach to the road network, and from every
    /// terrace stair's approach to its plateau's landings, so no resource or prop can ever cut a plateau off.
    fn stair_corridors(&mut self) {
        let hills = self.hills.clone();
        for h in &hills {
            for s in &h.stairs {
                let road = &self.road;
                let path = self.road_search(&[s.ap], &|c| matches!(road[c as usize], 1..=3), 2_000_000);
                self.mark_hidden(&path);
            }
            if let Some((_, tstairs)) = &h.top {
                let landings: Vec<i32> = h.stairs.iter().map(|s| s.landing()).collect();
                for s in tstairs {
                    let path = self.road_search(&[s.ap], &|c| landings.contains(&c), 2_000_000);
                    self.mark_hidden(&path);
                }
            }
        }
    }

    fn mark_hidden(&mut self, path: &[i32]) {
        for &c in path {
            let k = c as usize;
            if self.road[k] == 0 && self.map.stair[k] == 0 {
                self.road[k] = ROAD_HIDDEN;
            }
        }
    }
}
