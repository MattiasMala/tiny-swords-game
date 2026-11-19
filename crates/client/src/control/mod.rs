// OWNER: C4 (control) — spec 05-control-loop.md (input routing, selection, orders, placement, edge pan, zoom levels,
// keyboard shortcuts, cursor) + addenda C1-C5, 06-ui.md §3.4 / §8 (keys, shortcuts), docs/bevy-0.20-notes.md §14/§16.
//
// CONTRACT (other owners code against it — keep stable):
// * `Selection` resource: `units` (own units, or one foreign unit), `building`, `other` (node / sheep / point),
//   `groups[0..10]` (control groups 1-9 at index 1-9); `is_empty()`, `clear()`, `refs()`, `contains()`.
// * `Placement` resource: `mode` (`PlaceMode::{None, Place(kind), Move(id)}`), ghost `cell` / `ok` / `why`, wall-drag
//   `last`, keyboard-driven `kbd`, the pending click `action` (`ClickAction::{None, AttackMove, Rally, PickMove}`),
//   the pick-move keyboard highlight `pick`; `active()`, `cancel()`, `hint()` (05 §9.5 hint line).
// * `ControlAction` message: what the HUD / screens (C3) ask the world input to do (shortcut buttons, selection
//   panel, context actions, minimap right-click, Build screen, Units screen …).
// * `context_actions(...)`: the selection panel's context actions (05 §8.3, titles/descs verbatim).
// * `Markers` resource: ground order markers (05 §13) drawn here with gizmos.
// * `GraphicsQuality` resource: F2 lights level / F3 render-resolution step + the slow-frame watchdog (05 §14),
//   read by the renderers (C1/C2).
// The world cursor shape goes to `ui::UiState.world_cursor`; toasts / UI sounds through `UiState::toast/say/play`.
//
// Files: `view` (camera snapshot, projection, ground ray), `scene` (node / point tables, placement map), `pick`
// (what is under the mouse), `ctx` (JS `Ctl` functions over a SystemParam), `frame` (per-frame input: keys, camera,
// mouse), `draw` (markers, range rings, rally line, drag box), `quality` (F2/F3 + watchdog).

pub mod ctx;
pub mod draw;
pub mod frame;
pub mod pick;
pub mod quality;
pub mod scene;
pub mod view;

use crate::AppState;
use crate::mirror::Mirror;
use bevy::prelude::*;
use ts_sim::cmd::EntityRef;
use ts_sim::defs::BuildingKind;
use ts_sim::slab::{BuildingId, UnitId};

pub use ctx::{ActionId, ContextAction, context_actions};
pub use quality::GraphicsQuality;
pub use scene::SceneIndex;
pub use view::ViewInfo;

/// Camera pan speed, world px / s (`CAM_SPEED`), independent of zoom.
pub const CAM_SPEED: f32 = 1100.0;
/// Shift multiplier.
pub const CAM_MEDIUM: f32 = 2.0;
/// Ctrl multiplier (wins over Shift).
pub const CAM_FAST: f32 = 4.0;
/// Edge-scroll border (logical px).
pub const EDGE: f32 = 8.0;
/// Mouse travel (logical px) after which a left press becomes a box select.
pub const DRAG_MIN: f32 = 8.0;
/// Double-click window (s).
pub const DOUBLE_CLICK: f64 = 0.35;
/// Control group double-tap window (s).
pub const GROUP_DOUBLE: f64 = 0.4;
/// Order marker life (s).
pub const MARKER_LIFE: f32 = 0.6;

// ---------------------------------------------------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------------------------------------------------

