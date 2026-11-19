// OWNER: C3 (ui) — spec 06-ui.md (all of it + addenda UI1-UI4), 05-control-loop.md §3/§5/§13/§15 (UI input routing,
// toasts / banners / alerts, pause, help, test mode), 08-net addenda N1/N3 (reconnecting overlay, new strings).
//
// DESIGN: the original UI is immediate mode (redrawn every frame on a canvas). It is ported as such: `paint::Paint`
// collects primitives (images with source rects, rects, outlined labels) using the exact JS formulas and scales,
// `paint::sync_pool` maps them onto pooled absolutely-positioned bevy_ui nodes (only changed nodes are touched).
// Hit testing, hover, keyboard focus navigation (§3.2 `step`) and "pointer over UI" are computed by `Paint` like
// the JS `UI` object; bevy picking is not used for the UI (all UI nodes are `Pickable::IGNORE`).
//
// CONTRACT (other owners code against it):
// * `UiState` resource: `screen` (open management screen), `help`, `paused`, `pointer_over_ui`, `keyboard_captured`,
//   `esc_consumed` (Esc handled by the UI this frame), toasts (`toast` / `say`), banners, minimap pings, the alert,
//   `world_cursor` (C4 sets the world cursor shape, the UI shows it when the pointer is not over the UI),
//   `sounds` (UI sounds, emitted as `GameEventMsg(GameEvent::Play)` for the audio module).
// * `UiSet` system set (Update): world input should run `.after(UiSet)` or simply read last frame's flags.
// * Solo pause: `UiState::sim_paused()` is true while the solo match must not advance (pause menu / help, spec 05
//   §15.1); the local room driver should honour it.
// * Dev (native): env `TS_UI_SCREEN=unita|invia|lavoratori|ricerche|mercato|costruisci|test|help|pause` opens that
//   screen once the match is running (screenshots).

pub mod hud;
pub mod kit;
pub mod menu;
pub mod minimap;
pub mod overlays;
pub mod paint;
pub mod screens;
pub mod selection;

use crate::AppState;
use crate::assets::GameAssets;
use crate::audio::AudioSettings;
use crate::control::{ControlAction, Placement, Selection};
use crate::link::{CommandResultMsg, GameEventMsg, LinkRequest, LobbyState, MatchOverMsg, NetOut};
use crate::mirror::Mirror;
use crate::render::camera::{CameraRig, RtsCamera};
use bevy::ecs::system::SystemParam;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::text::{FontCx, LayoutCx};
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow};
use kit::Ui;
use paint::{CursorKind, Io, Measure, Paint, Pool};
use std::collections::{BTreeSet, VecDeque};
use ts_sim::defs::{FactionIdx, KingdomColor, Res as SimRes, Sound};
use ts_sim::events::{GameEvent, Msg};
use ts_sim::slab::UnitId;

pub use paint::CursorKind as Cursor;

/// Management screens (`UI.panel`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Screen {
    /// `Unità` (U)
    Unita,
    /// `Dove le mando?` (Invia, from Unità)
    Invia,
    /// `Lavoratori` (L)
    Lavoratori,
    /// `RICERCHE` (I)
    Ricerche,
    /// `Mercato` (K)
    Mercato,
    /// `COSTRUISCI` (C)
    Costruisci,
    /// `Modalità test` (F4, solo)
    Test,
}

/// A queued announcement (`G.banners`).
#[derive(Clone, Debug)]
pub struct Banner {
    pub text: String,
    pub color: KingdomColor,
    pub t: f32,
    pub dur: f32,
}

/// Invia state (`UI.send`).
#[derive(Clone, Debug, Default)]
pub struct SendState {
    pub ids: Vec<UnitId>,
    pub x: f32,
    pub y: f32,
    pub kbd: bool,
    pub go: bool,
}

/// End-of-match result of the local kingdom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchResult {
    Win,
    Lose,
}

