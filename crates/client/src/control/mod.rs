// OWNER: C4 (control) — spec 05-control-loop.md (input routing, selection, orders, placement, edge pan, zoom levels,
// keyboard shortcuts, cursor), 06-ui.md §3 (keys), docs/bevy-0.20-notes.md §14 (cursor → ground ray), §16 (input).
//
// SKELETON: WASD / arrows pan the `CamRig` focus (speed scales with distance), mouse wheel and `+`/`-` zoom.
// C4 TODO: selection (click / box / double click), orders via `link::NetOut::command`, placement ghost, control
// groups, edge pan, minimap clicks, cursor icons, pointer-over-UI blocking (§15.5 of the notes).

use crate::AppState;
use crate::render::camera::CamRig;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;

/// Pan speed in world units per second at zoom 1.
pub const PAN_SPEED: f32 = 22.0;
pub const ZOOM_MIN: f32 = 0.35;
pub const ZOOM_MAX: f32 = 3.0;

pub struct ControlPlugin;

impl Plugin for ControlPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, camera_keys.run_if(in_state(AppState::InGame)));
    }
}

fn camera_keys(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time<Real>>,
    mut rig: ResMut<CamRig>,
) {
    let mut d = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        d.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        d.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        d.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        d.x += 1.0;
    }
    if d != Vec2::ZERO {
        let step = PAN_SPEED / rig.zoom * time.delta_secs();
        rig.focus += d.normalize() * step;
        rig.focus = rig.focus.clamp(
            Vec2::ZERO,
            Vec2::new(ts_sim::MAP_W as f32, ts_sim::MAP_H as f32),
        );
    }
    let mut z = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 100.0,
    };
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        z += 1.0;
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        z -= 1.0;
    }
    if z != 0.0 {
        rig.zoom = (rig.zoom * 1.15f32.powf(z)).clamp(ZOOM_MIN, ZOOM_MAX);
    }
}
