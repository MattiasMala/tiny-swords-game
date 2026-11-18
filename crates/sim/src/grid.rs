//! Spatial grid for neighbour queries on units. OWNER: foundation.
//!
//! Rebuilt once per tick (in `World::step`, before the unit phase) from live, non-sheltered units; positions move a
//! little during the tick, so callers must always re-check the exact distance on the candidates (spec 03a §7.5 notes
//! the JS separation grid has the same property). Buckets keep units in slab-index order → deterministic.

use crate::math::V2;
use crate::slab::Slab;
use crate::slab::UnitId;
use crate::world::Unit;
use crate::{MAP_H, MAP_W, TILE};

/// Bucket size in px (2 cells). Aggro radii go up to 450 px → at most ~8x8 buckets per query.
pub const GRID_CELL: f32 = 128.0;

#[derive(Clone, Debug)]
pub struct SpatialGrid {
    pub cell: f32,
    pub cols: i32,
    pub rows: i32,
    buckets: Vec<Vec<UnitId>>,
}

impl Default for SpatialGrid {
    fn default() -> Self {
        SpatialGrid::new(GRID_CELL)
    }
}

impl SpatialGrid {
    pub fn new(cell: f32) -> SpatialGrid {
        let cols = ((MAP_W as f32 * TILE) / cell).ceil() as i32;
        let rows = ((MAP_H as f32 * TILE) / cell).ceil() as i32;
        SpatialGrid {
            cell,
            cols,
            rows,
            buckets: vec![Vec::new(); (cols * rows) as usize],
        }
    }

    #[inline]
    fn bucket_of(&self, p: V2) -> (i32, i32) {
        (
            ((p.x / self.cell).floor() as i32).clamp(0, self.cols - 1),
            ((p.y / self.cell).floor() as i32).clamp(0, self.rows - 1),
        )
    }

    /// Re-buckets every live unit that is not dead and not sheltered.
    pub fn rebuild(&mut self, units: &Slab<UnitId, Unit>) {
        for b in &mut self.buckets {
            b.clear();
        }
        for (id, u) in units.iter() {
            if u.dead || u.inside.is_some() {
                continue;
            }
            let (bx, by) = self.bucket_of(u.pos);
            self.buckets[(by * self.cols + bx) as usize].push(id);
        }
    }

    /// Candidate units whose bucket overlaps the square `p ± r` (row-major bucket order, then index order).
    /// Callers filter by exact distance / faction / state.
    pub fn query(&self, p: V2, r: f32, out: &mut Vec<UnitId>) {
        out.clear();
        let (x0, y0) = self.bucket_of(V2::new(p.x - r, p.y - r));
        let (x1, y1) = self.bucket_of(V2::new(p.x + r, p.y + r));
        for by in y0..=y1 {
            for bx in x0..=x1 {
                out.extend_from_slice(&self.buckets[(by * self.cols + bx) as usize]);
            }
        }
    }

    /// Same as [`query`](Self::query) but sorted by slab index (= global unit iteration order), for code that
    /// must reproduce "first unit in `G.units` order wins" semantics.
    pub fn query_sorted(&self, p: V2, r: f32, out: &mut Vec<UnitId>) {
        self.query(p, r, out);
        out.sort_unstable_by_key(|id| id.0.index);
    }
}
