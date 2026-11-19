//! Visual decorations (02 §5.15 redesigned per MAPGEN.md §5-§8, ref. §11.5-§11.8): bushes and rocks as transition
//! glue (cliff feet, beside stairs, coast and plateau corners, islets, wood edges, kingdom corners), water rocks
//! (singles and pairs near the shore, more at sea cliffs), ducks (one per pond, a few in the bays), drifting
//! clouds, walk-over details in clusters (flower patches in the meadows, wood-floor details under the trees,
//! pebbles, puddles), shore props (boats and docks at the beaches, wrecks, reeds and lily pads in sheltered water).
//! Nothing here blocks movement. Open meadows stay calm: details concentrate on the edges.

use super::grid::{cheb_dist, flood4};
use super::scenery::{PAINT_DRY, PAINT_SAND};
use super::{Gen, H, W, idx};
use crate::defs::PropKind as P;
use crate::map::{Decor, DecorKind};

impl Gen {
    fn push_decor(&mut self, kind: DecorKind, x: f32, y: f32, variant: u8, phase: f32) {
        self.map.decor.push(Decor {
            kind,
            x,
            y,
            variant,
            phase,
            scale: 1.0,
            solid: false,
        });
    }

    /// Bush (`v` random) at a px position, marking the cell's decoration occupancy.
    fn bush_at(&mut self, x: f32, y: f32) {
        let c = self.map.cell_at(x, y - 1.0);
        if c >= 0 {
            self.deco[c as usize] = 1;
        }
        let v = self.below(4) as u8;
        let ph = (self.rnd() * 8.0) as f32;
        self.push_decor(DecorKind::Bush, x, y, v, ph);
    }

    fn rock_at(&mut self, x: f32, y: f32) {
        let c = self.map.cell_at(x, y - 1.0);
        if c >= 0 {
            self.deco[c as usize] = 1;
        }
        let v = self.below(4) as u8;
        self.push_decor(DecorKind::Rock, x, y, v, 0.0);
    }

    /// Glue decoration inside cell `c` (`top` = hug the north side of the cell, e.g. at a cliff foot).
    fn glue(&mut self, c: i32, bush: bool, top: bool) {
        let (x, y) = (c % W, c / W);
        let px = x as f32 * 64.0 + 12.0 + (self.rnd() * 40.0) as f32;
        let py = y as f32 * 64.0 + if top { 18.0 + (self.rnd() * 14.0) as f32 } else { 30.0 + (self.rnd() * 26.0) as f32 };
        if bush {
            self.bush_at(px, py);
        } else {
            self.rock_at(px, py);
        }
    }

    fn glue_free(&self, c: i32) -> bool {
        self.map.valid(c) && {
            let i = c as usize;
            self.map.land[i] == 1
                && self.map.cliff[i] == 0
                && self.map.stair[i] == 0
                && self.used[i] == 0
                && self.deco[i] == 0
                && self.road[i] == 0
                && self.reserved[i] == 0
        }
    }

    pub fn decorate(&mut self) {
        self.beaches();
        self.stair_glue();
        self.cliff_glue();
        self.corner_glue();
        self.wood_glue();
        self.islet_decor();
        self.water_rocks();
        self.ponds_and_ducks();
        self.shore_props();
        self.flower_patches();
        self.wood_details();
        self.edge_details();
        self.clouds();
    }