/// UI state shared with the other client modules (see the module header).
#[derive(Resource, Debug)]
pub struct UiState {
    /// open management screen (`UI.panel`)
    pub screen: Option<Screen>,
    /// help overlay (`UI.help`)
    pub help: bool,
    /// pause / network menu (`G.paused`)
    pub paused: bool,
    /// the pointer is over some UI (world clicks / wheel must be ignored)
    pub pointer_over_ui: bool,
    /// a modal UI owns the keyboard (menu, help, pause, game over, management screen): no game keys
    pub keyboard_captured: bool,
    /// Escape was consumed by the UI this frame
    pub esc_consumed: bool,
    pub toast_text: String,
    /// toast remaining time (s), 2 s per toast
    pub toast_t: f32,
    pub banners: VecDeque<Banner>,
    /// attack pings on the minimap `(x px, y px, t)`
    pub pings: Vec<(f32, f32, f32)>,
    /// last alert point (px) and the game time it happened
    pub alert_at: Option<Vec2>,
    pub alert_t: f32,
    /// cursor shape wanted by the world input (C4)
    pub world_cursor: CursorKind,
    /// watching after defeat (network)
    pub spectate: bool,
    pub result: Option<MatchResult>,
    /// countdown to the game-over screen (2.5 s)
    pub end_t: f32,
    /// game time when the result happened
    pub end_time: f32,
    /// game-over screen shown (`G.state === 'over'`)
    pub over: bool,
    /// UI sounds to play (drained every frame into `GameEventMsg::Play`)
    pub sounds: Vec<Sound>,
    // ---- screen state ----
    pub fresh: bool,
    pub help_fresh: bool,
    pub marks: BTreeSet<UnitId>,
    pub slide: Option<SimRes>,
    pub sent: Option<(SimRes, u16, f64)>,
    pub send: Option<SendState>,
    pub map_drag: bool,
    /// lobby: INIZIA pressed (`Avvio della partita…`)
    pub starting: bool,
    /// colour of the local kingdom (kept after elimination)
    pub player_color: Option<KingdomColor>,
    pub player_idx: Option<FactionIdx>,
    /// `Mirror::generation` the state belongs to
    pub generation: u32,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            screen: None,
            help: false,
            paused: false,
            pointer_over_ui: false,
            keyboard_captured: false,
            esc_consumed: false,
            toast_text: String::new(),
            toast_t: 0.0,
            banners: VecDeque::new(),
            pings: Vec::new(),
            alert_at: None,
            alert_t: -1000.0,
            world_cursor: CursorKind::Arrow,
            spectate: false,
            result: None,
            end_t: 0.0,
            end_time: 0.0,
            over: false,
            sounds: Vec::new(),
            fresh: false,
            help_fresh: false,
            marks: BTreeSet::new(),
            slide: None,
            sent: None,
            send: None,
            map_drag: false,
            starting: false,
            player_color: None,
            player_idx: None,
            generation: u32::MAX,
        }
    }
}

impl UiState {
    /// `G.toast(text)`: single slot, 2 s.
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast_text = text.into();
        self.toast_t = 2.0;
    }

    /// `say(why)`: toast + `deny` when there is a refusal.
    pub fn say(&mut self, why: Option<String>) {
        if let Some(w) = why {
            self.toast(w);
            self.sounds.push(Sound::Deny);
        }
    }

    /// Queues a UI sound.
    pub fn play(&mut self, s: Sound) {
        self.sounds.push(s);
    }

    /// `G.announce` (banner queue, max 4 queued; new ones dropped).
    pub fn announce(&mut self, text: String, color: KingdomColor, dur: f32) {
        if self.banners.len() < 4 {
            self.banners.push_back(Banner {
                text,
                color,
                t: 0.0,
                dur,
            });
        }
    }

    /// A modal UI is open (spec 06 §3.4 `modal`, minus the menu state which the caller knows).
    pub fn modal(&self) -> bool {
        self.over || self.paused || self.help || self.screen.is_some()
    }

    /// The solo simulation must not advance (pause menu or help open, spec 05 §15.1).
    pub fn sim_paused(&self) -> bool {
        self.paused || self.help
    }

    /// `UI.open(name)`: toggle a management screen (also cancels placement / modes, done by the frame).
    pub fn open(&mut self, s: Screen) {
        self.screen = if self.screen == Some(s) { None } else { Some(s) };
        self.fresh = true;
        self.slide = None;
        self.marks.clear();
    }

    fn reset_match(&mut self) {
        self.screen = None;
        self.help = false;
        self.paused = false;
        self.toast_t = 0.0;
        self.banners.clear();
        self.pings.clear();
        self.alert_at = None;
        self.alert_t = -1000.0;
        self.spectate = false;
        self.result = None;
        self.end_t = 0.0;
        self.over = false;
        self.marks.clear();
        self.send = None;
        self.slide = None;
        self.sent = None;
        self.map_drag = false;
        self.starting = false;
    }
}

