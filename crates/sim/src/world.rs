//! The simulation state and the per-tick update order. OWNER: foundation (shared: add only what you need, with
//! minimal surgical edits, re-reading the file right before editing).
//!
//! All entity structs live here so every module owner sees the same data model. JS string states are Rust enums;
//! the JS name is kept in a comment. Positions are sim px (foot point), `f32`; resources `f64` (04a addenda E1).
//!
//! Per-tick ORDER (`World::step`, spec 03a §13 / 03b header / 04a §7, main.js:290-307):
//! commands → `time += dt` → field budgets → factions (economy timers, census, rates, trades, AI think, explore) →
//! spatial grid → units (`update_unit` in slab order) → `separate_units` → remove dead units → buildings (queues,
//! farms, tower/castle shots) → remove dead buildings → resource nodes (tree regrow, vein regen) → sheep (+wild
//! respawn) → capture points → projectiles → pickups → fog visible sets → end check.

use crate::ai::AiState;
use crate::cmd::Command;
use crate::defs::*;
use crate::events::{GameEvent, Msg, dur};
use crate::grid::SpatialGrid;
use crate::map::{GeneratedWorld, Map};
use crate::math::{BitGrid, Rect, V2, v2};
use crate::path::FieldCache;
use crate::rng::Rng;
use crate::slab::{BuildingId, NodeId, PickupId, ProjId, SheepId, Slab, UnitId};
use crate::{TICK_DT, TILE};
use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------------------------------------------------------
// Match setup
// ------------------------------------------------------------------------------------------------------------------

/// A human player in the match (lobby result).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SetupPlayer {
    pub color: KingdomColor,
    pub name: String,
}

/// `G.start(setup)` parameters (main.js:68-95). No players = demo battle (menu background, all AI).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchSetup {
    pub players: Vec<SetupPlayer>,
    /// one difficulty for every AI kingdom
    pub level: AiLevel,
    /// solo match (in-browser): test-mode commands allowed
    pub solo: bool,
}

impl Default for MatchSetup {
    fn default() -> Self {
        MatchSetup {
            players: Vec::new(),
            level: AiLevel::Normal,
            solo: true,
        }
    }
}

impl MatchSetup {
    pub fn is_demo(&self) -> bool {
        self.players.is_empty()
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Units
// ------------------------------------------------------------------------------------------------------------------

/// Combat/heal target: a unit or a building (JS `u.target`, `order.t`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    Unit(UnitId),
    Building(BuildingId),
}

/// A gatherable thing: static node or sheep (JS `u.node`, `order.first`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GatherTarget {
    Node(NodeId),
    Sheep(SheepId),
}

/// Action state (JS `u.state`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitState {
    /// 'idle'
    #[default]
    Idle,
    /// 'move'
    Move,
    /// 'attack' — busy until the attack animation ends
    Attack,
    /// 'block' — busy for `BLOCK_TIME`
    Block,
    /// 'work' — pawn working a node/site (separation weight 0.2)
    Work,
}

/// Carried load (JS `u.carry = {kind, amount}`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Carry {
    pub res: Res,
    pub amount: f32,
}

/// `move` / `amove` order payload (entities.js:336-340).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoveOrder {
    /// own formation slot (JS `x, y`)
    pub to: V2,
    /// common group goal (JS `ax, ay`, only when the group has > 1 unit)
    pub group: Option<V2>,
    /// group pace px/s (0 = none)
    pub pace: f32,
    /// monk in an amove-with-army: becomes an escorting idle monk on arrival
    pub escort: bool,
    /// runtime: switched from the group path to the own slot path (JS `o.own`)
    pub own: bool,
    /// amove: auto-raid step (JS `o.auto`)
    pub auto: bool,
    /// amove runtime: fought on the way (JS `o.fought`)
    pub fought: bool,
}

impl MoveOrder {
    pub fn to(p: V2) -> MoveOrder {
        MoveOrder {
            to: p,
            group: None,
            pace: 0.0,
            escort: false,
            own: false,
            auto: false,
            fought: false,
        }
    }
}

/// Gather order payload (JS `gatherOrder(n) = {k:'gather', res, first, px, py}`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GatherOrder {
    pub res: Res,
    /// node chosen by the player / rally (explored not required)
    pub first: Option<GatherTarget>,
    /// search origin (JS `px, py`), moves to the last node found
    pub origin: V2,
}

