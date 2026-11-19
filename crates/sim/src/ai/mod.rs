// OWNER: S5 (ai) — see docs/spec/04b-ai.md §1-§3 (levels, constants, AI state: defs::AI_LEVELS / AI_PLAN /
// AI_BASE / AI_HOME), §4 (plan + skip rule), §5 (aiBuild + findSpot with Fisher–Yates), §6 (pawn allocation
// aiJobMix/aiAssignPawns, houses/camps/rebuild/farms, factories, training, aiNextUnit, market), §7 (aiIntel),
// §8 (aiWant/aiPower/aiEdge), §9 (waves: aiSpare, chooseTarget, aiLaunch, aiMarch, aiDefend, aiCapture, strays),
// §11 (full tick order), §12 + addenda A1 (takeover fast-forward), A4, A5. Also sim/examples/simbench.rs and
// sim/tests/ai_*.rs.
//
// The AI acts only through `World::exec_command` (place/train/research/trade), `order_units` and `set_order` on
// its own pawns, and never reads fog (`explored()` is true for AI factions). All randomness: `World.rng`.
// Every faction carries an `AiState` (used only while `human == false`), so takeover / hand-back is toggling
// `Faction.human` while the state is kept (faction.rs `takeover` / `reclaim`).
//
// Submodules: `util` (freeBuilding, queued, nearestEnemyFaction, aiWorth, findSpot), `intel` (aiIntel, aiPower,
// aiEdge, aiWant), `econ` (pawns, aiBuild, plan, tech step, factories, training, aiNextUnit, market), `war`
// (aiSpare, chooseTarget, aiLaunch, aiMarch, aiDefend, aiCapture, strays).

pub mod econ;
pub mod intel;
pub mod util;
pub mod war;

#[cfg(test)]
mod tests;

use crate::World;
use crate::defs::*;
use crate::rng::Rng;
use crate::slab::UnitId;
use serde::{Deserialize, Serialize};

/// Phase of a running attack wave.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WavePhase {
    /// 'march'
    March,
    /// 'assault'
    Assault,
}

/// `ai.wave` (factions.js:739).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiWave {
    /// member unit ids (JS Set; keep insertion order, no duplicates)
    pub ids: Vec<UnitId>,
    pub target: FactionIdx,
    /// value at launch (grows with reinforcements)
    pub start: f32,
    pub phase: WavePhase,
    /// game time of the last leg order
    pub leg_t: f32,
    /// launch time (written, never read in JS)
    pub t0: f32,
}

/// `f.ai` (04b §3). Timers are in game seconds; `wave_t` is decremented by `level.think` inside think.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiState {
    pub level: AiLevel,
    /// index into `AI_PLAN` of the next plan item
    pub step: u16,
    /// countdown to the next think
    pub think_t: f32,
    /// countdown to the next wave
    pub wave_t: f32,
    /// minimum squad size to launch (+2 per launch, capped by `wave_need_cap`)
    pub wave_need: u16,
    /// kingdom last chosen as wave target
    pub target: Option<FactionIdx>,
    /// pawn rebalancing countdown
    pub shuffle_t: f32,
    /// desired army share per `UnitKind` (output of aiWant), `None` without barracks
    pub mix: Option<[f32; 9]>,
    pub wave: Option<AiWave>,
    /// game time when the wave countdown expired (waiting for power)
    pub due_t: Option<f32>,
    /// last game time the base was in danger
    pub attacked_t: f32,
    /// last game time the preferred unit was affordable
    pub lack_t: f32,
    /// absolute game time before which no capture squad is sent
    pub capture_t: f32,
}

impl AiState {
    /// `makeFaction` AI block: `thinkT = rand(1,3)`, `waveT = rand(first)` [RNG, in this order].
    pub fn new(level: AiLevel, rng: &mut Rng) -> AiState {
        let l = level.def();
        let think_t = rng.range(AI_THINK_START.0, AI_THINK_START.1);
        let wave_t = rng.range(l.first.0, l.first.1);
        AiState {
            level,
            step: 0,
            think_t,
            wave_t,
            wave_need: AI_WAVE_NEED_START,
            target: None,
            shuffle_t: AI_SHUFFLE_START,
            mix: None,
            wave: None,
            due_t: None,
            attacked_t: -999.0,
            lack_t: 0.0,
            capture_t: AI_CAPTURE_START,
        }
    }
}

