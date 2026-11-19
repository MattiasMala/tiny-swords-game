// OWNER: S2 (units) — unit tests for combat.rs / unit.rs / formation.rs / projectile.rs (spec 03a).

use super::*;
use crate::cmd::{EntityRef, OrderKind};
use crate::map::{GeneratedWorld, H, Map, W};
use crate::world::MatchSetup;

/// Flat all-land world with two AI kingdoms (castles far from the test area); their starting units removed.
pub(crate) fn test_world() -> World {
    let mut map = Map::with_size(W, H);
    map.land.iter_mut().for_each(|v| *v = 1);
    let g = GeneratedWorld {
        seed: 7,
        map,
        bases: vec![(20, 20), (260, 170)],
        plateaus: vec![],
        trees: vec![],
        golds: vec![],
        stones: vec![],
        bigs: vec![],
        meadows: vec![],
        points: vec![],
        chests: vec![],
        ponds: vec![],
        open_water: vec![],
    };
    let mut w = World::from_generated(g, &MatchSetup::default());
    w.units.clear();
    w.events.clear();
    w
}

const A: V2 = v2(6000.0, 6000.0);

fn spawn(w: &mut World, f: FactionIdx, k: UnitKind, p: V2) -> UnitId {
    let id = w.spawn_unit(f, k, p);
    w.grid.rebuild(&w.units);
    id
}

fn hp(w: &World, id: UnitId) -> f32 {
    w.units.get(id).unwrap().hp
}

#[test]
fn damage_formula_and_counters() {
    let m = Mods::default();
    // bonus table (damageTo)
    let dt = |a: UnitKind, t: UnitKind| damage_to(Attacker::Unit(a), a.def().dmg, Some(t));
    assert!((dt(UnitKind::Warrior, UnitKind::Lancer) - 19.5).abs() < 1e-4);
    assert!((dt(UnitKind::Lancer, UnitKind::Knight) - 30.0).abs() < 1e-4);
    assert!((dt(UnitKind::Lancer, UnitKind::Archer) - 22.0).abs() < 1e-4);
    assert!((dt(UnitKind::Archer, UnitKind::Warrior) - 20.4).abs() < 1e-4);
    assert!((dt(UnitKind::Knight, UnitKind::Catapult) - 36.0).abs() < 1e-4);
    assert!((dt(UnitKind::Knight, UnitKind::Pawn) - 23.4).abs() < 1e-4);
    assert!((dt(UnitKind::Warrior, UnitKind::Knight) - 13.0).abs() < 1e-4);
    let vb = |a: UnitKind| damage_to(Attacker::Unit(a), a.def().dmg, None);
    assert!((vb(UnitKind::Ram) - 220.0).abs() < 1e-4);
    assert!((vb(UnitKind::Archer) - 6.0).abs() < 1e-4);
    assert_eq!(
        damage_to(
            Attacker::Building(BuildingKind::Tower),
            15.0,
            Some(UnitKind::Warrior)
        ),
        15.0
    );
    // arrows: ram ×0.2, blockers/knights × arrowRes, then armor, min 1
    assert!((unit_damage(12.0, UnitKind::Ram, &m, true) - 2.4).abs() < 1e-4);
    assert!((unit_damage(12.0, UnitKind::Ram, &m, false) - 12.0).abs() < 1e-4);
    let mut sh = Mods::default();
    sh.apply_tech(TechId::Shields);
    sh.apply_tech(TechId::Armor);
    assert!((unit_damage(20.4, UnitKind::Warrior, &sh, true) - (20.4 * 0.5 - 3.0)).abs() < 1e-4);
    assert!((unit_damage(20.0, UnitKind::Knight, &sh, true) - 7.0).abs() < 1e-4);
    // archers get armor but not shields; rocks (no arrow) not halved
    assert!((unit_damage(20.0, UnitKind::Archer, &sh, true) - 17.0).abs() < 1e-4);
    assert!((unit_damage(20.0, UnitKind::Warrior, &sh, false) - 17.0).abs() < 1e-4);
    // min-1 clamp; monks/pawns/siege have no armor
    assert_eq!(unit_damage(2.0, UnitKind::Lancer, &sh, false), 1.0);
    assert_eq!(unit_damage(2.0, UnitKind::Monk, &sh, false), 2.0);
    assert_eq!(unit_damage(10.0, UnitKind::Catapult, &sh, false), 10.0);
}