/// Unit order (JS `u.order.k`, entities.js:279-288; 03a §5, 03b §3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Order {
    /// 'idle': guard post `post`; monks with `escort` follow the nearest own soldier
    Idle { post: V2, escort: bool },
    /// 'move'
    Move(MoveOrder),
    /// 'amove' (soldiers only)
    AMove(MoveOrder),
    /// 'attack'
    Attack { target: Target },
    /// 'heal' (monks)
    Heal { target: UnitId },
    /// 'explore' (scouts): current goal, unreachable cells (`bad`)
    Explore { goal: Option<V2>, bad: Vec<i32> },
    /// 'gather' (pawns)
    Gather(GatherOrder),
    /// 'build': `resume` = gather order to go back to afterwards
    Build {
        building: BuildingId,
        resume: Option<GatherOrder>,
    },
    /// 'repair'
    Repair {
        building: BuildingId,
        resume: Option<GatherOrder>,
    },
    /// 'drop': deliver the load to `building`, then idle
    Drop { building: BuildingId },
    /// 'shelter': hiding in `building` (re-picked when invalid), `back` = order to resume, `calm` = s without threat
    Shelter {
        building: Option<BuildingId>,
        back: Box<Order>,
        calm: f32,
    },
}

impl Order {
    pub fn idle_at(p: V2) -> Order {
        Order::Idle {
            post: p,
            escort: false,
        }
    }
    pub fn is_idle(&self) -> bool {
        matches!(self, Order::Idle { .. })
    }
    /// The order that defines the pawn's job (`jobOf`: shelter → back).
    pub fn job_order(&self) -> &Order {
        match self {
            Order::Shelter { back, .. } => back,
            o => o,
        }
    }
}

/// A unit (spec 03a §4 + 03b pawn fields). JS field names in comments where they differ.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub faction: FactionIdx,
    pub kind: UnitKind,
    /// feet position (JS `x, y`)
    pub pos: V2,
    pub hp: f32,
    pub max_hp: f32,
    /// current px/s (`unitSpeed`, refreshed on research; temporarily paced in `march`)
    pub speed: f32,
    pub state: UnitState,
    pub anim: AnimKind,
    /// lancer attack/block direction (and facing for client sheets)
    pub anim_dir: Dir5,
    /// time in the current animation (`animT`); the client randomises idle phases itself
    pub anim_t: f32,
    /// facing left
    pub flip: bool,
    /// `u.act` (recomputed every tick)
    pub act: Act,
    /// `u.job` = `jobOf(u)` (pawns only)
    pub job: Option<Res>,
    /// attack/heal cooldown left
    pub cd: f32,
    /// countdown to next target re-evaluation (`thinkT`)
    pub think_t: f32,
    pub target: Option<Target>,
    pub order: Order,
    /// monk heal pool
    pub faith: f32,
    /// monk: nearest enemy soldier / nearest own soldier (escort)
    pub foe: Option<UnitId>,
    pub lead: Option<UnitId>,
    // ---- pawn economy ----
    pub carry: Option<Carry>,
    pub tool: Option<PawnTool>,
    pub work_t: f32,
    pub pause_t: f32,
    /// booked node (`claim`)
    pub node: Option<GatherTarget>,
    /// unreachable nodes skipped by `findNode` (JS Set → small Vec)
    pub skip: Vec<GatherTarget>,
    /// sheltered in this building (invisible, untargetable)
    pub inside: Option<BuildingId>,
    /// has a direct player order, won't flee until calm
    pub brave: bool,
    pub fear_t: f32,
    pub look_t: f32,
    pub calm_t: f32,
    /// `estimateRates` cache: cells per leg (-1 unknown) and load per trip
    pub trip: f32,
    pub haul: f32,
    // ---- combat / movement runtime ----
    /// > 0 → hurt flash (replicated)
    pub hurt_t: f32,
    /// game time of last damage
    pub last_hurt: f32,
    /// moved this tick
    pub moving: bool,
    /// warrior attack1/attack2 alternation
    pub combo: u8,
    pub hit_done: bool,
    /// point aimed by the current attack (fallback for projectiles)
    pub aim: V2,
    pub block_t: f32,
    /// best remaining path length (stall detection)
    pub best_d: f32,
    pub stall_t: f32,
    /// locked on a breach target until this game time
    pub breach_t: f32,
    /// remaining path length px (`approach`)
    pub path_left: f32,
    /// following a path through own buildings (set by field queries)
    pub cross: bool,
    pub dead: bool,
    /// fog `explore()` change cache (`seenKey`)
    pub seen_key: u32,
}