impl World {
    /// `aiThink(f, level.think)` (04b §11): called by `update_faction` when `ai.think_t` expires (already reset).
    /// Runs only for AI-driven kingdoms (`human == false`); `dt` = the level period, not the elapsed time.
    pub fn ai_tick(&mut self, f: FactionIdx) {
        let Some(fac) = self.factions.get(f as usize) else {
            return;
        };
        if fac.human || !fac.alive || self.ai_castle(f).is_none() {
            return;
        }
        let dt = fac.ai.level.def().think;
        // 1. idle pawns to work
        self.ai_assign_pawns(f, dt);
        // 2. plan, houses, camps, rebuild, farms, tech step (`open` counted before anything is placed)
        let open: u32 = self.factions[f as usize].sites.iter().map(|v| *v as u32).sum();
        let step_cost = self.ai_plan(f, open);
        // 3. intel (once per think) + desired army
        let intel = self.ai_intel(f);
        let mix = self.ai_want(f, &intel);
        self.factions[f as usize].ai.mix = mix;
        // 4. defense
        self.ai_defend(f, &intel, step_cost);
        // 5. extra factories
        self.ai_factories(f, open);
        // 6. training
        self.ai_train(f, &intel, step_cost);
        // 7. market
        self.ai_market(f);
        // 8. waves
        self.factions[f as usize].ai.wave_t -= dt;
        let (has_wave, wave_t) = {
            let ai = &self.factions[f as usize].ai;
            (ai.wave.is_some(), ai.wave_t)
        };
        if has_wave {
            self.ai_march(f, &intel);
        } else if wave_t <= 0.0 && intel.danger == 0.0 {
            let now = self.time;
            let ai = &mut self.factions[f as usize].ai;
            if ai.due_t.is_none() {
                ai.due_t = Some(now);
            }
            self.ai_launch(f, &intel);
        }
        // 9. capture squads (no-op while a wave exists)
        self.ai_capture(f, &intel);
        // 10. strays home
        self.ai_strays(f);
    }

    /// AI takeover procedure (04b addenda A1): forget wave/dueT/target, `think_t = rand(1,3)`,
    /// `wave_t = max(wave_t, rand(gap))`, `capture_t = max(capture_t, time + 60)`, fast-forward `step` past what
    /// the kingdom already has (census must be fresh: `takeover` runs it right before).
    pub fn ai_reset_for_takeover(&mut self, f: FactionIdx) {
        let time = self.time;
        let think = self.rng.range(AI_THINK_START.0, AI_THINK_START.1);
        let level = self.factions[f as usize].ai.level.def();
        let gap = self.rng.range(level.gap.0, level.gap.1);
        let step = self.ai_plan_fast_forward(f);
        let ai = &mut self.factions[f as usize].ai;
        ai.wave = None;
        ai.due_t = None;
        ai.target = None;
        ai.think_t = think;
        ai.wave_t = ai.wave_t.max(gap);
        ai.capture_t = ai.capture_t.max(time + AI_CAPTURE_START);
        ai.step = step;
    }

    /// Plan fast-forward of addenda A1: index of the first plan step whose building is not yet owned in
    /// sufficient number (occurrences of that kind in `AI_PLAN[0..=i]` vs `has + sites`; farms/towers also
    /// skipped at the level cap) or whose tech is neither researched nor in progress.
    pub fn ai_plan_fast_forward(&self, f: FactionIdx) -> u16 {
        let fac = &self.factions[f as usize];
        let l = fac.ai.level.def();
        let mut step = 0usize;
        while step < AI_PLAN.len() {
            match AI_PLAN[step] {
                PlanStep::Build(k) => {
                    let need = AI_PLAN[..=step]
                        .iter()
                        .filter(|s| **s == PlanStep::Build(k))
                        .count() as u16;
                    let owned = fac.has[k as usize] + fac.sites[k as usize];
                    let cap = match k {
                        BuildingKind::Farm => Some(l.farms),
                        BuildingKind::Tower => Some(l.towers),
                        _ => None,
                    };
                    if cap.is_some_and(|c| owned >= c) || owned >= need {
                        step += 1;
                        continue;
                    }
                    break;
                }
                PlanStep::Tech(t) => {
                    if fac.techs.has(t) || fac.busy.has(t) {
                        step += 1;
                        continue;
                    }
                    break;
                }
            }
        }
        step as u16
    }
}
