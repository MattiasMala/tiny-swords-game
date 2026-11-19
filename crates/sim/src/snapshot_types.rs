//! Snapshot data types (per-recipient, fog-filtered) and the client-side mirror. OWNER: foundation (types);
//! builders and `ClientMirror::apply` live in `snapshot.rs` (S6).
//!
//! Spec 08 §A.7 (what the old host sent), §R6 (interest management) and 04a §16 / addenda E7 (what the UI needs).
//! Policy: one `Snapshot` per player per tick (20 Hz), built against what this recipient already received
//! (`snapshot::ViewerCache`). A snapshot with `full = true` resets the mirror (match start, (re)join).
//! * Units: every tick, row deltas: `units` = visible units that are new or changed (anim time excluded),
//!   `unit_moves` = known units that only moved (quarter-pixel deltas), `units_gone` = units the recipient knew
//!   that died or left its vision. Own units always (sheltered pawns too).
//! * Sheep and pickups: row deltas like units (`sheep` / `sheep_moves` / `sheep_gone`, `pickups` /
//!   `pickups_gone`).
//! * Projectiles: the complete visible list every tick (short-lived).
//! * Buildings: only new/changed ones (`buildings`), `buildings_hidden` = left the recipient's vision (the client
//!   keeps them as last-seen ghosts, 05 addenda C3), `buildings_gone` = known to be gone (spot seen empty).
//! * Resource nodes / capture points: only changed ones (seen changes; fogged spots keep the last-known state).
//! * Fog: newly explored cells (full grid in a full snapshot). Economy: own kingdom only, when it changes.
//! * Public kingdom list: only when it changes (empty = unchanged; it always has one row per kingdom).
//!
//! Wire size: positions/timers are quantized (`wire::*`: quarter pixel, 1/100 s) and sent as postcard varints;
//! the builder quantizes the in-memory values the same way so solo (no encoding) and network agree exactly.

use crate::cmd::QueueWhat;
use crate::defs::{
    Act, AiLevel, AnimKind, BuildingKind, Dir5, FactionIdx, KingdomColor, Mods, PawnTool,
    PointKind, Res, UnitKind,
};
use crate::events::GameEvent;
use crate::math::BitGrid;
use crate::slab::{BuildingId, NodeId, PickupId, PointId, ProjId, SheepId, UnitId};
use crate::world::{MatchOutcome, SheepState};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// `UnitView.flags` bits.
pub const UF_FLIP: u8 = 1;
/// hurt flash (`hurt_t > 0`)
pub const UF_HURT: u8 = 2;
/// sheltered inside a building (own units only)
pub const UF_INSIDE: u8 = 4;
/// unit is moving (client interpolation hint)
pub const UF_MOVING: u8 = 8;

/// One unit as seen by the recipient.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitView {
    pub id: UnitId,
    pub kind: UnitKind,
    pub faction: FactionIdx,
    #[serde(with = "wire::q4")]
    pub x: f32,
    #[serde(with = "wire::q4")]
    pub y: f32,
    /// `ceil(hp)`
    pub hp: u16,
    pub max_hp: u16,
    pub anim: AnimKind,
    pub dir: Dir5,
    /// sent when the anim changes / restarts; the client keeps advancing it locally
    #[serde(with = "wire::q100")]
    pub anim_t: f32,
    pub flags: u8,
    /// pawn adornments: carried load (else tool) for the sheet name
    pub carry: Option<Res>,
    pub tool: Option<PawnTool>,
    /// activity (`ACT_LABEL`), own units only meaningful
    pub act: Act,
    /// pawn job (Workers screen)
    pub job: Option<Res>,
    /// `floor(faith)` (monks)
    pub faith: u8,
}

/// A known unit whose only change since the last snapshot is its position: `x += dx / 4`, `y += dy / 4`
/// (quarter pixels, relative to the last value sent to this recipient — a few bytes per marching unit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitMove {
    pub id: UnitId,
    pub dx: i32,
    pub dy: i32,
}

/// A known sheep whose only change is its position (quarter pixels, like [`UnitMove`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheepMove {
    pub id: SheepId,
    pub dx: i32,
    pub dy: i32,
}

/// Queue entry of an own building (`[code, t, total]`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueueView {
    pub what: QueueWhat,
    #[serde(with = "wire::q100")]
    pub t: f32,
    #[serde(with = "wire::q100")]
    pub total: f32,
}

/// Rally point of an own building.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RallyView {
    #[serde(with = "wire::q4")]
    pub x: f32,
    #[serde(with = "wire::q4")]
    pub y: f32,
    /// target entity id (node / sheep / building) for display
    pub target: Option<crate::cmd::EntityRef>,
}

