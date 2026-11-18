// OWNER: S4 (faction) — see docs/spec/05-control-loop.md §12 (fog: explored + visible, what is gated), 04a §11.1
// (`explore(f)`: START_SIGHT disc, unit/building/point discs `(i-cx)²+(j-cy)² <= r²+r`, `seen_key` change cache,
// building sight centre `(b.x, b.y-30)`, sites 2 cells, point centre `(p.x, p.y-64)`, + mods.sight),
// 04b addenda A6 (explore keeps running for human-OWNED factions while AI-driven; `explored()` tests `human`),
// 05 addenda C3 (enemy building ghosts are client-side), ARCHITECTURE.md (visible sets at 5 Hz).
//
// Stub fns S4 must fill: explore (STUB), update_fog (STUB: visible = explored is not maintained yet).
// Done (verify): is_visible, explored, explored_cell. Snapshot deltas of `explored` are computed by the snapshot
// builder (diff against what it already sent), nothing to record here.
#![allow(dead_code, unused)]

use crate::World;
use crate::defs::*;
use crate::math::V2;

impl World {
    /// `explore(f)` (04a §11.1): marks explored cells in `fog.explored`. STUB.
    pub fn explore(&mut self, f: FactionIdx) {}

    /// Recomputes `fog.visible` for every owned faction every `VISIBLE_PERIOD` (0.2 s) from own units, buildings
    /// and points (same discs as `explore`). Called once per tick at the end of `World::step`. STUB.
    pub fn update_fog(&mut self) {}

    /// Is the point currently visible to faction `f`? (fog off in test mode / match over → true).
    pub fn is_visible(&self, f: FactionIdx, p: V2) -> bool {
        let fac = &self.factions[f as usize];
        if fac.fog_off || self.outcome.is_some() {
            return true;
        }
        let c = self.map.cell_of(p);
        c >= 0 && fac.fog.visible.get(c as usize)
    }

    /// `explored(f, x, y)`: always true for AI-driven factions (no fog); else the explored bit (false off-map).
    pub fn explored(&self, f: FactionIdx, p: V2) -> bool {
        let fac = &self.factions[f as usize];
        if !fac.human {
            return true;
        }
        let c = self.map.cell_of(p);
        c >= 0 && fac.fog.explored.get(c as usize)
    }

    /// Explored bit of a cell `(cx, cy)` (ignores the AI shortcut; used by snapshots/UI).
    pub fn explored_cell(&self, f: FactionIdx, cx: i32, cy: i32) -> bool {
        self.map.inside(cx, cy)
            && self.factions[f as usize]
                .fog
                .explored
                .get(self.map.idx(cx, cy) as usize)
    }
}
