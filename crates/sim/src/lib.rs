//! `ts_sim` — deterministic simulation of Tiny Swords — La Guerra dei Cinque Regni.
//!
//! Pure crate (deps: serde, postcard). Fixed 20 Hz tick, sim units = JS pixels (`TILE = 64.0`), seeded RNG,
//! map generation, AI, fog, I/O-free room (lobby / sessions / AI takeover) and the network protocol.
//! See docs/ARCHITECTURE.md and docs/IMPLEMENTATION-PLAN.md (ownership map below), specs in docs/spec/.
//!
//! Module ownership (IMPLEMENTATION-PLAN.md):
//! * foundation: `rng`, `math`, `slab`, `grid`, `defs`, `world` (shared), `events`, `cmd`, `proto`,
//!   `snapshot_types`
//! * S1 map: `map`, `mapgen`, `path`
//! * S2 units: `unit`, `combat`, `formation`, `projectile`
//! * S3 economy: `gather`, `building`, `point`, `sheep`
//! * S4 faction: `faction`, `command`, `fog`
//! * S5 ai: `ai`
//! * S6 net: `snapshot`, `room`
//!
//! Gameplay functions are `impl World` blocks spread over the owner modules (e.g. `World::hurt` in combat.rs,
//! `World::census` in faction.rs), so any module can call any other through `&mut World` with entity ids.

pub mod defs;
pub mod events;
pub mod grid;
pub mod math;
pub mod rng;
pub mod slab;
pub mod world;

pub mod map;
pub mod mapgen;
pub mod path;

pub mod combat;
pub mod formation;
pub mod projectile;
pub mod unit;

pub mod building;
pub mod gather;
pub mod point;
pub mod sheep;

pub mod command;
pub mod faction;
pub mod fog;

pub mod ai;

pub mod cmd;
pub mod proto;
pub mod room;
pub mod snapshot;
pub mod snapshot_types;

pub use world::World;

/// Pixels per map cell (all spec constants are in these px units).
pub const TILE: f32 = 64.0;
/// Map width in cells.
pub const MAP_W: usize = 288;
/// Map height in cells.
pub const MAP_H: usize = 192;
/// Simulation ticks per second.
pub const TICK_HZ: u32 = 20;
/// Fixed tick length in seconds.
pub const TICK_DT: f32 = 0.05;
/// Network protocol version (bump on any change of `proto` / `cmd` / `snapshot_types` / `events` / `defs` enums).
pub const PROTOCOL_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{MatchSetup, SetupPlayer};

    #[test]
    fn world_starts_and_steps() {
        let setup = MatchSetup {
            players: vec![SetupPlayer {
                color: defs::KingdomColor::Blue,
                name: "Ada".into(),
            }],
            level: defs::AiLevel::Hard,
            solo: true,
        };
        let mut w = World::new(7, &setup);
        assert_eq!(w.factions.len(), 5);
        assert_eq!(w.units.len(), 25, "4 pawns + 1 warrior per kingdom");
        assert_eq!(w.buildings.len(), 5);
        assert!(w.factions.iter().filter(|f| f.human).count() == 1);
        for f in &w.factions {
            assert_eq!(f.pawn_cap, 10);
            assert_eq!(f.troop_cap, 6);
            assert_eq!(f.pawn_n, 4);
        }
        for _ in 0..200 {
            w.step(&[]);
        }
        assert_eq!(w.tick, 200);
        assert!((w.time - 10.0).abs() < 1e-3);
        // a simple move order reaches its target
        let blue = w.factions.iter().find(|f| f.human).unwrap().idx;
        let warrior = w
            .units
            .iter()
            .find(|(_, u)| u.faction == blue && u.kind == defs::UnitKind::Warrior)
            .map(|(id, _)| id)
            .unwrap();
        let start = w.units.get(warrior).unwrap().pos;
        let goal = w.map.nearest_passable(start + math::v2(0.0, 192.0));
        let cmd = cmd::Command::Order {
            units: vec![warrior],
            kind: cmd::OrderKind::Move,
            x: goal.x,
            y: goal.y,
            target: None,
        };
        assert_eq!(w.step(&[(blue, cmd)]), vec![Ok(())]);
        for _ in 0..100 {
            w.step(&[]);
        }
        assert!(w.units.get(warrior).unwrap().pos.dist(goal) < 12.0);
    }
}