#[test]
fn hurt_pipeline_buildings_block_and_retaliation() {
    let mut w = test_world();
    let a = spawn(&mut w, 0, UnitKind::Knight, A);
    let b = spawn(&mut w, 1, UnitKind::Warrior, A + v2(40.0, 0.0));
    let src = DamageSource {
        unit: Some(a),
        faction: Some(0),
        arrow: false,
    };
    // attacking warriors never block
    w.units.get_mut(b).unwrap().state = UnitState::Attack;
    w.hurt(Target::Unit(b), 10.0, src);
    assert!((hp(&w, b) - 120.0).abs() < 1e-4);
    assert!((w.units.get(b).unwrap().hurt_t - HURT_FLASH).abs() < 1e-6);
    // instant retaliation (idle soldier without target)
    assert_eq!(w.units.get(b).unwrap().target, Some(Target::Unit(a)));
    assert_eq!(w.factions[1].threat, Some(a));
    // block roll: over many hits about 25% are blocked (×0.35) and start a 0.5 s block
    let mut blocks = 0;
    for _ in 0..400 {
        let u = w.units.get_mut(b).unwrap();
        u.state = UnitState::Idle;
        u.hp = 1000.0;
        w.hurt(Target::Unit(b), 10.0, src);
        let u = w.units.get(b).unwrap();
        if u.state == UnitState::Block {
            blocks += 1;
            assert!((u.hp - (1000.0 - 3.5)).abs() < 1e-3);
            assert!((u.block_t - BLOCK_TIME).abs() < 1e-6);
            assert!(u.flip, "faces the attacker on the left");
        } else {
            assert!((u.hp - 990.0).abs() < 1e-3);
        }
    }
    assert!((60..140).contains(&blocks), "blocks {blocks}");
    // buildings take 40%
    let castle = w.factions[1].castle.unwrap();
    let before = w.buildings.get(castle).unwrap().hp;
    w.hurt(Target::Building(castle), 100.0, src);
    assert!((before - w.buildings.get(castle).unwrap().hp - 40.0).abs() < 1e-3);
    // death + kill credit
    let k0 = w.factions[0].kills;
    w.hurt(Target::Unit(b), 5000.0, src);
    assert!(w.units.get(b).unwrap().dead);
    assert_eq!(w.factions[0].kills, k0 + 1);
    assert!(w.events.iter().any(|e| matches!(
        e,
        GameEvent::Sound {
            sound: Sound::Death,
            ..
        }
    )));
}

#[test]
fn human_alarm_and_alert() {
    let mut w = test_world();
    w.factions[1].human = true;
    let castle = w.factions[1].castle.unwrap();
    let cpos = w.buildings.get(castle).unwrap().pos;
    let a = spawn(&mut w, 0, UnitKind::Knight, cpos + v2(0.0, 300.0));
    let b = spawn(&mut w, 1, UnitKind::Archer, cpos + v2(0.0, 340.0));
    let src = DamageSource {
        unit: Some(a),
        faction: Some(0),
        arrow: false,
    };
    w.time = 50.0;
    w.hurt(Target::Unit(b), 5.0, src);
    let al = w.factions[1].alarm.expect("alarm near home");
    assert_eq!(al.pos, cpos + v2(0.0, 300.0));
    assert!(
        w.events
            .iter()
            .any(|e| matches!(e, GameEvent::Alert { to: 1, .. }))
    );
    // idle soldiers of the kingdom answer the alarm for 10 s; rams and monks do not
    let g = spawn(&mut w, 1, UnitKind::Warrior, cpos + v2(800.0, 300.0));
    let r = spawn(&mut w, 1, UnitKind::Ram, cpos + v2(800.0, 400.0));
    assert!(w.alarm_for(g).is_some());
    assert!(w.alarm_for(r).is_none());
    w.time = 61.0;
    assert!(w.alarm_for(g).is_none());
}

