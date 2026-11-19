//! End-to-end AI check (04b): five AI kingdoms at `Normale` for 25 game minutes.
//! - no panic;
//! - every kingdom reaches the Feudal Age (age 2);
//! - armies get built and at least one attack wave is launched;
//! - in release builds the sim runs faster than 50x realtime.

use std::time::Instant;
use ts_sim::World;
use ts_sim::defs::*;
use ts_sim::world::MatchSetup;

const MINUTES: u32 = 25;

#[test]
fn five_normal_ais_for_25_minutes() {
    let setup = MatchSetup {
        players: Vec::new(),
        level: AiLevel::Normal,
        solo: true,
    };
    let mut w = World::new(3, &setup);
    let n = w.factions.len();
    assert_eq!(n, 5);
    let ticks = (MINUTES as f32 * 60.0 / ts_sim::TICK_DT).round() as u64;
    let mut max_age = vec![1u8; n];
    let mut max_troops = vec![0u16; n];
    let mut waves = 0u32;
    let mut was_wave = vec![false; n];
    let start = Instant::now();
    for _ in 0..ticks {
        w.step(&[]);
        w.drain_events();
        for f in &w.factions {
            let i = f.idx as usize;
            max_age[i] = max_age[i].max(f.age);
            let soldiers = f
                .soldiers
                .iter()
                .filter(|id| w.units.get(**id).is_some_and(|u| !u.dead && ai_fighter(u.kind)))
                .count() as u16;
            max_troops[i] = max_troops[i].max(soldiers);
            let on = f.ai.wave.is_some();
            if on && !was_wave[i] {
                waves += 1;
            }
            was_wave[i] = on;
        }
        if w.outcome.is_some() {
            break;
        }
    }
    let secs = start.elapsed().as_secs_f64();
    let speed = w.time as f64 / secs;
    eprintln!(
        "{} ticks in {secs:.1} s ({speed:.0}x realtime); ages {max_age:?}; max fighters {max_troops:?}; waves {waves}",
        w.tick
    );
    for f in &w.factions {
        eprintln!(
            "  {} alive {} age {} step {} pawns {} troops {} kills {} razed {}",
            f.name(),
            f.alive,
            f.age,
            f.ai.step,
            f.pawn_n,
            f.troop_n,
            f.kills,
            f.razed
        );
    }
    for (i, a) in max_age.iter().enumerate() {
        assert!(*a >= 2, "kingdom {i} never reached the Feudal Age");
    }
    assert!(
        max_troops.iter().all(|t| *t >= 4),
        "every kingdom builds an army: {max_troops:?}"
    );
    assert!(waves >= 1, "at least one attack wave");
    assert!(
        w.factions.iter().any(|f| f.raided_t > 0.0),
        "a kingdom was raided"
    );
    if !cfg!(debug_assertions) {
        assert!(speed > 50.0, "sim too slow: {speed:.0}x realtime");
    }
}
