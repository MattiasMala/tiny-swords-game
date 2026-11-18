//! Snapshot data types (per-recipient, fog-filtered) and the client-side mirror. OWNER: foundation (types);
//! builders and `ClientMirror::apply` live in `snapshot.rs` (S6).
//!
//! Spec 08 §A.7 (what the old host sent), §R6 (interest management) and 04a §16 / addenda E7 (what the UI needs).
//! Policy: one `Snapshot` per player per tick (20 Hz). Units/sheep/projectiles/pickups: every tick, only visible
//! ones (own units always; sheltered own pawns included). Buildings: only changed/new ones + ids that disappeared;
//! the client keeps last-seen ghosts of enemy buildings (05 addenda C3). Resource nodes: deltas of amounts. Fog:
//! newly explored cells (full grid on (re)join). Economy: own faction only.

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
    pub x: f32,
    pub y: f32,
    /// `ceil(hp)`
    pub hp: u16,
    pub max_hp: u16,
    pub anim: AnimKind,
    pub dir: Dir5,
    /// sent when the anim changes; the client keeps advancing it locally
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

/// Queue entry of an own building (`[code, t, total]`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueueView {
    pub what: QueueWhat,
    pub t: f32,
    pub total: f32,
}

/// Rally point of an own building.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RallyView {
    pub x: f32,
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
    pub progress: f32,
    /// own buildings only (empty otherwise)
    pub queue: Vec<QueueView>,
    /// head-of-queue speed multiplier (`train_speed`) so the client advances the bar correctly (08 addenda N2)
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
    pub amount: f32,
}

/// Capture point state (always sent: ~36 small rows).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointView {
    pub id: PointId,
    pub kind: PointKind,
    pub owner: Option<FactionIdx>,
    pub by: Option<FactionIdx>,
    pub t: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheepView {
    pub id: SheepId,
    pub x: f32,
    pub y: f32,
    pub state: SheepState,
    pub flip: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjView {
    pub id: ProjId,
    pub x: f32,
    pub y: f32,
    pub ang: f32,
    pub faction: FactionIdx,
    pub rock: bool,
    /// rock arc height (visual) `sin(k*π)*150`
    pub z: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PickupView {
    pub id: PickupId,
    pub res: Res,
    pub x: f32,
    pub y: f32,
    /// visual hop height
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
    pub mods: Mods,
    /// market pressure per Res (gold unused)
    pub trades: [f32; 4],
    /// expected income per minute
    pub rates: [i32; 4],
    pub has: [u16; 14],
    pub sites: [u16; 14],
    pub kills: u32,
    pub razed: u32,
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
    /// recipient kingdom (`None` = spectator: sees everything)
    pub you: Option<FactionIdx>,
    /// first snapshot after (re)join: the mirror must reset
    pub full: bool,
    pub economy: Option<EconomyView>,
    pub factions: Vec<PublicFaction>,
    pub units: Vec<UnitView>,
    /// buildings new or changed since the last snapshot to this recipient
    pub buildings: Vec<BuildingView>,
    /// buildings the recipient knew that are now known to be gone
    pub buildings_gone: Vec<BuildingId>,
    pub nodes: Vec<NodeDelta>,
    pub points: Vec<PointView>,
    pub sheep: Vec<SheepView>,
    pub projectiles: Vec<ProjView>,
    pub pickups: Vec<PickupView>,
    pub explored: ExploredDelta,
    /// presentation events for this recipient (private ones only if addressed to it)
    pub events: Vec<GameEvent>,
    /// test mode flags (solo)
    pub frozen: bool,
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
    pub points: Vec<PointView>,
    pub sheep: BTreeMap<SheepId, SheepView>,
    pub projectiles: Vec<ProjView>,
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