#[test]
fn attack_timing_follows_frame_table() {
    let mut w = test_world();
    let a = spawn(&mut w, 0, UnitKind::Warrior, A);
    let b = spawn(&mut w, 1, UnitKind::Monk, A + v2(30.0, 0.0));
    w.units.get_mut(a).unwrap().target = Some(Target::Unit(b));
    w.start_attack(a, A + v2(30.0, 0.0));
    let u = w.units.get(a).unwrap();
    assert_eq!(u.state, UnitState::Attack);
    assert_eq!(u.anim, AnimKind::Attack2, "first warrior swing is attack2");
    assert!((u.cd - 0.95).abs() < 1e-6);
    let full = hp(&w, b);
    let mut hit_tick = None;
    let mut end_tick = None;
    for tick in 1..=10 {
        w.units.get_mut(a).unwrap().anim_t += TICK_DT;
        let busy = w.tick_action(a);
        if hit_tick.is_none() && hp(&w, b) < full {
            hit_tick = Some(tick);
        }
        if !busy {
            end_tick = Some(tick);
            break;
        }
    }
    // warrior: hit at frame 2 of 12 fps (0.1667 s → 4th tick), end at frame 4 (0.333 s → 7th tick)
    assert_eq!(hit_tick, Some(4));
    assert_eq!(end_tick, Some(7));
    assert!((full - hp(&w, b) - 13.0).abs() < 1e-4);
    // next swing alternates the animation
    w.start_attack(a, A + v2(30.0, 0.0));
    assert_eq!(w.units.get(a).unwrap().anim, AnimKind::Attack);
    // archer: hit at frame 5 of 14 fps (0.357 s → 8th tick) fires an arrow
    let r = spawn(&mut w, 0, UnitKind::Archer, A + v2(0.0, 300.0));
    w.units.get_mut(r).unwrap().target = Some(Target::Unit(b));
    w.start_attack(r, A);
    let n0 = w.projectiles.len();
    let mut fired = None;
    for tick in 1..=12 {
        w.units.get_mut(r).unwrap().anim_t += TICK_DT;
        w.tick_action(r);
        if fired.is_none() && w.projectiles.len() > n0 {
            fired = Some(tick);
        }
    }
    assert_eq!(fired, Some(8));
    // lancer attack direction from dir5
    let l = spawn(&mut w, 0, UnitKind::Lancer, A + v2(0.0, 900.0));
    w.start_attack(l, A + v2(0.0, 800.0));
    assert_eq!(w.units.get(l).unwrap().anim_dir, Dir5::Up);
}

#[test]
fn melee_whiffs_when_target_moved_away() {
    let mut w = test_world();
    let a = spawn(&mut w, 0, UnitKind::Warrior, A);
    let b = spawn(&mut w, 1, UnitKind::Archer, A + v2(30.0, 0.0));
    w.units.get_mut(a).unwrap().target = Some(Target::Unit(b));
    w.start_attack(a, A + v2(30.0, 0.0));
    w.units.get_mut(b).unwrap().pos = A + v2(52.0 * 1.5 + 1.0, 0.0);
    w.do_hit(a);
    assert_eq!(hp(&w, b), 70.0);
    w.units.get_mut(b).unwrap().pos = A + v2(52.0 * 1.5 - 1.0, 0.0);
    w.do_hit(a);
    assert!((hp(&w, b) - (70.0 - 13.0)).abs() < 1e-4);
}

#[test]
fn archer_kites_melee_while_reloading() {
    let mut w = test_world();
    let r = spawn(&mut w, 0, UnitKind::Archer, A);
    let e = spawn(&mut w, 1, UnitKind::Warrior, A + v2(80.0, 0.0));
    w.units.get_mut(r).unwrap().cd = 1.0;
    assert!(w.fight(r, Target::Unit(e)));
    let u = w.units.get(r).unwrap();
    assert!(u.pos.x < A.x - 1.0, "stepped away: {:?}", u.pos);
    assert_ne!(u.state, UnitState::Attack);
    // cd almost over → it stands and shoots
    w.units.get_mut(r).unwrap().cd = 0.2;
    let before = w.units.get(r).unwrap().pos;
    assert!(w.fight(r, Target::Unit(e)));
    assert_eq!(w.units.get(r).unwrap().pos, before);
    w.units.get_mut(r).unwrap().cd = 0.0;
    w.fight(r, Target::Unit(e));
    assert_eq!(w.units.get(r).unwrap().state, UnitState::Attack);
    // never kites another ranged unit
    let r2 = spawn(&mut w, 0, UnitKind::Archer, A + v2(0.0, 500.0));
    let e2 = spawn(&mut w, 1, UnitKind::Archer, A + v2(80.0, 500.0));
    w.units.get_mut(r2).unwrap().cd = 1.0;
    let before = w.units.get(r2).unwrap().pos;
    w.fight(r2, Target::Unit(e2));
    assert_eq!(w.units.get(r2).unwrap().pos, before);
}

