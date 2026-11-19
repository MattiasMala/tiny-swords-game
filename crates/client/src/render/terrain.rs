// OWNER: C1 (terrain) — docs/ARCHITECTURE.md "Rendering HD-2D" (block terrain, 32x32 chunks, visible faces only,
// per-vertex AO, Tiny Swords tileset textures), spec 02-world.md §2 (map layers), 09-asset-inventory.md §3, §11
// (tiers, cliffs on south edges, stairs, foam, surfaces), 05 §12 (fog of war on the ground).
//
// * `terrain_atlas`: the texture array cut from `Tilemap_color1..5` at load time (228 layers + 1 spare: WebGL2 takes a
//   count multiple of 6 for a cube array), ≤ the 256 WebGL2 guarantees, mip-mapped.
// * `terrain_mesh`: heights (levels, beaches, cliff columns, stair slopes), exact `ground_height`, chunk mesher
//   (visible faces, AO, jitter, grass lips).
// * `terrain_fog`: `FogTexture` (contract) — per-cell fog of war, applied here, on the water and by C2 on sprites.
// * this file: the `TerrainMaterial` (StandardMaterial + array texture + fog, WESL shader
//   `shaders/terrain.wesl`, embedded for wasm) and the chunk entities, rebuilt when `Mirror::generation` changes.

#[path = "terrain_atlas.rs"]
pub mod atlas;
#[path = "terrain_fog.rs"]
pub mod fog;
#[path = "terrain_mesh.rs"]
pub mod mesh;

// flat re-exports used across the client
pub use fog::{EXPLORED_SHADE, FogTexture};
pub use mesh::{MAX_TERRAIN_Y, SAND_Y, corners, flat_top, ground_height};

use crate::AppState;
use crate::mirror::Mirror;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

/// Terrain material: lit StandardMaterial base + texture array + fog of war.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExt>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TerrainExt {
    /// x, y = map size in cells; z = fog strength (1 on); w unused
    #[uniform(100)]
    pub map_size: Vec4,
    /// fog of war colour (linear rgb)
    #[uniform(100)]
    pub fog_color: Vec4,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub tiles: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    pub fog: Handle<Image>,
}

impl MaterialExtension for TerrainExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://ts_client/render/shaders/terrain.wesl".into()
    }
}

/// Fog of war colour as linear RGBA.
pub fn fog_color_linear() -> Vec4 {
    let c = Color::srgb_u8(fog::FOG_RGB[0], fog::FOG_RGB[1], fog::FOG_RGB[2]).to_linear();
    Vec4::new(c.red, c.green, c.blue, 1.0)
}

/// Marker of terrain chunk entities.
#[derive(Component)]
pub struct TerrainMesh;

/// Source sheets, the built texture array and the shared material.
#[derive(Resource, Default)]
pub struct TerrainTextures {
    pub sources: Vec<Handle<Image>>,
    /// the `texture_2d_array` (None until the sources are loaded)
    pub array: Option<Handle<Image>>,
    pub material: Option<Handle<TerrainMaterial>>,
}

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/terrain.wesl");
        app.add_plugins((
            MaterialPlugin::<TerrainMaterial>::default(),
            fog::FogPlugin,
        ))
        .init_resource::<TerrainTextures>()
        .add_systems(Startup, load_sources)
        .add_systems(Update, build_atlas)
        .add_systems(
            Update,
            (rebuild_terrain, sync_fog_strength).run_if(in_state(AppState::InGame)),
        );
    }
}

fn load_sources(server: Res<AssetServer>, mut tt: ResMut<TerrainTextures>) {
    tt.sources = atlas::TILESET_PATHS
        .iter()
        .map(|p| server.load::<Image>(*p))
        .collect();
}

