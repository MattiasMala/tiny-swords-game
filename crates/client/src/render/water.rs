// OWNER: C1 (terrain) — water: docs/ARCHITECTURE.md (water plane with animated foam along the shore), spec 09 §4
// (colour `#47aba9`, `Water Foam.png` 16 frames × 192 px, 10 fps, synchronised, centred on every foam cell),
// 02 §5.11 (foam cells: land cells with a non-land 8-neighbour; the water cell of a sea cliff is covered by the
// cliff's own wet foot), 05 §12 (fog of war on the water).
//
// * The sea is one big plane at `WATER_Y` (map + margin) with a WESL shader (`shaders/water.wesl`): pack colour,
//   lighter shallows near the coast (per-cell distance-to-land texture), gentle pixel-snapped wave glints, fog of
//   war, distance fog. Lit by the scene (darkens at night), receives cliff shadows, casts none.
// * Foam: one quad of 3×3 tiles per foam cell at the water line, batched per 32×32 chunk. The 16 frames are a
//   mip-mapped texture array built at load time; the shader picks `floor(t·10) mod 16` (all cells in sync). Land
//   columns hide the centre of each quad, so only the ring outside the coast shows (as in the 2D game).

use crate::AppState;
use crate::mirror::Mirror;
use crate::render::consts::WATER_Y;
use crate::render::terrain::atlas::rgba_mips;
use crate::render::terrain::{FogTexture, fog_color_linear};
use crate::WATER_COLOR;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::shader::ShaderRef;
use ts_sim::map::Map;

/// Foam sheet (relative to the asset root).
pub const FOAM_PATH: &str = "Terrain/Tileset/Water Foam.png";
/// Foam frames / frame size / fps (spec 09 §4.2).
pub const FOAM_FRAMES: u32 = 16;
pub const FOAM_PX: u32 = 192;
pub const FOAM_FPS: f32 = 10.0;
/// Shallow-water distance range (cells) of the shore texture.
const SHORE_CELLS: f32 = 4.0;
/// Sea margin around the map (cells).
const MARGIN: f32 = 96.0;
/// Chunk size of the foam batches (cells).
const FOAM_CHUNK: i32 = 32;

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExt>;
pub type FoamMaterial = ExtendedMaterial<StandardMaterial, FoamExt>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct WaterExt {
    /// x, y = map size in cells; z = fog strength; w = shore range (cells)
    #[uniform(100)]
    pub map_size: Vec4,
    #[uniform(100)]
    pub fog_color: Vec4,
    /// shallow water colour (linear)
    #[uniform(100)]
    pub shallow: Vec4,
    #[texture(103)]
    #[sampler(104)]
    pub fog: Handle<Image>,
    #[texture(105)]
    #[sampler(106)]
    pub shore: Handle<Image>,
}

impl MaterialExtension for WaterExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://ts_client/render/shaders/water.wesl".into()
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct FoamExt {
    /// x, y = map size in cells; z = fog strength; w = fps
    #[uniform(100)]
    pub map_size: Vec4,
    #[uniform(100)]
    pub fog_color: Vec4,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub frames: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    pub fog: Handle<Image>,
}

impl MaterialExtension for FoamExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://ts_client/render/shaders/foam.wesl".into()
    }
}

#[derive(Component)]
pub struct WaterPlane;

#[derive(Component)]
pub struct FoamMesh;

#[derive(Resource, Default)]
pub struct WaterAssets {
    pub foam_src: Handle<Image>,
    pub foam_frames: Option<Handle<Image>>,
    pub shore: Option<Handle<Image>>,
    pub water_mat: Option<Handle<WaterMaterial>>,
    pub foam_mat: Option<Handle<FoamMaterial>>,
}

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/water.wesl");
        embedded_asset!(app, "shaders/foam.wesl");
        app.add_plugins((
            MaterialPlugin::<WaterMaterial>::default(),
            MaterialPlugin::<FoamMaterial>::default(),
        ))
        .init_resource::<WaterAssets>()
        .add_systems(Startup, load_foam)
        .add_systems(Update, build_foam_frames)
        .add_systems(Update, rebuild_water.run_if(in_state(AppState::InGame)));
    }
}

fn load_foam(server: Res<AssetServer>, mut wa: ResMut<WaterAssets>) {
    wa.foam_src = server.load(FOAM_PATH);
}