#[test]
fn target_weights_monk_and_pawn() {
    let mut w = test_world();
    let s = spawn(&mut w, 0, UnitKind::Warrior, A);
    // pawn at 150: weighted 330 ≥ aggro 300 → ignored
    let _p = spawn(&mut w, 1, UnitKind::Pawn, A + v2(150.0, 0.0));
    assert_eq!(w.acquire_target(s, false), None);
    // a warrior at 200 vs a monk at 250 (weighted 150): the monk wins
    let e = spawn(&mut w, 1, UnitKind::Warrior, A + v2(0.0, 200.0));
    let m = spawn(&mut w, 1, UnitKind::Monk, A + v2(-250.0, 0.0));
    assert_eq!(w.acquire_target(s, false), Some(Target::Unit(m)));
    w.units.get_mut(m).unwrap().dead = true;
    w.grid.rebuild(&w.units);
    assert_eq!(w.acquire_target(s, false), Some(Target::Unit(e)));
    // guard leash: the post far away → nothing within 430 of the post
    w.set_post(s, A + v2(-2000.0, 0.0));
    assert_eq!(w.acquire_target(s, false), None);
    // roaming (alarm) ignores the leash
    assert_eq!(w.acquire_target(s, true), Some(Target::Unit(e)));
}

#[test]
fn monk_heals_lowest_ratio_within_pool() {
    let mut w = test_world();
    let m = spawn(&mut w, 0, UnitKind::Monk, A);
    let a = spawn(&mut w, 0, UnitKind::Warrior, A + v2(100.0, 0.0));
    let b = spawn(&mut w, 0, UnitKind::Archer, A + v2(-100.0, 0.0));
    w.units.get_mut(a).unwrap().hp = 100.0; // 0.77
    w.units.get_mut(b).unwrap().hp = 40.0; // 0.57
    w.units.get_mut(m).unwrap().think_t = 0.0;
    w.update_monk(m);
    let u = w.units.get(m).unwrap();
    assert_eq!(u.state, UnitState::Attack);
    assert_eq!(u.target, Some(Target::Unit(b)));
    w.units.get_mut(m).unwrap().faith = 25.0;
    w.do_hit(m);
    assert!((hp(&w, b) - 65.0).abs() < 1e-4, "limited by faith");
    assert!(w.units.get(m).unwrap().faith.abs() < 1e-4);
    // heal never exceeds max hp
    w.units.get_mut(m).unwrap().faith = 100.0;
    w.units.get_mut(m).unwrap().target = Some(Target::Unit(a));
    w.do_hit(m);
    assert!((hp(&w, a) - 130.0).abs() < 1e-4);
    assert!((w.units.get(m).unwrap().faith - 70.0).abs() < 1e-4);
}

#[test]
fn monk_steps_away_from_enemy_soldier() {
    let mut w = test_world();
    let m = spawn(&mut w, 0, UnitKind::Monk, A);
    let _e = spawn(&mut w, 1, UnitKind::Warrior, A + v2(100.0, 0.0));
    w.units.get_mut(m).unwrap().think_t = 0.0;
    w.update_monk(m);
    let u = w.units.get(m).unwrap();
    assert!(u.pos.x < A.x);
    // the guard post moves with the monk
    assert_eq!(w.post_of(m), Some(u.pos));
}

#[test]
fn arrows_hit_first_enemy_and_rocks_splash() {
    let mut w = test_world();
    let r = spawn(&mut w, 0, UnitKind::Archer, A);
    let near = spawn(&mut w, 1, UnitKind::Knight, A + v2(150.0, 0.0));
    let far = spawn(&mut w, 1, UnitKind::Warrior, A + v2(300.0, 0.0));
    let dmg = w.factions[0].mods.unit_dmg(UnitKind::Archer);
    w.fire_arrow(
        0,
        Shooter::Unit {
            id: r,
            kind: UnitKind::Archer,
        },
        A + v2(16.0, -34.0),
        A + v2(300.0, -28.0),
        dmg,
    );
    for _ in 0..20 {
        w.update_projectiles();
    }
    assert!(w.projectiles.is_empty());
    // the knight in the path takes it (no archer bonus vs knights), the warrior behind is safe
    assert!((hp(&w, near) - (190.0 - 12.0)).abs() < 1e-3);
    assert_eq!(hp(&w, far), 130.0);
    // rock: splash 72 px on enemies, not on own units
    let c = spawn(&mut w, 0, UnitKind::Catapult, A + v2(0.0, 600.0));
    let own = spawn(&mut w, 0, UnitKind::Warrior, A + v2(1000.0, 1030.0));
    let e1 = spawn(&mut w, 1, UnitKind::Archer, A + v2(1000.0, 1000.0));
    let e2 = spawn(&mut w, 1, UnitKind::Archer, A + v2(1060.0, 1000.0));
    let e3 = spawn(&mut w, 1, UnitKind::Archer, A + v2(1080.0, 1000.0));
    w.fire_rock(
        0,
        Shooter::Unit {
            id: c,
            kind: UnitKind::Catapult,
        },
        A + v2(20.0, 550.0),
        A + v2(1000.0, 1000.0),
        42.0,
    );
    for _ in 0..80 {
        w.update_projectiles();
    }
    assert!((hp(&w, e1) - 28.0).abs() < 1e-3);
    assert!((hp(&w, e2) - 28.0).abs() < 1e-3);
    assert_eq!(hp(&w, e3), 70.0);
    assert_eq!(hp(&w, own), 130.0);
}