impl Unit {
    /// Fresh unit with the faction's stats (`spawnUnit` minus RNG draws and registration).
    pub fn new(faction: FactionIdx, kind: UnitKind, pos: V2, mods: &Mods) -> Unit {
        let d = kind.def();
        Unit {
            faction,
            kind,
            pos,
            hp: mods.unit_max_hp(kind),
            max_hp: mods.unit_max_hp(kind),
            speed: mods.unit_speed(kind),
            state: UnitState::Idle,
            anim: AnimKind::Idle,
            anim_dir: Dir5::Right,
            anim_t: 0.0,
            flip: false,
            act: Act::Idle,
            job: None,
            cd: 0.0,
            think_t: 0.0,
            target: None,
            order: Order::idle_at(pos),
            faith: d.faith,
            foe: None,
            lead: None,
            carry: None,
            tool: None,
            work_t: 0.0,
            pause_t: 0.0,
            node: None,
            skip: Vec::new(),
            inside: None,
            brave: false,
            fear_t: 0.0,
            look_t: 0.0,
            calm_t: 0.0,
            trip: -1.0,
            haul: 0.0,
            hurt_t: 0.0,
            last_hurt: -99.0,
            moving: false,
            combo: 0,
            hit_done: false,
            aim: pos,
            block_t: 0.0,
            best_d: f32::INFINITY,
            stall_t: 0.0,
            breach_t: -99.0,
            path_left: 0.0,
            cross: false,
            dead: false,
            seen_key: u32::MAX,
        }
    }

    #[inline]
    pub fn def(&self) -> &'static UnitDef {
        self.kind.def()
    }

    /// Alive and on the map (not sheltered): targetable / visible.
    #[inline]
    pub fn active(&self) -> bool {
        !self.dead && self.inside.is_none()
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Buildings
// ------------------------------------------------------------------------------------------------------------------

/// Production queue entry (`{unit|tech, t, total}`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueueItem {
    pub what: crate::cmd::QueueWhat,
    pub t: f32,
    pub total: f32,
}

/// Rally target (kept only if a node/sheep or an own building).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum RallyTarget {
    Gather(GatherTarget),
    Building(BuildingId),
}

/// `b.rally = {x, y, t}`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rally {
    pub pos: V2,
    pub target: Option<RallyTarget>,
}

/// A building or construction site (spec 03b §8.3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Building {
    pub faction: FactionIdx,
    pub kind: BuildingKind,
    /// top-left cell
    pub cx: i32,
    pub cy: i32,
    /// foot point: `x = (cx + w/2)*64`, `y = (cy + h)*64 - 4`
    pub pos: V2,
    /// pixel footprint (`x0, y0, x1, y1`)
    pub rect: Rect,
    /// house sprite variant 0..2
    pub variant: u8,
    pub queue: Vec<QueueItem>,
    pub rally: Rally,
    pub hp: f32,
    pub max_hp: f32,
    pub done: bool,
    /// construction seconds accumulated
    pub progress: f32,
    /// tower/castle shot cooldown
    pub cd: f32,
    /// farm breeding timer
    pub breed_t: f32,
    /// last "no room" toast time
    pub housed_t: f32,
    /// sheltered pawns
    pub inside: Vec<UnitId>,
    /// farm sheep home `(b.x, b.y1 + 70)` (read from the farm each time: 03b addenda G1)
    pub pasture: V2,
    pub dead: bool,
    pub seen_key: u32,
}

impl Building {
    /// Fresh building (`addBuilding` minus RNG draws, blocking and registration).
    pub fn new(
        faction: FactionIdx,
        kind: BuildingKind,
        cx: i32,
        cy: i32,
        done: bool,
        mods: &Mods,
    ) -> Building {
        let d = kind.def();
        let max_hp = mods.building_max_hp(kind);
        let mut b = Building {
            faction,
            kind,
            cx,
            cy,
            pos: V2::ZERO,
            rect: Rect::default(),
            variant: 0,
            queue: Vec::new(),
            rally: Rally {
                pos: V2::ZERO,
                target: None,
            },
            hp: if done { max_hp } else { max_hp * SITE_START_HP },
            max_hp,
            done,
            progress: if done { d.work } else { 0.0 },
            cd: 0.0,
            breed_t: 0.0,
            housed_t: -99.0,
            inside: Vec::new(),
            pasture: V2::ZERO,
            dead: false,
            seen_key: u32::MAX,
        };
        b.place_at(cx, cy);
        b.rally = Rally {
            pos: v2(b.pos.x, b.rect.y1 + RALLY_DY),
            target: None,
        };
        b.pasture = v2(b.pos.x, b.rect.y1 + PASTURE_DY);
        b
    }

    /// `buildingPlace(kind, cx, cy)` geometry.
    pub fn place_at(&mut self, cx: i32, cy: i32) {
        let d = self.kind.def();
        self.cx = cx;
        self.cy = cy;
        self.rect = Rect::of_cells(cx, cy, d.w, d.h);
        self.pos = v2(
            (cx as f32 + d.w as f32 / 2.0) * TILE,
            (cy + d.h) as f32 * TILE - 4.0,
        );
    }