    /// Sand beaches on some stretches of low coast (sea only, not under cliffs, not at ponds): long runs chosen by
    /// a low-frequency noise, 1 cell deep (2 where the zone is strong).
    fn beaches(&mut self) {
        let sea = self.sea_mask();
        let n = self.map.len();
        let mut sand = vec![0u8; n];
        for y in 2..H - 2 {
            for x in 2..W - 2 {
                let c = idx(x, y) as usize;
                let m = &self.map;
                if m.land[c] != 1 || m.level[c] != 0 || m.cliff[c] != 0 || m.stair[c] != 0 || self.road[c] != 0 {
                    continue;
                }
                if m.cliff[c - W as usize] != 0 {
                    continue;
                }
                let b = self.noise.at(x as f64 * 0.045 + 50.0, y as f64 * 0.045 + 20.0);
                if b < 0.64 {
                    continue;
                }
                let w = W as usize;
                let touch1 = [c - 1, c + 1, c - w, c + w].iter().any(|k| sea[*k] != 0);
                let touch2 = b > 0.74
                    && [c - 2, c + 2, c - 2 * w, c + 2 * w, c + w + 1, c + w - 1].iter().any(|k| *k < n && sea[*k] != 0);
                if touch1 || touch2 {
                    sand[c] = 1;
                }
            }
        }
        // drop short isolated bits (< 3 cells in their component)
        let (lab, sizes) = super::grid::components4(&sand, W, H);
        for c in 0..n {
            if sand[c] != 0 && sizes[lab[c] as usize] >= 3 && self.paint[c] == 0 {
                self.paint[c] = PAINT_SAND;
            }
        }
    }

    /// Sea = water 4-connected to the map border (ponds and inner lakes excluded).
    pub fn sea_mask(&self) -> Vec<u8> {
        let mut starts = Vec::new();
        for x in 0..W {
            starts.push(x);
            starts.push((H - 1) * W + x);
        }
        for y in 0..H {
            starts.push(y * W);
            starts.push(y * W + W - 1);
        }
        let land = &self.map.land;
        flood4(W, H, &starts, |k| land[k] == 0)
    }

    /// Bushes flanking every stair (outer side of the lower half), a rock or bush on the other side of the approach.
    fn stair_glue(&mut self) {
        let mut stairs = Vec::new();
        for h in &self.hills {
            stairs.extend(h.stairs.iter().copied());
            if let Some(t) = &h.top {
                stairs.extend(t.1.iter().copied());
            }
        }
        for s in stairs {
            let outer = s.lo + s.dir;
            if self.glue_free(outer) && self.chance(0.8) {
                let bush = self.chance(0.75);
                self.glue(outer, bush, true);
            }
            let other = s.ap - s.dir;
            if self.glue_free(other) && self.chance(0.45) {
                let bush = self.chance(0.5);
                self.glue(other, bush, true);
            }
            let high = s.hi + s.dir;
            if self.map.valid(high) && self.map.level[high as usize] == self.map.level[s.hi as usize] && self.glue_free(high) && self.chance(0.3) {
                self.glue(high, true, false);
            }
        }
    }

    /// Bushes and rocks at the feet of land cliffs (sparse, in runs' ends more often).
    fn cliff_glue(&mut self) {
        for y in 2..H - 3 {
            for x in 2..W - 2 {
                let k = idx(x, y);
                let ki = k as usize;
                if self.map.cliff[ki] == 0 || self.map.land[ki] != 1 {
                    continue;
                }
                let foot = k + W;
                if !self.glue_free(foot) {
                    continue;
                }
                let end = self.map.cliff[ki - 1] == 0 || self.map.cliff[ki + 1] == 0;
                let p = if end { 0.4 } else { 0.1 };
                if self.chance(p) {
                    let bush = self.chance(0.6);
                    self.glue(foot, bush, true);
                    if !bush && self.chance(0.3) {
                        let k2 = foot + if self.chance(0.5) { 1 } else { -1 };
                        if self.detail_free(k2) {
                            self.soft_prop_in(P::Pebbles, k2);
                        }
                    }
                }
            }
        }
    }

