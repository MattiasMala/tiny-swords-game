//! AI takeover / reclaim (04b §12, addenda A1): the AI drives a kingdom only while `human == false`, its state
//! survives hand-backs, and a takeover fast-forwards the plan instead of rebuilding what the kingdom owns.

use ts_sim::World;
use ts_sim::defs::*;
use ts_sim::world::{MatchSetup, SetupPlayer};

fn run(w: &mut World, secs: f32) {
    let n = (secs / ts_sim::TICK_DT).round() as u32;
    for _ in 0..n {
        w.step(&[]);
        w.drain_events();
        if w.outcome.is_some() {
            break;
        }
    }
}

#[test]
fn takeover_and_reclaim() {
    let setup = MatchSetup {
        players: vec![SetupPlayer {
            color: KingdomColor::Blue,
            name: "Ada".into(),
        }],
        level: AiLevel::Normal,
        solo: false,
    };
    let mut w = World::new(5, &setup);
    let me = w.factions.iter().find(|f| f.human).unwrap().idx;
    let frozen = w.factions[me as usize].ai.clone();
    run(&mut w, 120.0);
    // the AI never touches a human kingdom
    assert_eq!(w.factions[me as usize].ai, frozen);
    assert_eq!(w.factions[me as usize].sites.iter().sum::<u16>(), 0);

    // the player leaves: the AI takes over and builds
    w.takeover(me);
    assert!(!w.factions[me as usize].human);
    let ai = &w.factions[me as usize].ai;
    assert!(ai.wave_t >= 160.0 || ai.wave_t >= frozen.wave_t, "no instant wave");
    assert!(ai.capture_t >= w.time + 59.0);
    run(&mut w, 180.0);
    let f = &w.factions[me as usize];
    if f.alive {
        assert!(f.ai.step > 0, "the AI advanced the plan");
        let built: u16 = f.has.iter().sum::<u16>() + f.sites.iter().sum::<u16>();
        assert!(built > 1, "the AI placed buildings");
    }

    // the player comes back: AI state kept, wave forgotten, no more AI thinking
    w.reclaim(me);
    assert!(w.factions[me as usize].human);
    assert!(w.factions[me as usize].ai.wave.is_none());
    let kept = w.factions[me as usize].ai.clone();
    run(&mut w, 30.0);
    assert_eq!(w.factions[me as usize].ai.step, kept.step);
    assert_eq!(w.factions[me as usize].ai.think_t, kept.think_t);

    // second takeover: the plan is fast-forwarded, not restarted
    w.takeover(me);
    let step = w.factions[me as usize].ai.step;
    assert_eq!(step, w.ai_plan_fast_forward(me));
    if w.factions[me as usize].has[BuildingKind::Barracks as usize] > 0 {
        assert!(step >= 2, "barracks already owned → past the barracks step");
    }
}
