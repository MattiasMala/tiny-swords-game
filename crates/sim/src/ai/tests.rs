//! AI unit tests (rules of docs/spec/04b-ai.md that can be checked without a full economy).

use super::intel::{ai_edge, ai_power};
use super::*;
use crate::math::{V2, v2};
use crate::world::{MatchSetup, Order};

fn demo(level: AiLevel) -> World {
    World::new(
        11,
        &MatchSetup {
            players: Vec::new(),
            level,
            solo: true,
        },
    )
}

fn castle(w: &World, f: FactionIdx) -> V2 {
    w.ai_castle(f).expect("castle")
}

/// A passable point `dy` px south of the castle front.
fn near_castle(w: &World, f: FactionIdx, dx: f32, dy: f32) -> V2 {
    let c = w.factions[f as usize].castle.unwrap();
    let b = w.buildings.get(c).unwrap();
    w.map.nearest_passable(v2(b.pos.x + dx, b.rect.y1 + dy))
}

#[test]
fn power_and_edge() {
    let m = Mods::default();
    // same unit, same mods → even
    for t in [UnitKind::Warrior, UnitKind::Lancer, UnitKind::Archer, UnitKind::Knight] {
        assert!(ai_edge(&m, t, &m, t).abs() < 1e-5, "{t:?}");
        assert!(ai_power(&m, t, &m, t) > 0.0);
    }
    // bonuses: knight hunts catapults, lancer hunts knights
    assert!(ai_edge(&m, UnitKind::Knight, &m, UnitKind::Catapult) > 0.0);
    // against non-fighters only the bonus counts
    assert_eq!(ai_edge(&m, UnitKind::Warrior, &m, UnitKind::Ram), 0.0);
    assert_eq!(ai_edge(&m, UnitKind::Warrior, &m, UnitKind::Monk), 0.0);
    // attack upgrades raise the edge
    let mut strong = Mods::default();
    strong.apply_tech(TechId::Blades);
    assert!(ai_edge(&strong, UnitKind::Warrior, &m, UnitKind::Warrior) > 0.0);
    // edges are antisymmetric between fighters
    let a = ai_edge(&m, UnitKind::Archer, &m, UnitKind::Knight);
    let b = ai_edge(&m, UnitKind::Knight, &m, UnitKind::Archer);
    assert!((a + b).abs() < 1e-4);
}

#[test]
fn want_needs_factories_and_sums_to_one() {
    let mut w = demo(AiLevel::Normal);
    let intel = w.ai_intel(0);
    assert_eq!(w.ai_want(0, &intel), None, "no barracks → no mix");
    w.factions[0].has[BuildingKind::Barracks as usize] = 1;
    let mix = w.ai_want(0, &intel).unwrap();
    assert!((mix[UnitKind::Warrior as usize] - 1.0).abs() < 1e-5, "age 1: warriors only");
    w.factions[0].age = 2;
    w.factions[0].has[BuildingKind::Archery as usize] = 1;
    w.factions[0].has[BuildingKind::Stable as usize] = 1;
    let mix = w.ai_want(0, &intel).unwrap();
    let s: f32 = mix.iter().sum();
    assert!((s - 1.0).abs() < 1e-4);
    for t in [UnitKind::Warrior, UnitKind::Lancer, UnitKind::Archer, UnitKind::Knight] {
        assert!(mix[t as usize] >= 0.15 / 4.0 - 1e-6, "{t:?} always gets a floor share");
    }
    assert_eq!(mix[UnitKind::Catapult as usize], 0.0);
    assert_eq!(mix[UnitKind::Monk as usize], 0.0);
}