    /// Convex corners of the coast (rock/bush), of the plateaus (bush on the rim) and of the kingdom zones.
    fn corner_glue(&mut self) {
        for y in 3..H - 3 {
            for x in 3..W - 3 {
                let c = idx(x, y);
                let ci = c as usize;
                let m = &self.map;
                if m.land[ci] != 1 || m.cliff[ci] != 0 || m.stair[ci] != 0 {
                    continue;
                }
                let lvl = m.level[ci];
                let other = |k: i32| {
                    let k = k as usize;
                    m.land[k] != 1 || m.level[k] != lvl || m.cliff[k] != 0
                };
                let (n, s, e, w) = (other(c - W), other(c + W), other(c + 1), other(c - 1));
                let corner = (n || s) && (e || w) && !(n && s) && !(e && w);
                if !corner || !self.glue_free(c) {
                    continue;
                }
                let water = [c - W, c + W, c + 1, c - 1].iter().any(|k| self.map.land[*k as usize] == 0);
                let p = if water { 0.35 } else if lvl > 0 { 0.3 } else { 0.15 };
                if self.chance(p) {
                    let bush = if water { self.chance(0.45) } else { self.chance(0.7) };
                    self.glue(c, bush, false);
                }
            }
        }
        // kingdom corners: a bush just outside each corner of the cleared zone
        let bases = self.bases.clone();
        for (bx, by) in bases {
            for (dx, dy) in [(-10, -8), (10, -8), (-10, 8), (10, 8)] {
                let c = idx(bx + dx, by + dy);
                if self.map.valid(c) && self.map.land[c as usize] == 1 && self.used[c as usize] == 0 && self.deco[c as usize] == 0 && self.chance(0.6) {
                    self.glue(c, true, false);
                }
            }
        }
    }

    /// A few bushes around the woods.
    fn wood_glue(&mut self) {
        let woods = self.woods.clone();
        for (x, y, r) in woods {
            let n = 1 + self.below(3);
            for _ in 0..n {
                let ox = (self.rnd() * 2.0 - 1.0) * (r as f64 + 1.5);
                let oy = (self.rnd() * 2.0 - 1.0) * (r as f64 + 1.5);
                let c = idx((x as f64 + ox).round() as i32, (y as f64 + oy).round() as i32);
                if self.glue_free(c) {
                    self.glue(c, true, false);
                }
            }
        }
    }

    /// One decoration per islet: bush, rock, unreachable tree or nothing (raised islets: on the top row).
    fn islet_decor(&mut self) {
        let isl = self.islets.clone();
        for (x0, y0, w, h) in isl {
            let r = self.rnd();
            let x = x0 + self.below(w as u32);
            let y = y0;
            let px = x as f32 * 64.0 + 16.0 + (self.rnd() * 32.0) as f32;
            let py = y as f32 * 64.0 + 30.0 + (self.rnd() * 20.0) as f32;
            if r < 0.4 {
                let v = self.below(4) as u8;
                let ph = (self.rnd() * 8.0) as f32;
                self.push_decor(DecorKind::Bush, px, py, v, ph);
            } else if r < 0.6 {
                let v = self.below(4) as u8;
                self.push_decor(DecorKind::Rock, px, py, v, 0.0);
            } else if r < 0.82 && w * h >= 2 {
                let v = if self.pine_at(x as f64, y as f64) { self.below(2) } else { 2 + self.below(2) } as u8;
                let ph = (self.rnd() * 8.0) as f32;
                let q = self.map.center(idx(x, y));
                self.push_decor(DecorKind::Tree, q.x, q.y + 22.0, v, ph);
                if w >= 3 && self.chance(0.5) {
                    let v2 = self.below(4) as u8;
                    let ph2 = (self.rnd() * 8.0) as f32;
                    self.push_decor(DecorKind::Bush, q.x + 70.0, q.y + 20.0, v2, ph2);
                }
            }
            let _ = h;
        }
        // detached small islands (land 2 from shaping): a tree or bush on some cells
        for y in 2..H - 2 {
            for x in 2..W - 2 {
                let c = idx(x, y) as usize;
                if self.map.land[c] == 2 && self.map.cliff[c] == 0 && self.deco[c] == 0 && self.chance(0.12) {
                    self.deco[c] = 1;
                    let q = self.map.center(c as i32);
                    if self.chance(0.5) {
                        let v = if self.pine_at(x as f64, y as f64) { self.below(2) } else { 2 + self.below(2) } as u8;
                        let ph = (self.rnd() * 8.0) as f32;
                        self.push_decor(DecorKind::Tree, q.x, q.y + 22.0, v, ph);
                    } else {
                        let v = self.below(4) as u8;
                        let ph = (self.rnd() * 8.0) as f32;
                        self.push_decor(DecorKind::Bush, q.x, q.y + 16.0, v, ph);
                    }
                }
            }
        }
    }