/// Cuts the foam strip into a mip-mapped 16-layer array (premultiplied box filter for clean edges).
fn build_foam_frames(mut wa: ResMut<WaterAssets>, mut images: ResMut<Assets<Image>>) {
    if wa.foam_frames.is_some() {
        return;
    }
    let Some(src) = images.get(&wa.foam_src) else {
        return;
    };
    let Some(data) = src.data.as_ref() else {
        return;
    };
    let (w, h) = (src.width() as usize, src.height() as usize);
    let fp = FOAM_PX as usize;
    if w < fp * FOAM_FRAMES as usize || h < fp || data.len() < w * h * 4 {
        warn_once!("Water Foam.png: dimensioni inattese {w}x{h}");
        return;
    }
    let mips = mip_count(FOAM_PX);
    let mut out = Vec::new();
    for f in 0..FOAM_FRAMES as usize {
        let mut layer = vec![0u8; fp * fp * 4];
        for y in 0..fp {
            let s = (y * w + f * fp) * 4;
            layer[y * fp * 4..(y + 1) * fp * 4].copy_from_slice(&data[s..s + fp * 4]);
        }
        out.extend(rgba_mips(&layer, fp, mips));
    }
    let mut img = Image::new_uninit(
        Extent3d {
            width: FOAM_PX,
            height: FOAM_PX,
            depth_or_array_layers: FOAM_FRAMES,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = mips;
    img.data_order = TextureDataOrder::LayerMajor;
    img.data = Some(out);
    img.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    wa.foam_frames = Some(images.add(img));
}

/// Number of mip levels of a square texture of side `n`.
fn mip_count(n: u32) -> u32 {
    32 - n.max(1).leading_zeros()
}

/// Per-cell distance to the nearest land cell (0 on land … 255 at `SHORE_CELLS` or more), R8, linear.
fn shore_image(map: &Map) -> Image {
    let (w, h) = (map.w.max(1) as usize, map.h.max(1) as usize);
    let mut d = vec![f32::INFINITY; w * h];
    for (i, v) in d.iter_mut().enumerate() {
        if map.land.get(i).copied().unwrap_or(0) != 0 {
            *v = 0.0;
        }
    }
    // two-pass chamfer distance (1, √2)
    let s2 = std::f32::consts::SQRT_2;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + 1.0);
            }
            if y > 0 {
                v = v.min(d[i - w] + 1.0);
                if x > 0 {
                    v = v.min(d[i - w - 1] + s2);
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1] + s2);
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1] + 1.0);
            }
            if y + 1 < h {
                v = v.min(d[i + w] + 1.0);
                if x + 1 < w {
                    v = v.min(d[i + w + 1] + s2);
                }
                if x > 0 {
                    v = v.min(d[i + w - 1] + s2);
                }
            }
            d[i] = v;
        }
    }
    let data: Vec<u8> = d
        .iter()
        .map(|v| ((v / SHORE_CELLS).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    let mut img = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    img
}

/// Foam cells (spec 02 §5.11): land cells (`land > 0`) with a non-land 8-neighbour (outside counts as water).
pub fn is_foam_cell(map: &Map, x: i32, y: i32) -> bool {
    let ground = |x: i32, y: i32| map.inside(x, y) && map.land[map.idx(x, y) as usize] != 0;
    if !ground(x, y) {
        return false;
    }
    for dy in -1..=1 {
        for dx in -1..=1 {
            if (dx != 0 || dy != 0) && !ground(x + dx, y + dy) {
                return true;
            }
        }
    }
    false
}

/// Foam quads of one chunk (3×3 tiles centred on each foam cell, slightly above the water).
fn foam_chunk(map: &Map, ccx: i32, ccy: i32) -> Option<Mesh> {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut uv: Vec<[f32; 2]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let y = WATER_Y + 0.02;
    for cy in ccy * FOAM_CHUNK..((ccy + 1) * FOAM_CHUNK).min(map.h) {
        for cx in ccx * FOAM_CHUNK..((ccx + 1) * FOAM_CHUNK).min(map.w) {
            if !is_foam_cell(map, cx, cy) {
                continue;
            }
            let (x0, z0) = (cx as f32 - 1.0, cy as f32 - 1.0);
            let base = pos.len() as u32;
            pos.extend([
                [x0, y, z0],
                [x0 + 3.0, y, z0],
                [x0 + 3.0, y, z0 + 3.0],
                [x0, y, z0 + 3.0],
            ]);
            uv.extend([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
            idx.extend([base, base + 3, base + 2, base, base + 2, base + 1]);
        }
    }
    if idx.is_empty() {
        return None;
    }
    let n = pos.len();
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx)),
    )
}

