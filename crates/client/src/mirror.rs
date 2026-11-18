//! Client world view. OWNER: foundation (C4 may extend the interpolation).
//!
//! `Mirror` holds the `ClientMirror` kept up to date by snapshots (`ClientMirror::apply`, S6), the map regenerated
//! locally from the seed on `Init` (`mapgen::generate(seed, setup)`; the map never travels, 08-net R5), and a
//! two-snapshot interpolation buffer for unit positions (render one tick behind, `unit_pos_interp`).
//!
//! Renderers watch `Mirror::generation` (bumped on every `Init`) to rebuild static scenery, and read `view` for
//! the dynamic state.

use bevy::prelude::*;
use std::collections::BTreeMap;
use ts_sim::defs::{FactionIdx, KingdomColor, NodeKind, TREE_WOOD};
use ts_sim::map::{GeneratedWorld, Map};
use ts_sim::mapgen;
use ts_sim::slab::UnitId;
use ts_sim::snapshot_types::{ClientMirror, Snapshot};
use ts_sim::world::MatchSetup;

#[derive(Resource, Default)]
pub struct Mirror {
    /// a match is loaded (Init received)
    pub active: bool,
    /// bumped on every Init (renderers rebuild the static map when it changes)
    pub generation: u32,
    pub seed: u32,
    pub setup: MatchSetup,
    /// colour of faction `i` (base order)
    pub colors: Vec<KingdomColor>,
    /// own faction (`None` = spectator)
    pub you: Option<FactionIdx>,
    pub rejoin: bool,
    /// regenerated world (map, scenery lists)
    pub world: Option<GeneratedWorld>,
    /// `Map::hash` matched the server's
    pub hash_ok: bool,
    /// dynamic state from snapshots
    pub view: ClientMirror,
    /// `Some(winner)` once `MatchOver` arrived
    pub winner: Option<Option<FactionIdx>>,
    /// unit positions of the previous snapshot (px)
    pub prev_units: BTreeMap<UnitId, Vec2>,
    /// real time (s) the previous / current snapshot arrived
    pub prev_t: f64,
    pub cur_t: f64,
    /// snapshots received since Init
    pub snapshots: u64,
}

impl Mirror {
    /// `ServerMsg::Init`: regenerate the map and reset the view.
    pub fn init(
        &mut self,
        seed: u32,
        setup: MatchSetup,
        colors: Vec<KingdomColor>,
        you: Option<FactionIdx>,
        rejoin: bool,
        map_hash: u64,
    ) {
        let g = mapgen::generate(seed, &setup);
        self.hash_ok = g.map.hash() == map_hash;
        if !self.hash_ok {
            error!(
                "mappa diversa dal server (seme {seed}): hash locale {:x} != {map_hash:x}",
                g.map.hash()
            );
        }
        let amounts = initial_node_amounts(&g);
        self.active = true;
        self.generation = self.generation.wrapping_add(1);
        self.seed = seed;
        self.setup = setup;
        self.colors = colors;
        self.you = you;
        self.rejoin = rejoin;
        self.view = ClientMirror::new(you, amounts);
        self.world = Some(g);
        self.winner = None;
        self.prev_units.clear();
        self.prev_t = 0.0;
        self.cur_t = 0.0;
        self.snapshots = 0;
    }

    /// Back to "no match" (menu).
    pub fn clear(&mut self) {
        let generation = self.generation.wrapping_add(1);
        *self = Mirror {
            generation,
            ..default()
        };
    }

    /// Applies a snapshot received at real time `now` (s).
    pub fn apply(&mut self, s: &Snapshot, now: f64) {
        self.prev_units = self
            .view
            .units
            .iter()
            .map(|(id, u)| (*id, Vec2::new(u.x, u.y)))
            .collect();
        self.prev_t = if self.snapshots == 0 { now } else { self.cur_t };
        self.cur_t = now;
        self.view.apply(s);
        self.snapshots += 1;
    }

    pub fn map(&self) -> Option<&Map> {
        self.world.as_ref().map(|g| &g.map)
    }

    /// Interpolated unit position in sim px at real time `now`: lerp from the previous to the current snapshot
    /// over one tick (the view is drawn ~one tick behind the sim). New units snap to their current position.
    pub fn unit_pos_interp(&self, id: UnitId, now: f64) -> Option<Vec2> {
        let u = self.view.units.get(&id)?;
        let cur = Vec2::new(u.x, u.y);
        let Some(prev) = self.prev_units.get(&id) else {
            return Some(cur);
        };
        let span = (self.cur_t - self.prev_t).max(ts_sim::TICK_DT as f64);
        let a = ((now - self.cur_t) / span).clamp(0.0, 1.0) as f32;
        // teleports (shelter exit, respawn) do not glide
        if prev.distance_squared(cur) > 128.0 * 128.0 {
            return Some(cur);
        }
        Some(prev.lerp(cur, a))
    }
}

/// Node amounts in `NodeId` order (spec 02 §8): trees, golds small+big, stones small+big (as `World::new`).
pub fn initial_node_amounts(g: &GeneratedWorld) -> Vec<f32> {
    let mut v: Vec<f32> = g.trees.iter().map(|_| TREE_WOOD).collect();
    for (kind, small) in [(NodeKind::Gold, &g.golds), (NodeKind::Stone, &g.stones)] {
        v.extend(small.iter().map(|d| d.amount));
        v.extend(
            g.bigs
                .iter()
                .filter(|(k, _)| *k == kind)
                .map(|(_, d)| d.amount),
        );
    }
    v
}

pub struct MirrorPlugin;

impl Plugin for MirrorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mirror>();
    }
}
