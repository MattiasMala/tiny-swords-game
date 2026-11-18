//! Generational slab with typed ids. OWNER: foundation.
//!
//! * O(1) insert / remove / get. Freed slots are reused (LIFO) with a bumped generation, so a stale id never
//!   resolves to a newer entity (replaces JS `G.index` + `dead` checks, spec 03a §10.4).
//! * Iteration is **deterministic, in slot-index order** (not hash order). NOTE: because slots are reused this is
//!   not strictly creation order (spec 05 addenda D3 asks for "creation order"); it is deterministic on server and
//!   client, which is what matters for the authoritative sim. Never iterate a `HashMap` in gameplay code.
//! * To mutate the world while walking a slab, take the id list first: `for id in world.units.ids() { ... }`.
//!
//! Typed ids: [`UnitId`], [`BuildingId`], [`SheepId`], [`ProjId`], [`PickupId`] (slab-backed, generational) and
//! [`NodeId`], [`PointId`] (plain indices into `Vec`s: resource nodes and capture points are never removed, and the
//! client regenerates them from the seed in the same order — spec 02 §8 / 03b §2.1).

use core::marker::PhantomData;
use serde::{Deserialize, Serialize};

/// Raw slab key: slot index + generation.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Key {
    pub index: u32,
    pub generation: u32,
}

/// Implemented by every slab-backed typed id.
pub trait SlabId: Copy + Eq {
    fn from_key(k: Key) -> Self;
    fn key(self) -> Key;
}

macro_rules! typed_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub Key);
        impl SlabId for $name {
            #[inline]
            fn from_key(k: Key) -> Self {
                $name(k)
            }
            #[inline]
            fn key(self) -> Key {
                self.0
            }
        }
    };
}

typed_id!(
    /// A unit (`Unit`, spec 03a §4).
    UnitId
);
typed_id!(
    /// A building or construction site (`Building`, spec 03b §8).
    BuildingId
);
typed_id!(
    /// A sheep (wild or farm-bred, spec 03b §2.1).
    SheepId
);
typed_id!(
    /// An arrow or catapult rock (spec 03a §14).
    ProjId
);
typed_id!(
    /// A pickup: loot or map chest (spec 03b §2.4).
    PickupId
);

/// Index into `World.nodes` (trees, then golds small then big, then stones small then big — spec 02 §8).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct NodeId(pub u32);

/// Index into `World.points` (capture points, in generation order).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct PointId(pub u32);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
enum Entry<T> {
    Occupied { generation: u32, value: T },
    Vacant { generation: u32 },
}

/// Generational slab keyed by the typed id `I`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
pub struct Slab<I, T> {
    entries: Vec<Entry<T>>,
    free: Vec<u32>,
    len: usize,
    #[serde(skip)]
    _id: PhantomData<fn() -> I>,
}

impl<I: SlabId, T> Default for Slab<I, T> {
    fn default() -> Self {
        Slab::new()
    }
}

impl<I: SlabId, T> Slab<I, T> {
    pub fn new() -> Self {
        Slab {
            entries: Vec::new(),
            free: Vec::new(),
            len: 0,
            _id: PhantomData,
        }
    }

    pub fn with_capacity(n: usize) -> Self {
        Slab {
            entries: Vec::with_capacity(n),
            free: Vec::new(),
            len: 0,
            _id: PhantomData,
        }
    }

