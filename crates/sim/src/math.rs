//! Small 2D math helpers (no external crate). OWNER: foundation.
//!
//! Sim units are JS pixels (`TILE = 64.0` px per cell), `x` east, `y` south.

use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};
use serde::{Deserialize, Serialize};

/// 2D vector / point in sim pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct V2 {
    pub x: f32,
    pub y: f32,
}

/// Shorthand constructor.
#[inline]
pub const fn v2(x: f32, y: f32) -> V2 {
    V2 { x, y }
}

impl V2 {
    pub const ZERO: V2 = V2 { x: 0.0, y: 0.0 };

    #[inline]
    pub const fn new(x: f32, y: f32) -> V2 {
        V2 { x, y }
    }
    #[inline]
    pub fn len(self) -> f32 {
        self.x.hypot(self.y)
    }
    #[inline]
    pub fn len2(self) -> f32 {
        self.x * self.x + self.y * self.y
    }
    #[inline]
    pub fn dist(self, o: V2) -> f32 {
        (o.x - self.x).hypot(o.y - self.y)
    }
    #[inline]
    pub fn dist2(self, o: V2) -> f32 {
        let dx = o.x - self.x;
        let dy = o.y - self.y;
        dx * dx + dy * dy
    }
    /// Unit vector; returns `ZERO` for a (near) zero vector.
    #[inline]
    pub fn normalize(self) -> V2 {
        let l = self.len();
        if l > 1e-6 {
            V2::new(self.x / l, self.y / l)
        } else {
            V2::ZERO
        }
    }
    #[inline]
    pub fn dot(self, o: V2) -> f32 {
        self.x * o.x + self.y * o.y
    }
    /// Perpendicular (rotated +90° in screen space: (x, y) -> (-y, x)).
    #[inline]
    pub fn perp(self) -> V2 {
        V2::new(-self.y, self.x)
    }
    #[inline]
    pub fn lerp(self, o: V2, t: f32) -> V2 {
        V2::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }
    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    #[inline]
    pub fn round(self) -> V2 {
        V2::new(self.x.round(), self.y.round())
    }
    /// Cell index components `(floor(x/64), floor(y/64))` (may be out of map).
    #[inline]
    pub fn cell(self) -> (i32, i32) {
        (
            (self.x / crate::TILE).floor() as i32,
            (self.y / crate::TILE).floor() as i32,
        )
    }
}

impl Add for V2 {
    type Output = V2;
    #[inline]
    fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for V2 {
    type Output = V2;
    #[inline]
    fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for V2 {
    type Output = V2;
    #[inline]
    fn mul(self, k: f32) -> V2 {
        V2::new(self.x * k, self.y * k)
    }
}
impl Div<f32> for V2 {
    type Output = V2;
    #[inline]
    fn div(self, k: f32) -> V2 {
        V2::new(self.x / k, self.y / k)
    }
}
impl Neg for V2 {
    type Output = V2;
    #[inline]
    fn neg(self) -> V2 {
        V2::new(-self.x, -self.y)
    }
}
impl AddAssign for V2 {
    #[inline]
    fn add_assign(&mut self, o: V2) {
        self.x += o.x;
        self.y += o.y;
    }
}
impl SubAssign for V2 {
    #[inline]
    fn sub_assign(&mut self, o: V2) {
        self.x -= o.x;
        self.y -= o.y;
    }
}
impl MulAssign<f32> for V2 {
    #[inline]
    fn mul_assign(&mut self, k: f32) {
        self.x *= k;
        self.y *= k;
    }
}
impl DivAssign<f32> for V2 {
    #[inline]
    fn div_assign(&mut self, k: f32) {
        self.x /= k;
        self.y /= k;
    }
}

/// `clamp(v, a, b)` as in `assets.js:603` (no panic when a > b: returns a).
#[inline]
pub fn clamp(v: f32, a: f32, b: f32) -> f32 {
    if v < a {
        a
    } else if v > b {
        b
    } else {
        v
    }
}

#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Axis-aligned pixel rectangle (`x0 <= x1`, `y0 <= y1`), e.g. a building footprint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub const fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect { x0, y0, x1, y1 }
    }
    /// Footprint of `w x h` cells whose top-left cell is `(cx, cy)`.
    pub fn of_cells(cx: i32, cy: i32, w: i32, h: i32) -> Rect {
        let t = crate::TILE;
        Rect::new(
            cx as f32 * t,
            cy as f32 * t,
            (cx + w) as f32 * t,
            (cy + h) as f32 * t,
        )
    }
    /// `rectDist(px, py, b)` (`entities.js:39-41`): 0 inside, else distance to the rectangle.
    #[inline]
    pub fn dist(&self, p: V2) -> f32 {
        let dx = (self.x0 - p.x).max(0.0).max(p.x - self.x1);
        let dy = (self.y0 - p.y).max(0.0).max(p.y - self.y1);
        dx.hypot(dy)
    }
    #[inline]
    pub fn contains(&self, p: V2) -> bool {
        p.x >= self.x0 && p.x <= self.x1 && p.y >= self.y0 && p.y <= self.y1
    }
    #[inline]
    pub fn center(&self) -> V2 {
        V2::new((self.x0 + self.x1) * 0.5, (self.y0 + self.y1) * 0.5)
    }
}