#[test]
fn separation_pushes_apart() {
    let mut w = test_world();
    let a = spawn(&mut w, 0, UnitKind::Warrior, A);
    let b = spawn(&mut w, 1, UnitKind::Warrior, A + v2(10.0, 0.0));
    w.separate_units();
    let pa = w.units.get(a).unwrap().pos;
    let pb = w.units.get(b).unwrap().pos;
    assert!((pa.x - (A.x - 2.64)).abs() < 1e-3 && (pb.x - (A.x + 12.64)).abs() < 1e-3);
    // busy (attacking) units are pushed 5x less
    let c = spawn(&mut w, 0, UnitKind::Warrior, A + v2(0.0, 500.0));
    let d = spawn(&mut w, 0, UnitKind::Warrior, A + v2(10.0, 500.0));
    w.units.get_mut(c).unwrap().state = UnitState::Attack;
    w.separate_units();
    let pc = w.units.get(c).unwrap().pos;
    let pd = w.units.get(d).unwrap().pos;
    assert!((pc.x - (A.x - 2.64 * 0.2)).abs() < 1e-3);
    assert!((pd.x - (A.x + 12.64)).abs() < 1e-3);
}

/// Runs the unit phase of `World::step` only (no factions/AI, no buildings).
fn unit_ticks(w: &mut World, n: usize, mut stop: impl FnMut(&World) -> bool) {
    for _ in 0..n {
        w.tick += 1;
        w.time = w.tick as f32 * TICK_DT;
        w.fields.begin_tick();
        w.grid.rebuild(&w.units);
        for id in w.units.ids() {
            if w.unit_alive(id) {
                w.update_unit(id);
            }
        }
        w.separate_units();
        w.remove_dead_units();
        w.update_projectiles();
        if stop(w) {
            break;
        }
    }
}

#[test]
fn soldier_engages_and_kills_through_ticks() {
    let mut w = test_world();
    let k = spawn(&mut w, 0, UnitKind::Knight, A);
    let e = spawn(&mut w, 1, UnitKind::Catapult, A + v2(200.0, 0.0));
    // a catapult that never shoots (huge cd; siege never kites): the knight acquires it, charges and kills it
    w.units.get_mut(e).unwrap().cd = 1e9;
    unit_ticks(&mut w, 300, |w| !w.unit_alive(e));
    assert!(!w.unit_alive(e), "catapult killed (110 hp / 36 per hit)");
    assert_eq!(w.factions[0].kills, 1);
    assert!(w.unit_alive(k));
}

#[test]
fn breach_picks_a_reachable_palisade_toward_the_goal() {
    let mut w = test_world();
    // enemy palisade column x = 100 across the whole map: the east side is unreachable for faction 0
    for cy in 0..H {
        w.add_building(1, BuildingKind::Wall, 100, cy, true);
    }
    let s = spawn(&mut w, 0, UnitKind::Warrior, v2(95.5 * 64.0, 60.5 * 64.0));
    let goal = v2(110.5 * 64.0, 60.5 * 64.0);
    let g = w.breach(s, goal).expect("a palisade to knock down");
    let b = w.buildings.get(g).unwrap();
    assert_eq!(b.kind, BuildingKind::Wall);
    assert_eq!(b.cy, 60, "the one between the unit and the goal");
    // walls are never auto-targeted
    assert_eq!(w.acquire_target(s, true), None);
}

