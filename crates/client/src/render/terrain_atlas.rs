// OWNER: C1 (terrain) — terrain texture array cut at load time from the Tiny Swords tilesets (spec 09 §3).
//
// One 64×64 layer per face kind, with a full mip chain (no shimmering when the camera zooms out; the shader keeps
// texels crisp when magnified):
// * top faces: 8 surfaces × 16 edge masks (N/E/S/W neighbour lower). Grass tiers come from
//   `Tilemap_color{1..5}` (§3.6 / reference mapping: low = 3, mid = 1, top = 2, dry = 4, forest floor = 5);
//   sand / dirt / path are palette remaps of color1 (§3.7). Edge masks use the ELEVATED autotile block
//   (cols 5-8, rows 0-3, §3.1/§3.3): scalloped navy outline, rock band on south edges.
// * ramps (stair lower halves): grass of the surface with step bands (8 layers).
// * walls: cliff cells (cols 5-8 = left cap / middle / right cap / pillar, §3.4) × {plain rock, foot on land
//   (row 4, grass tufts), foot in water (row 5, foam lip)} (12 layers).

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use ts_sim::map::Surface;

/// Texel size of a layer.
pub const TILE_PX: usize = 64;
/// Surfaces with a texture (Sand, GrassLow, GrassMid, GrassTop, GrassDry, ForestFloor, Dirt, Path).
pub const SURF_N: usize = 8;
/// Grass slots with "overlay" variants (flat autotile set, transparent outside the outline).
const GRASS_FIRST: usize = 1;
const GRASS_N: usize = 5;
const FLAT_BASE: u32 = (SURF_N * 16) as u32;
const RAMP_BASE: u32 = FLAT_BASE + (GRASS_N * 16) as u32;
const WALL_BASE: u32 = RAMP_BASE + SURF_N as u32;
/// Total number of layers.
pub const LAYERS: u32 = WALL_BASE + 12;
/// Mip levels (64 → 1).
pub const MIPS: u32 = 7;

/// Source sheets (relative to the asset root).
pub const TILESET_PATHS: [&str; 5] = [
    "Terrain/Tileset/Tilemap_color1.png",
    "Terrain/Tileset/Tilemap_color2.png",
    "Terrain/Tileset/Tilemap_color3.png",
    "Terrain/Tileset/Tilemap_color4.png",
    "Terrain/Tileset/Tilemap_color5.png",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallKind {
    Plain = 0,
    Foot = 1,
    Wet = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallCap {
    Left = 0,
    Mid = 1,
    Right = 2,
    Pillar = 3,
}

/// Texture slot of a surface (water → sand: only used for walls of cliff-in-sea cells).
pub fn surface_index(s: Surface) -> usize {
    match s {
        Surface::Water | Surface::Sand => 0,
        Surface::GrassLow => 1,
        Surface::GrassMid => 2,
        Surface::GrassTop => 3,
        Surface::GrassDry => 4,
        Surface::ForestFloor => 5,
        Surface::Dirt => 6,
        Surface::Path => 7,
    }
}

/// Top-face layer: surface slot × edge mask (bit 0 N, 1 E, 2 S, 3 W = that neighbour is lower).
#[inline]
pub fn top_layer(surf: usize, mask: u32) -> u32 {
    (surf.min(SURF_N - 1) * 16) as u32 + (mask & 15)
}

/// Overlay layer of a grass slot (`None` for sand / dirt / path): the flat autotile edge for mask `mask`, alpha 0
/// outside the outline so that the shader shows the neighbouring ground (`under` layer) there.
#[inline]
pub fn flat_layer(surf: usize, mask: u32) -> Option<u32> {
    if (GRASS_FIRST..GRASS_FIRST + GRASS_N).contains(&surf) {
        Some(FLAT_BASE + ((surf - GRASS_FIRST) * 16) as u32 + (mask & 15))
    } else {
        None
    }
}

/// Overlay priority of a surface slot: a cell draws its outline over a same-height neighbour of lower priority.
#[inline]
pub fn overlay_priority(surf: usize) -> u8 {
    match surf {
        0 => 0, // sand
        7 => 1, // path
        6 => 2, // dirt
        5 => 3, // forest floor
        4 => 4, // dry
        1 => 5, // low
        3 => 6, // top
        _ => 7, // mid
    }
}

#[inline]
pub fn ramp_layer(surf: usize) -> u32 {
    RAMP_BASE + surf.min(SURF_N - 1) as u32
}

#[inline]
pub fn wall_layer(kind: WallKind, cap: WallCap) -> u32 {
    WALL_BASE + kind as u32 * 4 + cap as u32
}

/// RGBA8 (sRGB) pixels of one layer.
type Layer = Vec<u8>;

/// A decoded sheet (RGBA8, row-major).
pub struct Sheet<'a> {
    pub data: &'a [u8],
    pub width: usize,
    pub height: usize,
}

impl Sheet<'_> {
    /// Cell `(col, row)` of the 64-px grid (out-of-range texels are transparent).
    fn cell(&self, col: usize, row: usize) -> Layer {
        let mut out = vec![0u8; TILE_PX * TILE_PX * 4];
        for y in 0..TILE_PX {
            let sy = row * TILE_PX + y;
            if sy >= self.height {
                break;
            }
            for x in 0..TILE_PX {
                let sx = col * TILE_PX + x;
                if sx >= self.width {
                    break;
                }
                let s = (sy * self.width + sx) * 4;
                let d = (y * TILE_PX + x) * 4;
                if s + 4 <= self.data.len() {
                    out[d..d + 4].copy_from_slice(&self.data[s..s + 4]);
                }
            }
        }
        out
    }
}