/// Bitset over the map cells (`W*H` bits), used for fog `explored`/`visible`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BitGrid {
    pub len: u32,
    pub words: Vec<u64>,
}

impl BitGrid {
    pub fn new(len: usize) -> BitGrid {
        BitGrid {
            len: len as u32,
            words: vec![0; len.div_ceil(64)],
        }
    }
    /// Grid sized for the whole map.
    pub fn for_map() -> BitGrid {
        BitGrid::new(crate::MAP_W * crate::MAP_H)
    }
    #[inline]
    pub fn get(&self, i: usize) -> bool {
        i < self.len as usize && (self.words[i >> 6] >> (i & 63)) & 1 == 1
    }
    /// Sets bit `i`; returns true if it changed.
    #[inline]
    pub fn set(&mut self, i: usize) -> bool {
        if i >= self.len as usize {
            return false;
        }
        let w = &mut self.words[i >> 6];
        let m = 1u64 << (i & 63);
        let changed = *w & m == 0;
        *w |= m;
        changed
    }
    #[inline]
    pub fn unset(&mut self, i: usize) {
        if i < self.len as usize {
            self.words[i >> 6] &= !(1u64 << (i & 63));
        }
    }
    pub fn clear(&mut self) {
        self.words.iter_mut().for_each(|w| *w = 0);
    }
    pub fn fill(&mut self) {
        self.words.iter_mut().for_each(|w| *w = !0);
        let rem = self.len as usize % 64;
        if rem != 0
            && let Some(last) = self.words.last_mut()
        {
            *last = (1u64 << rem) - 1;
        }
    }
    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_ops() {
        let a = v2(3.0, 4.0);
        assert_eq!(a.len(), 5.0);
        assert_eq!((a - v2(3.0, 0.0)).len(), 4.0);
        let n = a.normalize();
        assert!((n.len() - 1.0).abs() < 1e-6);
        assert_eq!(V2::ZERO.normalize(), V2::ZERO);
        assert_eq!(a * 2.0, v2(6.0, 8.0));
    }

    #[test]
    fn rect_dist() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(r.dist(v2(5.0, 5.0)), 0.0);
        assert_eq!(r.dist(v2(13.0, 14.0)), 5.0);
    }

    #[test]
    fn bitgrid() {
        let mut b = BitGrid::new(130);
        assert!(b.set(129));
        assert!(!b.set(129));
        assert!(b.get(129) && !b.get(128) && !b.get(500));
        b.fill();
        assert_eq!(b.count(), 130);
    }
}
