// OWNER: C1 (terrain) — docs/ARCHITECTURE.md "Rendering HD-2D" (block terrain, 32x32 chunks, visible faces only,
// per-vertex AO, Tiny Swords tileset textures), spec 02-world.md §2 (map layers), 09-asset-inventory.md §3.6/§3.7/§11
// (tiers, cliffs on south edges, stairs, foam, surfaces).
//
// SKELETON (foundation): `build_preview_mesh` makes ONE flat-shaded mesh of the whole map — a top quad per land cell
// at its level height, coloured by `Surface`, plus side quads where the neighbour is lower (rock for cliffs, sand
// for shores). Rebuilt when `Mirror::generation` changes. Replace with textured chunked meshes (keep
// `TerrainPlugin` and the generation-watch pattern).

use crate::AppState;
use crate::mirror::Mirror;
use crate::render::consts::{SEABED_Y, level_y};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use ts_sim::map::{Map, Surface};

/// Marker of terrain entities (despawned on a new match / leaving the game).
#[derive(Component)]
pub struct TerrainMesh;

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, rebuild_terrain.run_if(in_state(AppState::InGame)));
    }
}

fn rebuild_terrain(
    mut commands: Commands,
    mirror: Res<Mirror>,
    old: Query<Entity, With<TerrainMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut built: Local<Option<u32>>,
) {
    if *built == Some(mirror.generation) {
        return;
    }
    let Some(map) = mirror.map() else { return };
    *built = Some(mirror.generation);
    for e in &old {
        commands.entity(e).despawn();
    }
    let mesh = build_preview_mesh(map);
    commands.spawn((
        TerrainMesh,
        DespawnOnExit(AppState::InGame),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.95,
            reflectance: 0.1,
            ..default()
        })),
        Transform::IDENTITY,
    ));
}

fn surface_color(s: Surface) -> [f32; 3] {
    let c = match s {
        Surface::Water => [0x47, 0xab, 0xa9],
        Surface::Sand => [0xe0, 0xc4, 0x8a],
        Surface::GrassLow => [0x5f, 0xae, 0x5a],
        Surface::GrassMid => [0xa5, 0xc2, 0x5a],
        Surface::GrassTop => [0x7c, 0xc3, 0x5a],
        Surface::GrassDry => [0xa9, 0xa6, 0x5a],
        Surface::ForestFloor => [0x4c, 0x9a, 0x7f],
        Surface::Dirt => [0xa0, 0x78, 0x50],
        Surface::Path => [0xc0, 0x9a, 0x68],
    };
    [
        srgb_to_linear(c[0]),
        srgb_to_linear(c[1]),
        srgb_to_linear(c[2]),
    ]
}

fn srgb_to_linear(v: u8) -> f32 {
    Color::srgb_u8(v, 0, 0).to_linear().red
}

/// Height of the top of a cell for the preview (`None` = water).
fn top_of(map: &Map, cx: i32, cy: i32) -> Option<f32> {
    if !map.inside(cx, cy) {
        return None;
    }
    let c = map.idx(cx, cy);
    if map.land[c as usize] == 0 {
        None
    } else {
        Some(level_y(map.level[c as usize]))
    }
}

#[derive(Default)]
struct MeshBuf {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl MeshBuf {
    /// Quad `a b c d` (any winding) facing `n`.
    fn quad(&mut self, q: [Vec3; 4], n: Vec3, c: [f32; 3]) {
        let base = self.pos.len() as u32;
        for p in q {
            self.pos.push(p.to_array());
            self.nrm.push(n.to_array());
            self.col.push([c[0], c[1], c[2], 1.0]);
        }
        let facing = (q[1] - q[0]).cross(q[2] - q[0]).dot(n) > 0.0;
        if facing {
            self.idx
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        } else {
            self.idx
                .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
}

/// One mesh for the whole map (preview quality).
pub fn build_preview_mesh(map: &Map) -> Mesh {
    let mut b = MeshBuf::default();
    let rock = [
        srgb_to_linear(0x7a),
        srgb_to_linear(0x6a),
        srgb_to_linear(0x5e),
    ];
    let shore = [
        srgb_to_linear(0xc8),
        srgb_to_linear(0xa8),
        srgb_to_linear(0x6a),
    ];
    for cy in 0..map.h {
        for cx in 0..map.w {
            let Some(h) = top_of(map, cx, cy) else {
                continue;
            };
            let c = map.idx(cx, cy) as usize;
            let mut col = surface_color(map.surface[c]);
            if map.stair[c] != 0 {
                col = [col[0] * 1.15, col[1] * 1.1, col[2] * 1.05];
            }
            if map.cliff[c] != 0 {
                col = rock;
            }
            if (cx + cy) % 2 == 0 {
                col = [col[0] * 0.96, col[1] * 0.96, col[2] * 0.96];
            }
            let (x0, x1, z0, z1) = (cx as f32, cx as f32 + 1.0, cy as f32, cy as f32 + 1.0);
            b.quad(
                [
                    Vec3::new(x0, h, z0),
                    Vec3::new(x1, h, z0),
                    Vec3::new(x1, h, z1),
                    Vec3::new(x0, h, z1),
                ],
                Vec3::Y,
                col,
            );
            // sides where the neighbour is lower
            for (dx, dy, n) in [
                (0, -1, Vec3::NEG_Z),
                (0, 1, Vec3::Z),
                (-1, 0, Vec3::NEG_X),
                (1, 0, Vec3::X),
            ] {
                let (nh, side_col) = match top_of(map, cx + dx, cy + dy) {
                    Some(nh) if nh < h => (nh, rock),
                    Some(_) => continue,
                    None => (SEABED_Y, shore),
                };
                let q = match (dx, dy) {
                    (0, -1) => [
                        Vec3::new(x0, nh, z0),
                        Vec3::new(x1, nh, z0),
                        Vec3::new(x1, h, z0),
                        Vec3::new(x0, h, z0),
                    ],
                    (0, 1) => [
                        Vec3::new(x0, nh, z1),
                        Vec3::new(x1, nh, z1),
                        Vec3::new(x1, h, z1),
                        Vec3::new(x0, h, z1),
                    ],
                    (-1, 0) => [
                        Vec3::new(x0, nh, z0),
                        Vec3::new(x0, nh, z1),
                        Vec3::new(x0, h, z1),
                        Vec3::new(x0, h, z0),
                    ],
                    _ => [
                        Vec3::new(x1, nh, z0),
                        Vec3::new(x1, nh, z1),
                        Vec3::new(x1, h, z1),
                        Vec3::new(x1, h, z0),
                    ],
                };
                b.quad(q, n, side_col);
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, b.nrm)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, b.col)
    .with_inserted_indices(Indices::U32(b.idx))
}