    #[inline]
    pub fn def(&self) -> &'static BuildingDef {
        self.kind.def()
    }

    /// Footprint cells (row-major).
    pub fn cells(&self) -> Vec<i32> {
        let d = self.def();
        let mut v = Vec::with_capacity((d.w * d.h) as usize);
        for y in self.cy..self.cy + d.h {
            for x in self.cx..self.cx + d.w {
                v.push(y * crate::map::W + x);
            }
        }
        v
    }

    /// Footprint centre in cells (zone rules).
    pub fn center_cells(&self) -> (f32, f32) {
        let d = self.def();
        (
            self.cx as f32 + d.w as f32 / 2.0,
            self.cy as f32 + d.h as f32 / 2.0,
        )
    }

    /// `rectDist(p, b)`.
    #[inline]
    pub fn dist(&self, p: V2) -> f32 {
        self.rect.dist(p)
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Factions
// ------------------------------------------------------------------------------------------------------------------

/// Where/when the kingdom was last hit at home (`f.alarm`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Alarm {
    pub pos: V2,
    pub t: f32,
}

/// Per-faction fog of war (S4 `fog.rs` fills it; 05 §12, 04a §11.1, 04b addenda A6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FogState {
    /// explored cells (`f.seen`): sim state for auto-work and scouts; mirrored to the client
    pub explored: BitGrid,
    /// currently visible cells (5 Hz); snapshot filtering uses it
    pub visible: BitGrid,
    /// `seeT` (explore period 0.25 s)
    pub see_t: f32,
    /// visible-set period timer (0.2 s)
    pub vis_t: f32,
    /// START_SIGHT disc already marked
    pub started: bool,
}

impl Default for FogState {
    fn default() -> Self {
        FogState {
            explored: BitGrid::for_map(),
            visible: BitGrid::for_map(),
            see_t: 0.0,
            vis_t: 0.0,
            started: false,
        }
    }
}

/// A kingdom (spec 04a §3). Every faction carries an `AiState`; it is used only while `human == false`, so AI
/// takeover / hand-back is just toggling `human` (04b §12, addenda A1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Faction {
    pub idx: FactionIdx,
    pub color: KingdomColor,
    /// base centre cell
    pub base: (i32, i32),
    /// controlled by a player right now (JS `human`); false = AI drives it
    pub human: bool,
    /// slot belongs to a human player (token), even while disconnected/AI-driven: exploration keeps running
    pub owned: bool,
    /// a connection is currently attached (UI: human / AI / disconnected)
    pub connected: bool,
    pub player_name: Option<String>,
    pub alive: bool,
    pub res: Stock,
    pub age: u8,
    pub techs: TechSet,
    /// techs queued/in progress in any building
    pub busy: TechSet,
    pub castle: Option<BuildingId>,
    pub mods: Mods,
    /// recent market trades per resource (index = Res; gold unused)
    pub trades: [f64; 4],
    pub pawn_n: u16,
    pub troop_n: u16,
    pub pawn_cap: u16,
    pub troop_cap: u16,
    /// completed, alive buildings per kind (census)
    pub has: [u16; 14],
    /// unfinished, alive buildings per kind (census + place)
    pub sites: [u16; 14],
    /// live units (census, up to 1 s stale)
    pub pawns: Vec<UnitId>,
    pub soldiers: Vec<UnitId>,
    /// house variant counter (`houses++ % 3`)
    pub houses: u32,
    /// expected income per minute (humans, every 1 s)
    pub rate: [i32; 4],
    pub kills: u32,
    pub razed: u32,
    pub gathered: f64,
    /// last enemy unit that hurt us and when
    pub threat: Option<UnitId>,
    pub threat_t: f32,
    /// last "under attack" alert time
    pub alert_t: f32,
    /// 1-second census timer
    pub tick_t: f32,
    /// time the last AI wave was launched against this kingdom
    pub raided_t: f32,
    pub alarm: Option<Alarm>,
    pub fog: FogState,
    /// test mode: fog removed for this player
    pub fog_off: bool,
    pub ai: AiState,
}