/// UI systems (Update).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UiSet;

/// Procedural textures used by the UI (rings, discs, minimap layers).
#[derive(Resource, Default)]
pub struct UiGfx {
    /// ring textures with stroke/radius ratios `RING_RATIOS`
    pub rings: Vec<Handle<Image>>,
    pub disc: Handle<Image>,
    pub minimap: minimap::MiniMapGfx,
}

pub const RING_RATIOS: [f32; 5] = [0.06, 0.12, 0.2, 0.3, 0.45];

impl UiGfx {
    /// Ring texture whose stroke / radius ratio is closest to `ratio`.
    pub fn ring(&self, ratio: f32) -> Handle<Image> {
        let mut best = 0;
        for (i, r) in RING_RATIOS.iter().enumerate() {
            if (r - ratio).abs() < (RING_RATIOS[best] - ratio).abs() {
                best = i;
            }
        }
        self.rings.get(best).cloned().unwrap_or_default()
    }
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiState>()
            .init_resource::<Paint>()
            .init_resource::<Pool>()
            .init_resource::<UiGfx>()
            .init_resource::<minimap::FogCache>()
            .configure_sets(Update, UiSet.after(crate::link::LinkSet))
            .add_systems(Startup, setup_gfx)
            .add_systems(
                Update,
                (
                    track_match,
                    consume_events,
                    minimap::update_layers,
                    ui_frame,
                    paint::sync_pool,
                    emit_sounds,
                    apply_cursor,
                )
                    .chain()
                    .in_set(UiSet),
            );
    }
}

fn setup_gfx(
    mut gfx: ResMut<UiGfx>,
    mut images: ResMut<Assets<Image>>,
    mut pool: ResMut<Pool>,
    ga: Res<GameAssets>,
) {
    pool.font = ga.font.clone();
    gfx.rings = RING_RATIOS
        .iter()
        .map(|r| images.add(minimap::circle_image(128, Some(*r))))
        .collect();
    gfx.disc = images.add(minimap::circle_image(64, None));
}

// ---------------------------------------------------------------------------------------------- shared params

/// Everything the UI reads / writes besides the painter.
#[derive(SystemParam)]
pub struct Env<'w, 's> {
    pub state: Res<'w, State<AppState>>,
    pub mirror: Res<'w, Mirror>,
    pub lobby: ResMut<'w, LobbyState>,
    pub out: ResMut<'w, NetOut>,
    pub req: MessageWriter<'w, LinkRequest>,
    pub audio: ResMut<'w, AudioSettings>,
    pub rig: ResMut<'w, CameraRig>,
    pub sel: ResMut<'w, Selection>,
    pub place: ResMut<'w, Placement>,
    pub ctl: MessageWriter<'w, ControlAction>,
    pub cams: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<RtsCamera>>,
    pub gfx: Res<'w, UiGfx>,
}

impl Env<'_, '_> {
    /// Own faction index.
    pub fn you(&self) -> Option<FactionIdx> {
        self.mirror.you
    }

    /// Colour of faction `f`.
    pub fn color_of(&self, f: FactionIdx) -> KingdomColor {
        self.mirror
            .colors
            .get(f as usize)
            .copied()
            .unwrap_or(KingdomColor::Blue)
    }