/// Spawns the sea plane and the foam batches for each new match.
#[allow(clippy::too_many_arguments)]
fn rebuild_water(
    mut commands: Commands,
    mirror: Res<Mirror>,
    fog: Res<FogTexture>,
    mut wa: ResMut<WaterAssets>,
    old: Query<Entity, Or<(With<WaterPlane>, With<FoamMesh>)>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut water_mats: ResMut<Assets<WaterMaterial>>,
    mut foam_mats: ResMut<Assets<FoamMaterial>>,
    mut built: Local<Option<u32>>,
) {
    if *built == Some(mirror.generation) {
        return;
    }
    let Some(map) = mirror.map() else { return };
    let Some(frames) = wa.foam_frames.clone() else {
        return;
    };
    *built = Some(mirror.generation);
    for e in &old {
        commands.entity(e).despawn();
    }
    let size = Vec4::new(map.w as f32, map.h as f32, 1.0, SHORE_CELLS);
    // shore texture (per match)
    let shore = images.add(shore_image(map));
    if let Some(old) = wa.shore.replace(shore.clone()) {
        images.remove(&old);
    }
    let shallow = Color::srgb_u8(0x5c, 0xc0, 0xb4).to_linear();
    let water_mat = water_mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: WATER_COLOR,
            perceptual_roughness: 0.55,
            reflectance: 0.12,
            ..default()
        },
        extension: WaterExt {
            map_size: size,
            fog_color: fog_color_linear(),
            shallow: Vec4::new(shallow.red, shallow.green, shallow.blue, 1.0),
            fog: fog.image.clone(),
            shore,
        },
    });
    let foam_mat = foam_mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.8,
            reflectance: 0.1,
            ..default()
        },
        extension: FoamExt {
            map_size: Vec4::new(map.w as f32, map.h as f32, 1.0, FOAM_FPS),
            fog_color: fog_color_linear(),
            frames,
            fog: fog.image.clone(),
        },
    });
    wa.water_mat = Some(water_mat.clone());
    wa.foam_mat = Some(foam_mat.clone());
    let (w, h) = (map.w as f32, map.h as f32);
    commands.spawn((
        WaterPlane,
        Name::new("sea"),
        DespawnOnExit(AppState::InGame),
        NotShadowCaster,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(w + MARGIN * 2.0, h + MARGIN * 2.0))),
        MeshMaterial3d(water_mat),
        Transform::from_xyz(w * 0.5, WATER_Y, h * 0.5),
    ));
    for ccy in 0..(map.h + FOAM_CHUNK - 1) / FOAM_CHUNK {
        for ccx in 0..(map.w + FOAM_CHUNK - 1) / FOAM_CHUNK {
            if let Some(m) = foam_chunk(map, ccx, ccy) {
                commands.spawn((
                    FoamMesh,
                    Name::new(format!("foam {ccx},{ccy}")),
                    DespawnOnExit(AppState::InGame),
                    NotShadowCaster,
                    Mesh3d(meshes.add(m)),
                    MeshMaterial3d(foam_mat.clone()),
                    Transform::IDENTITY,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foam_cells_ring_the_coast() {
        let mut m = Map::with_size(6, 6);
        for y in 1..5 {
            for x in 1..5 {
                let i = m.idx(x, y) as usize;
                m.land[i] = 1;
            }
        }
        assert!(is_foam_cell(&m, 1, 1));
        assert!(!is_foam_cell(&m, 2, 2));
        assert!(!is_foam_cell(&m, 0, 0));
        assert!(foam_chunk(&m, 0, 0).is_some());
    }

    #[test]
    fn mips_sizes() {
        assert_eq!(mip_count(192), 8);
        let base = vec![255u8; 192 * 192 * 4];
        let all = rgba_mips(&base, 192, 8);
        let expect: usize = (0..8).map(|l| (192usize >> l).max(1).pow(2) * 4).sum();
        assert_eq!(all.len(), expect);
    }
}