impl Faction {
    pub fn new(
        idx: FactionIdx,
        color: KingdomColor,
        base: (i32, i32),
        level: AiLevel,
        rng: &mut Rng,
    ) -> Faction {
        let tick_t = rng.f32();
        Faction {
            idx,
            color,
            base,
            human: false,
            owned: false,
            connected: false,
            player_name: None,
            alive: true,
            res: Stock::START,
            age: 1,
            techs: TechSet::default(),
            busy: TechSet::default(),
            castle: None,
            mods: Mods::default(),
            trades: [0.0; 4],
            pawn_n: 0,
            troop_n: 0,
            pawn_cap: 0,
            troop_cap: 0,
            has: [0; 14],
            sites: [0; 14],
            pawns: Vec::new(),
            soldiers: Vec::new(),
            houses: 0,
            rate: [0; 4],
            kills: 0,
            razed: 0,
            gathered: 0.0,
            threat: None,
            threat_t: -99.0,
            alert_t: -99.0,
            tick_t,
            raided_t: -999.0,
            alarm: None,
            fog: FogState::default(),
            fog_off: false,
            ai: AiState::new(level, rng),
        }
    }

    /// `'Regno ' + COLOR_NAME[color]`.
    pub fn name(&self) -> String {
        self.color.kingdom_name()
    }

    #[inline]
    pub fn side(&self) -> u8 {
        side_of(self.idx)
    }

    #[inline]
    pub fn has_tech(&self, t: TechId) -> bool {
        self.techs.has(t)
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Resource nodes, sheep, points, projectiles, pickups
// ------------------------------------------------------------------------------------------------------------------

/// Tree / gold vein / stone vein / big deposit (never removed; ids = `NodeId` index).
/// Stumps = trees with `amount == 0` (regrow timer running); chests are pickups.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceNode {
    pub kind: NodeKind,
    /// cell (big deposit: top-left of the 2x2)
    pub cell: i32,
    pub big: bool,
    /// entity foot point: tree `(cx, cy+22)`; gold `(cx, cy)`; stone `(cx, cy+6)`; big `(cx+32, cy+32)`
    pub pos: V2,
    /// tree sprite variant 0..3
    pub variant: u8,
    /// wood / gold / stone left
    pub amount: f32,
    pub max: f32,
    /// tree: seconds until regrow (when amount == 0)
    pub regrow: f32,
    /// vein: regeneration accumulator
    pub regen: f32,
    /// pawns booked (`users`)
    pub users: u8,
}

impl ResourceNode {
    pub fn res(&self) -> Res {
        node_res(self.kind)
    }
    /// Cells occupied (1 or the 2x2 quad).
    pub fn cells(&self, map: &Map) -> Vec<i32> {
        if self.big {
            map.quad(self.cell).to_vec()
        } else {
            vec![self.cell]
        }
    }
    /// `nodeValid`.
    pub fn valid(&self) -> bool {
        self.amount > 0.0
    }
    /// `nodeRoom`.
    pub fn room(&self) -> bool {
        self.users
            < if self.big {
                BIG_USERS
            } else {
                node_users(self.kind)
            }
    }
}

/// Sheep home (03b addenda G1: farm by id, so a moved farm moves its flock).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SheepHome {
    /// index into `World.meadows`
    Meadow(u16),
    Farm(BuildingId),
    /// farm destroyed: keep grazing the last pasture forever
    Spot(V2),
}

/// Sheep animation/behaviour state (JS `s.state`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SheepState {
    /// 'idle'
    #[default]
    Idle,
    /// 'grass'
    Grass,
    /// 'move'
    Move,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheep {
    pub pos: V2,
    pub home: SheepHome,
    pub state: SheepState,
    /// state timer
    pub t: f32,
    pub anim_t: f32,
    pub flip: bool,
    /// wander target
    pub target: V2,
    pub users: u8,
    pub dead: bool,
}

/// Capture point (spec 03b §11.2), 2x2 neutral structure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapturePoint {
    pub kind: PointKind,
    /// top-left cell
    pub cell: i32,
    /// anchor `((cx+1)*64, (cy+2)*64 - 4)`
    pub pos: V2,
    pub rect: Rect,
    /// centre in cells `(cx+1, cy+1)` (outpost build radius)
    pub gx: f32,
    pub gy: f32,
    pub owner: Option<FactionIdx>,
    /// faction currently capturing
    pub by: Option<FactionIdx>,
    /// capture progress s
    pub t: f32,
    pub seen_key: u32,
}

impl CapturePoint {
    pub fn new(kind: PointKind, cell: i32) -> CapturePoint {
        let (cx, cy) = (cell % crate::map::W, cell / crate::map::W);
        CapturePoint {
            kind,
            cell,
            pos: v2((cx + 1) as f32 * TILE, (cy + 2) as f32 * TILE - 4.0),
            rect: Rect::of_cells(cx, cy, 2, 2),
            gx: (cx + 1) as f32,
            gy: (cy + 1) as f32,
            owner: None,
            by: None,
            t: 0.0,
            seen_key: u32::MAX,
        }
    }
}