#[test]
fn scout_explores_frontier_then_reports() {
    let mut w = test_world();
    w.factions[0].human = true;
    w.factions[0].owned = true;
    w.factions[0].fog.explored.fill();
    // one unexplored lattice cell (x = 2 + 3k, y = 2 + 3k)
    let (gx, gy) = (95, 95);
    let c = gy * W + gx;
    w.factions[0].fog.explored.unset(c as usize);
    let s = spawn(&mut w, 0, UnitKind::Scout, A);
    w.order_units(0, &[s], OrderKind::Explore, 0.0, 0.0, None);
    w.update_scout(s);
    match &w.units.get(s).unwrap().order {
        Order::Explore { goal: Some(g), .. } => assert_eq!(*g, w.map.center(c)),
        o => panic!("{o:?}"),
    }
    // once the cell is explored, nothing is left: idle + toast for the human
    w.factions[0].fog.explored.fill();
    w.update_scout(s);
    assert!(w.units.get(s).unwrap().order.is_idle());
    assert!(w.events.iter().any(|e| matches!(
        e,
        GameEvent::Toast {
            to: 0,
            msg: crate::events::Msg::ExploreDone
        }
    )));
    // only scouts take the explore order
    let k = spawn(&mut w, 0, UnitKind::Knight, A);
    w.order_units(0, &[k], OrderKind::Explore, 0.0, 0.0, None);
    assert!(w.units.get(k).unwrap().order.is_idle());
}

#[test]
fn march_and_formation_arrive() {
    let mut w = test_world();
    let ids: Vec<UnitId> = (0..6)
        .map(|i| {
            let k = if i % 2 == 0 {
                UnitKind::Warrior
            } else {
                UnitKind::Archer
            };
            spawn(&mut w, 0, k, A + v2(i as f32 * 30.0, 0.0))
        })
        .collect();
    w.order_units(0, &ids, OrderKind::Move, A.x + 600.0, A.y, None);
    unit_ticks(&mut w, 400, |w| ids.iter().all(|id| w.units.get(*id).unwrap().order.is_idle()));
    for id in &ids {
        let u = w.units.get(*id).unwrap();
        assert!(u.order.is_idle(), "{:?}", u.order);
        assert!(u.pos.x > A.x + 400.0, "{:?}", u.pos);
    }
    // warriors (front) end ahead of the archers (behind)
    let wx: f32 = ids.iter().step_by(2).map(|i| w.units.get(*i).unwrap().pos.x).sum::<f32>() / 3.0;
    let ax: f32 = ids.iter().skip(1).step_by(2).map(|i| w.units.get(*i).unwrap().pos.x).sum::<f32>() / 3.0;
    assert!(wx > ax + 40.0, "front {wx} back {ax}");
}

#[test]
fn ai_idle_after_target_dies_human_raids_on() {
    let mut w = test_world();
    let k = spawn(&mut w, 0, UnitKind::Knight, A);
    let e = spawn(&mut w, 1, UnitKind::Pawn, A + v2(100.0, 0.0));
    let e2 = spawn(&mut w, 1, UnitKind::Pawn, A + v2(500.0, 0.0));
    w.order_units(0, &[k], OrderKind::Attack, 0.0, 0.0, Some(EntityRef::Unit(e)));
    w.units.get_mut(e).unwrap().dead = true;
    w.update_soldier(k);
    assert!(w.units.get(k).unwrap().order.is_idle(), "AI: idle");
    w.factions[0].human = true;
    w.units.get_mut(e).unwrap().dead = false;
    w.order_units(0, &[k], OrderKind::Attack, 0.0, 0.0, Some(EntityRef::Unit(e)));
    w.units.get_mut(e).unwrap().dead = true;
    w.grid.rebuild(&w.units);
    w.update_soldier(k);
    match w.units.get(k).unwrap().order {
        Order::AMove(m) => {
            assert!(m.auto);
            assert_eq!(m.to, w.units.get(e2).unwrap().pos);
        }
        ref o => panic!("{o:?}"),
    }
}

