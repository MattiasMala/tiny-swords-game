//! Per-frame world input (05 §5 keyboard, §6 `Ctl.update`, §9.2 `place`, §11 camera), UI requests
//! (`ControlAction`), the placement ghost for the sprite renderer and the solo pause flag for the link.
//!
//! Runs after the UI (`UiSet`): `UiState.pointer_over_ui` / `keyboard_captured` / `esc_consumed` are this frame's.
//! The UI owns F1, P, F4, U L I K C and the Escape steps help → screen → pause; this module handles the Escape
//! steps placement → click mode → selection (when the UI left the key unconsumed), M / F2 / F3 (any state) and
//! every world key of 05 §5.1.

use super::ctx::{Ctx, building_foot};
use super::quality::GraphicsQuality;
use super::{
    CAM_FAST, CAM_MEDIUM, CAM_SPEED, ClickAction, ControlAction, DRAG_MIN, EDGE, MARKER_LIFE,
    PlaceMode, Selection,
};
use crate::audio::AudioSettings;
use crate::link::SimPaused;
use crate::mirror::Mirror;
use crate::render::sprites::Ghost;
use crate::ui::UiState;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use ts_sim::TILE;
use ts_sim::cmd::EntityRef;

/// Raw mouse state kept between frames (JS `Input` + `Ctl.drag`).
#[derive(Resource, Debug, Default)]
pub struct FrameInput {
    /// window point where a potential box drag started
    pub drag: Option<Vec2>,
    /// last left / right press: window point and the ground under it (orders go where the button was pressed)
    pub down: Vec2,
    pub down_ground: Option<Vec2>,
    pub rdown: Vec2,
    pub rdown_ground: Option<Vec2>,
    /// cursor of the previous frame (`moved` test)
    pub last_mouse: Option<Vec2>,
    /// current cursor (window logical px) and the ground under it
    pub mouse: Option<Vec2>,
    pub mouse_ground: Option<Vec2>,
    /// mirror generation the per-match state belongs to
    pub generation: Option<u32>,
}

/// M (audio), F2 (lights), F3 (resolution): always, in every state (05 §5.1).
pub fn global_keys(
    mut kev: MessageReader<KeyboardInput>,
    mut audio: ResMut<AudioSettings>,
    mut q: ResMut<GraphicsQuality>,
    mut ui: ResMut<UiState>,
) {
    for k in kev.read() {
        if k.state != ButtonState::Pressed || k.repeat {
            continue;
        }
        match k.key_code {
            KeyCode::KeyM => audio.enabled = !audio.enabled,
            KeyCode::F2 => {
                let t = q.cycle_lights();
                ui.toast(t);
            }
            KeyCode::F3 => {
                let t = q.cycle_resolution();
                ui.toast(t);
            }
            _ => {}
        }
    }
}

/// New match → `Ctl.reset()`; solo pause flag for the local room.
pub fn track_match(
    mirror: Res<Mirror>,
    ui: Res<UiState>,
    lobby: Res<crate::link::LobbyState>,
    mut fi: ResMut<FrameInput>,
    mut sel: ResMut<Selection>,
    mut place: ResMut<super::Placement>,
    mut marks: ResMut<super::Markers>,
    mut paused: ResMut<SimPaused>,
) {
    if fi.generation != Some(mirror.generation) {
        fi.generation = Some(mirror.generation);
        fi.drag = None;
        // a rejoin keeps the ids (same match): keep the groups, drop the rest
        if !mirror.rejoin {
            *sel = Selection::default();
        } else {
            sel.clear();
        }
        place.cancel();
        marks.list.clear();
    }
    // solo: pause menu / help stop the simulation (05 §4.2, §15.1); a network match never stops
    paused.0 = !lobby.remote && ui.sim_paused();
}

/// Nav key direction (`NAV`).
fn nav_dir(code: KeyCode) -> Option<IVec2> {
    match code {
        KeyCode::ArrowLeft | KeyCode::KeyA => Some(IVec2::new(-1, 0)),
        KeyCode::ArrowRight | KeyCode::KeyD => Some(IVec2::new(1, 0)),
        KeyCode::ArrowUp | KeyCode::KeyW => Some(IVec2::new(0, -1)),
        KeyCode::ArrowDown | KeyCode::KeyS => Some(IVec2::new(0, 1)),
        _ => None,
    }
}