/// `Ctl.sel` (05 §7). Any entity can be selected; only own units receive orders.
#[derive(Resource, Debug, Clone, Default)]
pub struct Selection {
    /// selected units in selection order (own units, or a single foreign unit)
    pub units: Vec<UnitId>,
    /// selected building (own or foreign)
    pub building: Option<BuildingId>,
    /// selected resource node / sheep / capture point
    pub other: Option<EntityRef>,
    /// control groups 1..9 (index 0 unused)
    pub groups: [Vec<UnitId>; 10],
    /// `lastPick {id, t}` (double click) — survives `clear`
    pub(crate) last_pick: Option<(EntityRef, f64)>,
    /// `lastGroup {n, t}` (double tap) — survives `clear`
    pub(crate) last_group: Option<(usize, f64)>,
    /// `idleI` round robin
    pub(crate) idle_i: usize,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.units.is_empty() && self.building.is_none() && self.other.is_none()
    }

    /// Deselect everything (groups and click memory are kept).
    pub fn clear(&mut self) {
        self.units.clear();
        self.building = None;
        self.other = None;
    }

    /// Every selected entity (units first).
    pub fn refs(&self) -> Vec<EntityRef> {
        let mut v: Vec<EntityRef> = self.units.iter().map(|u| EntityRef::Unit(*u)).collect();
        if let Some(b) = self.building {
            v.push(EntityRef::Building(b));
        }
        if let Some(o) = self.other {
            v.push(o);
        }
        v
    }

    pub fn contains(&self, e: EntityRef) -> bool {
        match e {
            EntityRef::Unit(u) => self.units.contains(&u),
            EntityRef::Building(b) => self.building == Some(b),
            other => self.other == Some(other),
        }
    }

    /// `list[0]`.
    pub fn first(&self) -> Option<EntityRef> {
        if let Some(b) = self.building
            && self.units.is_empty()
        {
            return Some(EntityRef::Building(b));
        }
        self.units
            .first()
            .map(|u| EntityRef::Unit(*u))
            .or(self.building.map(EntityRef::Building))
            .or(self.other)
    }

    /// Adds one entity (set semantics).
    pub(crate) fn add(&mut self, e: EntityRef) {
        match e {
            EntityRef::Unit(u) => {
                if !self.units.contains(&u) {
                    self.units.push(u);
                }
            }
            EntityRef::Building(b) => self.building = Some(b),
            other => self.other = Some(other),
        }
    }

    pub(crate) fn remove(&mut self, e: EntityRef) {
        match e {
            EntityRef::Unit(u) => self.units.retain(|x| *x != u),
            EntityRef::Building(b) => {
                if self.building == Some(b) {
                    self.building = None;
                }
            }
            other => {
                if self.other == Some(other) {
                    self.other = None;
                }
            }
        }
    }

    /// Own units of the local player among the selection (`Ctl.mine()`).
    pub fn mine(&self, mirror: &Mirror) -> Vec<UnitId> {
        let Some(me) = mirror.you else {
            return Vec::new();
        };
        self.units
            .iter()
            .copied()
            .filter(|id| mirror.view.units.get(id).is_some_and(|u| u.faction == me))
            .collect()
    }

    /// The single selected own building (`list.length == 1`).
    pub fn own_building(&self, mirror: &Mirror) -> Option<BuildingId> {
        let me = mirror.you?;
        if !self.units.is_empty() || self.other.is_some() {
            return None;
        }
        let b = self.building?;
        mirror
            .view
            .buildings
            .get(&b)
            .filter(|v| v.faction == me && !mirror.view.ghosts.contains(&b))
            .map(|_| b)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Placement / pending click
// ---------------------------------------------------------------------------------------------------------------

/// `G.placing` kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaceMode {
    #[default]
    None,
    /// new construction
    Place(BuildingKind),
    /// relocating an own building
    Move(BuildingId),
}

/// `Ctl.mode`: what the next left click does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClickAction {
    #[default]
    None,
    /// `amove` (X)
    AttackMove,
    /// `rally` (G)
    Rally,
    /// `pickmove` (V without a selected building)
    PickMove,
}

/// Placement ghost + pending click mode (05 §9, §10).
#[derive(Resource, Debug, Clone, Default)]
pub struct Placement {
    pub mode: PlaceMode,
    /// top-left footprint cell of the ghost
    pub cell: IVec2,
    /// currently placeable
    pub ok: bool,
    /// refusal for the current cell (`None` when ok)
    pub why: Option<String>,
    /// last cell placed during a palisade drag
    pub last: Option<IVec2>,
    /// ghost driven by the keyboard
    pub kbd: bool,
    /// pending left-click action
    pub action: ClickAction,
    /// pick-move keyboard highlight (`pickB`)
    pub pick: Option<BuildingId>,
    /// Enter / Space pressed while placing (consumed by the placement step)
    pub(crate) enter: bool,
    /// kind being placed or moved (for the ghost)
    pub(crate) kind: Option<BuildingKind>,
}

impl Placement {
    /// Placing a building or in a click mode (`G.placing || Ctl.mode`).
    pub fn active(&self) -> bool {
        self.placing() || self.action != ClickAction::None
    }

    /// A ghost is shown (`G.placing`).
    pub fn placing(&self) -> bool {
        self.mode != PlaceMode::None
    }

    /// Cancels placement and the pending click mode.
    pub fn cancel(&mut self) {
        self.mode = PlaceMode::None;
        self.kind = None;
        self.last = None;
        self.enter = false;
        self.action = ClickAction::None;
        self.pick = None;
    }

    /// The building kind being placed / moved.
    pub fn kind(&self) -> Option<BuildingKind> {
        match self.mode {
            PlaceMode::None => None,
            PlaceMode::Place(k) => Some(k),
            PlaceMode::Move(_) => self.kind,
        }
    }