/// A building (visible now, or own).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BuildingView {
    pub id: BuildingId,
    pub kind: BuildingKind,
    pub faction: FactionIdx,
    pub cx: i32,
    pub cy: i32,
    pub variant: u8,
    pub hp: u16,
    pub max_hp: u16,
    pub done: bool,
    /// construction seconds (re-sent at most every 0.5 s while only timers change)
    #[serde(with = "wire::q100")]
    pub progress: f32,
    /// own buildings only (empty otherwise); queue timers re-sent at most every 0.5 s (client advances them)
    pub queue: Vec<QueueView>,
    /// head-of-queue speed multiplier (`train_speed`) so the client advances the bar correctly (08 addenda N2)
    #[serde(with = "wire::q100")]
    pub train_speed: f32,
    pub rally: Option<RallyView>,
    /// own pawns sheltered inside (indicator `×n`)
    pub sheltered: u8,
}

/// Changed amount of a resource node (`NodeId` = index in the generated order).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeDelta {
    pub id: NodeId,
    /// wood / gold / stone left (tree with 0 = stump)
    #[serde(with = "wire::q10")]
    pub amount: f32,
}

/// Capture point state (sent when it changes and the recipient can see it, or it concerns the recipient).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointView {
    pub id: PointId,
    pub kind: PointKind,
    pub owner: Option<FactionIdx>,
    pub by: Option<FactionIdx>,
    #[serde(with = "wire::q100")]
    pub t: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheepView {
    pub id: SheepId,
    #[serde(with = "wire::q4")]
    pub x: f32,
    #[serde(with = "wire::q4")]
    pub y: f32,
    pub state: SheepState,
    pub flip: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjView {
    pub id: ProjId,
    #[serde(with = "wire::q4")]
    pub x: f32,
    #[serde(with = "wire::q4")]
    pub y: f32,
    #[serde(with = "wire::q100")]
    pub ang: f32,
    pub faction: FactionIdx,
    pub rock: bool,
    /// rock arc height (visual) `sin(k*π)*150`
    #[serde(with = "wire::q4")]
    pub z: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PickupView {
    pub id: PickupId,
    pub res: Res,
    #[serde(with = "wire::q4")]
    pub x: f32,
    #[serde(with = "wire::q4")]
    pub y: f32,
    /// visual hop height
    #[serde(with = "wire::q4")]
    pub hop: f32,
    /// `life < 5`: blink
    pub fading: bool,
    pub chest: bool,
}

/// Who controls a kingdom (top bar / lobby).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Control {
    /// player connected
    Human,
    /// AI kingdom from the start
    #[default]
    Ai,
    /// human-owned slot, player disconnected (AI drives it until reclaim)
    Disconnected,
}

/// Public per-kingdom info (everyone sees it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublicFaction {
    pub idx: FactionIdx,
    pub color: KingdomColor,
    pub alive: bool,
    pub control: Control,
    pub player_name: Option<String>,
    pub kills: u32,
    pub razed: u32,
    pub age: u8,
}

/// Own kingdom economy (04a §16 + E7; everything the HUD / screens need).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EconomyView {
    /// `floor(res)` in Res order
    pub res: [u32; 4],
    pub pawn_cap: u16,
    pub troop_cap: u16,
    pub pawn_n: u16,
    pub troop_n: u16,
    pub age: u8,
    /// researched bitmask (TechSet)
    pub techs: u32,
    /// researching bitmask
    pub busy: u32,
    /// not sent on the wire: `ClientMirror::apply` rebuilds it with `Mods::from_techs(techs)` (as the JS clients did)
    #[serde(skip)]
    pub mods: Mods,
    /// market pressure per Res (gold unused)
    #[serde(with = "wire::q100x4")]
    pub trades: [f32; 4],
    /// expected income per minute
    pub rates: [i32; 4],
    pub has: [u16; 14],
    pub sites: [u16; 14],
    pub kills: u32,
    pub razed: u32,
    #[serde(with = "wire::q10")]
    pub gathered: f32,
    pub level: AiLevel,
}

/// Newly explored cells, or the full grid on (re)join.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum ExploredDelta {
    #[default]
    None,
    Cells(Vec<u32>),
    Full(BitGrid),
}