/// Zoom keys by character (Italian keyboards put + and − elsewhere): `+`, `=` on the Equal key → in, `-` → out.
fn zoom_key(k: &KeyboardInput) -> i32 {
    match &k.logical_key {
        Key::Character(c) if c.as_str() == "+" => 1,
        Key::Character(c) if c.as_str() == "=" && k.key_code == KeyCode::Equal => 1,
        Key::Character(c) if c.as_str() == "-" => -1,
        _ => match k.key_code {
            KeyCode::NumpadAdd => 1,
            KeyCode::NumpadSubtract => -1,
            _ => 0,
        },
    }
}

/// The world input frame (`handleNav` + `handleKeys` world part + `Ctl.update`).
#[allow(clippy::too_many_arguments)]
pub fn control_frame(
    mut ctx: Ctx,
    mut fi: ResMut<FrameInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut kev: MessageReader<KeyboardInput>,
    mut wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let dt = ctx.time.delta_secs().min(0.05);
    let window = windows.single().ok();
    let focused = window.is_some_and(|w| w.focused);
    let cursor = window.and_then(|w| w.cursor_position());
    let map_ground = |ctx: &Ctx, s: Vec2| ctx.view.ground_at(ctx.mirror.map(), s);
    fi.mouse = cursor;
    fi.mouse_ground = cursor.and_then(|s| map_ground(&ctx, s));
    let moved = match (fi.last_mouse, cursor) {
        (Some(a), Some(b)) => (a - b).abs().element_sum() > 2.0,
        (None, Some(_)) => true,
        _ => false,
    };
    if cursor.is_some() {
        fi.last_mouse = cursor;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let pressed = mouse.just_pressed(MouseButton::Left);
    let released = mouse.just_released(MouseButton::Left);
    let rpressed = mouse.just_pressed(MouseButton::Right);
    let lmb = mouse.pressed(MouseButton::Left);
    if let Some(c) = cursor {
        if pressed {
            fi.down = c;
            fi.down_ground = fi.mouse_ground;
        }
        if rpressed {
            fi.rdown = c;
            fi.rdown_ground = fi.mouse_ground;
        }
    }

    // ---------------- state
    let me = ctx.mirror.you;
    let alive = me.is_some_and(|y| {
        ctx.mirror
            .view
            .factions
            .iter()
            .find(|f| f.idx == y)
            .is_none_or(|f| f.alive)
    });
    let ui_over = ctx.ui.over;
    let in_play = ctx.mirror.active
        && alive
        && !ctx.ui.spectate
        && ctx.ui.result.is_none()
        && !ui_over;
    let screen_open = ctx.ui.screen.is_some();
    let overlay = ctx.ui.paused || ctx.ui.help || ui_over;
    let modal = overlay || screen_open;
    let free = !ctx.ui.pointer_over_ui;

    // ---------------- keys: collect (navs with repeats, other codes without)
    let mut navs: Vec<IVec2> = Vec::new();
    let mut hits: Vec<KeyCode> = Vec::new();
    let mut zoom = 0;
    let mut enter = false;
    for k in kev.read() {
        if k.state != ButtonState::Pressed {
            continue;
        }
        if let Some(n) = nav_dir(k.key_code) {
            navs.push(n);
        }
        if k.repeat {
            continue;
        }
        let z = zoom_key(k);
        if z != 0 {
            zoom += z;
            continue;
        }
        if matches!(
            k.key_code,
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space
        ) {
            enter = true;
        }
        hits.push(k.key_code);
    }

    ctx.refresh();
    // markers age with the presentation clock; frozen while the solo game is paused
    let solo_frozen = ctx.solo() && (ctx.ui.paused || ctx.ui.help);
    if !solo_frozen {
        for m in ctx.marks.list.iter_mut() {
            m.t += dt;
        }
        ctx.marks.list.retain(|m| m.t < MARKER_LIFE);
    }

    // ---------------- handleNav (placing / pick-move)
    let placing_nav =
        !modal && in_play && (ctx.place.placing() || ctx.place.action == ClickAction::PickMove);
    if placing_nav {
        for n in &navs {
            ctx.key_move(n.x, n.y);
        }
        if enter {
            ctx.key_confirm();
        }
    }

    // ---------------- handleKeys (world part)
    for code in hits {
        if code == KeyCode::Escape {
            if ctx.ui.esc_consumed || !in_play || modal {
                continue;
            }
            if ctx.place.placing() {
                ctx.place.mode = PlaceMode::None;
                ctx.place.last = None;
            } else if ctx.place.action != ClickAction::None {
                ctx.place.action = ClickAction::None;
                ctx.place.pick = None;
            } else if !ctx.sel.is_empty() {
                ctx.sel.clear();
            }
            continue;
        }
        if overlay || !ctx.mirror.active {
            continue;
        }
        // SHORTCUTS that do not open a screen: only without a screen
        if !screen_open && in_play {
            match code {
                KeyCode::KeyV => {
                    let why = ctx.move_mode();
                    if why.is_some() {
                        ctx.say(why);
                    }
                    continue;
                }
                KeyCode::Tab => {
                    ctx.select_army();
                    continue;
                }
                KeyCode::Backspace => {
                    ctx.recall(None);
                    continue;
                }
                _ => {}
            }
        }
        if screen_open {
            continue;
        }
        if !in_play {
            continue;
        }
        match code {
            KeyCode::Period | KeyCode::NumpadDecimal => ctx.next_idle(),
            KeyCode::KeyH => ctx.home(),
            KeyCode::Space => {
                if !ctx.place.placing() && ctx.place.action != ClickAction::PickMove {
                    ctx.center_on_selection();
                }
            }
            KeyCode::Digit1
            | KeyCode::Digit2
            | KeyCode::Digit3
            | KeyCode::Digit4
            | KeyCode::Digit5
            | KeyCode::Digit6
            | KeyCode::Digit7
            | KeyCode::Digit8
            | KeyCode::Digit9 => {
                let n = match code {
                    KeyCode::Digit1 => 1,
                    KeyCode::Digit2 => 2,
                    KeyCode::Digit3 => 3,
                    KeyCode::Digit4 => 4,
                    KeyCode::Digit5 => 5,
                    KeyCode::Digit6 => 6,
                    KeyCode::Digit7 => 7,
                    KeyCode::Digit8 => 8,
                    _ => 9,
                };
                ctx.group(n, shift);
            }
            KeyCode::KeyX | KeyCode::KeyE | KeyCode::KeyF | KeyCode::KeyG | KeyCode::Delete => {
                ctx.act_key(code);
            }
            _ => {}
        }
    }

    // ---------------- camera (05 §11.1; suspended under modal overlays, 05 addenda C2)
    if !ctx.mirror.active || overlay {
        fi.drag = None;
        return;
    }
    if zoom != 0 && !screen_open {
        // `+` / `−`: around the screen centre
        ctx.rig.zoom_step(zoom.signum(), None);
    }
    let keys_allowed =
        !screen_open && !ctx.place.placing() && ctx.place.action != ClickAction::PickMove;
    let held = |a: KeyCode, b: KeyCode| (keys.pressed(a) || keys.pressed(b)) as i32 as f32;
    let (mut mx, mut my) = if keys_allowed {
        (
            held(KeyCode::ArrowRight, KeyCode::KeyD) - held(KeyCode::ArrowLeft, KeyCode::KeyA),
            held(KeyCode::ArrowDown, KeyCode::KeyS) - held(KeyCode::ArrowUp, KeyCode::KeyW),
        )
    } else {
        (0.0, 0.0)
    };
    if let (Some(m), Some(w)) = (cursor, window)
        && fi.drag.is_none()
        && focused
    {
        if m.x < EDGE {
            mx = -1.0;
        } else if m.x > w.width() - EDGE {
            mx = 1.0;
        }
        if m.y < EDGE {
            my = -1.0;
        } else if m.y > w.height() - EDGE {
            my = 1.0;
        }
    }
    if mx != 0.0 || my != 0.0 {
        let speed = CAM_SPEED
            * if ctrl {
                CAM_FAST
            } else if shift {
                CAM_MEDIUM
            } else {
                1.0
            };
        // diagonal not normalised (×√2), like the JS
        let f = ctx.rig.focus + Vec2::new(mx, my) * speed * dt;
        ctx.rig.focus = f;
        ctx.rig.clamp();
    }
    let w_sum: f32 = wheel.read().map(|e| e.y.signum()).sum();
    if w_sum != 0.0 && free && !screen_open {
        // one step per frame, wheel up = zoom in, around the mouse
        ctx.rig.zoom_step(if w_sum > 0.0 { 1 } else { -1 }, fi.mouse_ground);
    }

    // ---------------- mouse (selection / orders / placement)
    if !in_play || screen_open {
        fi.drag = None;
        ctx.ui.world_cursor = crate::ui::Cursor::Arrow;
        return;
    }
    if ctx.place.placing() {
        place_step(&mut ctx, &mut fi, shift, free, pressed, released, rpressed, lmb, moved);
        ctx.ui.world_cursor = ctx.cursor(cursor, fi.mouse_ground);
        return;
    }
    if rpressed && free {
        let s = fi.rdown;
        let g = fi.rdown_ground;
        let trainer = ctx.sel.own_building(&ctx.mirror).filter(|b| {
            ctx.mirror
                .view
                .buildings
                .get(b)
                .is_some_and(|v| !v.kind.def().trains.is_empty())
        });
        if ctx.place.action != ClickAction::None {
            // right click cancels a pending mode, no order
            ctx.place.action = ClickAction::None;
            ctx.place.pick = None;
        } else if let Some(g) = g {
            let target = ctx.pick_at(s, Some(g));
            match trainer {
                Some(b) if ctx.mirror.view.buildings.get(&b).is_some_and(|v| v.done) => {
                    ctx.set_rally(b, g, target)
                }
                _ => ctx.order(g, target, false),
            }
        }
    }
    if pressed && free {
        let s = fi.down;
        let g = fi.down_ground;
        match ctx.place.action {
            ClickAction::AttackMove => {
                if let Some(g) = g {
                    let target = ctx.pick_at(s, Some(g));
                    ctx.order(g, target, true);
                }
                ctx.place.action = ClickAction::None;
            }
            ClickAction::Rally => {
                if let (Some(EntityRef::Building(b)), Some(g)) = (ctx.sel.first(), g) {
                    let target = ctx.pick_at(s, Some(g));
                    ctx.set_rally(b, g, target);
                }
                ctx.place.action = ClickAction::None;
            }
            ClickAction::PickMove => {
                let t = ctx.pick_at(s, g);
                let own = match t {
                    Some(EntityRef::Building(b)) => ctx
                        .mirror
                        .view
                        .buildings
                        .get(&b)
                        .is_some_and(|v| Some(v.faction) == me)
                        .then_some(b),
                    _ => None,
                };
                let why = match own {
                    Some(b) => ctx.start_moving(b, false),
                    None => Some("Clicca un tuo edificio".to_string()),
                };
                // mode stays `pickmove` on failure
                ctx.say(why);
            }
            ClickAction::None => fi.drag = Some(s),
        }
    }
    if let Some(d) = fi.drag
        && released
    {
        fi.drag = None;
        let m = cursor.unwrap_or(d);
        if m.distance(d) > DRAG_MIN {
            ctx.box_select(d, m, shift);
        } else {
            let g = fi.down_ground;
            ctx.click_select(fi.down, g, shift);
        }
    }
    if !lmb && !released {
        fi.drag = None;
    }
    ctx.ui.world_cursor = ctx.cursor(cursor, fi.mouse_ground);
}

/// `place(shift, free, lx, ly)` (05 §9.2) once per frame while a ghost is shown.
#[allow(clippy::too_many_arguments)]
fn place_step(
    ctx: &mut Ctx,
    fi: &mut FrameInput,
    shift: bool,
    free: bool,
    pressed: bool,
    released: bool,
    rpressed: bool,
    lmb: bool,
    moved: bool,
) {
    let Some(kind) = ctx.place.kind() else {
        ctx.place.cancel();
        return;
    };
    let d = kind.def();
    let click = pressed && free;
    let key = ctx.place.enter;
    let press = click || key;
    ctx.place.enter = false;
    if moved || click {
        // the mouse takes the ghost back
        ctx.place.kbd = false;
    }
    if !ctx.place.kbd {
        let p = if click { fi.down_ground } else { fi.mouse_ground };
        if let Some(p) = p {
            ctx.place.cell = IVec2::new(
                (p.x / TILE - d.w as f32 / 2.0).round() as i32,
                (p.y / TILE - d.h as f32 / 2.0).round() as i32,
            );
        }
    }
    let cell = ctx.place.cell;
    let why = ctx.place_why(cell.x, cell.y);
    ctx.place.ok = why.is_none();
    ctx.place.why = why.clone();
    if rpressed {
        // right click cancels (even over the HUD)
        ctx.place.mode = PlaceMode::None;
        ctx.place.last = None;
        return;
    }
    if press {
        let refused = why.or_else(|| ctx.place_confirm());
        if refused.is_some() {
            // ghost stays
            ctx.say(refused);
            return;
        }
        ctx.play(ts_sim::defs::Sound::Click);
        ctx.place.last = Some(cell);
        if matches!(ctx.place.mode, PlaceMode::Move(_)) || !shift {
            ctx.place.mode = PlaceMode::None;
        }
        return;
    }
    // palisade drag line (the release frame still counts)
    if kind == ts_sim::defs::BuildingKind::Wall
        && matches!(ctx.place.mode, PlaceMode::Place(_))
        && shift
        && free
        && (lmb || released)
        && let Some(last) = ctx.place.last
        && last != cell
    {
        ctx.wall_line(last, cell);
        ctx.place.last = Some(cell);
    }
    if !lmb {
        ctx.place.last = None;
    }
}

/// `ControlAction` requests from the UI.
pub fn ui_actions(mut ctx: Ctx, mut reqs: MessageReader<ControlAction>) {
    for r in reqs.read() {
        match r.clone() {
            ControlAction::StartPlacing { kind, keyboard } => ctx.start_placing(kind, keyboard),
            ControlAction::MoveMode => {
                let why = ctx.move_mode();
                if why.is_some() {
                    ctx.say(why);
                }
            }
            ControlAction::StartMoving(b) => {
                let why = ctx.start_moving(b, false);
                ctx.say(why);
            }
            ControlAction::SelectArmy => ctx.select_army(),
            ControlAction::Recall(units) => ctx.recall(units),
            ControlAction::NextIdle => ctx.next_idle(),
            ControlAction::Home => ctx.home(),
            ControlAction::CenterSelection => ctx.center_on_selection(),
            ControlAction::LookAt(p) => ctx.look_at(p),
            ControlAction::Select(ids) => ctx.select(&ids, false),
            ControlAction::Deselect(u) => {
                ctx.sel.remove(EntityRef::Unit(u));
                ctx.refresh();
            }
            ControlAction::OrderAt(p) => {
                let target = ctx.pick_world(p);
                ctx.order(p, target, false);
            }
            ControlAction::Act(a) => ctx.act(a),
            ControlAction::Cancel => ctx.place.cancel(),
        }
    }
}

/// Mirrors the placement into the sprite renderer's `Ghost` (C2 draws the translucent building + footprint).
pub fn sync_ghost(ctx: Ctx, mut ghost: ResMut<Ghost>) {
    let kind = ctx.place.kind().filter(|_| ctx.place.placing());
    let moving = match ctx.place.mode {
        PlaceMode::Move(b) => Some(b),
        _ => None,
    };
    let variant = moving
        .and_then(|b| ctx.mirror.view.buildings.get(&b))
        .map_or(0, |b| b.variant);
    let want = Ghost {
        kind,
        cell: ctx.place.cell,
        valid: ctx.place.ok,
        variant,
        moving,
        chain: Vec::new(),
    };
    if ghost.kind != want.kind
        || ghost.cell != want.cell
        || ghost.valid != want.valid
        || ghost.variant != want.variant
        || ghost.moving != want.moving
        || !ghost.chain.is_empty()
    {
        *ghost = want;
    }
    let _ = building_foot;
}

/// Native input testing (`--place KIND`, `--select pawns|army|castle`): applied once per match after a few
/// snapshots so screenshots can show the ghost, range rings and selection.
pub fn dev_inputs(mut ctx: Ctx, dev: Res<crate::dev::DevArgs>, mut done: Local<Option<u32>>) {
    if (dev.place.is_none() && dev.select.is_none())
        || *done == Some(ctx.mirror.generation)
        || ctx.mirror.snapshots < 10
    {
        return;
    }
    *done = Some(ctx.mirror.generation);
    let me = ctx.mirror.you;
    match dev.select.as_deref() {
        Some("castle") => ctx.home(),
        Some("army") => ctx.select_army(),
        Some("pawns") => {
            let ids: Vec<EntityRef> = ctx
                .mirror
                .view
                .units
                .values()
                .filter(|u| Some(u.faction) == me && u.kind == ts_sim::defs::UnitKind::Pawn)
                .map(|u| EntityRef::Unit(u.id))
                .collect();
            ctx.select(&ids, false);
        }
        _ => {}
    }
    if let Some(key) = dev.place.as_deref()
        && let Some(kind) = ts_sim::defs::BuildingKind::ALL
            .iter()
            .copied()
            .find(|k| k.def().key == key)
    {
        ctx.start_placing(kind, true);
    }
}
