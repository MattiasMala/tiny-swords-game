//! Mask utilities for the generator: separable rectangle morphology, connected components, distance transforms,
//! summed-area tables and value noise. Pure integer / `+ - * /` arithmetic (deterministic on every target).
//!
//! Masks are `&[u8]` (0 / non-zero) laid out row-major with an explicit width/height, so the same code works on
//! the whole map and on small local windows (plateau shapes).

/// A row-major `u8` mask with its size.
#[derive(Clone, Debug)]
pub struct Mask {
    pub w: i32,
    pub h: i32,
    pub v: Vec<u8>,
}

impl Mask {
    pub fn new(w: i32, h: i32) -> Mask {
        Mask {
            w,
            h,
            v: vec![0; (w.max(0) * h.max(0)) as usize],
        }
    }

    #[inline]
    pub fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> bool {
        self.inside(x, y) && self.v[(y * self.w + x) as usize] != 0
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, on: bool) {
        if self.inside(x, y) {
            self.v[(y * self.w + x) as usize] = on as u8;
        }
    }

    pub fn count(&self) -> usize {
        self.v.iter().filter(|v| **v != 0).count()
    }

    /// Erosion by the rectangle `[-l, r] x [-u, d]` (outside counts as empty).
    pub fn erode(&self, l: i32, r: i32, u: i32, d: i32) -> Mask {
        let hpass = pass_h(&self.v, self.w, self.h, l, r, true);
        let v = pass_v(&hpass, self.w, self.h, u, d, true);
        Mask {
            w: self.w,
            h: self.h,
            v,
        }
    }

    /// Dilation by the rectangle `[-r, l] x [-d, u]` (the reflection of the erosion element, so
    /// `open = erode(e).dilate(e)` with the same arguments is a proper opening).
    pub fn dilate(&self, l: i32, r: i32, u: i32, d: i32) -> Mask {
        let hpass = pass_h(&self.v, self.w, self.h, r, l, false);
        let v = pass_v(&hpass, self.w, self.h, d, u, false);
        Mask {
            w: self.w,
            h: self.h,
            v,
        }
    }

    /// Opening with a `k x k` square (removes features narrower than k).
    pub fn open(&self, k: i32) -> Mask {
        let a = (k - 1) / 2;
        let b = k - 1 - a;
        self.erode(a, b, a, b).dilate(a, b, a, b)
    }

    /// Closing with a `k x k` square (fills gaps/holes narrower than k).
    pub fn close(&self, k: i32) -> Mask {
        let a = (k - 1) / 2;
        let b = k - 1 - a;
        self.dilate(a, b, a, b).erode(a, b, a, b)
    }

    pub fn and(&mut self, o: &Mask) {
        for (a, b) in self.v.iter_mut().zip(&o.v) {
            *a = (*a != 0 && *b != 0) as u8;
        }
    }

    pub fn and_not(&mut self, o: &Mask) {
        for (a, b) in self.v.iter_mut().zip(&o.v) {
            *a = (*a != 0 && *b == 0) as u8;
        }
    }

    pub fn or(&mut self, o: &Mask) {
        for (a, b) in self.v.iter_mut().zip(&o.v) {
            *a = (*a != 0 || *b != 0) as u8;
        }
    }

    /// 4-connected component labels (`-1` = empty) and the size of each component.
    pub fn components(&self) -> (Vec<i32>, Vec<u32>) {
        components4(&self.v, self.w, self.h)
    }

    /// Keeps only the largest 4-connected component (ties: lowest label = first in scan order).
    pub fn keep_largest(&mut self) -> usize {
        let (lab, sizes) = self.components();
        let Some((best, &n)) = sizes.iter().enumerate().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0)))
        else {
            return 0;
        };
        for (v, l) in self.v.iter_mut().zip(&lab) {
            *v = (*l == best as i32) as u8;
        }
        n as usize
    }
}

/// Horizontal window pass: `all` = every cell of `[x-l, x+r]` set (outside = empty), else any.
fn pass_h(m: &[u8], w: i32, h: i32, l: i32, r: i32, all: bool) -> Vec<u8> {
    let mut out = vec![0u8; m.len()];
    let mut pre = vec![0i32; w as usize + 1];
    for y in 0..h {
        let row = (y * w) as usize;
        for x in 0..w as usize {
            pre[x + 1] = pre[x] + (m[row + x] != 0) as i32;
        }
        for x in 0..w {
            let a = x - l;
            let b = x + r;
            out[row + x as usize] = if all {
                (a >= 0 && b < w && pre[(b + 1) as usize] - pre[a as usize] == b - a + 1) as u8
            } else {
                let a = a.clamp(0, w);
                let b = (b + 1).clamp(0, w);
                (b > a && pre[b as usize] - pre[a as usize] > 0) as u8
            };
        }
    }
    out
}