    /// Solo match (local room).
    pub fn solo(&self) -> bool {
        !self.lobby.remote
    }

    /// Sends a command (refusals come back as `CommandResultMsg` → toast + deny).
    pub fn command(&mut self, c: ts_sim::cmd::Command) {
        self.out.command(c);
    }

    /// Screen position of a sim point (px), if the camera exists.
    pub fn to_screen(&self, x: f32, y: f32) -> Option<Vec2> {
        let (cam, tf) = self.cams.single().ok()?;
        crate::render::camera::px_to_screen(cam, tf, self.mirror.map(), x, y, 0.0)
    }
}

#[derive(SystemParam)]
pub struct Core<'w, 's> {
    pub paint: ResMut<'w, Paint>,
    pub fcx: ResMut<'w, FontCx>,
    pub lcx: ResMut<'w, LayoutCx>,
    pub fonts: Res<'w, Assets<Font>>,
    pub ga: Res<'w, GameAssets>,
    pub windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    pub mouse: Res<'w, ButtonInput<MouseButton>>,
    pub keys: Res<'w, ButtonInput<KeyCode>>,
    pub kev: MessageReader<'w, 's, KeyboardInput>,
    pub time: Res<'w, Time<Real>>,
    pub last_mouse: Local<'s, Option<Vec2>>,
    pub dev_screen: Local<'s, Option<u32>>,
}

/// Per-frame derived game info used by the HUD and the screens.
pub struct View {
    /// local kingdom colour (menu colour before a match)
    pub color: KingdomColor,
    pub you: Option<FactionIdx>,
    pub res: [u32; 4],
}

// ---------------------------------------------------------------------------------------------- systems

/// Resets the per-match UI state on every new match and computes win / lose (main.js:100-113, 330).
fn track_match(
    mut st: ResMut<UiState>,
    mirror: Res<Mirror>,
    state: Res<State<AppState>>,
    time: Res<Time<Real>>,
    mut sel: ResMut<Selection>,
    mut place: ResMut<Placement>,
) {
    let dt = time.delta_secs();
    if *state.get() != AppState::InGame {
        if st.generation != u32::MAX {
            st.reset_match();
            st.generation = u32::MAX;
            st.player_color = None;
            st.player_idx = None;
        }
        return;
    }
    if !mirror.active {
        return;
    }
    if st.generation != mirror.generation {
        let rejoin_spectator = mirror.you.is_none() && st.player_color.is_some();
        st.reset_match();
        st.generation = mirror.generation;
        if let Some(y) = mirror.you {
            st.player_idx = Some(y);
            st.player_color = mirror.colors.get(y as usize).copied();
        } else if rejoin_spectator {
            // re-entered as an eliminated spectator (Init{you: None})
            st.spectate = true;
        }
        if mirror.you.is_none() && !rejoin_spectator {
            st.spectate = true;
        }
    }
    // toast / banners / pings timers (`updateView`; frozen while the solo game is paused)
    let solo_paused = st.sim_paused() && mirror.you.is_some();
    if !solo_paused {
        st.toast_t = (st.toast_t - dt).max(0.0);
        if let Some(b) = st.banners.front_mut() {
            b.t += dt;
            if b.t >= b.dur {
                st.banners.pop_front();
            }
        }
        for p in st.pings.iter_mut() {
            p.2 += dt;
        }
        st.pings.retain(|p| p.2 < 4.0);
    }
    // result
    let alive: Vec<&ts_sim::snapshot_types::PublicFaction> =
        mirror.view.factions.iter().filter(|f| f.alive).collect();
    if mirror.view.factions.is_empty() {
        return;
    }
    if st.spectate && alive.len() <= 1 && !st.over {
        st.spectate = false;
        st.over = true;
        if st.result.is_none() {
            st.result = Some(MatchResult::Lose);
            st.end_time = mirror.view.time;
        }
    }
    if st.result.is_none()
        && let Some(y) = st.player_idx
    {
        let me_alive = mirror
            .view
            .factions
            .iter()
            .find(|f| f.idx == y)
            .is_some_and(|f| f.alive);
        let res = if !me_alive {
            Some(MatchResult::Lose)
        } else if alive.len() == 1 {
            Some(MatchResult::Win)
        } else {
            None
        };
        if let Some(r) = res {
            st.result = Some(r);
            st.end_t = 2.5;
            st.end_time = mirror.view.time;
            st.screen = None;
            sel.clear();
            place.cancel();
            st.play(if r == MatchResult::Win {
                Sound::Win
            } else {
                Sound::Lose
            });
        }
    }
    if st.result.is_some() && !st.over && !st.spectate {
        st.end_t -= dt;
        if st.end_t <= 0.0 {
            st.over = true;
        }
    }
}

