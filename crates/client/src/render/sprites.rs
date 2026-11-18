// OWNER: C2 (sprites) — docs/ARCHITECTURE.md (2D sprites standing in the 3D world, alpha-masked camera-facing quads that
// cast shadows, batched per texture), spec 01 (sheets, anchors, folder rules), 03a §anims, 09 (inventory, anchors),
// `defs.rs` render anchors/crops.
//
// SKELETON: placeholder boxes so the snapshot pipeline is visible end-to-end — one box per unit (faction colour,
// interpolated with `Mirror::unit_pos_interp`) and per building (footprint size, dimmed while under construction).
// Entities are keyed by sim id and spawned/despawned to follow `Mirror::view`.
// C2 TODO: replace with textured billboards from `assets::GameAssets` (anims, flips, hurt flash, shadows), trees,
// deposits, decor, sheep, projectiles, pickups, ghosts of enemy buildings.

use crate::AppState;
use crate::mirror::Mirror;
use crate::render::consts::{ground_y_px, level_y};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use ts_sim::slab::{BuildingId, UnitId};

#[derive(Component)]
pub struct UnitVisual(pub UnitId);

#[derive(Component)]
pub struct BuildingVisual(pub BuildingId);

/// Shared placeholder meshes/materials and the id → entity maps.
#[derive(Resource, Default)]
struct Placeholders {
    unit_mesh: Handle<Mesh>,
    cube: Handle<Mesh>,
    /// one material per faction colour index (KingdomColor as usize), + 1 neutral
    mats: Vec<Handle<StandardMaterial>>,
    site_mats: Vec<Handle<StandardMaterial>>,
    units: HashMap<UnitId, Entity>,
    /// entity + `done` flag it was spawned with
    buildings: HashMap<BuildingId, (Entity, bool)>,
    generation: u32,
}

pub struct SpritesPlugin;

impl Plugin for SpritesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Placeholders>()
            .add_systems(Startup, init_placeholders)
            .add_systems(OnExit(AppState::InGame), clear_maps)
            .add_systems(Update, sync_placeholders.run_if(in_state(AppState::InGame)));
    }
}

fn init_placeholders(
    mut ph: ResMut<Placeholders>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    ph.unit_mesh = meshes.add(Cuboid::new(0.45, 0.9, 0.45));
    ph.cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    for c in ts_sim::defs::KingdomColor::ALL {
        let [r, g, b] = c.rgb();
        ph.mats.push(materials.add(StandardMaterial {
            base_color: Color::srgb_u8(r, g, b),
            perceptual_roughness: 0.8,
            ..default()
        }));
        ph.site_mats.push(materials.add(StandardMaterial {
            base_color: Color::srgba_u8(r, g, b, 140),
            alpha_mode: AlphaMode::Blend,
            ..default()
        }));
    }
}

fn clear_maps(mut ph: ResMut<Placeholders>) {
    ph.units.clear();
    ph.buildings.clear();
}

fn sync_placeholders(
    mut commands: Commands,
    mut ph: ResMut<Placeholders>,
    mirror: Res<Mirror>,
    time: Res<Time<Real>>,
    mut tf: Query<&mut Transform>,
) {
    let Some(map) = mirror.map() else { return };
    if ph.generation != mirror.generation {
        ph.generation = mirror.generation;
        for (_, e) in ph.units.drain() {
            commands.entity(e).despawn();
        }
        for (_, (e, _)) in ph.buildings.drain() {
            commands.entity(e).despawn();
        }
    }
    let now = time.elapsed_secs_f64();
    let color_idx = |f: u8| {
        mirror
            .colors
            .get(f as usize)
            .map(|c| *c as usize)
            .unwrap_or(0)
    };
    // units
    let gone: Vec<UnitId> = ph
        .units
        .keys()
        .filter(|id| !mirror.view.units.contains_key(id))
        .copied()
        .collect();
    for id in gone {
        if let Some(e) = ph.units.remove(&id) {
            commands.entity(e).despawn();
        }
    }
    for (id, u) in &mirror.view.units {
        let p = mirror
            .unit_pos_interp(*id, now)
            .unwrap_or(Vec2::new(u.x, u.y));
        let y = ground_y_px(map, p.x, p.y) + 0.45;
        let pos = Vec3::new(p.x / ts_sim::TILE, y, p.y / ts_sim::TILE);
        if let Some(e) = ph.units.get(id) {
            if let Ok(mut t) = tf.get_mut(*e) {
                t.translation = pos;
            }
        } else {
            let mat = ph.mats[color_idx(u.faction)].clone();
            let e = commands
                .spawn((
                    UnitVisual(*id),
                    DespawnOnExit(AppState::InGame),
                    Mesh3d(ph.unit_mesh.clone()),
                    MeshMaterial3d(mat),
                    Transform::from_translation(pos),
                ))
                .id();
            ph.units.insert(*id, e);
        }
    }
    // buildings (static: spawn once, respawn when `done` flips)
    let gone: Vec<BuildingId> = ph
        .buildings
        .keys()
        .filter(|id| {
            mirror
                .view
                .buildings
                .get(id)
                .is_none_or(|b| b.done != ph.buildings[*id].1)
        })
        .copied()
        .collect();
    for id in gone {
        if let Some((e, _)) = ph.buildings.remove(&id) {
            commands.entity(e).despawn();
        }
    }
    for (id, b) in &mirror.view.buildings {
        if ph.buildings.contains_key(id) {
            continue;
        }
        let d = b.kind.def();
        let c = map.idx(b.cx, b.cy);
        let base = if map.valid(c) {
            level_y(map.level[c as usize])
        } else {
            0.0
        };
        let hgt = if b.done { 1.4 } else { 0.4 };
        let mat = if b.done {
            ph.mats[color_idx(b.faction)].clone()
        } else {
            ph.site_mats[color_idx(b.faction)].clone()
        };
        let e = commands
            .spawn((
                BuildingVisual(*id),
                DespawnOnExit(AppState::InGame),
                Mesh3d(ph.cube.clone()),
                MeshMaterial3d(mat),
                Transform::from_xyz(
                    b.cx as f32 + d.w as f32 * 0.5,
                    base + hgt * 0.5,
                    b.cy as f32 + d.h as f32 * 0.5,
                )
                .with_scale(Vec3::new(d.w as f32 * 0.9, hgt, d.h as f32 * 0.9)),
            ))
            .id();
        ph.buildings.insert(*id, (e, b.done));
    }
}