/// Vertical window pass (see [`pass_h`]).
fn pass_v(m: &[u8], w: i32, h: i32, u: i32, d: i32, all: bool) -> Vec<u8> {
    let mut out = vec![0u8; m.len()];
    let mut pre = vec![0i32; h as usize + 1];
    for x in 0..w {
        for y in 0..h as usize {
            pre[y + 1] = pre[y] + (m[y * w as usize + x as usize] != 0) as i32;
        }
        for y in 0..h {
            let a = y - u;
            let b = y + d;
            out[(y * w + x) as usize] = if all {
                (a >= 0 && b < h && pre[(b + 1) as usize] - pre[a as usize] == b - a + 1) as u8
            } else {
                let a = a.clamp(0, h);
                let b = (b + 1).clamp(0, h);
                (b > a && pre[b as usize] - pre[a as usize] > 0) as u8
            };
        }
    }
    out
}

/// 4-connected components of the non-zero cells.
pub fn components4(m: &[u8], w: i32, h: i32) -> (Vec<i32>, Vec<u32>) {
    let mut lab = vec![-1i32; m.len()];
    let mut sizes = Vec::new();
    let mut stack: Vec<i32> = Vec::new();
    for s in 0..m.len() {
        if m[s] == 0 || lab[s] >= 0 {
            continue;
        }
        let id = sizes.len() as i32;
        let mut n = 0u32;
        lab[s] = id;
        stack.push(s as i32);
        while let Some(c) = stack.pop() {
            n += 1;
            let (x, y) = (c % w, c / w);
            let mut visit = |k: i32| {
                let k = k as usize;
                if m[k] != 0 && lab[k] < 0 {
                    lab[k] = id;
                    stack.push(k as i32);
                }
            };
            if x > 0 {
                visit(c - 1);
            }
            if x < w - 1 {
                visit(c + 1);
            }
            if y > 0 {
                visit(c - w);
            }
            if y < h - 1 {
                visit(c + w);
            }
        }
        sizes.push(n);
    }
    (lab, sizes)
}

/// 4-connected flood fill from `start` over cells where `ok` holds; returns the visited mask.
pub fn flood4(w: i32, h: i32, starts: &[i32], ok: impl Fn(usize) -> bool) -> Vec<u8> {
    let n = (w * h) as usize;
    let mut seen = vec![0u8; n];
    let mut stack: Vec<i32> = Vec::new();
    for &s in starts {
        if s >= 0 && (s as usize) < n && ok(s as usize) && seen[s as usize] == 0 {
            seen[s as usize] = 1;
            stack.push(s);
        }
    }
    while let Some(c) = stack.pop() {
        let (x, y) = (c % w, c / w);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let k = (ny * w + nx) as usize;
            if seen[k] == 0 && ok(k) {
                seen[k] = 1;
                stack.push(k as i32);
            }
        }
    }
    seen
}

/// Chebyshev distance (8-neighbour chamfer, exact) from every cell to the nearest cell with `src != 0`,
/// saturated at `cap`. Cells outside the grid are not sources.
pub fn cheb_dist(src: &[u8], w: i32, h: i32, cap: u8) -> Vec<u8> {
    chamfer(src, w, h, cap, true)
}

/// Manhattan distance (4-neighbour chamfer) to the nearest source cell, saturated at `cap`.
pub fn manh_dist(src: &[u8], w: i32, h: i32, cap: u8) -> Vec<u8> {
    chamfer(src, w, h, cap, false)
}

fn chamfer(src: &[u8], w: i32, h: i32, cap: u8, diag: bool) -> Vec<u8> {
    let wu = w as usize;
    let mut d: Vec<u8> = src.iter().map(|v| if *v != 0 { 0 } else { cap }).collect();
    let relax = |d: &mut Vec<u8>, c: usize, k: usize| {
        let v = d[k].saturating_add(1).min(cap);
        if v < d[c] {
            d[c] = v;
        }
    };
    for y in 0..h as usize {
        for x in 0..wu {
            let c = y * wu + x;
            if x > 0 {
                relax(&mut d, c, c - 1);
            }
            if y > 0 {
                relax(&mut d, c, c - wu);
                if diag {
                    if x > 0 {
                        relax(&mut d, c, c - wu - 1);
                    }
                    if x + 1 < wu {
                        relax(&mut d, c, c - wu + 1);
                    }
                }
            }
        }
    }
    for y in (0..h as usize).rev() {
        for x in (0..wu).rev() {
            let c = y * wu + x;
            if x + 1 < wu {
                relax(&mut d, c, c + 1);
            }
            if y + 1 < h as usize {
                relax(&mut d, c, c + wu);
                if diag {
                    if x + 1 < wu {
                        relax(&mut d, c, c + wu + 1);
                    }
                    if x > 0 {
                        relax(&mut d, c, c + wu - 1);
                    }
                }
            }
        }
    }
    d
}

