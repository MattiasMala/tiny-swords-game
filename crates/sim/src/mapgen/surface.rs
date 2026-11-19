//! Per-cell ground material (`Map.surface`) and water depth (`Map.depth`) — 09 §3.6/§3.7/§11.2, MAPGEN.md §3.
//!
//! Colour by tier: level 0 = `GrassLow` (color3), level 1 = `GrassMid` (color1), level 2 = `GrassTop` (color2);
//! pine-wood floor = `ForestFloor` (color5) under dense pine woods on the lowland; a few dry meadows = `GrassDry`
//! (color4) in blocky patches, graveyards and dead-tree corners; `Sand` on beach stretches; `Dirt` on farm plots,
//! camps, hamlets; `Path` on roads and castle plazas. Cliff cells take the surface of the tier above them (the rim
//! of the raised block), stairs the surface of their upper tier.

use super::grid::{Mask, cheb_dist};
use super::land::COARSE;
use super::roads::ROAD_CORE;
use super::scenery::{PAINT_DIRT, PAINT_DRY, PAINT_SAND};
use super::{Gen, H, W, idx};
use crate::map::Surface;

fn tier(level: u8) -> Surface {
    match level {
        0 => Surface::GrassLow,
        1 => Surface::GrassMid,
        _ => Surface::GrassTop,
    }
}

impl Gen {
    pub fn surfaces(&mut self) {
        let n = self.map.len();
        // ---- dry meadows: coarse blocky noise patches on the open lowland ----
        let cw = W / COARSE;
        let ch = H / COARSE;
        let mut coarse = vec![0u8; (cw * ch) as usize];
        for cy in 0..ch {
            for cx in 0..cw {
                let v = self.noise.fbm(cx as f64 * 0.16 + 71.0, cy as f64 * 0.16 + 37.0);
                coarse[(cy * cw + cx) as usize] = (v > 0.71) as u8;
            }
        }
        let dry = self.upscale(&coarse, cw, ch, 0.5);

        // ---- pine-wood floor: dense pines (≥ 4 within the 5x5 window), dilated, cleaned ----
        let mut pines = vec![0u8; n];
        for t in &self.trees {
            if t.variant < 2 {
                pines[t.cell as usize] = 1;
            }
        }
        let sat = super::grid::Sat::new(&pines, W, H);
        let mut forest = Mask::new(W, H);
        for y in 0..H {
            for x in 0..W {
                let c = (y * W + x) as usize;
                if sat.sum(x - 2, y - 2, x + 2, y + 2) >= 4 && self.map.land[c] == 1 && self.map.level[c] == 0 {
                    forest.v[c] = 1;
                }
            }
        }
        let mut forest = forest.dilate(1, 1, 1, 1).close(3).open(3);
        for c in 0..n {
            let m = &self.map;
            if !(m.land[c] == 1 && m.level[c] == 0 && m.cliff[c] == 0 && m.stair[c] == 0) {
                forest.v[c] = 0;
            }
        }

        for c in 0..n {
            let m = &self.map;
            let s = if m.cliff[c] != 0 {
                // rim of the tier above
                let up = c.wrapping_sub(W as usize);
                if up < n { tier(m.level[up]) } else { Surface::GrassLow }
            } else if m.land[c] == 0 {
                Surface::Water
            } else if m.stair[c] != 0 {
                let lvl = if m.stair[c] % 2 == 1 { m.level[c] + 1 } else { m.level[c] };
                tier(lvl)
            } else if m.land[c] == 2 {
                tier(m.level[c])
            } else if matches!(self.road[c], 1..=3) {
                Surface::Path
            } else if self.paint[c] == PAINT_SAND {
                Surface::Sand
            } else if self.paint[c] == PAINT_DIRT {
                Surface::Dirt
            } else if self.paint[c] == PAINT_DRY {
                Surface::GrassDry
            } else if m.level[c] == 0 && forest.v[c] != 0 {
                Surface::ForestFloor
            } else if m.level[c] == 0 && dry.v[c] != 0 && self.reserved[c] == 0 && self.edge0[c] >= 2 {
                Surface::GrassDry
            } else {
                tier(m.level[c])
            };
            self.map.surface[c] = s;
        }
        // spur roads read as paths only where they are core cells next to other path cells (no lone dots)
        for c in 0..n {
            if self.map.surface[c] == Surface::Path && self.road[c] == ROAD_CORE {
                let (x, y) = ((c as i32) % W, (c as i32) / W);
                let around = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .filter(|(dx, dy)| {
                        self.map.inside(x + dx, y + dy)
                            && self.map.surface[idx(x + dx, y + dy) as usize] == Surface::Path
                    })
                    .count();
                if around == 0 {
                    self.map.surface[c] = tier(self.map.level[c]);
                }
            }
        }

        // ---- water depth: Chebyshev distance to the nearest ground (land > 0 or sea cliff), capped at 15 ----
        let ground: Vec<u8> = (0..n).map(|i| (self.map.land[i] != 0 || self.map.cliff[i] != 0) as u8).collect();
        self.map.depth = cheb_dist(&ground, W, H, 15);
    }
}