const fn rgb(hex: u32) -> [u8; 3] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]
}

/// Dark rock shown through the transparent corners of the elevated outline.
const EDGE_FILL: [u8; 3] = rgb(0x2b4450);
/// Shadowed grass between the tufts at a wall foot.
const TUFT_FILL: [u8; 3] = rgb(0x3c7662);
/// Water behind the foam lip of a wet foot.
const WET_FILL: [u8; 3] = rgb(0x47aba9);
/// Mint shore rim of the flat set (spec 09 §3.2), dropped in overlays.
const MINT: [u8; 3] = rgb(0xc6f0db);
/// Rounded cap corners.
const CAP_FILL: [u8; 3] = rgb(0x22343c);

/// Palette-remap targets (spec 09 §3.7 proposals): base colour of sand, dirt, path.
const SAND_BASE: [u8; 3] = rgb(0xd8c28e);
const DIRT_BASE: [u8; 3] = rgb(0x9a7058);
const PATH_BASE: [u8; 3] = rgb(0xc29a68);
/// Base grass colour of color1 (luminance reference of the remap).
const GRASS1_BASE: [u8; 3] = rgb(0x9bb94e);

fn lum(c: [u8; 3]) -> f32 {
    (0.3 * c[0] as f32 + 0.59 * c[1] as f32 + 0.11 * c[2] as f32) / 255.0
}

