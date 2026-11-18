// OWNER: C2 (sprites) — spec 07-light-sfx.md (day/night cycle `DAY_LENGTH`, phases, night tint, torches/point lights,
// `defs::day_phase`), docs/bevy-0.20-notes.md §9 (WebGL2: 1 directional light, 1 cascade).
//
// SKELETON: one shadow-casting `DirectionalLight` (sun from the south-west, high) + `GlobalAmbientLight`.
// `DayTime` = current day phase 0..1: `--time T` overrides, else `defs::day_phase(mirror.view.time)`.
// The light only dims a little with the phase for now. C2 TODO: real sun path/colours per spec 07, night point lights.

use crate::dev::DevArgs;
use crate::mirror::Mirror;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;

/// Day phase 0..1 used by the lighting.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct DayTime(pub f32);

#[derive(Component)]
pub struct Sun;

pub struct LightPlugin;

impl Plugin for LightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DayTime>()
            .insert_resource(GlobalAmbientLight {
                color: Color::WHITE,
                brightness: 350.0,
                ..default()
            })
            .add_systems(Startup, spawn_sun)
            .add_systems(Update, update_day);
    }
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        Sun,
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: true,
            color: Color::srgb(1.0, 0.96, 0.88),
            ..default()
        },
        // light travels along its forward (-Z): from up / south-west towards north-east
        Transform::from_xyz(-0.5, 1.0, 0.6).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 1.0,
            maximum_distance: 90.0,
            first_cascade_far_bound: 90.0,
            overlap_proportion: 0.2,
        }
        .build(),
    ));
}

fn update_day(
    dev: Res<DevArgs>,
    mirror: Res<Mirror>,
    mut day: ResMut<DayTime>,
    mut sun: Query<&mut DirectionalLight, With<Sun>>,
) {
    let phase = dev
        .time
        .unwrap_or_else(|| ts_sim::defs::day_phase(mirror.view.time));
    if (day.0 - phase).abs() > 1e-4 {
        day.0 = phase;
    }
    // placeholder curve: full light around midday (0.25..0.75), dimmer towards night
    let k = 1.0 - (phase - 0.5).abs() * 1.2;
    for mut l in &mut sun {
        l.illuminance = 9_000.0 * k.clamp(0.35, 1.0);
    }
}
