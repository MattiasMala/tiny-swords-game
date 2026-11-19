//! Generator tests: success, determinism and every gameplay invariant of spec 02 §6 on 50 seeds.

use super::check::check_invariants;
use super::*;
use crate::map::DecorKind;

#[test]
fn fifty_seeds_valid_and_deterministic() {
    let setup = MatchSetup::default();
    let mut attempts_total = 0;
    for k in 0..50u32 {
        let seed = k.wrapping_mul(2_654_435_761).wrapping_add(17);
        let t0 = std::time::Instant::now();
        let (g, attempts, checked) = generate_report(seed, &setup);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        // budget: < 300 ms per map in release (the dev profile here is optimised too; leave slack for debuginfo
        // builds and a busy machine)
        let budget = if cfg!(debug_assertions) { 1500.0 } else { 300.0 };
        assert!(ms < budget, "seed {seed}: generation took {ms:.0} ms");
        assert!(checked, "seed {seed}: sound() never passed");
        attempts_total += attempts;
        if let Err(e) = check_invariants(&g) {
            panic!("seed {seed}: {e}");
        }
        if k % 5 == 0 {
            let again = generate(seed, &setup);
            assert_eq!(g.map.hash(), again.map.hash(), "seed {seed} not deterministic");
            assert_eq!(g.trees, again.trees);
            assert_eq!(g.map.decor, again.map.decor);
            assert_eq!(g.map.surface, again.map.surface);
        }
    }
    assert!(attempts_total <= 50 * 4, "too many attempts: {attempts_total}");
}

#[test]
fn chest_kinds_cycle_and_points_mix() {
    let g = generate(12_345, &MatchSetup::default());
    // chests keep the kind given at creation (wood, gold, meat, stone, ...): all four present
    for r in Res::ALL {
        assert!(g.chests.iter().any(|c| c.res == *r));
    }
    // capture point kinds are POINT_MIX entries
    for p in &g.points {
        assert!(POINT_MIX.contains(&p.kind));
    }
    // every Extra prop except the chest pickup sprite appears somewhere over a few maps
    let mut seen = [false; 48];
    for s in [12_345u32, 7, 99, 2024] {
        let g = generate(s, &MatchSetup::default());
        for d in &g.map.decor {
            if let DecorKind::Prop(p) = d.kind {
                seen[p as usize] = true;
            }
        }
    }
    let missing: Vec<&str> = (0..48)
        .filter(|i| !seen[*i] && PROP_INFO[*i].0 != "chest")
        .map(|i| PROP_INFO[i].0)
        .collect();
    assert!(missing.is_empty(), "props never used: {missing:?}");
}