    /// Hint line above the selection panel (05 §9.5), `None` when nothing is pending.
    pub fn hint(&self) -> Option<&'static str> {
        match (self.mode, self.action) {
            (PlaceMode::Move(_), _) => Some(
                "Clic, Invio o Spazio: sposta qui  ·  Frecce o WASD: muovi la sagoma  ·  Destro / Esc: annulla",
            ),
            (PlaceMode::Place(BuildingKind::Wall), _) => Some(
                "Clic, Invio o Spazio: piazza  ·  Maiusc + trascina: una fila  ·  Frecce o WASD: muovi la sagoma  ·  Destro / Esc: annulla",
            ),
            (PlaceMode::Place(_), _) => Some(
                "Clic, Invio o Spazio: piazza (Maiusc: più d'uno)  ·  Frecce o WASD: muovi la sagoma  ·  Destro / Esc: annulla",
            ),
            (PlaceMode::None, ClickAction::AttackMove) => {
                Some("Clic sul punto da attaccare  ·  Esc: annulla")
            }
            (PlaceMode::None, ClickAction::Rally) => {
                Some("Clic sul punto di raccolta  ·  Esc: annulla")
            }
            (PlaceMode::None, ClickAction::PickMove) => Some(
                "Clic sull'edificio da spostare, oppure Frecce (o WASD) e Invio  ·  Esc: annulla",
            ),
            (PlaceMode::None, ClickAction::None) => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Requests from the UI
// ---------------------------------------------------------------------------------------------------------------

/// What the HUD / screens ask the world input to do. Refusals become a toast + `deny`, successes the JS sound.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum ControlAction {
    /// Build screen: place a new building (`startPlacing(kind)`); keyboard = ghost driven by the arrows
    StartPlacing { kind: BuildingKind, keyboard: bool },
    /// V shortcut (`moveMode`)
    MoveMode,
    /// `Sposta` context action on a given building
    StartMoving(BuildingId),
    /// Tab shortcut (`selectArmy`)
    SelectArmy,
    /// Backspace shortcut / Units screen `Richiama` (`recall(units)`; `None` = the whole army)
    Recall(Option<Vec<UnitId>>),
    /// `.` / idle-pawn button (`nextIdle`)
    NextIdle,
    /// H (`home`)
    Home,
    /// Space (`centerOnSelection`)
    CenterSelection,
    /// alert bell / minimap / Units screen: `lookAt(px)`
    LookAt(Vec2),
    /// replace the selection (portrait click, Units screen)
    Select(Vec<EntityRef>),
    /// Shift+portrait: remove one unit from the selection
    Deselect(UnitId),
    /// minimap right click: `order(px)` with the normal right-click semantics
    OrderAt(Vec2),
    /// a context action button of the selection panel
    Act(ActionId),
    /// cancel placement / pending click
    Cancel,
}

// ---------------------------------------------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------------------------------------------

/// Order marker kinds and colours (`MARK_COLOR`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    /// `#9be37a`
    Move,
    /// `#ff6a5a`
    Attack,
    /// `#ffe27a`
    Gather,
    /// `#ffffff`
    Rally,
}

impl MarkKind {
    pub fn color(self) -> Color {
        match self {
            MarkKind::Move => Color::srgb_u8(0x9b, 0xe3, 0x7a),
            MarkKind::Attack => Color::srgb_u8(0xff, 0x6a, 0x5a),
            MarkKind::Gather => Color::srgb_u8(0xff, 0xe2, 0x7a),
            MarkKind::Rally => Color::WHITE,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Marker {
    pub kind: MarkKind,
    /// sim px
    pub pos: Vec2,
    pub t: f32,
}

/// Ground order markers (0.6 s).
#[derive(Resource, Debug, Default)]
pub struct Markers {
    pub list: Vec<Marker>,
}

impl Markers {
    /// `mark(kind, x, y)`.
    pub fn mark(&mut self, kind: MarkKind, pos: Vec2) {
        self.list.push(Marker { kind, pos, t: 0.0 });
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------------------------------------------

/// World input systems (Update, after the UI so its pointer / keyboard flags and Esc handling are current).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControlSet;

pub struct ControlPlugin;

impl Plugin for ControlPlugin {
    fn build(&self, app: &mut App) {
        draw::setup_gizmos(app);
        app.init_resource::<Selection>()
            .init_resource::<Placement>()
            .init_resource::<Markers>()
            .init_resource::<ViewInfo>()
            .init_resource::<SceneIndex>()
            .init_resource::<GraphicsQuality>()
            .init_resource::<frame::FrameInput>()
            .add_message::<ControlAction>()
            .configure_sets(
                Update,
                ControlSet
                    .after(crate::link::LinkSet)
                    .after(crate::ui::UiSet),
            )
            .add_systems(
                PostUpdate,
                view::update_view_info.after(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(OnExit(AppState::InGame), reset_match)
            .add_systems(
                Update,
                (
                    frame::global_keys,
                    quality::watch,
                    (
                        scene::refresh_scene,
                        frame::track_match,
                        frame::control_frame,
                        frame::ui_actions,
                        frame::dev_inputs,
                        frame::sync_ghost,
                        draw::draw_world,
                        draw::drag_box,
                    )
                        .chain()
                        .run_if(in_state(AppState::InGame)),
                )
                    .chain()
                    .in_set(ControlSet),
            );
    }
}

/// `Ctl.reset()` when leaving a match (groups / click memory included: a new match has new ids).
fn reset_match(
    mut sel: ResMut<Selection>,
    mut place: ResMut<Placement>,
    mut marks: ResMut<Markers>,
    mut ghost: ResMut<crate::render::sprites::Ghost>,
) {
    *sel = Selection::default();
    place.cancel();
    marks.list.clear();
    *ghost = default();
}