/// Recolours the grass pixels of a tile to `target`, keeping the shading (luminance ratio to the grass base).
fn remap(tile: &mut Layer, target: [u8; 3]) {
    let l0 = lum(GRASS1_BASE).max(0.01);
    for px in tile.chunks_exact_mut(4) {
        if px[3] == 0 {
            continue;
        }
        let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
        let grass = g >= b + 12 && g + 12 >= r;
        if !grass {
            continue;
        }
        let k = (lum([px[0], px[1], px[2]]) / l0).clamp(0.3, 1.4);
        for j in 0..3 {
            px[j] = (target[j] as f32 * k).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Fills every non-opaque texel with `fill` (layers are opaque faces).
fn fill_transparent(tile: &mut Layer, fill: [u8; 3]) {
    for px in tile.chunks_exact_mut(4) {
        if px[3] < 255 {
            // blend semi-transparent texels over the fill (baked shadows, anti-aliased rims)
            let a = px[3] as f32 / 255.0;
            for j in 0..3 {
                px[j] = (px[j] as f32 * a + fill[j] as f32 * (1.0 - a)).round() as u8;
            }
            px[3] = 255;
        }
    }
}

/// `edge(before, after)` of the JS autotiler (spec 09 §3.1).
#[inline]
fn edge(before: bool, after: bool) -> usize {
    match (before, after) {
        (true, true) => 1,
        (false, true) => 0,
        (true, false) => 2,
        (false, false) => 3,
    }
}

/// Sheet index (0-based) and remap target of a surface slot.
fn surface_source(surf: usize) -> (usize, Option<[u8; 3]>) {
    match surf {
        0 => (0, Some(SAND_BASE)),
        1 => (2, None),
        2 => (0, None),
        3 => (1, None),
        4 => (3, None),
        5 => (4, None),
        6 => (0, Some(DIRT_BASE)),
        _ => (0, Some(PATH_BASE)),
    }
}

/// Builds all layers from the five tileset sheets (`sheets[i]` = `Tilemap_color{i+1}`).
pub fn build_layers(sheets: &[Sheet; 5]) -> Vec<Layer> {
    let mut layers: Vec<Layer> = Vec::with_capacity(LAYERS as usize);
    // top faces
    for surf in 0..SURF_N {
        let (si, target) = surface_source(surf);
        let sheet = &sheets[si];
        let centre = sheet.cell(1, 1);
        for mask in 0..16u32 {
            let (n, e, s, w) = (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0, mask & 8 != 0);
            let col = 5 + edge(!w, !e);
            let row = edge(!n, !s);
            let mut t = if mask == 0 {
                centre.clone()
            } else {
                let mut t = centre.clone();
                let over = sheet.cell(col, row);
                for (d, o) in t.chunks_exact_mut(4).zip(over.chunks_exact(4)) {
                    if o[3] == 255 {
                        d.copy_from_slice(o);
                    } else {
                        d[0] = EDGE_FILL[0];
                        d[1] = EDGE_FILL[1];
                        d[2] = EDGE_FILL[2];
                        d[3] = 255;
                    }
                }
                t
            };
            if let Some(tg) = target {
                remap(&mut t, tg);
            }
            fill_transparent(&mut t, EDGE_FILL);
            layers.push(t);
        }
    }
    // grass overlays over lower-priority ground at the same height: flat autotile set (cols 0-3, rows 0-3, §3.2),
    // scalloped navy outline kept, mint shore rim and outside texels transparent
    for surf in GRASS_FIRST..GRASS_FIRST + GRASS_N {
        let (si, _) = surface_source(surf);
        let sheet = &sheets[si];
        let centre = sheet.cell(1, 1);
        for mask in 0..16u32 {
            let (n, e, s, w) = (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0, mask & 8 != 0);
            let mut t = centre.clone();
            if mask != 0 {
                let over = sheet.cell(edge(!w, !e), edge(!n, !s));
                for (d, o) in t.chunks_exact_mut(4).zip(over.chunks_exact(4)) {
                    let mint = (o[0] as i32 - MINT[0] as i32).abs() < 28
                        && (o[1] as i32 - MINT[1] as i32).abs() < 28
                        && (o[2] as i32 - MINT[2] as i32).abs() < 28;
                    if o[3] == 255 && !mint {
                        d.copy_from_slice(o);
                    } else {
                        d.copy_from_slice(&[0, 0, 0, 0]);
                    }
                }
            }
            layers.push(t);
        }
    }
    // ramps: surface grass with step bands (texture top = high edge)
    for surf in 0..SURF_N {
        let (si, target) = surface_source(surf);
        let mut t = sheets[si].cell(1, 1);
        if let Some(tg) = target {
            remap(&mut t, tg);
        }
        fill_transparent(&mut t, EDGE_FILL);
        for y in 0..TILE_PX {
            let k = match y % 16 {
                0 => 1.14,
                13 => 0.86,
                14 => 0.72,
                15 => 0.62,
                _ => 1.0,
            };
            for x in 0..TILE_PX {
                let side = if x < 2 || x >= TILE_PX - 2 { 0.7 } else { 1.0 };
                let i = (y * TILE_PX + x) * 4;
                for j in 0..3 {
                    t[i + j] = (t[i + j] as f32 * k * side).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        layers.push(t);
    }
    // walls: kind × cap
    let s1 = &sheets[0];
    for kind in [WallKind::Plain, WallKind::Foot, WallKind::Wet] {
        for cap in [WallCap::Left, WallCap::Mid, WallCap::Right, WallCap::Pillar] {
            let col = 5 + cap as usize;
            let mut t = match kind {
                WallKind::Wet => s1.cell(col, 5),
                _ => s1.cell(col, 4),
            };
            match kind {
                WallKind::Plain => {
                    // no foot: rows 52..63 repeat the rock of rows 14..25
                    let src = t.clone();
                    for y in 52..TILE_PX {
                        let sy = y - 38;
                        let (d, s) = (y * TILE_PX * 4, sy * TILE_PX * 4);
                        t[d..d + TILE_PX * 4].copy_from_slice(&src[s..s + TILE_PX * 4]);
                    }
                    fill_transparent(&mut t, CAP_FILL);
                }
                WallKind::Foot => {
                    // tufts gaps (bottom rows) show shadowed grass, cap corners dark rock
                    split_fill(&mut t, 50, CAP_FILL, TUFT_FILL);
                }
                WallKind::Wet => split_fill(&mut t, 50, CAP_FILL, WET_FILL),
            }
            layers.push(t);
        }
    }
    debug_assert_eq!(layers.len(), LAYERS as usize);
    layers
}

/// Fills transparent texels above row `split` with `upper`, below with `lower`.
fn split_fill(t: &mut Layer, split: usize, upper: [u8; 3], lower: [u8; 3]) {
    let (a, b) = t.split_at_mut(split * TILE_PX * 4);
    let mut a = a.to_vec();
    let mut b2 = b.to_vec();
    fill_transparent(&mut a, upper);
    fill_transparent(&mut b2, lower);
    t[..a.len()].copy_from_slice(&a);
    t[a.len()..].copy_from_slice(&b2);
}

fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// Full mip chain of a square RGBA8 sRGB image of side `n` (level 0 included): premultiplied-alpha 2×2 box filter
/// in linear space, wgpu level sizes `max(1, n >> level)`.
pub fn rgba_mips(base: &[u8], n: usize, levels: u32) -> Vec<u8> {
    let mut lut = [0f32; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        *v = srgb_to_linear(i as u8);
    }
    let mut out = base.to_vec();
    let mut cur: Vec<[f32; 4]> = base
        .chunks_exact(4)
        .map(|p| {
            let a = p[3] as f32 / 255.0;
            [lut[p[0] as usize] * a, lut[p[1] as usize] * a, lut[p[2] as usize] * a, a]
        })
        .collect();
    let mut size = n.max(1);
    for _ in 1..levels {
        let ns = (size / 2).max(1);
        let mut next = vec![[0f32; 4]; ns * ns];
        for y in 0..ns {
            for x in 0..ns {
                let mut acc = [0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((x * 2 + dx).min(size - 1), (y * 2 + dy).min(size - 1));
                    let p = cur[sy * size + sx];
                    for j in 0..4 {
                        acc[j] += p[j];
                    }
                }
                next[y * ns + x] = acc.map(|v| v * 0.25);
            }
        }
        for p in &next {
            let a = p[3];
            if a <= 1e-5 {
                out.extend([0, 0, 0, 0]);
            } else {
                out.extend([
                    linear_to_srgb(p[0] / a),
                    linear_to_srgb(p[1] / a),
                    linear_to_srgb(p[2] / a),
                    (a * 255.0).round().clamp(0.0, 255.0) as u8,
                ]);
            }
        }
        cur = next;
        size = ns;
    }
    out
}

/// Layer count actually allocated for an array of `n` layers. WebGL2 (wgpu-hal GLES) must fix the texture target at
/// creation and guesses it from the layer count: `n == 6` → cube, `n > 6 && n % 6 == 0` → cube array. A 2D array
/// with such a count (228 here!) gets the wrong target and samples garbage, so pad it with spare layers.
pub fn gl_safe_layers(n: usize) -> usize {
    let mut n = n.max(2);
    while n % 6 == 0 {
        n += 1;
    }
    n
}

/// Assembles the layers into a mip-mapped 2D array texture (`texture_2d_array` in the shader).
pub fn array_image(layers: &[Layer]) -> Image {
    let count = gl_safe_layers(layers.len());
    let mut data = Vec::with_capacity(count * TILE_PX * TILE_PX * 4 * 4 / 3 + 64);
    for l in layers {
        data.extend(rgba_mips(l, TILE_PX, MIPS));
    }
    // spare (never sampled) layers keep the GL texture target a 2D array
    let blank = vec![0u8; TILE_PX * TILE_PX * 4];
    for _ in layers.len()..count {
        data.extend(rgba_mips(&blank, TILE_PX, MIPS));
    }
    let mut img = Image::new_uninit(
        Extent3d {
            width: TILE_PX as u32,
            height: TILE_PX as u32,
            depth_or_array_layers: count as u32,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = MIPS;
    img.data_order = TextureDataOrder::LayerMajor;
    img.data = Some(data);
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
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_layers() -> (Vec<u32>, Vec<u32>) {
        let mut opaque = Vec::new();
        let mut overlays = Vec::new();
        for s in 0..SURF_N {
            for m in 0..16 {
                opaque.push(top_layer(s, m));
                if let Some(l) = flat_layer(s, m) {
                    overlays.push(l);
                }
            }
            opaque.push(ramp_layer(s));
        }
        for k in [WallKind::Plain, WallKind::Foot, WallKind::Wet] {
            for c in [WallCap::Left, WallCap::Mid, WallCap::Right, WallCap::Pillar] {
                opaque.push(wall_layer(k, c));
            }
        }
        (opaque, overlays)
    }

    #[test]
    fn layer_indices_are_distinct_and_in_range() {
        let (opaque, overlays) = all_layers();
        let mut seen = std::collections::BTreeSet::new();
        for l in opaque.iter().chain(&overlays) {
            assert!(seen.insert(*l), "layer {l} used twice");
        }
        assert_eq!(seen.len(), LAYERS as usize);
        assert!(seen.iter().all(|&l| l < LAYERS));
        // WebGL2 guarantees only 256 array layers
        assert!(LAYERS <= 256);
    }

    #[test]
    fn builds_from_blank_sheets() {
        let data = vec![0u8; 576 * 384 * 4];
        let sh = || Sheet {
            data: &data,
            width: 576,
            height: 384,
        };
        let sheets = [sh(), sh(), sh(), sh(), sh()];
        let layers = build_layers(&sheets);
        assert_eq!(layers.len(), LAYERS as usize);
        // faces are opaque; only the overlay tiles have transparent texels
        let (opaque, _) = all_layers();
        for l in opaque {
            assert!(layers[l as usize].chunks_exact(4).all(|p| p[3] == 255), "layer {l} not opaque");
        }
        let img = array_image(&layers);
        let count = gl_safe_layers(LAYERS as usize);
        assert!(count % 6 != 0 && count <= 256);
        assert_eq!(img.texture_descriptor.size.depth_or_array_layers as usize, count);
        let expect: usize = (0..MIPS).map(|m| (64usize >> m).max(1).pow(2) * 4).sum::<usize>() * count;
        assert_eq!(img.data.as_ref().map(|d| d.len()), Some(expect));
    }
}