/// Toasts, banners and alerts from the sim (`GameEventMsg`), command refusals (`CommandResultMsg`).
fn consume_events(
    mut st: ResMut<UiState>,
    mut events: MessageReader<GameEventMsg>,
    mut results: MessageReader<CommandResultMsg>,
    mut over: MessageReader<MatchOverMsg>,
    mirror: Res<Mirror>,
    rig: Res<CameraRig>,
    cams: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let me = st.player_color.unwrap_or(KingdomColor::Blue);
    let color_of = |f: FactionIdx| mirror.colors.get(f as usize).copied().unwrap_or(me);
    for GameEventMsg(e) in events.read() {
        match e {
            GameEvent::Toast { msg, .. } => {
                let t = msg.text();
                st.toast(t);
            }
            GameEvent::Say {
                msg, color, dur, ..
            } => {
                let c = color.map(color_of).unwrap_or(me);
                let d = if *dur > 0.0 { *dur } else { banner_dur(msg) };
                st.announce(msg.text(), c, d);
            }
            GameEvent::Alert { x, y, .. } => {
                // `Ctl.alert`: ping, banner + horn only when the spot is off screen
                st.pings.push((*x, *y, 0.0));
                st.alert_at = Some(Vec2::new(*x, *y));
                st.alert_t = mirror.view.time;
                let on_screen = (|| {
                    let (cam, tf) = cams.single().ok()?;
                    let w = windows.single().ok()?;
                    let p = crate::render::camera::px_to_screen(cam, tf, mirror.map(), *x, *y, 0.0)?;
                    Some(p.x >= 0.0 && p.y >= 0.0 && p.x <= w.width() && p.y <= w.height())
                })()
                .unwrap_or_else(|| (rig.cur_focus - Vec2::new(*x, *y)).length() < 600.0);
                if !on_screen {
                    st.announce("Il tuo regno è sotto attacco!".to_string(), me, 2.6);
                    st.play(Sound::Horn);
                }
            }
            _ => {}
        }
    }
    for r in results.read() {
        if let Some(err) = &r.err {
            st.say(Some(err.clone()));
        }
    }
    for _ in over.read() {
        // spectators reach the game-over screen when ≤ 1 kingdom is left (handled by track_match)
    }
}

fn banner_dur(m: &Msg) -> f32 {
    use ts_sim::events::dur;
    match m {
        Msg::DestroyEnemyCastles => dur::START,
        Msg::AgeReached { .. } => dur::AGE,
        Msg::ResearchDone(_) => dur::RESEARCH,
        Msg::BuildingDone(_) => dur::BUILDING_DONE,
        Msg::PointCaptured(_) => dur::CAPTURED,
        Msg::PointLost(_) => dur::LOST,
        Msg::Attacks { .. } => dur::ATTACKS,
        Msg::Fallen { .. } => dur::FALLEN,
        Msg::PlayerLeft { .. } | Msg::PlayerBack { .. } => dur::PLAYER,
        _ => dur::DEFAULT,
    }
}

fn emit_sounds(mut st: ResMut<UiState>, mut out: MessageWriter<GameEventMsg>, mirror: Res<Mirror>) {
    let to = mirror.you.unwrap_or(0);
    for s in st.sounds.drain(..) {
        out.write(GameEventMsg(GameEvent::Play { to, sound: s }));
    }
}