#[test]
fn order_units_attack_heal_and_formation() {
    let mut w = test_world();
    let k = spawn(&mut w, 0, UnitKind::Knight, A);
    let ram = spawn(&mut w, 0, UnitKind::Ram, A + v2(50.0, 0.0));
    let m = spawn(&mut w, 0, UnitKind::Monk, A + v2(100.0, 0.0));
    let p = spawn(&mut w, 0, UnitKind::Pawn, A + v2(150.0, 0.0));
    let e = spawn(&mut w, 1, UnitKind::Warrior, A + v2(600.0, 0.0));
    let ids = [k, ram, m, p];
    w.order_units(
        0,
        &ids,
        OrderKind::Attack,
        0.0,
        0.0,
        Some(EntityRef::Unit(e)),
    );
    assert_eq!(
        w.units.get(k).unwrap().order,
        Order::Attack {
            target: Target::Unit(e)
        }
    );
    assert!(
        w.units.get(ram).unwrap().order.is_idle(),
        "rams cannot hit units"
    );
    assert!(matches!(
        w.units.get(m).unwrap().order,
        Order::Idle { escort: true, .. }
    ));
    assert!(w.units.get(p).unwrap().order.is_idle());
    // own units cannot be attacked
    w.order_units(0, &[k], OrderKind::Attack, 0.0, 0.0, Some(EntityRef::Unit(p)));
    assert_eq!(
        w.units.get(k).unwrap().order,
        Order::Attack {
            target: Target::Unit(e)
        }
    );
    // explicit heal on an own siege unit is allowed (U3), never on itself
    w.order_units(0, &[m], OrderKind::Heal, 0.0, 0.0, Some(EntityRef::Unit(ram)));
    assert_eq!(w.units.get(m).unwrap().order, Order::Heal { target: ram });
    w.order_units(0, &[m], OrderKind::Heal, 0.0, 0.0, Some(EntityRef::Unit(m)));
    assert_eq!(w.units.get(m).unwrap().order, Order::Heal { target: ram });
    // attack-move: combatants amove with the slowest combatant pace (ram 62), pawn & monk plain move
    w.order_units(0, &ids, OrderKind::AMove, A.x + 2000.0, A.y, None);
    match w.units.get(k).unwrap().order {
        Order::AMove(mo) => {
            assert!((mo.pace - 62.0).abs() < 1e-3);
            assert!(mo.group.is_some());
        }
        ref o => panic!("{o:?}"),
    }
    assert!(matches!(w.units.get(m).unwrap().order, Order::Move(mo) if mo.escort));
    assert!(matches!(w.units.get(p).unwrap().order, Order::Move(mo) if !mo.escort));
    // stop
    w.order_units(0, &ids, OrderKind::Stop, 0.0, 0.0, None);
    assert!(
        ids.iter()
            .all(|id| w.units.get(*id).unwrap().order.is_idle())
    );
    // other factions' units are ignored
    w.order_units(1, &[k], OrderKind::Move, 0.0, 0.0, None);
    assert!(w.units.get(k).unwrap().order.is_idle());
}

#[test]
#[ignore = "slow smoke test: cargo test -p ts-sim --lib demo_battle_runs -- --ignored --nocapture"]
fn demo_battle_runs() {
    let mut w = World::new(3, &MatchSetup::default());
    let t0 = std::time::Instant::now();
    let mut fights = 0usize;
    for _ in 0..30000 {
        w.step(&[]);
        fights += w.units.values().filter(|u| u.state == UnitState::Attack).count();
        w.drain_events();
    }
    eprintln!(
        "demo: {} units, {} buildings, kills {:?}, attack-ticks {}, {:?}",
        w.units.len(),
        w.buildings.len(),
        w.factions.iter().map(|f| f.kills).collect::<Vec<_>>(),
        fights,
        t0.elapsed()
    );
}

#[test]
#[ignore = "perf smoke test: cargo test -p ts-sim --lib mass_battle -- --ignored --nocapture"]
fn mass_battle_perf() {
    let mut w = test_world();
    let kinds = [
        UnitKind::Warrior,
        UnitKind::Lancer,
        UnitKind::Archer,
        UnitKind::Knight,
        UnitKind::Monk,
        UnitKind::Catapult,
    ];
    for f in 0..2u8 {
        let ids: Vec<UnitId> = (0..250)
            .map(|i| {
                let p = A + v2((i % 25) as f32 * 30.0, (i / 25) as f32 * 30.0 + f as f32 * 1500.0);
                w.spawn_unit(f, kinds[i % kinds.len()], p)
            })
            .collect();
        w.order_units(f, &ids, OrderKind::AMove, A.x + 400.0, A.y + 750.0, None);
    }
    let t0 = std::time::Instant::now();
    let mut worst = std::time::Duration::ZERO;
    for _ in 0..1200 {
        let t = std::time::Instant::now();
        unit_ticks(&mut w, 1, |_| false);
        worst = worst.max(t.elapsed());
        w.drain_events();
    }
    eprintln!(
        "mass: {} units left, kills {:?}, total {:?}, worst tick {:?}",
        w.units.len(),
        w.factions.iter().map(|f| f.kills).collect::<Vec<_>>(),
        t0.elapsed(),
        worst
    );
}