/// Who fired a projectile (its def is still read if the shooter died).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shooter {
    Unit { id: UnitId, kind: UnitKind },
    Building { id: BuildingId, kind: BuildingKind },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjKind {
    /// straight, hits the first enemy it passes (has `vx` in JS)
    Arrow,
    /// catapult rock: lands on a fixed point, splash
    Rock,
}

/// Arrow or rock (spec 03a §14 / 03b §13).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Projectile {
    pub kind: ProjKind,
    pub faction: FactionIdx,
    pub shooter: Shooter,
    /// base damage (`unitDmg` / tower dmg); bonuses applied at impact
    pub dmg: f32,
    pub pos: V2,
    /// arrow velocity px/s
    pub vel: V2,
    /// sprite angle (arrow: velocity; rock: spin)
    pub ang: f32,
    /// arrow life s
    pub life: f32,
    /// rock start / landing point
    pub from: V2,
    pub to: V2,
    /// rock flight time / duration
    pub t: f32,
    pub dur: f32,
    pub dead: bool,
}

/// Loot or chest (spec 03b §2.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pickup {
    pub res: Res,
    pub amount: f32,
    /// map chest (prop `chest`, emits `chest_open` when taken, toast)
    pub chest: bool,
    pub pos: V2,
    /// hop start / landing
    pub from: V2,
    pub to: V2,
    pub t: f32,
    /// seconds left (`f32::INFINITY` for chests)
    pub life: f32,
    /// visual hop height
    pub hop: f32,
}

/// Who dealt damage (`by` in `hurt`): unit bookkeeping (threat, alarm, retaliation), kill credit, arrow rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DamageSource {
    /// attacking unit (also the shooter of an arrow/rock); `None` for buildings / demolish / elimination
    pub unit: Option<UnitId>,
    /// attacker's faction (kill/raze credit, loot)
    pub faction: Option<FactionIdx>,
    /// damage comes from an ARROW (archer/tower/castle; not rocks): ram ×0.2, shields
    pub arrow: bool,
}

impl DamageSource {
    pub const NONE: DamageSource = DamageSource {
        unit: None,
        faction: None,
        arrow: false,
    };
}

/// Final result of a match (server end: ≤ 1 kingdom alive).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchOutcome {
    pub winner: Option<FactionIdx>,
    pub time: f32,
}

// ------------------------------------------------------------------------------------------------------------------
// World
// ------------------------------------------------------------------------------------------------------------------

/// The whole authoritative simulation state.
#[derive(Clone, Debug)]
pub struct World {
    pub seed: u32,
    pub setup: MatchSetup,
    pub tick: u64,
    /// game time s (`G.time`) = `tick * TICK_DT`; day phase = `defs::day_phase(time)`
    pub time: f32,
    pub rng: Rng,
    pub map: Map,
    /// base centres (cells), index = faction idx
    pub bases: Vec<(i32, i32)>,
    /// wild sheep meadows (px)
    pub meadows: Vec<V2>,
    pub factions: Vec<Faction>,
    pub units: Slab<UnitId, Unit>,
    pub buildings: Slab<BuildingId, Building>,
    /// trees, golds (small, big), stones (small, big) — `NodeId` = index
    pub nodes: Vec<ResourceNode>,
    pub sheep: Slab<SheepId, Sheep>,
    /// capture points — `PointId` = index
    pub points: Vec<CapturePoint>,
    pub projectiles: Slab<ProjId, Projectile>,
    /// loot + map chests
    pub pickups: Slab<PickupId, Pickup>,
    /// presentation events of the current tick (drained by the room)
    pub events: Vec<GameEvent>,
    pub grid: SpatialGrid,
    pub fields: FieldCache,
    /// wild-sheep respawn timer (`G.sheepT`)
    pub sheep_t: f32,
    /// pickup scan timer (`G.pickT`)
    pub pick_t: f32,
    /// capture tick timer (`G.pointT`)
    pub point_t: f32,
    /// test mode: time frozen (commands still execute)
    pub frozen: bool,
    pub outcome: Option<MatchOutcome>,
}

impl World {
    /// `G.start(setup)` (main.js:68-95): generate the map, create scenery (nodes, points, sheep, chests) in the
    /// canonical order, factions (colours shuffled onto bases), castles + 4 pawns + 1 warrior each.
    pub fn new(seed: u32, setup: &MatchSetup) -> World {
        let generated = crate::mapgen::generate(seed, setup);
        World::from_generated(generated, setup)
    }

