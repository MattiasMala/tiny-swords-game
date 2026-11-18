//! Seeded deterministic RNGs. OWNER: foundation.
//!
//! * [`Rng`] = PCG32 (XSH-RR, 64-bit state): the per-match simulation RNG (`World.rng`). EVERY sim draw goes through
//!   it (spec 05 addenda D1 lists the call sites); presentation randomness (tree phase, gold shine, dust timers,
//!   fire seeds, animT) lives on the client (spec 03a addenda U6).
//! * [`Mulberry32`] = the exact port of `mulberry32` (`assets.js:604-611`, spec 01 §7), available to the map
//!   generator (spec 02 §5.1).
//!
//! Both are `serde`-serializable so a `World` can be snapshotted/restored exactly.

use serde::{Deserialize, Serialize};

const PCG_MULT: u64 = 6364136223846793005;

/// PCG32 random generator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    state: u64,
    inc: u64,
}

impl Rng {
    /// Seeds with the default stream.
    pub fn new(seed: u64) -> Rng {
        Rng::with_stream(seed, 0xda3e_39cb_94b9_5bdb)
    }

    /// Seeds with an explicit stream id (two different streams never correlate).
    pub fn with_stream(seed: u64, stream: u64) -> Rng {
        let mut r = Rng {
            state: 0,
            inc: (stream << 1) | 1,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    /// Independent child generator (consumes two draws).
    pub fn fork(&mut self) -> Rng {
        let seed = ((self.next_u32() as u64) << 32) | self.next_u32() as u64;
        let stream = self.next_u32() as u64;
        Rng::with_stream(seed, stream)
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `[0, 1)` (JS `Math.random()`).
    #[inline]
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform in `[0, 1)` with 32 bits of precision.
    #[inline]
    pub fn f64(&mut self) -> f64 {
        self.next_u32() as f64 / 4_294_967_296.0
    }

    /// `rand(a, b) = a + random()*(b-a)` (`assets.js:600`), in `[a, b)`.
    #[inline]
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + self.f32() * (b - a)
    }

    /// Uniform integer in `[0, n)`; returns 0 when `n == 0`.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next_u32() as u64 * n as u64) >> 32) as u32
    }

    /// `randInt(a, b)` (`assets.js:601`): uniform integer, both ends inclusive. Returns `a` if `b < a`.
    #[inline]
    pub fn int_incl(&mut self, a: i32, b: i32) -> i32 {
        if b <= a {
            return a;
        }
        let span = (b as i64 - a as i64 + 1) as u64;
        (a as i64 + ((self.next_u32() as u64 * span) >> 32) as i64) as i32
    }

    /// True with probability `p`.
    #[inline]
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    /// `pick(arr)` (`assets.js:602`); `None` for an empty slice.
    #[inline]
    pub fn pick<'a, T>(&mut self, s: &'a [T]) -> Option<&'a T> {
        if s.is_empty() {
            None
        } else {
            Some(&s[self.below(s.len() as u32) as usize])
        }
    }

    /// Index for `pick`, `None` for len 0.
    #[inline]
    pub fn pick_index(&mut self, len: usize) -> Option<usize> {
        if len == 0 {
            None
        } else {
            Some(self.below(len as u32) as usize)
        }
    }

    /// Fisher–Yates shuffle (replaces every JS `sort(() => Math.random() - 0.5)`, spec 04a addenda E8).
    pub fn shuffle<T>(&mut self, s: &mut [T]) {
        for i in (1..s.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            s.swap(i, j);
        }
    }

    /// Weighted draw: index `i` with probability `w[i] / sum(w)`; `None` if the sum is not positive.
    pub fn weighted(&mut self, w: &[f32]) -> Option<usize> {
        let total: f32 = w.iter().filter(|v| **v > 0.0).sum();
        if total <= 0.0 {
            return None;
        }
        let mut r = self.f32() * total;
        let mut last = None;
        for (i, v) in w.iter().enumerate() {
            if *v <= 0.0 {
                continue;
            }
            last = Some(i);
            if r < *v {
                return Some(i);
            }
            r -= *v;
        }
        last
    }
}

/// Exact port of the JS `mulberry32` (spec 01 §7) — u32 wrapping arithmetic, `>>>` = logical shift.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mulberry32 {
    a: u32,
}

impl Mulberry32 {
    pub fn new(seed: u32) -> Mulberry32 {
        Mulberry32 { a: seed }
    }

    /// Next value in `[0, 1)` as `f64` (identical to the JS function).
    pub fn next_f64(&mut self) -> f64 {
        self.a = self.a.wrapping_add(0x6D2B_79F5);
        let a = self.a;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }

    #[inline]
    pub fn f32(&mut self) -> f32 {
        self.next_f64() as f32
    }

    #[inline]
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + self.f32() * (b - a)
    }

    /// `floor(rng * n)` as used by world.js.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next_f64() * n as f64) as u32).min(n.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcg_deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
        let mut c = Rng::new(43);
        let same = (0..16).filter(|_| a.next_u32() == c.next_u32()).count();
        assert!(same < 4);
    }

    #[test]
    fn pcg_ranges() {
        let mut r = Rng::new(7);
        for _ in 0..10_000 {
            let f = r.f32();
            assert!((0.0..1.0).contains(&f));
            let x = r.range(-3.0, 5.0);
            assert!((-3.0..5.0).contains(&x));
            let i = r.int_incl(2, 4);
            assert!((2..=4).contains(&i));
            assert!(r.below(5) < 5);
        }
        assert_eq!(r.int_incl(3, 3), 3);
        assert_eq!(r.below(0), 0);
        // every value of int_incl reachable
        let mut seen = [false; 3];
        for _ in 0..1000 {
            seen[(r.int_incl(0, 2)) as usize] = true;
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn pcg_pick_chance_shuffle() {
        let mut r = Rng::new(1);
        let empty: [u8; 0] = [];
        assert!(r.pick(&empty).is_none());
        assert_eq!(r.pick(&[9]), Some(&9));
        assert!(!r.chance(0.0));
        assert!(r.chance(1.0));
        let mut v: Vec<u32> = (0..50).collect();
        r.shuffle(&mut v);
        let mut s = v.clone();
        s.sort();
        assert_eq!(s, (0..50).collect::<Vec<_>>());
        assert_ne!(v, s);
        assert_eq!(r.weighted(&[0.0, 0.0]), None);
        assert_eq!(r.weighted(&[0.0, 2.0]), Some(1));
    }

    #[test]
    fn pcg_serde_roundtrip() {
        let mut r = Rng::new(99);
        r.next_u32();
        let bytes = postcard::to_allocvec(&r).unwrap();
        let mut back: Rng = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(r.next_u32(), back.next_u32());
    }

    #[test]
    fn mulberry_matches_js() {
        // Reference values computed with the original JS function (node): mulberry32(12345).
        let mut m = Mulberry32::new(12345);
        assert_eq!(m.next_f64(), 0.9797282677609473);
        assert_eq!(m.next_f64(), 0.3067522644996643);
        assert_eq!(m.next_f64(), 0.484205421525985);
    }
}
