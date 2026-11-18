// OWNER: C1 (terrain) — RTS camera rig. Spec 05-control-loop.md (camera clamp, zoom levels, edge pan — input lives in
// control/), docs/ARCHITECTURE.md "Rendering HD-2D" (perspective, narrow FOV, fixed pitch, looking north).
//
// SKELETON: one persistent `Camera3d` (also renders the UI), perspective FOV 30°, pitched ~50° down, looking north
// (-Z). `CamRig` is the single source of truth: control/ writes `focus`/`zoom`, this module writes the Transform.
// On each new match the rig centres on the own castle (or `--cam X,Y`).
//
// C1 TODO: clamp to the map, smooth zoom, Hdr/Bloom/DistanceFog/tilt-shift look, `IsDefaultUiCamera` split if needed.

use crate::AppState;
use crate::dev::DevArgs;
use crate::mirror::Mirror;
use crate::render::consts::{GROUND_Y, px_to_xz, tile_center};
use bevy::prelude::*;
use ts_sim::defs::BuildingKind;

/// Default distance from the focus point (world units) at zoom 1.
pub const BASE_DISTANCE: f32 = 28.0;
/// Camera pitch below the horizon.
pub const PITCH_DEG: f32 = 50.0;
/// Vertical field of view.
pub const FOV_DEG: f32 = 30.0;

/// Marker of the main game camera.
#[derive(Component)]
pub struct RtsCamera;

/// Camera rig state.
#[derive(Resource, Debug, Clone)]
pub struct CamRig {
    /// world XZ point looked at (on the ground plane)
    pub focus: Vec2,
    /// 1 = default, > 1 closer
    pub zoom: f32,
    pub pitch_deg: f32,
}

impl Default for CamRig {
    fn default() -> Self {
        CamRig {
            focus: Vec2::new(ts_sim::MAP_W as f32 * 0.5, ts_sim::MAP_H as f32 * 0.5),
            zoom: 1.0,
            pitch_deg: PITCH_DEG,
        }
    }
}

impl CamRig {
    pub fn distance(&self) -> f32 {
        BASE_DISTANCE / self.zoom.max(0.05)
    }

    pub fn transform(&self) -> Transform {
        let p = self.pitch_deg.to_radians();
        let target = Vec3::new(self.focus.x, GROUND_Y, self.focus.y);
        let d = self.distance();
        let eye = target + Vec3::new(0.0, p.sin() * d, p.cos() * d);
        Transform::from_translation(eye).looking_at(target, Vec3::Y)
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CamRig>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (
                    center_on_match.run_if(in_state(AppState::InGame)),
                    apply_rig,
                )
                    .chain(),
            );
    }
}

fn spawn_camera(mut commands: Commands, mut rig: ResMut<CamRig>, dev: Res<DevArgs>) {
    if let Some(z) = dev.zoom {
        rig.zoom = z;
    }
    commands.spawn((
        RtsCamera,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEG.to_radians(),
            near: 0.5,
            far: 400.0,
            ..default()
        }),
        rig.transform(),
    ));
}

/// Centres the rig once per match (after the first snapshot): `--cam`, else own castle, else map centre.
fn center_on_match(
    mirror: Res<Mirror>,
    dev: Res<DevArgs>,
    mut rig: ResMut<CamRig>,
    mut done_gen: Local<Option<u32>>,
) {
    if !mirror.active || mirror.snapshots == 0 || *done_gen == Some(mirror.generation) {
        return;
    }
    *done_gen = Some(mirror.generation);
    if let Some(c) = dev.cam {
        rig.focus = tile_center(c.x, c.y);
        return;
    }
    let castle = mirror
        .view
        .buildings
        .values()
        .find(|b| Some(b.faction) == mirror.you && b.kind == BuildingKind::Castle);
    if let Some(b) = castle {
        // footprint with top-left (cx, cy): look at its centre
        let d = b.kind.def();
        rig.focus = Vec2::new(
            b.cx as f32 + d.w as f32 * 0.5,
            b.cy as f32 + d.h as f32 * 0.5,
        );
    } else if let Some(u) = mirror
        .view
        .units
        .values()
        .find(|u| Some(u.faction) == mirror.you)
    {
        rig.focus = px_to_xz(u.x, u.y);
    }
}

fn apply_rig(rig: Res<CamRig>, mut q: Query<&mut Transform, With<RtsCamera>>) {
    if !rig.is_changed() {
        return;
    }
    for mut t in &mut q {
        *t = rig.transform();
    }
}
