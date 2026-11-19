//! Camps, chests and prop vignettes (02 §5.13 redesigned per MAPGEN.md §10): props are composed into small scenes
//! instead of confetti — abandoned camps (tent, fire, supplies, chest), ruins on the plateaus (arch, wall, columns,
//! statue, obelisk, stone circle), farms (windmill, haystacks, scarecrow, pumpkins on a dirt plot, fence, beehive),
//! hamlet remains by the roads (well, cart, barrels, crates), gloomy graveyards (graves, dead tree, bones on dry
//! grass), standing stones on plateau rims, signposts at road forks, wagon wrecks by the roads.
//!
//! Solid props block their cell (`static_block`); the wide ones (windmill, ruin arch/wall) block two cells and are
//! anchored on the boundary between them.

use super::{Gen, H, W, idx};
use crate::defs::{PropKind as P, *};
use crate::map::{Decor, DecorKind, GenChest};

/// `paint[]` values (surface overrides).
pub const PAINT_DIRT: u8 = 1;
pub const PAINT_DRY: u8 = 2;
pub const PAINT_SAND: u8 = 3;

impl Gen {
    /// Solid prop on cell `c` (foot at the cell centre + 20 px, as the JS `prop()`).
    pub fn solid_prop(&mut self, p: P, c: i32) {
        self.used[c as usize] = 1;
        self.solid.push((c, p));
        let q = self.map.center(c);
        self.map.decor.push(Decor {
            kind: DecorKind::Prop(p),
            x: q.x,
            y: q.y + 20.0,
            variant: 0,
            phase: 0.0,
            scale: 1.0,
            solid: true,
        });
    }

    /// Wide solid prop over `c` and `c + 1`, anchored on their shared edge.
    pub fn solid_prop_wide(&mut self, p: P, c: i32) {
        for k in [c, c + 1] {
            self.used[k as usize] = 1;
            self.solid.push((k, p));
        }
        let q = self.map.center(c);
        self.map.decor.push(Decor {
            kind: DecorKind::Prop(p),
            x: q.x + 32.0,
            y: q.y + 20.0,
            variant: 0,
            phase: 0.0,
            scale: 1.0,
            solid: true,
        });
    }

    /// Non-solid prop at a px position (walk-over detail).
    pub fn soft_prop(&mut self, p: P, x: f32, y: f32) {
        let c = self.map.cell_at(x, y - 1.0);
        if c >= 0 {
            self.deco[c as usize] = 1;
        }
        self.map.decor.push(Decor {
            kind: DecorKind::Prop(p),
            x,
            y,
            variant: 0,
            phase: 0.0,
            scale: 1.0,
            solid: false,
        });
    }

    /// Non-solid prop jittered inside cell `c`.
    pub fn soft_prop_in(&mut self, p: P, c: i32) {
        let q = self.map.center(c);
        let x = q.x + ((self.rnd() - 0.5) * 28.0).round() as f32;
        let y = q.y + 10.0 + ((self.rnd() - 0.5) * 16.0).round() as f32;
        self.soft_prop(p, x, y);
    }

    fn chest(&mut self, c: i32) {
        self.used[c as usize] = 1;
        let res = Res::ALL[self.chests.len() % 4];
        self.chests.push(GenChest { cell: c, res });
    }

    /// Cell free for a non-solid detail.
    pub fn detail_free(&self, c: i32) -> bool {
        self.map.valid(c) && {
            let i = c as usize;
            self.map.land[i] == 1
                && self.map.cliff[i] == 0
                && self.map.stair[i] == 0
                && self.used[i] == 0
                && self.deco[i] == 0
                && self.road[i] == 0
        }
    }

    /// All cells of the rectangle satisfy `ok`.
    fn rect_ok(&self, x0: i32, y0: i32, w: i32, h: i32, ok: &dyn Fn(&Gen, i32) -> bool) -> bool {
        x0 >= 3
            && y0 >= 3
            && x0 + w <= W - 3
            && y0 + h <= H - 4
            && (y0..y0 + h).all(|y| (x0..x0 + w).all(|x| ok(self, idx(x, y))))
    }