    /// Water rocks: singles and pairs 1-3 cells off the shore, denser at sea cliffs and coast corners.
    fn water_rocks(&mut self) {
        let ground: Vec<u8> = (0..self.map.len())
            .map(|i| (self.map.land[i] != 0 || self.map.cliff[i] != 0) as u8)
            .collect();
        let d = cheb_dist(&ground, W, H, 16);
        let seacliff: Vec<u8> = (0..self.map.len())
            .map(|i| (self.map.land[i] == 0 && self.map.cliff[i] != 0) as u8)
            .collect();
        let dc = cheb_dist(&seacliff, W, H, 16);
        let mut count = 0;
        let mut taken = vec![0u8; self.map.len()];
        for y in 2..H - 2 {
            for x in 2..W - 2 {
                let c = idx(x, y) as usize;
                if ground[c] != 0 || taken[c] != 0 || !(1..=3).contains(&d[c]) {
                    continue;
                }
                let mut p = match d[c] {
                    1 => 0.004,
                    2 => 0.03,
                    _ => 0.012,
                };
                if dc[c] <= 2 {
                    p += 0.16;
                }
                if count >= 340 || !self.chance(p) {
                    continue;
                }
                let n = if self.chance(0.3) { 2 } else { 1 };
                let mut k = c as i32;
                for i in 0..n {
                    if i == 1 {
                        let (dx, dy) = [(1, 0), (0, 1), (1, 1), (-1, 1)][self.below(4) as usize];
                        k = idx(x + dx, y + dy);
                        let ki = k as usize;
                        if ground[ki] != 0 || taken[ki] != 0 || d[ki] == 0 {
                            break;
                        }
                    }
                    taken[k as usize] = 1;
                    let q = self.map.center(k);
                    let v = self.below(4) as u8;
                    let ph = (self.rnd() * 16.0) as f32;
                    let jx = ((self.rnd() - 0.5) * 24.0) as f32;
                    let jy = ((self.rnd() - 0.5) * 16.0) as f32;
                    self.push_decor(DecorKind::WaterRock, q.x + jx, q.y + jy, v, ph);
                    count += 1;
                }
            }
        }
    }

    /// One duck + lily pads + reeds per pond; a few ducks in the sea bays (≥ 1100 px apart).
    fn ponds_and_ducks(&mut self) {
        let ponds = self.ponds.clone();
        for (px, py) in ponds {
            let ph = (self.rnd() * 6.0) as f32;
            self.push_decor(DecorKind::Duck, px * 64.0, py * 64.0, 0, ph);
            let lx = px * 64.0 + ((self.rnd() - 0.5) * 60.0) as f32;
            self.soft_prop(P::Lilypads, lx.round(), (py * 64.0 + 30.0).round());
            // reeds on pond-edge water cells
            let (cx, cy) = (px as i32, py as i32);
            let mut reeds = 0;
            for dy in -3..=3 {
                for dx in -4..=4 {
                    let c = idx(cx + dx, cy + dy);
                    if !self.map.valid(c) || self.map.land[c as usize] != 0 {
                        continue;
                    }
                    let shore = [c - 1, c + 1, c - W, c + W].iter().any(|k| self.map.land[*k as usize] == 1);
                    if shore && reeds < 3 && self.chance(0.25) {
                        reeds += 1;
                        let q = self.map.center(c);
                        let jx = ((self.rnd() - 0.5) * 30.0).round() as f32;
                        self.soft_prop(P::Reeds, q.x + jx, q.y + 6.0);
                    }
                }
            }
        }
        // sea ducks
        let ground: Vec<u8> = self.map.land.iter().map(|v| (*v != 0) as u8).collect();
        let d = cheb_dist(&ground, W, H, 16);
        let mut ducks: Vec<(f32, f32)> = Vec::new();
        for y in 2..H - 2 {
            for x in 2..W - 2 {
                let c = idx(x, y) as usize;
                if ground[c] != 0 || !(3..=5).contains(&d[c]) || ducks.len() >= 12 || !self.chance(0.004) {
                    continue;
                }
                let q = self.map.center(c as i32);
                if ducks.iter().all(|p| (p.0 - q.x) * (p.0 - q.x) + (p.1 - q.y) * (p.1 - q.y) >= 1100.0 * 1100.0) {
                    ducks.push((q.x, q.y));
                    let ph = (self.rnd() * 6.0) as f32;
                    self.push_decor(DecorKind::Duck, q.x, q.y, 0, ph);
                }
            }
        }
    }