/// Every attack animation lands its hit and ends on the tick given by exact frame math (`k*dt*fps >= frame`,
/// i.e. `k*fps >= 20*frame` at 20 Hz) — boundaries such as the knight hit at exactly 0.25 s must not slip a tick
/// because of f32 accumulation of `anim_t`.
#[test]
fn attack_timing_exact_for_every_kind() {
    let mut w = test_world();
    let ticks_per_s = (1.0 / TICK_DT).round() as u32;
    for (i, kind) in [
        UnitKind::Warrior,
        UnitKind::Lancer,
        UnitKind::Archer,
        UnitKind::Knight,
        UnitKind::Monk,
        UnitKind::Ram,
        UnitKind::Catapult,
    ]
    .into_iter()
    .enumerate()
    {
        let a = kind.def().attack.expect("attack anim");
        let fps = a.fps.round() as u32;
        let want_hit = (1..100).find(|k| k * fps >= ticks_per_s * a.hit_frame as u32).unwrap();
        let want_end = (1..100).find(|k| k * fps >= ticks_per_s * a.frames as u32).unwrap();
        let p = A + v2(i as f32 * 300.0, 2000.0);
        let u = spawn(&mut w, 0, kind, p);
        w.start_attack(u, p + v2(500.0, 0.0));
        let (mut hit, mut end) = (None, None);
        for k in 1..100u32 {
            w.units.get_mut(u).unwrap().anim_t += TICK_DT;
            let busy = w.tick_action(u);
            if hit.is_none() && w.units.get(u).unwrap().hit_done {
                hit = Some(k);
            }
            if !busy {
                end = Some(k);
                break;
            }
        }
        assert_eq!(hit, Some(want_hit), "{kind:?} hit tick");
        assert_eq!(end, Some(want_end), "{kind:?} end tick");
    }
}

/// Two groups attack-move into each other on open ground; returns the survivors of each side.
fn duel(a: UnitKind, na: usize, b: UnitKind, nb: usize) -> (usize, usize) {
    let mut w = test_world();
    let mk = |w: &mut World, f: FactionIdx, k: UnitKind, n: usize, x0: f32| -> Vec<UnitId> {
        (0..n)
            .map(|i| w.spawn_unit(f, k, A + v2(x0 + (i / 5) as f32 * 40.0, (i % 5) as f32 * 40.0)))
            .collect()
    };
    let sa = mk(&mut w, 0, a, na, 0.0);
    let sb = mk(&mut w, 1, b, nb, 600.0);
    w.grid.rebuild(&w.units);
    w.order_units(0, &sa, OrderKind::AMove, A.x + 640.0, A.y + 80.0, None);
    w.order_units(1, &sb, OrderKind::AMove, A.x, A.y + 80.0, None);
    let alive = |w: &World, ids: &[UnitId]| ids.iter().filter(|id| w.unit_alive(**id)).count();
    unit_ticks(&mut w, 20 * 120, |w| alive(w, &sa) == 0 || alive(w, &sb) == 0);
    (alive(&w, &sa), alive(&w, &sb))
}

/// Equal-cost duels from the spec numbers (costs: warrior 98, lancer 100, archer 70, knight 140): the counter
/// triangle of the unit descriptions must hold — archers beat warriors, knights beat archers, lancers beat
/// knights, warriors beat lancers.
#[test]
fn equal_cost_counters_hold() {
    let (arch, war) = duel(UnitKind::Archer, 10, UnitKind::Warrior, 7);
    assert!(arch > 0 && war == 0, "archers {arch} vs warriors {war}");
    let (kn, arch) = duel(UnitKind::Knight, 5, UnitKind::Archer, 10);
    assert!(kn > 0 && arch == 0, "knights {kn} vs archers {arch}");
    let (lan, kn) = duel(UnitKind::Lancer, 7, UnitKind::Knight, 5);
    assert!(lan > 0 && kn == 0, "lancers {lan} vs knights {kn}");
    let (war, lan) = duel(UnitKind::Warrior, 7, UnitKind::Lancer, 7);
    assert!(war > 0 && lan == 0, "warriors {war} vs lancers {lan}");
}
