//! Headless AI benchmark: a 5-AI match (demo setup, every kingdom AI-driven) for N game minutes, printing
//! per-minute per-kingdom stats and the simulation speed.
//!
//! cargo run --release -p ts-sim --example simbench -- [--minutes 20] [--level easy|normal|hard|all] [--seed 1]
//!                                                     [--quiet] [--profile]
//!
//! Pawn jobs are printed as `[w g m s b i o]`: gathering wood / gold / meat / stone, building or repairing, idle,
//! other (shelter, drop, walking...). `--profile` drives the AI thinks from here (same cadence, right after each
//! step instead of at its start) to measure how much of the run time the AI itself takes.

use std::time::{Duration, Instant};
use ts_sim::World;
use ts_sim::defs::*;
use ts_sim::world::{MatchSetup, Order};

struct Args {
    minutes: u32,
    levels: Vec<AiLevel>,
    seed: u32,
    quiet: bool,
    profile: bool,
}

fn parse() -> Args {
    let mut a = Args {
        minutes: 20,
        levels: vec![AiLevel::Easy, AiLevel::Normal, AiLevel::Hard],
        seed: 1,
        quiet: false,
        profile: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        match k.as_str() {
            "--minutes" => a.minutes = it.next().and_then(|v| v.parse().ok()).unwrap_or(a.minutes),
            "--seed" => a.seed = it.next().and_then(|v| v.parse().ok()).unwrap_or(a.seed),
            "--quiet" => a.quiet = true,
            "--profile" => a.profile = true,
            "--level" => {
                a.levels = match it.next().as_deref() {
                    Some("easy") => vec![AiLevel::Easy],
                    Some("normal") => vec![AiLevel::Normal],
                    Some("hard") => vec![AiLevel::Hard],
                    _ => vec![AiLevel::Easy, AiLevel::Normal, AiLevel::Hard],
                }
            }
            _ => eprintln!("unknown argument {k}"),
        }
    }
    a
}

fn buildings_of(w: &World, f: FactionIdx) -> (u32, u32) {
    let (mut done, mut sites) = (0, 0);
    for b in w.buildings.values() {
        if b.faction == f && !b.dead {
            if b.done {
                done += 1
            } else {
                sites += 1
            }
        }
    }
    (done, sites)
}

/// Pawn jobs: gather wood / gold / meat / stone, build or repair, idle, other.
fn jobs_of(w: &World, f: FactionIdx) -> [u32; 7] {
    let mut j = [0u32; 7];
    for id in &w.factions[f as usize].pawns {
        let Some(u) = w.units.get(*id) else {
            continue;
        };
        if u.dead {
            continue;
        }
        let k = match &u.order {
            Order::Gather(g) => g.res as usize,
            Order::Build { .. } | Order::Repair { .. } => 4,
            Order::Idle { .. } => 5,
            _ => 6,
        };
        j[k] += 1;
    }
    j
}

fn print_minute(w: &World, minute: u32) {
    println!("-- minute {minute:>3} (t = {:.0} s) units {} buildings {}", w.time, w.units.len(), w.buildings.len());
    for f in &w.factions {
        let (done, sites) = buildings_of(w, f.idx);
        let r = f.res.floored();
        let j = jobs_of(w, f.idx);
        let wave = match &f.ai.wave {
            Some(wv) => format!("wave {}→{} ({:?})", wv.ids.len(), wv.target, wv.phase),
            None => format!("next wave {:.0}s (need {})", f.ai.wave_t, f.ai.wave_need),
        };
        println!(
            "   {:<14} {} age {} step {:>2} | res w{:>5} g{:>5} m{:>5} s{:>5} | pawns {:>2}/{:<2} [{} {} {} {} {} {} {}] troops {:>2}/{:<2} | bld {:>2}+{} | kills {:>3} razed {:>2} | {}",
            f.name(),
            if f.alive { " " } else { "X" },
            f.age,
            f.ai.step,
            r[0],
            r[1],
            r[2],
            r[3],
            f.pawn_n,
            f.pawn_cap,
            j[0],
            j[1],
            j[2],
            j[3],
            j[4],
            j[5],
            j[6],
            f.troop_n,
            f.troop_cap,
            done,
            sites,
            f.kills,
            f.razed,
            wave
        );
    }
}

fn run(level: AiLevel, a: &Args) {
    let setup = MatchSetup {
        players: Vec::new(),
        level,
        solo: true,
    };
    let t0 = Instant::now();
    let mut w = World::new(a.seed, &setup);
    let mapgen_t = t0.elapsed();
    println!("== level {} seed {} (mapgen {:.2?})", level.label(), a.seed, mapgen_t);
    let ticks_per_min = (60.0 / ts_sim::TICK_DT).round() as u64;
    let start = Instant::now();
    let mut waves = 0u32;
    let mut was_wave = vec![false; w.factions.len()];
    // --profile: think countdowns kept here (the AI state's own countdown is parked so it never fires)
    let mut think: Vec<f32> = w.factions.iter().map(|f| f.ai.think_t).collect();
    let mut ai_time = Duration::ZERO;
    for m in 1..=a.minutes {
        let tm = Instant::now();
        for _ in 0..ticks_per_min {
            if a.profile {
                for f in w.factions.iter_mut() {
                    f.ai.think_t = f32::MAX;
                }
            }
            w.step(&[]);
            if a.profile {
                let t = Instant::now();
                for i in 0..w.factions.len() {
                    think[i] -= ts_sim::TICK_DT;
                    if think[i] <= 0.0 {
                        think[i] = w.factions[i].ai.level.def().think;
                        if w.factions[i].alive && !w.factions[i].human {
                            w.ai_tick(i as FactionIdx);
                        }
                    }
                }
                ai_time += t.elapsed();
            }
            w.drain_events();
            for f in &w.factions {
                let on = f.ai.wave.is_some();
                if on && !was_wave[f.idx as usize] {
                    waves += 1;
                }
                was_wave[f.idx as usize] = on;
            }
        }
        if !a.quiet {
            print_minute(&w, m);
            println!(
                "   ({:.0} ticks/s this minute, waves so far {waves})",
                ticks_per_min as f64 / tm.elapsed().as_secs_f64()
            );
        }
        if w.outcome.is_some() {
            println!("   match over: {:?}", w.outcome);
            break;
        }
    }
    let el = start.elapsed().as_secs_f64();
    if a.quiet {
        print_minute(&w, (w.time / 60.0) as u32);
    }
    println!(
        "== {}: {} ticks in {:.2} s → {:.0} ticks/s ({:.0}x realtime), waves {waves}",
        level.label(),
        w.tick,
        el,
        w.tick as f64 / el,
        w.time as f64 / el
    );
    if a.profile {
        println!(
            "   AI thinks: {:.3} s of {:.2} s ({:.1}%)",
            ai_time.as_secs_f64(),
            el,
            100.0 * ai_time.as_secs_f64() / el
        );
    }
    println!();
}

fn main() {
    let a = parse();
    for &l in &a.levels {
        run(l, &a);
    }
}