/// Native dev helper: `TS_UI_SCREEN` opens a screen once the match runs.
fn dev_screen() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("TS_UI_SCREEN").ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// The UI frame (`render()` UI part, main.js:436-457, plus the UI key routing of main.js:221-274).
fn ui_frame(mut core: Core, mut st: ResMut<UiState>, mut env: Env) {
    let Ok(window) = core.windows.single() else {
        return;
    };
    let (w, h) = (window.width(), window.height());
    let cursor = window.cursor_position();
    let mouse = cursor.unwrap_or(core.last_mouse.unwrap_or(Vec2::splat(-1.0)));
    let moved = match (*core.last_mouse, cursor) {
        (Some(a), Some(b)) => (a - b).length() > 2.0,
        (None, Some(_)) => true,
        _ => false,
    };
    if cursor.is_some() {
        *core.last_mouse = cursor;
    }
    let io = Io {
        w,
        h,
        mouse,
        has_mouse: cursor.is_some(),
        lmb: core.mouse.pressed(MouseButton::Left),
        pressed: core.mouse.just_pressed(MouseButton::Left),
        released: core.mouse.just_released(MouseButton::Left),
        rpressed: core.mouse.just_pressed(MouseButton::Right),
        moved,
        shift: core.keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        dt: core.time.delta_secs(),
        clock: core.time.elapsed_secs_f64(),
    };
    let app_state = *env.state.get();
    let in_game = app_state == AppState::InGame && env.mirror.active;
    let in_menu = matches!(app_state, AppState::Menu | AppState::Lobby);
    if app_state == AppState::Loading {
        core.paint.begin(io);
        core.paint.end();
        st.pointer_over_ui = true;
        st.keyboard_captured = true;
        return;
    }

    // dev: open a screen for screenshots
    if in_game
        && core.dev_screen.is_none()
        && env.mirror.snapshots > 20
        && let Some(name) = dev_screen()
    {
        *core.dev_screen = Some(env.mirror.generation);
        match name.as_str() {
            "unita" => st.open(Screen::Unita),
            "lavoratori" => st.open(Screen::Lavoratori),
            "ricerche" => st.open(Screen::Ricerche),
            "mercato" => st.open(Screen::Mercato),
            "costruisci" => st.open(Screen::Costruisci),
            "test" => st.open(Screen::Test),
            "help" => st.help = true,
            "pause" => st.paused = true,
            "invia" => {
                let ids: Vec<UnitId> = env
                    .mirror
                    .view
                    .units
                    .values()
                    .filter(|u| Some(u.faction) == env.mirror.you)
                    .map(|u| u.id)
                    .collect();
                st.send = Some(SendState {
                    ids,
                    x: env.rig.focus.x,
                    y: env.rig.focus.y,
                    kbd: false,
                    go: false,
                });
                st.screen = Some(Screen::Invia);
            }
            _ => {}
        }
        st.fresh = false;
    }

    // ---------------- keys (main.js:221-274, UI part)
    st.esc_consumed = false;
    let over_or_result = st.over || st.result.is_some();
    let modal = in_menu || st.over || st.paused || st.help || st.screen.is_some();
    if !modal {
        core.paint.kbd = false;
    }
    let mut navs: Vec<(f32, f32)> = Vec::new();
    let mut presses: Vec<KeyCode> = Vec::new();
    let mut confirm = false;
    for k in core.kev.read() {
        if k.state != ButtonState::Pressed {
            continue;
        }
        let nav = match k.key_code {
            KeyCode::ArrowLeft | KeyCode::KeyA => Some((-1.0, 0.0)),
            KeyCode::ArrowRight | KeyCode::KeyD => Some((1.0, 0.0)),
            KeyCode::ArrowUp | KeyCode::KeyW => Some((0.0, -1.0)),
            KeyCode::ArrowDown | KeyCode::KeyS => Some((0.0, 1.0)),
            _ => None,
        };
        if let Some(n) = nav {
            navs.push(n);
        }
        if k.repeat {
            continue;
        }
        if matches!(
            k.key_code,
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space
        ) {
            confirm = true;
        }
        presses.push(k.key_code);
    }
    if modal {
        for (dx, dy) in navs {
            navigate(&mut core.paint, &mut st, dx, dy, &env);
        }
        if confirm {
            // `UI.press()`
            if st.screen == Some(Screen::Invia)
                && let Some(s) = st.send.as_mut()
                && s.kbd
            {
                s.go = true;
            } else if core.paint.kbd && core.paint.focus.is_some() {
                core.paint.enter = true;
            }
        }
    }
    for code in presses {
        match code {
            KeyCode::F1 => {
                st.help = !st.help;
                st.help_fresh = true;
                continue;
            }
            _ => {}
        }
        if !in_game || over_or_result {
            if code == KeyCode::Escape && st.help {
                st.help = false;
                st.esc_consumed = true;
            }
            continue;
        }
        match code {
            KeyCode::Escape => {
                st.esc_consumed = true;
                if st.help {
                    st.help = false;
                } else if let Some(s) = st.screen {
                    st.screen = if s == Screen::Invia {
                        Some(Screen::Unita)
                    } else {
                        None
                    };
                    st.fresh = true;
                } else if st.paused {
                    st.paused = false;
                } else if env.place.active() || !env.sel.is_empty() {
                    // placement / mode / selection: control (C4) cancels them
                    st.esc_consumed = false;
                } else {
                    st.paused = !st.paused;
                }
                continue;
            }
            KeyCode::KeyP => {
                st.paused = !st.paused;
                continue;
            }
            _ => {}
        }
        if st.paused || st.help {
            continue;
        }
        if code == KeyCode::F4 {
            if env.solo() {
                if !st.spectate {
                    st.open(Screen::Test);
                    env.place.cancel();
                }
            } else {
                st.say(Some(ts_sim::cmd::SOLO_TEST_MODE.to_string()));
            }
            continue;
        }
        if st.spectate {
            continue;
        }
        let scr = match code {
            KeyCode::KeyU => Some(Screen::Unita),
            KeyCode::KeyL => Some(Screen::Lavoratori),
            KeyCode::KeyI => Some(Screen::Ricerche),
            KeyCode::KeyK => Some(Screen::Mercato),
            KeyCode::KeyC => Some(Screen::Costruisci),
            _ => None,
        };
        if let Some(s) = scr {
            st.open(s);
            env.place.cancel();
        }
    }

    // ---------------- draw
    let font = core.ga.font.clone();
    let Core {
        paint,
        fcx,
        lcx,
        fonts,
        ga,
        ..
    } = &mut core;
    paint.begin(io);
    let mut sounds = Vec::new();
    {
        let mut u = Ui {
            p: paint,
            m: Measure {
                fcx,
                lcx,
                fonts,
                font,
            },
            ga,
            sounds: &mut sounds,
        };
        if in_menu {
            menu::draw_menu(&mut u, &mut st, &mut env);
        } else if in_game {
            let view = make_view(&env, &st);
            u.p.locked = st.paused || st.help || st.over || st.screen.is_some();
            if st.spectate {
                hud::draw_spectator(&mut u, &mut st, &mut env, &view);
            } else {
                hud::draw_hud(&mut u, &mut st, &mut env, &view);
            }
            u.p.locked = false;
            if st.over {
                overlays::draw_game_over(&mut u, &mut st, &mut env, &view);
            } else if st.paused {
                overlays::draw_pause(&mut u, &mut st, &mut env, &view);
            } else if st.screen.is_some() && !st.spectate {
                screens::draw_manage(&mut u, &mut st, &mut env, &view);
            }
        }
        if st.help {
            u.p.locked = false;
            overlays::draw_help(&mut u, &mut st);
        }
        if in_game || app_state == AppState::Lobby {
            overlays::draw_reconnecting(&mut u, &env);
        }
        // `UI.drawFocus`
        if u.p.kbd
            && let Some(r) = u.p.focus_rect
        {
            let (rw, rh) = (r.width(), r.height());
            let k = (rw.min(rh) / 70.0).clamp(0.45, 1.0);
            u.brackets(r.min.x - 5.0, r.min.y - 5.0, rw + 10.0, rh + 10.0, k);
        }
        // cursor choice (main.js: `UI.cursor = Ctl.cursor()` when playing and not over the UI)
        if in_game && !st.paused && !st.help && !st.over && !u.p.is_hot() && u.p.cursor == CursorKind::Arrow
        {
            u.p.cursor = st.world_cursor;
        }
    }
    st.sounds.extend(sounds);
    paint.end();
    st.pointer_over_ui = paint.hot || in_menu;
    st.keyboard_captured = in_menu || st.modal();
}

