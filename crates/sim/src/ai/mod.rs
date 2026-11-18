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
// Stub fns S5 must fill: ai_tick (STUB: no-op), ai_reset_for_takeover (MINIMAL: timers + wave reset, no plan
// fast-forward). Add private submodules under ai/ freely.
#![allow(dead_code, unused)]

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
    /// STUB: no-op.
    pub fn ai_tick(&mut self, f: FactionIdx) {}

    /// AI takeover procedure (04b addenda A1): forget wave/dueT/target, `think_t = rand(1,3)`,
    /// `wave_t = max(wave_t, rand(gap))`, `capture_t = max(capture_t, time + 60)`, fast-forward `step` past what
    /// the kingdom already has (census must be fresh). MINIMAL: everything but the plan fast-forward.
    pub fn ai_reset_for_takeover(&mut self, f: FactionIdx) {
        let time = self.time;
        let think = self.rng.range(AI_THINK_START.0, AI_THINK_START.1);
        let level = self.factions[f as usize].ai.level.def();
        let gap = self.rng.range(level.gap.0, level.gap.1);
        let ai = &mut self.factions[f as usize].ai;
        ai.wave = None;
        ai.due_t = None;
        ai.target = None;
        ai.think_t = think;
        ai.wave_t = ai.wave_t.max(gap);
        ai.capture_t = ai.capture_t.max(time + AI_CAPTURE_START);
    }
}