    fn paint_rect(&mut self, x0: i32, y0: i32, w: i32, h: i32, v: u8, round: bool) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                if round && w > 2 && h > 2 && (x == x0 || x == x0 + w - 1) && (y == y0 || y == y0 + h - 1) {
                    continue;
                }
                if self.map.inside(x, y) {
                    let k = idx(x, y) as usize;
                    if self.map.land[k] == 1 && self.map.cliff[k] == 0 && self.map.stair[k] == 0 {
                        self.paint[k] = v;
                    }
                }
            }
        }
    }

    /// `spot()` with an aesthetic acceptance weight: random free cell far from bases, `tries` attempts.
    fn spot_w(&mut self, tries: u32, test: &dyn Fn(&Gen, i32) -> bool, weight: &dyn Fn(&Gen, i32) -> f64) -> i32 {
        for _ in 0..tries {
            let x = 3 + self.below((W - 6) as u32);
            let y = 3 + self.below((H - 6) as u32);
            let c = idx(x, y);
            if !self.far_from_bases(x as f64, y as f64) || !test(self, c) {
                continue;
            }
            if self.rnd() < weight(self, c) {
                return c;
            }
        }
        -1
    }

    /// Number of trees / used cells within Chebyshev `r`.
    fn used_around(&self, c: i32, r: i32) -> i32 {
        let (x, y) = (c % W, c / W);
        let mut n = 0;
        for dy in -r..=r {
            for dx in -r..=r {
                if self.map.inside(x + dx, y + dy) && self.used[idx(x + dx, y + dy) as usize] != 0 {
                    n += 1;
                }
            }
        }
        n
    }

    fn near_road(&self, c: i32, r: i32) -> bool {
        let (x, y) = (c % W, c / W);
        (-r..=r).any(|dy| {
            (-r..=r).any(|dx| {
                self.map.inside(x + dx, y + dy) && self.road[idx(x + dx, y + dy) as usize] == super::roads::ROAD_CORE
            })
        })
    }

    pub fn scenery(&mut self) {
        self.camps();
        self.loose_chests();
        self.ruins();
        self.farms();
        self.hamlets();
        self.graveyards();
        self.standing_stones();
        self.signposts();
        self.wrecks_and_trees();
    }

    /// Abandoned camps (36 attempts, 02 §5.13): tent, campfire, supplies, chest; near woods and edges.
    fn camps(&mut self) {
        let room3 = |g: &Gen, c: i32| {
            let (x, y) = (c % W, c / W);
            x > 3 && y > 3 && x < W - 4 && y < H - 4 && (-1..=1).all(|dy| (-1..=1).all(|dx| g.free_low(idx(x + dx, y + dy))))
        };
        let weight = |g: &Gen, c: i32| {
            if g.used_around(c, 4) >= 3 || g.edge0[c as usize] <= 5 { 1.0 } else { 0.3 }
        };
        for i in 0..36 {
            let c = self.spot_w(300, &room3, &weight);
            if c < 0 {
                continue;
            }
            self.solid_prop(P::Tent, c - W);
            self.solid_prop(P::Campfire, c + 1);
            self.solid_prop([P::Barrel, P::Crate, P::Logs][i % 3], c - W + 1);
            self.solid_prop([P::Hay, P::Cart, P::Crate][i % 3], c - 1);
            self.chest(c);
            let (x, y) = (c % W, c / W);
            self.paint_rect(x - 1, y - 1, 3, 3, PAINT_DIRT, false);
            // a few walk-over leftovers
            for (dx, dy, p) in [(-1, 1, P::Sticks), (1, 1, P::Stump)] {
                let k = idx(x + dx, y + dy);
                if self.detail_free(k) && self.chance(0.6) {
                    self.soft_prop_in(p, k);
                }
            }
        }
    }

    /// Loose chests (64 attempts): at forest edges, coasts and plateau feet rather than in open meadows.
    fn loose_chests(&mut self) {
        let weight = |g: &Gen, c: i32| {
            if g.used_around(c, 2) >= 2 || g.edge0[c as usize] <= 2 { 1.0 } else { 0.35 }
        };
        for _ in 0..64 {
            let c = self.spot_w(300, &|g, c| g.free_low(c), &weight);
            if c >= 0 {
                self.chest(c);
            }
        }
    }

    /// Ruins: a main piece with companions on a plateau (or the lowland), pebbles and grass around.
    fn ruins(&mut self) {
        let n = 8 + self.below(5);
        let mains = [P::RuinArch, P::RuinWall, P::StoneCircle, P::Obelisk, P::Statue, P::Shrine, P::RuinArch, P::Column];
        for i in 0..n {
            let high = self.chance(0.65);
            let ok = move |g: &Gen, c: i32| if high { g.free_high(c) } else { g.free_low(c) };
            let test = |g: &Gen, c: i32| {
                let (x, y) = (c % W, c / W);
                let lvl = g.map.level[c as usize];
                g.rect_ok(x - 2, y - 2, 5, 4, &|g2, k| {
                    g2.map.level[k as usize] == lvl && if lvl > 0 { g2.free_high(k) } else { g2.free_low(k) }
                })
            };
            let c = self.spot_w(400, &|g, c| ok(g, c) && test(g, c), &|_, _| 1.0);
            if c < 0 {
                continue;
            }
            let main = mains[(i as usize) % mains.len()];
            let (x, y) = (c % W, c / W);
            if matches!(main, P::RuinArch | P::RuinWall) {
                self.solid_prop_wide(main, c);
            } else {
                self.solid_prop(main, c);
            }
            let comps = 1 + self.below(3);
            let pool = [P::Column, P::Column, P::Boulder, P::Menhir, P::RuinWall, P::Statue];
            let mut placed = 0;
            for _ in 0..12 {
                if placed >= comps {
                    break;
                }
                let dx = self.below(5) - 2;
                let dy = self.below(3) - 1;
                if dx.abs() < 2 && dy == 0 {
                    continue;
                }
                let k = idx(x + dx, y + dy);
                let lvl_ok = self.map.level[k as usize] == self.map.level[c as usize];
                if !lvl_ok || !(self.free_high(k) || self.free_low(k)) {
                    continue;
                }
                let p = pool[self.below(pool.len() as u32) as usize];
                if p == P::RuinWall {
                    if self.free_high(k + 1) || self.free_low(k + 1) {
                        self.solid_prop_wide(p, k);
                    } else {
                        continue;
                    }
                } else {
                    self.solid_prop(p, k);
                }
                placed += 1;
            }
            for _ in 0..3 + self.below(3) {
                let k = idx(x + self.below(5) - 2, y + self.below(4) - 1);
                if self.detail_free(k) {
                    let p = [P::Pebbles, P::Grass, P::TallGrass, P::Bones, P::Pebbles][self.below(5) as usize];
                    self.soft_prop_in(p, k);
                }
            }
            if self.chance(0.2) {
                let k = idx(x, y + 1);
                if self.detail_free(k) {
                    self.soft_prop_in(P::ChestOpen, k);
                }
            }
        }
    }

    /// Farms: dirt plot with pumpkins, windmill, haystacks, scarecrow, fence, beehive.
    fn farms(&mut self) {
        let n = 4 + self.below(4);
        for _ in 0..n {
            let pw = 4 + self.below(3);
            let ph = 2 + self.below(2);
            let test = |g: &Gen, c: i32| {
                let (x, y) = (c % W, c / W);
                g.edge0[c as usize] >= 3 && g.rect_ok(x - 2, y - 2, pw + 4, ph + 3, &|g2, k| g2.free_low(k))
            };
            let weight = |g: &Gen, c: i32| if g.near_road(c, 6) { 1.0 } else { 0.25 };
            let c = self.spot_w(500, &test, &weight);
            if c < 0 {
                continue;
            }
            let (x0, y0) = (c % W, c / W);
            self.paint_rect(x0, y0, pw, ph, PAINT_DIRT, false);
            for y in y0..y0 + ph {
                for x in x0..x0 + pw {
                    let k = idx(x, y);
                    if self.chance(0.45) {
                        self.soft_prop_in(P::Pumpkins, k);
                    } else {
                        self.deco[k as usize] = 1;
                    }
                }
            }
            // windmill on one side of the plot (top row level), haystacks next to it
            let left = self.chance(0.5);
            let wx = if left { x0 - 2 } else { x0 + pw };
            let wc = idx(wx, y0);
            if self.free_low(wc) && self.free_low(wc + 1) {
                self.solid_prop_wide(P::Windmill, wc);
            }
            let hay_y = y0 + 1;
            for k in 0..1 + self.below(2) {
                let hc = idx(wx + k, hay_y);
                if self.free_low(hc) {
                    self.solid_prop(P::Hay, hc);
                }
            }
            // scarecrow in the field
            let sc = idx(x0 + 1 + self.below((pw - 2) as u32), y0 + self.below(ph as u32));
            self.solid_prop(P::Scarecrow, sc);
            // fence along the bottom edge (pieces)
            let fy = y0 + ph;
            for fx in x0..x0 + pw {
                let fc = idx(fx, fy);
                if (fx - x0) % 2 == 0 && self.free_low(fc) && self.chance(0.6) {
                    self.solid_prop(P::Fence, fc);
                }
            }
            if self.chance(0.5) {
                let bc = idx(if left { x0 + pw } else { x0 - 1 }, y0 + ph - 1);
                if self.free_low(bc) {
                    self.solid_prop(P::Beehive, bc);
                }
            }
        }
    }

    /// Hamlet remains near the roads: well, cart, barrels, crates, flowers.
    fn hamlets(&mut self) {
        let n = 4 + self.below(4);
        for _ in 0..n {
            let test = |g: &Gen, c: i32| {
                let (x, y) = (c % W, c / W);
                g.rect_ok(x - 1, y - 1, 4, 3, &|g2, k| g2.free_low(k))
            };
            let weight = |g: &Gen, c: i32| if g.near_road(c, 3) { 1.0 } else { 0.05 };
            let c = self.spot_w(900, &test, &weight);
            if c < 0 {
                continue;
            }
            let (x, y) = (c % W, c / W);
            self.solid_prop(P::Well, c);
            let pieces = [(2, 0, P::Cart), (2, 1, P::Barrel), (1, 1, P::Crate), (-1, 1, P::Barrel)];
            for (dx, dy, p) in pieces {
                let k = idx(x + dx, y + dy);
                if self.free_low(k) && self.chance(0.75) {
                    self.solid_prop(p, k);
                }
            }
            for _ in 0..2 {
                let k = idx(x + self.below(5) - 2, y + self.below(3) - 1);
                if self.detail_free(k) {
                    let p = [P::Flowers, P::FlowersWhite, P::Grass][self.below(3) as usize];
                    self.soft_prop_in(p, k);
                }
            }
            self.paint_rect(x - 1, y - 1, 3, 3, PAINT_DIRT, true);
        }
    }

    /// Graveyards in remote corners: graves in rows, a dead tree, bones, dry grass.
    fn graveyards(&mut self) {
        let n = 2 + self.below(3);
        for _ in 0..n {
            let test = |g: &Gen, c: i32| {
                let (x, y) = (c % W, c / W);
                g.rect_ok(x - 1, y - 1, 7, 5, &|g2, k| g2.free_low(k))
                    && g.bases.iter().all(|b| (b.0 - x).abs() + (b.1 - y).abs() > 34)
            };
            let weight = |g: &Gen, c: i32| if g.edge0[c as usize] <= 6 { 1.0 } else { 0.3 };
            let c = self.spot_w(700, &test, &weight);
            if c < 0 {
                continue;
            }
            let (x, y) = (c % W, c / W);
            self.paint_rect(x - 1, y - 1, 7, 5, PAINT_DRY, true);
            let rows = 1 + self.below(2);
            for r in 0..rows {
                for k in 0..3 {
                    if self.chance(0.8) {
                        let gc = idx(x + k * 2, y + r * 2);
                        if self.free_low(gc) {
                            self.solid_prop(P::Grave, gc);
                        }
                    }
                }
            }
            let dc = idx(x + 5, y);
            if self.free_low(dc) {
                self.solid_prop(P::Deadtree, dc);
            }
            for _ in 0..2 {
                let k = idx(x + self.below(6), y + 1 + self.below(3));
                if self.detail_free(k) {
                    let p = [P::Bones, P::Pebbles, P::Mushrooms][self.below(3) as usize];
                    self.soft_prop_in(p, k);
                }
            }
        }
    }

    /// Menhirs and boulders on plateau rims and at cliff feet; a shrine/statue/obelisk on some terraces.
    fn standing_stones(&mut self) {
        let n = 10 + self.below(8);
        let rimmy = |g: &Gen, c: i32| {
            let (x, y) = (c % W, c / W);
            let lvl = g.map.level[c as usize];
            [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                let k = idx(x + dx, y + dy) as usize;
                g.map.level[k] != lvl || g.map.cliff[k] != 0
            })
        };
        for _ in 0..n {
            let c = self.spot_w(
                400,
                &|g, c| (g.free_high(c) || g.free_low(c)) && rimmy(g, c),
                &|_, _| 1.0,
            );
            if c >= 0 {
                let p = if self.chance(0.5) { P::Menhir } else { P::Boulder };
                self.solid_prop(p, c);
                let k = c + if self.chance(0.5) { 1 } else { -1 };
                if self.detail_free(k) && self.chance(0.5) {
                    self.soft_prop_in(P::Pebbles, k);
                }
            }
        }
        for h in 0..self.hills.len() {
            let Some((cells, _)) = self.hills[h].top.clone() else {
                continue;
            };
            if !self.chance(0.6) {
                continue;
            }
            for _ in 0..20 {
                let c = cells[self.below(cells.len() as u32) as usize];
                if self.free_high(c) {
                    let p = [P::Shrine, P::Statue, P::Obelisk][self.below(3) as usize];
                    self.solid_prop(p, c);
                    break;
                }
            }
        }
    }

    /// Signposts where the spurs meet the main roads.
    fn signposts(&mut self) {
        let js = self.junctions.clone();
        let mut put: Vec<i32> = Vec::new();
        for j in js {
            if put.iter().any(|p| (p % W - j % W).abs() + (p / W - j / W).abs() < 10) {
                continue;
            }
            let (x, y) = (j % W, j / W);
            for (dx, dy) in [(1, -1), (-1, -1), (1, 1), (-1, 1), (2, 0), (-2, 0)] {
                let k = idx(x + dx, y + dy);
                if self.free_low(k) && self.far_from_bases((x + dx) as f64, (y + dy) as f64) {
                    self.solid_prop(P::Signpost, k);
                    put.push(j);
                    break;
                }
            }
        }
    }

    /// Wagon wrecks beside the roads, dead trees in dry corners, beehives near meadows.
    fn wrecks_and_trees(&mut self) {
        let n = 3 + self.below(3);
        for _ in 0..n {
            let c = self.spot_w(600, &|g, c| g.free_low(c) && g.near_road(c, 1), &|_, _| 1.0);
            if c >= 0 {
                self.solid_prop(P::WagonWreck, c);
                let k = c + W;
                if self.detail_free(k) && self.chance(0.6) {
                    self.soft_prop_in(P::Sticks, k);
                }
            }
        }
        let n = 4 + self.below(5);
        for _ in 0..n {
            let c = self.spot_w(400, &|g, c| g.free_low(c) && g.edge0[c as usize] <= 4, &|_, _| 1.0);
            if c >= 0 {
                self.solid_prop(P::Deadtree, c);
                let (x, y) = (c % W, c / W);
                self.paint_rect(x - 1, y - 1, 3, 3, PAINT_DRY, true);
            }
        }
        let meadows = self.meadows.clone();
        for m in meadows.iter().take(8) {
            if !self.chance(0.5) {
                continue;
            }
            let c = self.map.cell_of(*m);
            let (x, y) = (c % W, c / W);
            for (dx, dy) in [(3, -2), (-3, -2), (3, 2), (-3, 2)] {
                let k = idx(x + dx, y + dy);
                if self.free_low(k) && self.far_from_bases((x + dx) as f64, (y + dy) as f64) {
                    self.solid_prop(P::Beehive, k);
                    break;
                }
            }
        }
    }
}
