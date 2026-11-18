// OWNER: C1 (terrain) — docs/ARCHITECTURE.md (water plane with animated foam along the shore), spec 09 §3 (water
// colour `#47aba9`, foam strips, water rocks), 02 §2 (`open_water`, ponds).
//
// SKELETON: one flat opaque plane at `WATER_Y` covering the map plus a margin, spawned per match.
// C1 TODO: custom WESL water material (foam rim from the shore distance, gentle waves), ponds, splash FX hooks.

use crate::mirror::Mirror;
use crate::render::consts::WATER_Y;
use crate::{AppState, WATER_COLOR};
use bevy::prelude::*;

#[derive(Component)]
pub struct WaterPlane;

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, spawn_water.run_if(in_state(AppState::InGame)));
    }
}

fn spawn_water(
    mut commands: Commands,
    mirror: Res<Mirror>,
    existing: Query<(), With<WaterPlane>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !existing.is_empty() {
        return;
    }
    let Some(map) = mirror.map() else { return };
    let margin = 64.0;
    let (w, h) = (map.w as f32 + margin * 2.0, map.h as f32 + margin * 2.0);
    commands.spawn((
        WaterPlane,
        DespawnOnExit(AppState::InGame),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(w, h))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: WATER_COLOR,
            perceptual_roughness: 0.3,
            reflectance: 0.2,
            ..default()
        })),
        Transform::from_xyz(map.w as f32 * 0.5, WATER_Y, map.h as f32 * 0.5),
    ));
}