    /// Boats and docks at the beaches, shipwrecks in the shallows, reeds and lily pads in sheltered coves.
    fn shore_props(&mut self) {
        let sea = self.sea_mask();
        let mut boats = 0;
        let mut docks = 0;
        let mut wrecks = 0;
        let mut reeds = 0;
        let mut lilies = 0;
        let mut last: Vec<i32> = Vec::new();
        for y in 3..H - 3 {
            for x in 3..W - 3 {
                let c = idx(x, y);
                let ci = c as usize;
                if sea[ci] == 0 || self.map.cliff[ci] != 0 {
                    continue;
                }
                let north = c - W;
                let ni = north as usize;
                if self.map.land[ni] != 1 {
                    continue;
                }
                if last.iter().any(|p| (p % W - x).abs() + (p / W - y).abs() < 7) {
                    continue;
                }
                let beach = self.paint[ni] == PAINT_SAND;
                let land_n = [c - 1, c + 1, c + W, c - W - 1, c - W + 1]
                    .iter()
                    .filter(|k| self.map.land[**k as usize] != 0)
                    .count();
                let q = self.map.center(c);
                let jx = ((self.rnd() - 0.5) * 30.0).round() as f32;
                let r = self.rnd();
                if beach && r < 0.05 && boats < 10 {
                    boats += 1;
                    self.soft_prop(P::Boat, q.x + jx, q.y + 14.0);
                    last.push(c);
                } else if beach && r < 0.08 && docks < 7 && self.map.land[(c + W) as usize] == 0 {
                    docks += 1;
                    self.soft_prop(P::Dock, q.x, q.y + 14.0);
                    last.push(c);
                } else if beach && r < 0.095 && wrecks < 4 {
                    wrecks += 1;
                    self.soft_prop(P::Shipwreck, q.x + jx, q.y + 14.0);
                    last.push(c);
                } else if land_n >= 3 && r < 0.18 && reeds < 40 {
                    reeds += 1;
                    self.soft_prop(P::Reeds, q.x + jx, q.y + 6.0);
                    last.push(c);
                } else if land_n >= 2 && r < 0.24 && lilies < 26 {
                    lilies += 1;
                    self.soft_prop(P::Lilypads, q.x + jx, q.y + 6.0);
                    last.push(c);
                }
            }
        }
    }

    /// Flower patches (one dominant flower per patch) in the open meadows and on the plateaus.
    fn flower_patches(&mut self) {
        let flowers = [P::Flowers, P::FlowersRed, P::FlowersBlue, P::FlowersWhite, P::Clover];
        let want = (70.0 * self.rich).round() as usize;
        let mut made = 0;
        let mut t = 0;
        while made < want && t < 3000 {
            t += 1;
            let x = 4 + self.below((W - 8) as u32);
            let y = 4 + self.below((H - 8) as u32);
            let c = idx(x, y);
            if !self.detail_free(c) || self.map.no_build[c as usize] != 0 {
                continue;
            }
            let ci = c as usize;
            if self.map.level[ci] == 0 && self.edge0[ci] < 3 {
                continue;
            }
            let dom = flowers[self.below(flowers.len() as u32) as usize];
            let n = 3 + self.below(5);
            for _ in 0..n {
                let k = idx(x + self.below(5) - 2, y + self.below(3) - 1);
                if !self.detail_free(k) || self.map.level[k as usize] != self.map.level[ci] {
                    continue;
                }
                let p = match self.below(10) {
                    0..=5 => dom,
                    6 => P::Clover,
                    7 => P::Grass,
                    8 => P::TallGrass,
                    _ => flowers[self.below(flowers.len() as u32) as usize],
                };
                self.soft_prop_in(p, k);
            }
            made += 1;
        }
    }