    /// Same as [`World::new`] from an already generated map (tests, server reuse).
    pub fn from_generated(g: GeneratedWorld, setup: &MatchSetup) -> World {
        let mut w = World {
            seed: g.seed,
            setup: setup.clone(),
            tick: 0,
            time: 0.0,
            rng: Rng::new(g.seed as u64 ^ 0x7469_6e79_7377_6f72),
            map: g.map,
            bases: g.bases.clone(),
            meadows: g.meadows.clone(),
            factions: Vec::new(),
            units: Slab::new(),
            buildings: Slab::new(),
            nodes: Vec::new(),
            sheep: Slab::new(),
            points: Vec::new(),
            projectiles: Slab::new(),
            pickups: Slab::new(),
            events: Vec::new(),
            grid: SpatialGrid::default(),
            fields: FieldCache::new(),
            sheep_t: SHEEP_FIRST_RESPAWN,
            pick_t: 0.0,
            point_t: 0.0,
            frozen: false,
            outcome: None,
        };
        // ---- scenery, canonical order (spec 02 §8): trees, golds small+big, stones small+big, points ----
        for t in &g.trees {
            let c = w.map.center(t.cell);
            w.nodes.push(ResourceNode {
                kind: NodeKind::Tree,
                cell: t.cell,
                big: false,
                pos: v2(c.x, c.y + TREE_FOOT_DY),
                variant: t.variant,
                amount: TREE_WOOD,
                max: TREE_WOOD,
                regrow: 0.0,
                regen: 0.0,
                users: 0,
            });
        }
        let bigs_of = |k: NodeKind| {
            g.bigs
                .iter()
                .filter(move |(bk, _)| *bk == k)
                .map(|(_, d)| *d)
        };
        let golds: Vec<_> = g
            .golds
            .iter()
            .copied()
            .chain(bigs_of(NodeKind::Gold))
            .collect();
        let stones: Vec<_> = g
            .stones
            .iter()
            .copied()
            .chain(bigs_of(NodeKind::Stone))
            .collect();
        for (kind, list) in [(NodeKind::Gold, golds), (NodeKind::Stone, stones)] {
            for d in list {
                let c = w.map.center(d.cell);
                let pos = if d.big {
                    v2(c.x + 32.0, c.y + 32.0)
                } else if kind == NodeKind::Stone {
                    v2(c.x, c.y + 6.0)
                } else {
                    c
                };
                w.nodes.push(ResourceNode {
                    kind,
                    cell: d.cell,
                    big: d.big,
                    pos,
                    variant: 0,
                    amount: d.amount,
                    max: d.amount,
                    regrow: 0.0,
                    regen: 0.0,
                    users: 0,
                });
            }
        }
        for p in &g.points {
            w.points.push(CapturePoint::new(p.kind, p.cell));
        }
        // 3 sheep per meadow (main.js:76)
        for m in 0..w.meadows.len() {
            for _ in 0..SHEEP_PER_MEADOW {
                w.make_sheep(SheepHome::Meadow(m as u16));
            }
        }
        // chests → never-expiring pickups at (cx, cy + 6) (main.js:77)
        for ch in &g.chests {
            let c = w.map.center(ch.cell);
            w.drop_pickup(ch.res, v2(c.x, c.y + 6.0), TREASURE, f32::INFINITY, true);
        }
        // ---- factions: COLORS shuffled (Fisher–Yates) onto bases (05 addenda D2) ----
        let mut colors: Vec<KingdomColor> = KingdomColor::ALL.to_vec();
        w.rng.shuffle(&mut colors);
        for (i, color) in colors.into_iter().enumerate().take(w.bases.len()) {
            let mut f = Faction::new(i as FactionIdx, color, w.bases[i], setup.level, &mut w.rng);
            if let Some(p) = setup.players.iter().find(|p| p.color == color) {
                f.human = true;
                f.owned = true;
                f.connected = true;
                f.player_name = Some(p.name.clone());
            }
            w.factions.push(f);
        }
        for f in 0..w.factions.len() as FactionIdx {
            w.setup_faction(f);
        }
        if !setup.is_demo() {
            w.emit(GameEvent::Say {
                to: None,
                msg: Msg::DestroyEnemyCastles,
                color: None,
                dur: dur::START,
            });
        }
        w
    }

    /// `setupFaction(f)` (factions.js:64-69): castle done at `(base.x-2, base.y-1)`, 4 pawns + 1 warrior deployed
    /// then idled, census.
    pub fn setup_faction(&mut self, f: FactionIdx) {
        let (bx, by) = self.factions[f as usize].base;
        let castle = self.add_building(f, BuildingKind::Castle, bx - 2, by - 1, true);
        self.factions[f as usize].castle = Some(castle);
        for kind in [
            UnitKind::Pawn,
            UnitKind::Pawn,
            UnitKind::Pawn,
            UnitKind::Pawn,
            UnitKind::Warrior,
        ] {
            if let Some(u) = self.deploy(castle, kind) {
                self.idle(u);
            }
        }
        self.census(f);
    }

