//! What the AI knows about the others (04b §7 `aiIntel`) and the desired army mix (§8 `aiPower` / `aiEdge` /
//! `aiWant`).

use crate::World;
use crate::defs::*;
use crate::math::V2;
use crate::slab::UnitId;

/// One enemy kingdom in [`Intel::foes`] (JS `{o, near, mix, defense}`).
#[derive(Clone, Debug, PartialEq)]
pub struct Foe {
    pub o: FactionIdx,
    /// `1 / (1 + dist² / 4e6)` (+1 for the current wave target)
    pub near: f32,
    /// weighted value per unit kind (0 = absent)
    pub mix: [f32; 9],
    /// value of its soldiers at home + 150 per tower (+ castle)
    pub defense: f32,
}

/// `intel` (factions.js:561-593).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Intel {
    /// alive enemy kingdoms with a castle, in faction order
    pub foes: Vec<Foe>,
    pub total: f32,
    /// worth of armed enemy units within `AI_HOME` of our castle
    pub danger: f32,
    /// position of the closest of them (`intel.dangerAt`)
    pub danger_at: Option<V2>,
    /// own non-pawn units within `AI_HOME` (slab order)
    pub home: Vec<UnitId>,
    /// worth of the armed ones among `home`
    pub home_value: f32,
}

impl Intel {
    pub fn foe(&self, o: FactionIdx) -> Option<&Foe> {
        self.foes.iter().find(|e| e.o == o)
    }
}

/// `aiPower(f, t, o, e)`: strength per cost² of `f`'s unit `t` against `o`'s unit `e`.
pub fn ai_power(fm: &Mods, t: UnitKind, om: &Mods, e: UnitKind) -> f32 {
    let d = t.def();
    let de = e.def();
    let armor = if de.has_dmg() && !de.siege {
        om.armor
    } else {
        0.0
    };
    let hit = ((d.dmg + fm.atk[t as usize]) * d.bonus_vs(e) - armor).max(1.0);
    let reach = 1.0 + (d.range - 60.0).max(0.0) / 1500.0;
    let tough = fm.unit_max_hp(t) / (1.0 - d.block * 0.65);
    let cost = ai_cost(t);
    if d.cd <= 0.0 || cost <= 0.0 {
        return 0.0;
    }
    hit / d.cd * reach * tough / (cost * cost)
}

/// `aiEdge(f, t, o, e)`: log of the power ratio (> 0 when `t` wins at equal spending); only the bonus counts
/// against non-fighters (monks, rams, scouts, pawns).
pub fn ai_edge(fm: &Mods, t: UnitKind, om: &Mods, e: UnitKind) -> f32 {
    if !ai_fighter(e) {
        return t.def().bonus_vs(e).ln();
    }
    let a = ai_power(fm, t, om, e);
    let b = ai_power(om, e, fm, t);
    if a <= 0.0 || b <= 0.0 {
        return 0.0;
    }
    (a / b).ln()
}

impl World {
    /// `aiIntel(f)` (04b §7). Scans every unit once.
    pub fn ai_intel(&self, f: FactionIdx) -> Intel {
        let mut intel = Intel::default();
        let Some(c) = self.ai_castle(f) else {
            return intel;
        };
        let target = self.factions[f as usize].ai.target;
        // per faction index → position in foes (factions are few)
        let mut slot = [usize::MAX; 8];
        let mut castles = [V2::ZERO; 8];
        for o in &self.factions {
            if o.idx == f || !o.alive {
                continue;
            }
            let Some(oc) = self.ai_castle(o.idx) else {
                continue;
            };
            let near = 1.0 / (1.0 + oc.dist2(c) / 4e6) + if target == Some(o.idx) { 1.0 } else { 0.0 };
            if (o.idx as usize) < slot.len() {
                slot[o.idx as usize] = intel.foes.len();
                castles[o.idx as usize] = oc;
            }
            intel.foes.push(Foe {
                o: o.idx,
                near,
                mix: [0.0; 9],
                defense: 0.0,
            });
        }
        let mut dd = f32::INFINITY;
        for (id, u) in self.units.iter() {
            if u.dead || u.kind == UnitKind::Pawn {
                continue;
            }
            let d = u.pos.dist(c);
            let worth = if u.max_hp > 0.0 {
                ai_cost(u.kind) * u.hp / u.max_hp
            } else {
                0.0
            };
            if u.faction == f {
                if d < AI_HOME {
                    intel.home.push(id);
                    if u.def().has_dmg() {
                        intel.home_value += worth;
                    }
                }
                continue;
            }
            let Some(&si) = slot.get(u.faction as usize) else {
                continue;
            };
            if si == usize::MAX {
                continue;
            }
            let e = &mut intel.foes[si];
            let w = e.near + if d < AI_HOME + 800.0 { 2.0 } else { 0.0 };
            e.mix[u.kind as usize] += worth * w;
            if u.pos.dist(castles[u.faction as usize]) < AI_HOME {
                e.defense += worth;
            }
            intel.total += worth * w;
            if d < AI_HOME && u.def().has_dmg() {
                intel.danger += worth;
                if d < dd {
                    dd = d;
                    intel.danger_at = Some(u.pos);
                }
            }
        }
        for e in intel.foes.iter_mut() {
            let towers = self.factions[e.o as usize].has[BuildingKind::Tower as usize];
            e.defense += 150.0 * (towers as f32 + 1.0);
        }
        intel
    }

    /// `aiWant(f, intel)` (04b §8): desired share per buildable fighter (warrior, lancer, archer, knight), or
    /// `None` when no such unit can be trained.
    pub fn ai_want(&self, f: FactionIdx, intel: &Intel) -> Option<[f32; 9]> {
        let fac = &self.factions[f as usize];
        let counter = fac.ai.level.def().counter;
        let types: Vec<UnitKind> = UnitKind::ALL
            .iter()
            .copied()
            .filter(|t| {
                let d = t.def();
                ai_fighter(*t)
                    && !d.siege
                    && fac.has[d.from as usize] > 0
                    && fac.age >= d.age
            })
            .collect();
        if types.is_empty() {
            return None;
        }
        let base_w = |k: UnitKind| {
            AI_BASE
                .iter()
                .find(|(b, _)| *b == k)
                .map(|(_, w)| *w)
        };
        let mut score = [0.0f32; 9];
        for &t in &types {
            let mut s = 0.0f32;
            if intel.total > 300.0 {
                for e in &intel.foes {
                    let om = &self.factions[e.o as usize].mods;
                    for k in UnitKind::ALL {
                        let m = e.mix[*k as usize];
                        if m != 0.0 {
                            s += m / intel.total * ai_edge(&fac.mods, t, om, *k);
                        }
                    }
                }
            } else {
                let all: f32 = AI_BASE.iter().map(|(_, w)| *w).sum();
                for (k, w) in AI_BASE {
                    s += w / all * ai_edge(&fac.mods, t, &fac.mods, k);
                }
            }
            score[t as usize] = base_w(t).unwrap_or(1.0) * (counter * s).exp();
        }
        let sum: f32 = types.iter().map(|t| score[*t as usize]).sum();
        let mut want = [0.0f32; 9];
        let n = types.len() as f32;
        for &t in &types {
            want[t as usize] = if sum > 0.0 && sum.is_finite() {
                0.85 * score[t as usize] / sum + 0.15 / n
            } else {
                1.0 / n
            };
        }
        Some(want)
    }
}