    /// Wood-floor details next to the trees (ferns and mushrooms under pines; logs, stumps, sticks under birches).
    fn wood_details(&mut self) {
        let trees: Vec<(i32, u8)> = self.trees.iter().map(|t| (t.cell, t.variant)).collect();
        for (c, v) in trees {
            if !self.chance(0.1) {
                continue;
            }
            let (x, y) = (c % W, c / W);
            let (dx, dy) = [(1, 0), (-1, 0), (0, 1), (1, 1), (-1, 1)][self.below(5) as usize];
            let k = idx(x + dx, y + dy);
            if !self.detail_free(k) {
                continue;
            }
            let p = if v < 2 {
                [P::Ferns, P::Mushrooms, P::Ferns, P::Sticks][self.below(4) as usize]
            } else {
                [P::FallenLog, P::Stump, P::Sticks, P::Mushrooms, P::Ferns][self.below(5) as usize]
            };
            self.soft_prop_in(p, k);
        }
    }

    /// Grass tufts at the feet of cliffs and along coasts, puddles by the roads, bones on dry ground.
    fn edge_details(&mut self) {
        let want = (110.0 * self.rich).round() as usize;
        let mut made = 0;
        let mut t = 0;
        while made < want && t < 6000 {
            t += 1;
            let x = 3 + self.below((W - 6) as u32);
            let y = 3 + self.below((H - 6) as u32);
            let c = idx(x, y);
            if !self.detail_free(c) {
                continue;
            }
            let ci = c as usize;
            if self.map.level[ci] == 0 && self.edge0[ci] > 2 {
                continue;
            }
            let p = [P::Grass, P::TallGrass, P::Grass, P::Pebbles][self.below(4) as usize];
            self.soft_prop_in(p, c);
            made += 1;
        }
        // puddles on the roads
        let mut puddles = 0;
        let mut t = 0;
        while puddles < 14 && t < 4000 {
            t += 1;
            let c = idx(3 + self.below((W - 6) as u32), 3 + self.below((H - 6) as u32));
            let ci = c as usize;
            if self.road[ci] == super::roads::ROAD_CORE && self.deco[ci] == 0 && self.reserved[ci] == 0 {
                self.soft_prop_in(P::Puddle, c);
                puddles += 1;
            }
        }
        // bones in the dry corners
        for c in 0..self.map.len() as i32 {
            if self.paint[c as usize] == PAINT_DRY && self.detail_free(c) && self.chance(0.06) {
                self.soft_prop_in(P::Bones, c);
            }
        }
    }

    /// 32 clouds drifting east (02 §6): variant `i % 8`, speed 7..16 px/s in `scale`.
    fn clouds(&mut self) {
        let count = (8.0 * (W * H) as f64 / (144.0 * 96.0)).round() as usize;
        for i in 0..count {
            let x = (self.rnd() * (W as f64 * 64.0 + 300.0) - 300.0) as f32;
            let y = (self.rnd() * (H as f64 * 64.0 - 50.0) - 100.0) as f32;
            let speed = (7.0 + self.rnd() * 9.0) as f32;
            self.map.decor.push(Decor {
                kind: DecorKind::Cloud,
                x,
                y,
                variant: (i % 8) as u8,
                phase: 0.0,
                scale: speed,
                solid: false,
            });
        }
    }

    /// `openWater`: water cells with no ground within Chebyshev 1 and ground within 5 (splash FX).
    pub fn open_water(&self) -> Vec<i32> {
        let mut out = Vec::new();
        for y in 1..H - 1 {
            for x in 1..W - 1 {
                let c = idx(x, y) as usize;
                let d = self.map.depth[c];
                if self.map.land[c] == 0 && self.map.cliff[c] == 0 && (2..=5).contains(&d) {
                    out.push(c as i32);
                }
            }
        }
        out
    }
}