/// `UI.navigate(dx, dy)`.
fn navigate(p: &mut Paint, st: &mut UiState, dx: f32, dy: f32, env: &Env) {
    if st.screen == Some(Screen::Invia)
        && let Some(s) = st.send.as_mut()
    {
        let (mw, mh) = env
            .mirror
            .map()
            .map_or((ts_sim::MAP_W as f32, ts_sim::MAP_H as f32), |m| {
                (m.w as f32, m.h as f32)
            });
        s.kbd = true;
        s.x = (s.x + dx * ts_sim::TILE * 3.0).clamp(0.0, mw * ts_sim::TILE);
        s.y = (s.y + dy * ts_sim::TILE * 3.0).clamp(0.0, mh * ts_sim::TILE);
        return;
    }
    p.kbd = true;
    p.nav = Some((dx, dy));
}

/// Fog disabled for the local view (test mode, spectator, match over).
pub fn fog_off(mirror: &Mirror) -> bool {
    mirror.view.fog_off || mirror.you.is_none() || mirror.view.outcome.is_some()
}

/// `Fog.knows(x, y)`: the cell was explored by the local player.
pub fn knows(mirror: &Mirror, x: f32, y: f32) -> bool {
    if fog_off(mirror) {
        return true;
    }
    let Some(m) = mirror.map() else {
        return false;
    };
    let c = m.cell_at(x, y);
    m.valid(c) && mirror.view.explored.get(c as usize)
}