/// One tick for one recipient.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u64,
    /// game time s (day phase is derived: `defs::day_phase(time)`)
    pub time: f32,
    /// recipient kingdom (`None` = spectator / eliminated player: sees everything)
    pub you: Option<FactionIdx>,
    /// first snapshot after (re)join: the mirror must reset
    pub full: bool,
    /// own economy when it changed (`None` = unchanged; always `None` for spectators)
    pub economy: Option<EconomyView>,
    /// public kingdom list when it changed (empty = unchanged)
    pub factions: Vec<PublicFaction>,
    /// visible units new or changed since the last snapshot to this recipient
    pub units: Vec<UnitView>,
    /// known units that only moved (position deltas)
    pub unit_moves: Vec<UnitMove>,
    /// units the recipient knew that died or left its vision
    pub units_gone: Vec<UnitId>,
    /// buildings new or changed since the last snapshot to this recipient (also: visible again)
    pub buildings: Vec<BuildingView>,
    /// buildings that left the recipient's vision: keep them as last-seen ghosts
    pub buildings_hidden: Vec<BuildingId>,
    /// buildings the recipient knew that are now known to be gone
    pub buildings_gone: Vec<BuildingId>,
    pub nodes: Vec<NodeDelta>,
    /// capture points that changed (all of them in a full snapshot)
    pub points: Vec<PointView>,
    /// visible sheep new or changed since the last snapshot to this recipient
    pub sheep: Vec<SheepView>,
    /// known sheep that only moved
    pub sheep_moves: Vec<SheepMove>,
    /// sheep the recipient knew that died or left its vision
    pub sheep_gone: Vec<SheepId>,
    /// complete visible list
    pub projectiles: Vec<ProjView>,
    /// visible pickups new or changed since the last snapshot to this recipient
    pub pickups: Vec<PickupView>,
    /// pickups the recipient knew that were taken, expired or left its vision
    pub pickups_gone: Vec<PickupId>,
    pub explored: ExploredDelta,
    /// presentation events for this recipient (private ones only if addressed to it)
    pub events: Vec<GameEvent>,
    /// test mode: time frozen (solo)
    pub frozen: bool,
    /// no fog for this recipient: test-mode fog off, spectator / eliminated player, match over
    pub fog_off: bool,
    pub outcome: Option<MatchOutcome>,
}

/// The client's world view, kept up to date by `apply(&Snapshot)` (implemented in snapshot.rs by S6). The static
/// map comes from `mapgen::generate(seed, setup)`; `node_amounts` starts from the generated amounts.
#[derive(Clone, Debug, Default)]
pub struct ClientMirror {
    pub you: Option<FactionIdx>,
    pub tick: u64,
    pub time: f32,
    pub economy: Option<EconomyView>,
    pub factions: Vec<PublicFaction>,
    pub units: BTreeMap<UnitId, UnitView>,
    /// visible + remembered (ghost) buildings
    pub buildings: BTreeMap<BuildingId, BuildingView>,
    /// subset of `buildings` not currently visible (last-seen ghosts)
    pub ghosts: BTreeSet<BuildingId>,
    pub node_amounts: Vec<f32>,
    /// capture points sorted by id (index = `PointId` once the first full snapshot arrived)
    pub points: Vec<PointView>,
    pub sheep: BTreeMap<SheepId, SheepView>,
    pub projectiles: Vec<ProjView>,
    /// known pickups sorted by id
    pub pickups: Vec<PickupView>,
    pub explored: BitGrid,
    /// events received and not yet consumed by the presentation layer
    pub events: Vec<GameEvent>,
    pub frozen: bool,
    pub fog_off: bool,
    pub outcome: Option<MatchOutcome>,
}

impl ClientMirror {
    /// Empty mirror for a match (node amounts from the regenerated world).
    pub fn new(you: Option<FactionIdx>, node_amounts: Vec<f32>) -> ClientMirror {
        ClientMirror {
            you,
            node_amounts,
            explored: BitGrid::for_map(),
            ..Default::default()
        }
    }

    /// Take the pending events.
    pub fn drain_events(&mut self) -> Vec<GameEvent> {
        core::mem::take(&mut self.events)
    }
}

/// Wire quantization for `f32` fields (`#[serde(with = "wire::q4")]`): the value travels as a zigzag-varint `i32`
/// of `round(v * scale)` (NaN -> 0, out of range saturates).
pub mod wire {
    #[inline]
    pub fn to_i(v: f32, scale: f32) -> i32 {
        (v * scale).round() as i32
    }

    /// Rounds `v` to the wire precision (the snapshot builder stores values already quantized).
    #[inline]
    pub fn quant(v: f32, scale: f32) -> f32 {
        let q = (v * scale).round();
        if q.is_finite() && q.abs() < i32::MAX as f32 {
            q / scale
        } else {
            0.0
        }
    }

    macro_rules! quantized {
        ($name:ident, $scale:expr, $doc:literal) => {
            #[doc = $doc]
            pub mod $name {
                use serde::{Deserialize, Deserializer, Serialize, Serializer};
                pub const SCALE: f32 = $scale;
                pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
                    super::to_i(*v, SCALE).serialize(s)
                }
                pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
                    Ok(i32::deserialize(d)? as f32 / SCALE)
                }
            }
        };
    }
    quantized!(q4, 4.0, "Quarter units (positions in px).");
    quantized!(q10, 10.0, "Tenths (resource amounts).");
    quantized!(q100, 100.0, "Hundredths (timers in s, angles in rad, multipliers).");

    /// `[f32; 4]` in hundredths.
    pub mod q100x4 {
        use serde::{Deserialize, Deserializer, Serialize, Serializer};
        pub fn serialize<S: Serializer>(v: &[f32; 4], s: S) -> Result<S::Ok, S::Error> {
            v.map(|x| super::to_i(x, 100.0)).serialize(s)
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[f32; 4], D::Error> {
            Ok(<[i32; 4]>::deserialize(d)?.map(|x| x as f32 / 100.0))
        }
    }
}