    /// Number of live entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Inserts and returns the new id (reuses the most recently freed slot).
    pub fn insert(&mut self, value: T) -> I {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let e = &mut self.entries[index as usize];
            let generation = match e {
                Entry::Vacant { generation } => *generation,
                Entry::Occupied { generation, .. } => *generation,
            };
            *e = Entry::Occupied { generation, value };
            I::from_key(Key { index, generation })
        } else {
            let index = self.entries.len() as u32;
            self.entries.push(Entry::Occupied {
                generation: 0,
                value,
            });
            I::from_key(Key {
                index,
                generation: 0,
            })
        }
    }

    /// Removes and returns the entity; `None` for a stale/unknown id.
    pub fn remove(&mut self, id: I) -> Option<T> {
        let k = id.key();
        let e = self.entries.get_mut(k.index as usize)?;
        match e {
            Entry::Occupied { generation, .. } if *generation == k.generation => {
                let next = Entry::Vacant {
                    generation: k.generation.wrapping_add(1),
                };
                let old = core::mem::replace(e, next);
                self.free.push(k.index);
                self.len -= 1;
                match old {
                    Entry::Occupied { value, .. } => Some(value),
                    Entry::Vacant { .. } => None,
                }
            }
            _ => None,
        }
    }

    #[inline]
    pub fn get(&self, id: I) -> Option<&T> {
        let k = id.key();
        match self.entries.get(k.index as usize)? {
            Entry::Occupied { generation, value } if *generation == k.generation => Some(value),
            _ => None,
        }
    }

    #[inline]
    pub fn get_mut(&mut self, id: I) -> Option<&mut T> {
        let k = id.key();
        match self.entries.get_mut(k.index as usize)? {
            Entry::Occupied { generation, value } if *generation == k.generation => Some(value),
            _ => None,
        }
    }

    #[inline]
    pub fn contains(&self, id: I) -> bool {
        self.get(id).is_some()
    }

    /// Two distinct entries mutably at once; `None` if either is missing or `a == b`.
    pub fn get2_mut(&mut self, a: I, b: I) -> Option<(&mut T, &mut T)> {
        let (ka, kb) = (a.key(), b.key());
        if ka.index == kb.index || !self.contains(a) || !self.contains(b) {
            return None;
        }
        let (lo, hi, swap) = if ka.index < kb.index {
            (ka.index, kb.index, false)
        } else {
            (kb.index, ka.index, true)
        };
        let (left, right) = self.entries.split_at_mut(hi as usize);
        let (Entry::Occupied { value: va, .. }, Entry::Occupied { value: vb, .. }) =
            (&mut left[lo as usize], &mut right[0])
        else {
            return None;
        };
        if swap { Some((vb, va)) } else { Some((va, vb)) }
    }

    /// Live `(id, &T)` in slot-index order.
    pub fn iter(&self) -> impl Iterator<Item = (I, &T)> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                Entry::Occupied { generation, value } => Some((
                    I::from_key(Key {
                        index: i as u32,
                        generation: *generation,
                    }),
                    value,
                )),
                Entry::Vacant { .. } => None,
            })
    }

    /// Live `(id, &mut T)` in slot-index order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (I, &mut T)> + '_ {
        self.entries
            .iter_mut()
            .enumerate()
            .filter_map(|(i, e)| match e {
                Entry::Occupied { generation, value } => Some((
                    I::from_key(Key {
                        index: i as u32,
                        generation: *generation,
                    }),
                    value,
                )),
                Entry::Vacant { .. } => None,
            })
    }

    /// Live values in slot-index order.
    pub fn values(&self) -> impl Iterator<Item = &T> + '_ {
        self.iter().map(|(_, v)| v)
    }

    /// Snapshot of the live ids (index order) — iterate this while mutating the world.
    pub fn ids(&self) -> Vec<I> {
        self.iter().map(|(id, _)| id).collect()
    }

    /// Removes every entry for which `keep` returns false (index order).
    pub fn retain(&mut self, mut keep: impl FnMut(I, &T) -> bool) {
        let dead: Vec<I> = self
            .iter()
            .filter(|(id, v)| !keep(*id, v))
            .map(|(id, _)| id)
            .collect();
        for id in dead {
            self.remove(id);
        }
    }

    pub fn clear(&mut self) {
        let ids = self.ids();
        for id in ids {
            self.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_remove() {
        let mut s: Slab<UnitId, &str> = Slab::new();
        let a = s.insert("a");
        let b = s.insert("b");
        assert_eq!(s.len(), 2);
        assert_eq!(s.get(a), Some(&"a"));
        assert_eq!(s.remove(a), Some("a"));
        assert_eq!(s.get(a), None);
        assert_eq!(s.remove(a), None);
        assert_eq!(s.len(), 1);
        // slot reuse bumps the generation: the stale id stays dead
        let c = s.insert("c");
        assert_eq!(c.0.index, a.0.index);
        assert_ne!(c, a);
        assert_eq!(s.get(a), None);
        assert_eq!(s.get(c), Some(&"c"));
        assert_eq!(s.get(b), Some(&"b"));
    }

    #[test]
    fn iteration_is_index_order() {
        let mut s: Slab<BuildingId, u32> = Slab::new();
        let ids: Vec<_> = (0..10).map(|i| s.insert(i)).collect();
        s.remove(ids[3]);
        s.remove(ids[7]);
        let vals: Vec<u32> = s.values().copied().collect();
        assert_eq!(vals, vec![0, 1, 2, 4, 5, 6, 8, 9]);
        let n = s.insert(100); // reuses slot 7 (LIFO)
        assert_eq!(n.0.index, 7);
        let vals: Vec<u32> = s.values().copied().collect();
        assert_eq!(vals, vec![0, 1, 2, 4, 5, 6, 100, 8, 9]);
        assert_eq!(s.ids().len(), 9);
    }

    #[test]
    fn get2_and_retain() {
        let mut s: Slab<UnitId, i32> = Slab::new();
        let a = s.insert(1);
        let b = s.insert(2);
        {
            let (x, y) = s.get2_mut(b, a).unwrap();
            assert_eq!((*x, *y), (2, 1));
            *x += 10;
        }
        assert_eq!(s.get(b), Some(&12));
        assert!(s.get2_mut(a, a).is_none());
        s.retain(|_, v| *v > 5);
        assert_eq!(s.len(), 1);
        assert!(s.get(a).is_none());
        for (_, v) in s.iter_mut() {
            *v = 0;
        }
        assert_eq!(s.get(b), Some(&0));
    }

    #[test]
    fn serde_roundtrip() {
        let mut s: Slab<SheepId, u16> = Slab::new();
        let a = s.insert(5);
        let b = s.insert(6);
        s.remove(a);
        let bytes = postcard::to_allocvec(&s).unwrap();
        let mut back: Slab<SheepId, u16> = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back.get(b), Some(&6));
        assert_eq!(back.get(a), None);
        let c = back.insert(7);
        assert_eq!(c.0.index, a.0.index);
        assert_eq!(c.0.generation, a.0.generation + 1);
    }
}