fn make_view(env: &Env, st: &UiState) -> View {
    let you = env.mirror.you;
    let color = st
        .player_color
        .or_else(|| you.map(|y| env.color_of(y)))
        .unwrap_or(env.lobby.color);
    let res = env.mirror.view.economy.as_ref().map_or([0; 4], |e| e.res);
    View { color, you, res }
}

/// Window cursor from the UI cursor kind (§12; custom images, `s` ignored by the OS cursor).
fn apply_cursor(
    mut commands: Commands,
    paint: Res<Paint>,
    ga: Res<GameAssets>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut last: Local<Option<CursorKind>>,
) {
    let kind = paint.cursor;
    if *last == Some(kind) || ga.loaded == 0 {
        return;
    }
    let Ok(win) = windows.single() else {
        return;
    };
    let (path, hot) = match kind {
        CursorKind::Arrow => (format!("{}Cursors/Cursor_01.png", kit::UIP), (23, 18)),
        CursorKind::Hand => (format!("{}Cursors/Cursor_02.png", kit::UIP), (27, 19)),
        CursorKind::No => (format!("{}Cursors/Cursor_03.png", kit::UIP), (32, 31)),
        CursorKind::Attack => (kit::ui_icon(5), (22, 22)),
        CursorKind::Axe => ("Terrain/Resources/Tools/Tool_02.png".to_string(), (26, 26)),
        CursorKind::Pick => ("Terrain/Resources/Tools/Tool_04.png".to_string(), (26, 26)),
        CursorKind::Knife => ("Terrain/Resources/Tools/Tool_03.png".to_string(), (26, 26)),
        CursorKind::Hammer => ("Terrain/Resources/Tools/Tool_01.png".to_string(), (26, 26)),
    };
    let Some(handle) = ga.image(&path) else {
        return;
    };
    *last = Some(kind);
    commands
        .entity(win)
        .insert(CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
            handle,
            hotspot: hot,
            ..default()
        })));
}