    /// One fixed tick (`TICK_DT`). Executes `cmds` first (results in the same order), then advances the sim
    /// unless frozen (test mode).
    pub fn step(&mut self, cmds: &[(FactionIdx, Command)]) -> Vec<Result<(), String>> {
        let results: Vec<Result<(), String>> =
            cmds.iter().map(|(f, c)| self.exec_command(*f, c)).collect();
        if self.frozen {
            return results;
        }
        self.tick += 1;
        self.time = self.tick as f32 * TICK_DT;
        self.fields.begin_tick();
        for f in 0..self.factions.len() as FactionIdx {
            self.update_faction(f);
        }
        self.grid.rebuild(&self.units);
        for id in self.units.ids() {
            if self.units.get(id).is_some_and(|u| !u.dead) {
                self.update_unit(id);
            }
        }
        self.separate_units();
        self.remove_dead_units();
        for id in self.buildings.ids() {
            if self.buildings.get(id).is_some_and(|b| !b.dead) {
                self.update_building(id);
            }
        }
        self.remove_dead_buildings();
        self.update_nodes();
        self.update_sheep();
        self.update_points();
        self.update_projectiles();
        self.update_pickups();
        self.update_fog();
        self.check_end();
        results
    }

    // ---------------------------------------------------------------------------------------------------------
    // Small shared helpers (foundation)
    // ---------------------------------------------------------------------------------------------------------

    /// Push a presentation event.
    #[inline]
    pub fn emit(&mut self, e: GameEvent) {
        self.events.push(e);
    }

    /// Take this tick's events.
    pub fn drain_events(&mut self) -> Vec<GameEvent> {
        core::mem::take(&mut self.events)
    }

    #[inline]
    pub fn faction(&self, f: FactionIdx) -> &Faction {
        &self.factions[f as usize]
    }

    #[inline]
    pub fn faction_mut(&mut self, f: FactionIdx) -> &mut Faction {
        &mut self.factions[f as usize]
    }

    /// Unit exists and is not dead.
    #[inline]
    pub fn unit_alive(&self, id: UnitId) -> bool {
        self.units.get(id).is_some_and(|u| !u.dead)
    }

    /// Building exists and is not dead.
    #[inline]
    pub fn building_alive(&self, id: BuildingId) -> bool {
        self.buildings.get(id).is_some_and(|b| !b.dead)
    }

    /// Target exists and is alive (JS `t && !t.dead`).
    pub fn target_alive(&self, t: Target) -> bool {
        match t {
            Target::Unit(u) => self.unit_alive(u),
            Target::Building(b) => self.building_alive(b),
        }
    }

    /// Faction owning a target.
    pub fn target_faction(&self, t: Target) -> Option<FactionIdx> {
        match t {
            Target::Unit(u) => self.units.get(u).map(|u| u.faction),
            Target::Building(b) => self.buildings.get(b).map(|b| b.faction),
        }
    }

    /// Anchor position of a target (unit feet / building foot point).
    pub fn target_pos(&self, t: Target) -> Option<V2> {
        match t {
            Target::Unit(u) => self.units.get(u).map(|u| u.pos),
            Target::Building(b) => self.buildings.get(b).map(|b| b.pos),
        }
    }

    /// `distTo(p, t)`: rectangle distance for buildings, Euclidean for units (∞ if gone).
    pub fn dist_to(&self, p: V2, t: Target) -> f32 {
        match t {
            Target::Unit(u) => self
                .units
                .get(u)
                .map(|u| u.pos.dist(p))
                .unwrap_or(f32::INFINITY),
            Target::Building(b) => self
                .buildings
                .get(b)
                .map(|b| b.dist(p))
                .unwrap_or(f32::INFINITY),
        }
    }

    /// Position of a gather target (`None` if the sheep is gone).
    pub fn gather_pos(&self, g: GatherTarget) -> Option<V2> {
        match g {
            GatherTarget::Node(n) => self.nodes.get(n.0 as usize).map(|n| n.pos),
            GatherTarget::Sheep(s) => self.sheep.get(s).filter(|s| !s.dead).map(|s| s.pos),
        }
    }

    /// Cell index of a position (-1 outside).
    #[inline]
    pub fn cell_of(&self, p: V2) -> i32 {
        self.map.cell_of(p)
    }

    /// Kingdoms still alive.
    pub fn alive_factions(&self) -> impl Iterator<Item = &Faction> + '_ {
        self.factions.iter().filter(|f| f.alive)
    }

    /// Day phase `[0, 1)` for the lighting (spec 07 §2.2).
    pub fn day_phase(&self) -> f32 {
        day_phase(self.time)
    }
}