#[test]
fn job_mix_normalized_and_follows_stock() {
    let mut w = demo(AiLevel::Normal);
    let m = w.ai_job_mix(0);
    assert!((m.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    assert_eq!(m[Res::Stone as usize], 0.0, "age 1, farm step: no stone");
    let wood = m[Res::Wood as usize];
    w.factions[0].res[Res::Wood] = 900.0;
    let m2 = w.ai_job_mix(0);
    assert!(m2[Res::Wood as usize] < wood, "plenty of wood → fewer woodcutters");
}

#[test]
fn plan_fast_forward() {
    let mut w = demo(AiLevel::Normal);
    w.census(0);
    assert_eq!(w.ai_plan_fast_forward(0), 0);
    let f = &mut w.factions[0];
    f.has[BuildingKind::Farm as usize] = 3; // normal cap
    f.has[BuildingKind::Barracks as usize] = 1;
    f.sites[BuildingKind::Depot as usize] = 1;
    f.techs.insert(TechId::Age2);
    f.has[BuildingKind::Archery as usize] = 1;
    f.has[BuildingKind::Smith as usize] = 1;
    f.has[BuildingKind::Stable as usize] = 1;
    // 0 farm .. 8 stable done, 9 blades missing
    assert_eq!(w.ai_plan_fast_forward(0), 9);
    w.factions[0].busy.insert(TechId::Blades);
    // 10 tower skipped (normal: 0 towers), 11 axes missing
    assert_eq!(w.ai_plan_fast_forward(0), 11);
    // only one farm: step 4 (second farm) stops
    w.factions[0].has[BuildingKind::Farm as usize] = 1;
    assert_eq!(w.ai_plan_fast_forward(0), 4);
}

#[test]
fn takeover_reset_keeps_state_and_fast_forwards() {
    let mut w = demo(AiLevel::Hard);
    w.time = 500.0;
    w.factions[1].ai.wave = Some(AiWave {
        ids: Vec::new(),
        target: 0,
        start: 100.0,
        phase: WavePhase::March,
        leg_t: 0.0,
        t0: 0.0,
    });
    w.factions[1].ai.due_t = Some(1.0);
    w.factions[1].ai.wave_t = -5.0;
    w.factions[1].ai.wave_need = 8;
    w.factions[1].has[BuildingKind::Farm as usize] = 1;
    w.ai_reset_for_takeover(1);
    let ai = &w.factions[1].ai;
    assert!(ai.wave.is_none() && ai.due_t.is_none() && ai.target.is_none());
    assert!((1.0..=3.0).contains(&ai.think_t));
    assert!(ai.wave_t >= 90.0, "no instant wave after takeover");
    assert!(ai.capture_t >= 560.0);
    assert_eq!(ai.wave_need, 8, "other state kept");
    assert_eq!(ai.step, 1);
}

#[test]
fn find_spot_is_placeable_with_corridor() {
    let mut w = demo(AiLevel::Normal);
    let (bx, by) = w.factions[0].base;
    for kind in [BuildingKind::House, BuildingKind::Barracks, BuildingKind::Farm] {
        let (cx, cy) = w.ai_find_spot(kind, bx, by + 1, 4, 18).expect("spot near the castle");
        assert!(w.map.can_place(kind, cx, cy, &[]));
        let d = kind.def();
        for y in cy - 1..=cy + d.h {
            for x in cx - 1..=cx + d.w {
                assert!(w.map.passable(w.map.idx(x, y), 0));
            }
        }
        assert!((cx - bx).abs() <= 18 + d.w && (cy - by).abs() <= 19 + d.h);
    }
}

#[test]
fn intel_sees_danger_and_home() {
    let mut w = demo(AiLevel::Normal);
    let p = near_castle(&w, 0, 0.0, 200.0);
    let foe = w.spawn_unit(1, UnitKind::Knight, p);
    let mine = w.spawn_unit(0, UnitKind::Archer, near_castle(&w, 0, 64.0, 150.0));
    let intel = w.ai_intel(0);
    assert!(intel.danger >= 139.0, "knight worth 140");
    assert_eq!(intel.danger_at, Some(p));
    assert!(intel.home.contains(&mine));
    assert!(intel.home_value >= 70.0);
    let e = intel.foe(1).unwrap();
    assert!(e.mix[UnitKind::Knight as usize] > 0.0);
    assert!(e.defense >= 150.0, "castle arrows count");
    assert!(intel.total > 0.0);
    let _ = foe;
}

#[test]
fn spare_keeps_a_garrison_and_sends_melee_first() {
    let mut w = demo(AiLevel::Normal);
    let mut ids = Vec::new();
    for i in 0..6 {
        let k = if i < 2 { UnitKind::Archer } else { UnitKind::Warrior };
        let p = near_castle(&w, 0, -200.0 + i as f32 * 60.0, 140.0);
        ids.push(w.spawn_unit(0, k, p));
    }
    w.census(0);
    let intel = w.ai_intel(0);
    let s = w.ai_spare(0, &intel);
    // 1 starting warrior + 6 new = 7 fighters; keep = max(2, round(7*0.15)) = 2
    assert_eq!(s.total, 7);
    assert_eq!(s.squad.len(), 5);
    for id in &s.squad {
        assert_ne!(w.units.get(*id).unwrap().kind, UnitKind::Archer, "archers stay home");
    }
    // a unit in the wave is not spare
    w.factions[0].ai.wave = Some(AiWave {
        ids: vec![s.squad[0]],
        target: 1,
        start: 0.0,
        phase: WavePhase::March,
        leg_t: -99.0,
        t0: 0.0,
    });
    let s2 = w.ai_spare(0, &intel);
    assert!(!s2.squad.contains(&s.squad[0]));
}

#[test]
fn choose_target_avoids_recently_raided() {
    let mut w = demo(AiLevel::Normal);
    w.time = 1000.0;
    let mut counts = [0u32; 5];
    for _ in 0..400 {
        let t = w.ai_choose_target(0, None, 0.0).unwrap();
        assert_ne!(t, 0);
        counts[t as usize] += 1;
    }
    let before = counts[1];
    w.factions[1].raided_t = 990.0;
    let mut counts2 = [0u32; 5];
    for _ in 0..400 {
        counts2[w.ai_choose_target(0, None, 0.0).unwrap() as usize] += 1;
    }
    assert!(counts2[1] < before.max(1), "raided kingdom picked less ({} vs {before})", counts2[1]);
    // dead kingdoms are never picked
    w.factions[2].alive = false;
    for _ in 0..100 {
        assert_ne!(w.ai_choose_target(0, None, 0.0), Some(2));
    }
}

#[test]
fn next_unit_follows_mix_and_monks() {
    let mut w = demo(AiLevel::Normal);
    let intel = w.ai_intel(0);
    assert_eq!(w.ai_next_unit(0, &intel, false), None, "nothing buildable without a mix");
    w.factions[0].has[BuildingKind::Barracks as usize] = 1;
    w.factions[0].ai.mix = w.ai_want(0, &intel);
    assert_eq!(w.ai_next_unit(0, &intel, false), Some(UnitKind::Warrior));
    // cheap: nothing affordable → None
    w.factions[0].res = Stock([0.0; 4]);
    assert_eq!(w.ai_next_unit(0, &intel, true), None);
    // a monk every 5 fighters (age 3 + monastery)
    w.factions[0].res = Stock([1000.0; 4]);
    w.factions[0].age = 3;
    w.factions[0].has[BuildingKind::Monastery as usize] = 1;
    for i in 0..5 {
        w.spawn_unit(0, UnitKind::Warrior, near_castle(&w, 0, i as f32 * 40.0, 150.0));
    }
    w.census(0);
    w.factions[0].has[BuildingKind::Monastery as usize] = 1;
    w.factions[0].has[BuildingKind::Barracks as usize] = 1;
    assert_eq!(w.ai_next_unit(0, &intel, false), Some(UnitKind::Monk));
}

#[test]
fn skip_rule_skips_towers_on_normal() {
    let mut w = demo(AiLevel::Normal);
    w.census(0);
    w.factions[0].ai.step = 10; // build tower
    w.ai_plan(0, 5);
    assert_eq!(w.factions[0].ai.step, 11, "normal has 0 towers: step skipped");
}

#[test]
fn strays_go_home_but_not_capturers() {
    let mut w = demo(AiLevel::Normal);
    // a warrior idle far from home, away from any point
    let c = castle(&w, 0);
    let mut far = None;
    'search: for r in [1400.0f32, 1800.0, 2200.0] {
        for a in 0..16 {
            let ang = a as f32 / 16.0 * std::f32::consts::TAU;
            let p = w.map.nearest_passable(c + v2(ang.cos(), ang.sin()) * r);
            if p.dist(c) > 1000.0 && !w.points.iter().any(|q| q.pos.dist(p) < CAPTURE_RADIUS + 50.0) {
                far = Some(p);
                break 'search;
            }
        }
    }
    let p = far.expect("free spot far from the castle");
    let u = w.spawn_unit(0, UnitKind::Warrior, p);
    w.census(0);
    w.ai_strays(0);
    assert!(matches!(w.units.get(u).unwrap().order, Order::AMove(_) | Order::Move(_)));
    if let Some(pt) = w.points.first().map(|q| q.pos) {
        let q = w.map.nearest_passable(pt + v2(0.0, 40.0));
        let v = w.spawn_unit(0, UnitKind::Warrior, q);
        w.census(0);
        w.ai_strays(0);
        if q.dist(c) > 900.0 {
            assert!(w.units.get(v).unwrap().order.is_idle(), "capturing units stay");
        }
    }
}

#[test]
fn ai_tick_is_noop_for_humans_and_safe_for_dead() {
    let mut w = demo(AiLevel::Hard);
    w.factions[0].human = true;
    let before = w.factions[0].ai.clone();
    w.ai_tick(0);
    assert_eq!(w.factions[0].ai, before);
    w.factions[1].alive = false;
    w.ai_tick(1);
    // a normal think runs without panicking
    for f in 2..5 {
        w.ai_tick(f);
    }
}