/// Builds the texture array once all five tilesets are loaded (CPU copies available).
fn build_atlas(
    mut tt: ResMut<TerrainTextures>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<TerrainMaterial>>,
    fog: Res<FogTexture>,
) {
    if tt.array.is_some() || tt.sources.len() != 5 {
        return;
    }
    // all five sheets must be loaded with their CPU pixels (RGBA8); cut the layers from borrowed data (no copies)
    let layers = {
        let mut sheets: Vec<atlas::Sheet> = Vec::with_capacity(5);
        for h in &tt.sources {
            let Some(img) = images.get(h) else { return };
            let Some(data) = img.data.as_deref() else {
                warn_once!("tileset senza dati CPU: terreno non texturizzato");
                return;
            };
            let (w, hh) = (img.width() as usize, img.height() as usize);
            // only RGBA8 sheets are supported (PNG default)
            if data.len() < w * hh * 4 {
                warn_once!("tileset in formato inatteso: terreno non texturizzato");
                return;
            }
            sheets.push(atlas::Sheet {
                data,
                width: w,
                height: hh,
            });
        }
        let Ok(sheets) = <[atlas::Sheet; 5]>::try_from(sheets) else {
            return;
        };
        atlas::build_layers(&sheets)
    };
    let array = images.add(atlas::array_image(&layers));
    let mat = mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.92,
            metallic: 0.0,
            reflectance: 0.08,
            ..default()
        },
        extension: TerrainExt {
            map_size: Vec4::new(ts_sim::MAP_W as f32, ts_sim::MAP_H as f32, 1.0, 0.0),
            fog_color: fog_color_linear(),
            tiles: array.clone(),
            fog: fog.image.clone(),
        },
    });
    info!("terreno: atlante di {} strati pronto", layers.len());
    tt.array = Some(array);
    tt.material = Some(mat);
    // the sheets were only needed to cut the layers: release their CPU and GPU copies
    tt.sources.clear();
}

/// Mesh of one chunk.
fn chunk_to_mesh(c: mesh::ChunkMesh) -> Mesh {
    let n = c.pos.len();
    let indices = if n <= u16::MAX as usize {
        Indices::U16(c.idx.iter().map(|&i| i as u16).collect())
    } else {
        Indices::U32(c.idx)
    };
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, c.pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, c.nrm)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, c.uv)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, c.uv1)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, c.col)
    .with_inserted_indices(indices)
}

/// (Re)builds all chunks when a new match is loaded (the map geometry never changes during a match).
fn rebuild_terrain(
    mut commands: Commands,
    mirror: Res<Mirror>,
    tt: Res<TerrainTextures>,
    old: Query<Entity, With<TerrainMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut built: Local<Option<(u32, usize)>>,
) {
    // rebuild for a new generation, or when the chunks were despawned (left `InGame` and came back to the same
    // match without a new `Init`)
    if let Some((g, n)) = *built
        && g == mirror.generation
        && (n == 0 || !old.is_empty())
    {
        return;
    }
    let Some(map) = mirror.map() else { return };
    let Some(mat) = tt.material.clone() else {
        return;
    };
    let mut spawned = 0;
    for e in &old {
        commands.entity(e).despawn();
    }
    info!("terreno: costruzione chunk (generazione {})", mirror.generation);
    let (ncx, ncy) = (
        (map.w + mesh::CHUNK - 1) / mesh::CHUNK,
        (map.h + mesh::CHUNK - 1) / mesh::CHUNK,
    );
    for ccy in 0..ncy {
        for ccx in 0..ncx {
            let c = mesh::build_chunk(map, ccx, ccy);
            if c.is_empty() {
                continue;
            }
            commands.spawn((
                TerrainMesh,
                Name::new(format!("terrain {ccx},{ccy}")),
                DespawnOnExit(AppState::InGame),
                Mesh3d(meshes.add(chunk_to_mesh(c))),
                MeshMaterial3d(mat.clone()),
                Transform::IDENTITY,
            ));
            spawned += 1;
        }
    }
    *built = Some((mirror.generation, spawned));
}

/// Keeps the material's fog switch and map size in sync (fog off for spectators / end of match is already in the
/// texture; this only handles non-standard map sizes).
fn sync_fog_strength(
    mirror: Res<Mirror>,
    tt: Res<TerrainTextures>,
    mut mats: ResMut<Assets<TerrainMaterial>>,
    mut last: Local<Option<(i32, i32)>>,
) {
    let Some(map) = mirror.map() else { return };
    let Some(h) = tt.material.as_ref() else {
        return;
    };
    if *last == Some((map.w, map.h)) {
        return;
    }
    if let Some(mut m) = mats.get_mut(h) {
        m.extension.map_size = Vec4::new(map.w as f32, map.h as f32, 1.0, 0.0);
        *last = Some((map.w, map.h));
    }
}