/// Summed-area table of a mask: `(w+1) x (h+1)`, query inclusive rectangles (outside = 0).
#[derive(Clone, Debug)]
pub struct Sat {
    w: i32,
    h: i32,
    s: Vec<u32>,
}

impl Sat {
    pub fn new(m: &[u8], w: i32, h: i32) -> Sat {
        let sw = (w + 1) as usize;
        let mut s = vec![0u32; sw * (h + 1) as usize];
        for y in 0..h as usize {
            let mut row = 0u32;
            for x in 0..w as usize {
                row += (m[y * w as usize + x] != 0) as u32;
                s[(y + 1) * sw + x + 1] = s[y * sw + x + 1] + row;
            }
        }
        Sat { w, h, s }
    }

    /// Number of set cells in the inclusive rectangle (clipped to the grid).
    pub fn sum(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> u32 {
        let (x0, y0) = (x0.max(0), y0.max(0));
        let (x1, y1) = (x1.min(self.w - 1), y1.min(self.h - 1));
        if x1 < x0 || y1 < y0 {
            return 0;
        }
        let sw = (self.w + 1) as usize;
        let (a, b) = (x0 as usize, x1 as usize + 1);
        let (c, d) = (y0 as usize, y1 as usize + 1);
        self.s[d * sw + b] + self.s[c * sw + a] - self.s[c * sw + b] - self.s[d * sw + a]
    }
}

/// Value noise on a 32x32 wrapping lattice, smoothstep-bilinear (02 §5.4), values in [0, 1).
#[derive(Clone, Debug)]
pub struct Noise {
    lattice: Vec<f64>,
}

impl Noise {
    pub fn new(values: impl Iterator<Item = f64>) -> Noise {
        let mut lattice: Vec<f64> = values.take(1024).collect();
        lattice.resize(1024, 0.5);
        Noise { lattice }
    }

    pub fn at(&self, x: f64, y: f64) -> f64 {
        let (xf, yf) = (x.floor(), y.floor());
        let (fx, fy) = (x - xf, y - yf);
        let (xi, yi) = (xf as i64, yf as i64);
        let l = |a: i64, b: i64| self.lattice[((b & 31) * 32 + (a & 31)) as usize];
        let (a, b, c, d) = (l(xi, yi), l(xi + 1, yi), l(xi, yi + 1), l(xi + 1, yi + 1));
        let u = fx * fx * (3.0 - 2.0 * fx);
        let w = fy * fy * (3.0 - 2.0 * fy);
        a + (b - a) * u + (c - a) * w + (a - b - c + d) * u * w
    }

    /// Two octaves (`f`, `2f`) normalised to [0, 1).
    pub fn fbm(&self, x: f64, y: f64) -> f64 {
        (self.at(x, y) * 2.0 + self.at(x * 2.0 + 17.3, y * 2.0 + 9.1)) / 3.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morphology() {
        let mut m = Mask::new(10, 8);
        for y in 1..6 {
            for x in 1..7 {
                m.set(x, y, true);
            }
        }
        m.set(7, 3, true); // 1-cell spur
        m.set(3, 3, false); // 1-cell hole
        let o = m.open(3);
        assert!(!o.get(7, 3));
        let c = m.close(3);
        assert!(c.get(3, 3));
        assert!(c.get(1, 1) && c.get(6, 5));
        let e = m.erode(1, 1, 1, 1);
        assert!(e.get(5, 2) && !e.get(1, 1) && !e.get(2, 2));
    }

    #[test]
    fn distances_and_sat() {
        let mut src = vec![0u8; 25];
        src[12] = 1;
        let d = cheb_dist(&src, 5, 5, 200);
        assert_eq!(d[0], 2);
        assert_eq!(d[7], 1);
        let m = manh_dist(&src, 5, 5, 200);
        assert_eq!(m[0], 4);
        let s = Sat::new(&src, 5, 5);
        assert_eq!(s.sum(0, 0, 4, 4), 1);
        assert_eq!(s.sum(0, 0, 1, 1), 0);
        assert_eq!(s.sum(2, 2, 2, 2), 1);
    }
}
