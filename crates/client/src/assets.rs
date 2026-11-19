// OWNER: C2 (sprites) — spec 01-defs-assets.md (folder rules, sheets), 09-asset-inventory.md §1.2 (assets.json format),
// §12 (baked shadows), 05-control-loop.md §2 / 06-ui.md §11.5 (loading screen: `Caricamento sprite ${loaded}/${total}`).
//
// DECISIONS:
// * Manifest = `docs/spec/assets.json` embedded with `include_str!` (588 PNGs with frame size / anchors / fps).
//   Paths in it start with `assets/`; handles are keyed by the path WITHOUT that prefix (the asset root).
// * Asset root: native = `<repo>/assets` (absolute, set in lib.rs from CARGO_MANIFEST_DIR); web = `assets` relative
//   to the page → `GET /assets/...`, served by ts-server from the repo's `assets/`.
// * Font: `fonts/Fredoka.ttf` is EMBEDDED in the binary (`include_bytes!`) and added with `Font::from_bytes`.
// * WORLD SPRITE ATLAS (C2). The pack holds ~95 M pixels (380 MB as RGBA), 75 % of it empty frame margins. At load
//   time every world sprite sheet is cut into frames, each frame is cropped to its opaque bounding box, baked
//   semi-transparent ground shadows are removed (alpha < 255 → 0, spec 09 §12) and the crops are shelf-packed into a
//   few `ATLAS_PAGE`² pages (`SpriteAtlas`). The 3D billboard renderer draws from these pages only: one dynamic mesh
//   per page and material flavour → a handful of draw calls, ~10 % of the texture memory.
//   Images are classified by `use_of`:
//     - `Use::World` (unit sheets, particle FX, resources, bushes/rocks/duck): loaded CPU-only (never uploaded),
//       packed, then REMOVED from `Assets<Image>` (`GameAssets::image` still returns the handle but it has no data).
//     - `Use::Both` (buildings, Extra units/props/resources, clouds, bar / bracket / ribbon art): normal images (UI
//       thumbnails, portraits) and also packed (shadow-stripped copy).
//     - `Use::Ui` (everything else in UI Elements, icons, tileset, cursor tools): normal images, not packed.
//   `Terrain/Tileset/Water Background color.png` (a flat colour) is not loaded at all.
// * The loading screen counts loaded images; the state switches to `Menu` once the atlas is packed.

use crate::AppState;
use crate::dev::DevArgs;
use crate::link::{LinkRequest, LobbyState};
use bevy::asset::{LoadState, RenderAssetUsages};
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::Deserialize;

/// Embedded manifest (docs/spec/assets.json).
pub const MANIFEST_JSON: &str = include_str!("../../../docs/spec/assets.json");
/// Embedded UI font (fonts/Fredoka.ttf, weight 600 is synthesised by the renderer as needed).
pub const FONT_BYTES: &[u8] = include_bytes!("../../../fonts/Fredoka.ttf");

/// Side of one atlas page in texels (safe on every WebGL2 device).
pub const ATLAS_PAGE: u32 = 2048;
/// Transparent gutter around every packed crop.
const GUTTER: u32 = 1;
/// Pixels scanned per frame while loading (keeps the loading screen responsive on wasm).
const SCAN_BUDGET: u64 = 8_000_000;

/// One `assets.json` row.
#[derive(Deserialize, Clone, Debug)]
pub struct AssetEntry {
    /// repo-relative path (`assets/...`)
    pub path: String,
    pub w: u32,
    pub h: u32,
    pub fw: u32,
    pub fh: u32,
    pub frames: u32,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub ax: Option<f32>,
    #[serde(default)]
    pub ay: Option<f32>,
    #[serde(default)]
    pub fps: Option<f32>,
}

impl AssetEntry {
    /// Path relative to the asset root (no `assets/` prefix).
    pub fn rel(&self) -> &str {
        self.path.strip_prefix("assets/").unwrap_or(&self.path)
    }
}

/// Parsed manifest + loaded handles + the UI font.
#[derive(Resource, Default)]
pub struct GameAssets {
    pub manifest: Vec<AssetEntry>,
    /// rel path → index in `manifest` / `images`
    pub by_path: HashMap<String, usize>,
    /// one handle per manifest row (`Handle::default()` when not wanted). `Use::World` images are emptied once the
    /// sprite atlas is built (use `SpriteAtlas` for them).
    pub images: Vec<Handle<Image>>,
    pub font: Handle<Font>,
    pub loaded: usize,
    pub total: usize,
    pub failed: usize,
}

impl GameAssets {
    /// Image by path relative to `assets/` (e.g. `Terrain/Tileset/Tilemap_color1.png`).
    pub fn image(&self, rel: &str) -> Option<Handle<Image>> {
        self.by_path.get(rel).map(|&i| self.images[i].clone())
    }

    pub fn entry(&self, rel: &str) -> Option<&AssetEntry> {
        self.by_path.get(rel).map(|&i| &self.manifest[i])
    }
}

/// How an image is used (see the header).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    Skip,
    Ui,
    World,
    Both,
}

/// UI art that the world overlays also draw (bars, selection brackets, pennants, castle name plates).
const OVERLAY_ART: [&str; 7] = [
    "UI Elements/UI Elements/Bars/SmallBar_Base.png",
    "UI Elements/UI Elements/Bars/SmallBar_Fill.png",
    "UI Elements/UI Elements/Bars/BigBar_Base.png",
    "UI Elements/UI Elements/Bars/BigBar_Fill.png",
    "UI Elements/UI Elements/Cursors/Cursor_04.png",
    "UI Elements/UI Elements/Ribbons/SmallRibbons.png",
    "UI Elements/UI Banners from the store page/Ribbons/Ribbon_",
];

pub fn use_of(rel: &str) -> Use {
    if rel == "Terrain/Tileset/Water Background color.png" {
        return Use::Skip;
    }
    if OVERLAY_ART.iter().any(|p| rel.starts_with(p)) {
        return Use::Both;
    }
    if rel.starts_with("UI Elements/")
        || rel.starts_with("Terrain/Tileset/")
        || rel.starts_with("Terrain/Resources/Tools/")
        || rel.starts_with("Extra/Icons/")
    {
        return Use::Ui;
    }
    if rel.starts_with("Units/")
        || rel.starts_with("Particle FX/")
        || rel.starts_with("Terrain/Resources/")
        || rel.starts_with("Terrain/Decorations/Bushes/")
        || rel.starts_with("Terrain/Decorations/Rocks")
        || rel.starts_with("Terrain/Decorations/Rubber Duck/")
    {
        return Use::World;
    }
    // Buildings, Extra/{Buildings,Units,Props,Resources}, clouds
    Use::Both
}

/// Whether semi-transparent pixels of this image are baked ground shadows to strip (world sprites).
fn strips_shadow(rel: &str) -> bool {
    !(rel.starts_with("UI Elements/") || rel.starts_with("Particle FX/"))
}

// ---------------------------------------------------------------------------------------------------------------
// Sprite atlas
// ---------------------------------------------------------------------------------------------------------------

/// Index of a sheet in `SpriteAtlas::sheets`.
pub type SheetId = u32;

/// One frame cropped into an atlas page. `w == 0` = empty frame (draw nothing).
#[derive(Clone, Copy, Debug, Default)]
pub struct AtlasFrame {
    pub page: u16,
    /// texel position of the crop in the page
    pub px: u16,
    pub py: u16,
    /// crop size
    pub w: u16,
    pub h: u16,
    /// crop offset inside the original frame (px, origin top-left)
    pub ox: u16,
    pub oy: u16,
}

impl AtlasFrame {
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// Intersects the frame-space rect `[x0, x1) × [y0, y1)` (original frame px) with the crop. Returns the visible
    /// part in frame px and its UV rect `(u0, v0, u1, v1)` in the page.
    pub fn sub(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Option<([f32; 4], [f32; 4])> {
        if self.is_empty() {
            return None;
        }
        let cx0 = x0.max(self.ox as f32);
        let cy0 = y0.max(self.oy as f32);
        let cx1 = x1.min((self.ox + self.w) as f32);
        let cy1 = y1.min((self.oy + self.h) as f32);
        if cx1 <= cx0 || cy1 <= cy0 {
            return None;
        }
        let p = ATLAS_PAGE as f32;
        let u0 = (self.px as f32 + cx0 - self.ox as f32) / p;
        let v0 = (self.py as f32 + cy0 - self.oy as f32) / p;
        let u1 = (self.px as f32 + cx1 - self.ox as f32) / p;
        let v1 = (self.py as f32 + cy1 - self.oy as f32) / p;
        Some(([cx0, cy0, cx1, cy1], [u0, v0, u1, v1]))
    }

    /// Whole crop: frame-space rect + UVs.
    pub fn full(&self) -> Option<([f32; 4], [f32; 4])> {
        self.sub(0.0, 0.0, f32::MAX, f32::MAX)
    }
}

/// A sprite sheet (horizontal strip of frames) or a named region.
#[derive(Clone, Copy, Debug)]
pub struct Sheet {
    pub fw: f32,
    pub fh: f32,
    /// anchor (pivot) in frame px: the pixel drawn on the entity's foot point
    pub ax: f32,
    pub ay: f32,
    pub fps: f32,
    pub frames: u16,
    /// index of frame 0 in `SpriteAtlas::frames`
    pub first: u32,
}

/// The packed world sprite atlas (ready once `ready`).
#[derive(Resource, Default)]
pub struct SpriteAtlas {
    pub ready: bool,
    pub pages: Vec<Handle<Image>>,
    pub frames: Vec<AtlasFrame>,
    pub sheets: Vec<Sheet>,
    /// rel path (or `gen:` / `region:` name) → sheet
    pub by_name: HashMap<String, SheetId>,
}

impl SpriteAtlas {
    pub fn sheet_id(&self, name: &str) -> Option<SheetId> {
        self.by_name.get(name).copied()
    }

    #[inline]
    pub fn sheet(&self, id: SheetId) -> &Sheet {
        &self.sheets[id as usize]
    }

    /// Frame `f` of a sheet, wrapped like `drawSheet` (`((f % n) + n) % n`).
    #[inline]
    pub fn frame(&self, id: SheetId, f: i64) -> &AtlasFrame {
        let s = &self.sheets[id as usize];
        let n = s.frames.max(1) as i64;
        let f = ((f % n) + n) % n;
        &self.frames[s.first as usize + f as usize]
    }
}

/// Named sub-regions of UI art packed for the world overlays: `(name, rel path, [x, y, w, h])`.
pub fn overlay_regions() -> Vec<(String, &'static str, [u32; 4])> {
    let mut v = Vec::new();
    const SB: &str = "UI Elements/UI Elements/Bars/SmallBar_Base.png";
    const BB: &str = "UI Elements/UI Elements/Bars/BigBar_Base.png";
    for (i, x) in [0u32, 128, 256].into_iter().enumerate() {
        v.push((format!("region:smallbar:{i}"), SB, [x, 0, 64, 64]));
        v.push((format!("region:bigbar:{i}"), BB, [x, 0, 64, 64]));
    }
    v.push((
        "region:bigbar_fill".to_string(),
        "UI Elements/UI Elements/Bars/BigBar_Fill.png",
        [0, 22, 64, 21],
    ));
    const CUR: &str = "UI Elements/UI Elements/Cursors/Cursor_04.png";
    for (i, (x, y)) in [(0u32, 0u32), (96, 0), (0, 96), (96, 96)]
        .into_iter()
        .enumerate()
    {
        v.push((format!("region:bracket:{i}"), CUR, [x, y, 32, 32]));
    }
    const RIB: [&str; 5] = [
        "UI Elements/UI Banners from the store page/Ribbons/Ribbon_Blue.png",
        "UI Elements/UI Banners from the store page/Ribbons/Ribbon_Red.png",
        "UI Elements/UI Banners from the store page/Ribbons/Ribbon_Yellow.png",
        "UI Elements/UI Banners from the store page/Ribbons/Ribbon_Purple.png",
        "UI Elements/UI Banners from the store page/Ribbons/Ribbon_Black.png",
    ];
    for (ci, path) in RIB.iter().enumerate() {
        v.push((format!("region:pennant:{ci}"), *path, [51, 448, 90, 108]));
        // small ribbon (castle name plate): SmallRibbons.png, G3, row (ci*2)*64, h 64
        for (i, x) in [0u32, 128, 256].into_iter().enumerate() {
            v.push((
                format!("region:ribbon:{ci}:{i}"),
                "UI Elements/UI Elements/Ribbons/SmallRibbons.png",
                [x, ci as u32 * 2 * 64, 64, 64],
            ));
        }
    }
    v
}

/// Procedurally generated atlas sprites: `(name, w, h, rgba)`.
fn generated_sprites() -> Vec<(String, u32, u32, Vec<u8>)> {
    let mut v = Vec::new();
    // solid white block (bar fills, footprints, dashes)
    v.push(("gen:white".to_string(), 4, 4, vec![255u8; 4 * 4 * 4]));
    // selection ellipses (as seen on screen): radii 24×10 and 38×13, stroke 2.5 px (spec 07 §8.3)
    for (name, rx, ry) in [("gen:ring", 24.0f32, 10.0f32), ("gen:ring_big", 38.0, 13.0)] {
        let w = (rx * 2.0 + 6.0).ceil() as u32;
        let h = (ry * 2.0 + 6.0).ceil() as u32;
        let mut px = vec![0u8; (w * h * 4) as usize];
        let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
        const SS: u32 = 4;
        for y in 0..h {
            for x in 0..w {
                let mut cov = 0u32;
                for sy in 0..SS {
                    for sx in 0..SS {
                        let fx = x as f32 + (sx as f32 + 0.5) / SS as f32 - cx;
                        let fy = y as f32 + (sy as f32 + 0.5) / SS as f32 - cy;
                        // distance to the ellipse, approximated in normalised space
                        let k = ((fx / rx).powi(2) + (fy / ry).powi(2)).sqrt();
                        let g = Vec2::new(fx / (rx * rx), fy / (ry * ry)).length().max(1e-4);
                        let d = (k * k - 1.0) / (2.0 * g);
                        if d.abs() <= 1.25 {
                            cov += 1;
                        }
                    }
                }
                let a = (cov * 255 / (SS * SS)) as u8;
                let i = ((y * w + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[255, 255, 255, a]);
            }
        }
        v.push((name.to_string(), w, h, px));
    }
    // radial glow (night halos, spec 07 §4 glow sprite stops)
    {
        let n = 64u32;
        let mut px = vec![0u8; (n * n * 4) as usize];
        let stops: [(f32, [f32; 4]); 4] = [
            (0.0, [255.0, 220.0, 160.0, 1.0]),
            (0.35, [255.0, 176.0, 92.0, 0.55]),
            (0.7, [255.0, 140.0, 60.0, 0.16]),
            (1.0, [255.0, 120.0, 40.0, 0.0]),
        ];
        for y in 0..n {
            for x in 0..n {
                let dx = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let dy = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let r = (dx * dx + dy * dy).sqrt().min(1.0);
                let mut c = stops[3].1;
                for k in 0..3 {
                    let (t0, c0) = stops[k];
                    let (t1, c1) = stops[k + 1];
                    if r >= t0 && r <= t1 {
                        let f = (r - t0) / (t1 - t0);
                        for j in 0..4 {
                            c[j] = c0[j] + (c1[j] - c0[j]) * f;
                        }
                        break;
                    }
                }
                let i = ((y * n + x) * 4) as usize;
                px[i] = c[0] as u8;
                px[i + 1] = c[1] as u8;
                px[i + 2] = c[2] as u8;
                px[i + 3] = (c[3] * 255.0) as u8;
            }
        }
        v.push(("gen:glow".to_string(), n, n, px));
    }
    v
}

/// Per-image scan result (frame bounding boxes in frame px; `None` = empty frame).
#[derive(Clone, Debug)]
struct Scanned {
    bboxes: Vec<Option<[u32; 4]>>,
}

/// Incremental atlas construction state.
#[derive(Resource, Default)]
struct AtlasBuild {
    scanned: Vec<Option<Scanned>>,
    /// manifest indices still to scan (packed images)
    todo: Vec<usize>,
    started: bool,
    done: bool,
}

fn rgba8(img: &Image) -> Option<std::borrow::Cow<'_, [u8]>> {
    let fmt = img.texture_descriptor.format;
    if matches!(
        fmt,
        TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
    ) {
        return img.data.as_deref().map(std::borrow::Cow::Borrowed);
    }
    let conv = img.convert(TextureFormat::Rgba8UnormSrgb)?;
    conv.data.map(std::borrow::Cow::Owned)
}

/// Opaque bounding box of every frame (`alpha == 255` when stripping baked shadows, else `alpha > 0`).
fn scan_image(e: &AssetEntry, data: &[u8], w: u32, h: u32) -> Scanned {
    let strip = strips_shadow(e.rel());
    let fw = e.fw.clamp(1, w.max(1));
    let fh = e.fh.clamp(1, h.max(1));
    let frames = e.frames.max(1);
    let mut bboxes = Vec::with_capacity(frames as usize);
    for f in 0..frames {
        let x0 = f * fw;
        if x0 >= w {
            bboxes.push(None);
            continue;
        }
        let x1 = (x0 + fw).min(w);
        let (mut bx0, mut by0, mut bx1, mut by1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for y in 0..fh.min(h) {
            let row = (y * w) as usize * 4;
            for x in x0..x1 {
                let a = data[row + x as usize * 4 + 3];
                let solid = if strip { a == 255 } else { a > 0 };
                if solid {
                    bx0 = bx0.min(x - x0);
                    bx1 = bx1.max(x - x0 + 1);
                    by0 = by0.min(y);
                    by1 = by1.max(y + 1);
                }
            }
        }
        bboxes.push(if bx0 == u32::MAX {
            None
        } else {
            Some([bx0, by0, bx1 - bx0, by1 - by0])
        });
    }
    Scanned { bboxes }
}

/// Shelf packer over fixed-size pages.
struct Packer {
    pages: Vec<Vec<u8>>,
    x: u32,
    y: u32,
    shelf_h: u32,
}

impl Packer {
    fn new() -> Packer {
        Packer {
            pages: Vec::new(),
            x: 0,
            y: 0,
            shelf_h: 0,
        }
    }

    /// Reserves `w×h` (+gutter); returns (page, x, y).
    fn alloc(&mut self, w: u32, h: u32) -> (u16, u32, u32) {
        let (gw, gh) = (w + GUTTER * 2, h + GUTTER * 2);
        if self.pages.is_empty() {
            self.new_page();
        }
        if self.x + gw > ATLAS_PAGE {
            self.y += self.shelf_h;
            self.x = 0;
            self.shelf_h = 0;
        }
        if self.y + gh > ATLAS_PAGE {
            self.new_page();
        }
        let r = (
            (self.pages.len() - 1) as u16,
            self.x + GUTTER,
            self.y + GUTTER,
        );
        self.x += gw;
        self.shelf_h = self.shelf_h.max(gh);
        r
    }

    fn new_page(&mut self) {
        self.pages
            .push(vec![0u8; (ATLAS_PAGE * ATLAS_PAGE * 4) as usize]);
        self.x = 0;
        self.y = 0;
        self.shelf_h = 0;
    }

    /// Copies `w×h` texels from `src` (row stride `sw`, origin `sx, sy`) into page `p` at `(dx, dy)`.
    #[allow(clippy::too_many_arguments)]
    fn blit(
        &mut self,
        p: u16,
        dx: u32,
        dy: u32,
        src: &[u8],
        sw: u32,
        sx: u32,
        sy: u32,
        w: u32,
        h: u32,
        strip: bool,
    ) {
        let page = &mut self.pages[p as usize];
        for y in 0..h {
            let s = (((sy + y) * sw + sx) * 4) as usize;
            let d = (((dy + y) * ATLAS_PAGE + dx) * 4) as usize;
            let n = (w * 4) as usize;
            let (srow, drow) = (&src[s..s + n], &mut page[d..d + n]);
            if strip {
                for (sp, dp) in srow.chunks_exact(4).zip(drow.chunks_exact_mut(4)) {
                    if sp[3] == 255 {
                        dp.copy_from_slice(sp);
                    }
                }
            } else {
                drow.copy_from_slice(srow);
            }
        }
    }
}

pub fn parse_manifest() -> Vec<AssetEntry> {
    match serde_json::from_str(MANIFEST_JSON) {
        Ok(v) => v,
        Err(e) => {
            error!("assets.json: {e}");
            Vec::new()
        }
    }
}

pub struct AssetsPlugin;

impl Plugin for AssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameAssets>()
            .init_resource::<SpriteAtlas>()
            .init_resource::<AtlasBuild>()
            .add_systems(Startup, start_loading)
            .add_systems(OnEnter(AppState::Loading), spawn_loading_screen)
            .add_systems(
                Update,
                (scan_loaded, track_loading)
                    .chain()
                    .run_if(in_state(AppState::Loading)),
            );
    }
}

fn start_loading(
    server: Res<AssetServer>,
    mut fonts: ResMut<Assets<Font>>,
    mut ga: ResMut<GameAssets>,
    mut build: ResMut<AtlasBuild>,
) {
    ga.font = fonts.add(Font::from_bytes(FONT_BYTES.to_vec()));
    let manifest = parse_manifest();
    let mut images = Vec::with_capacity(manifest.len());
    let mut by_path = HashMap::default();
    let mut total = 0;
    build.scanned = vec![None; manifest.len()];
    for (i, e) in manifest.iter().enumerate() {
        by_path.insert(e.rel().to_string(), i);
        match use_of(e.rel()) {
            Use::Skip => images.push(Handle::default()),
            Use::World => {
                images.push(
                    server
                        .load_builder()
                        .with_settings(|s: &mut ImageLoaderSettings| {
                            s.asset_usage = RenderAssetUsages::MAIN_WORLD;
                        })
                        .load::<Image>(e.rel().to_string()),
                );
                build.todo.push(i);
                total += 1;
            }
            Use::Both => {
                images.push(server.load::<Image>(e.rel().to_string()));
                build.todo.push(i);
                total += 1;
            }
            Use::Ui => {
                images.push(server.load::<Image>(e.rel().to_string()));
                total += 1;
            }
        }
    }
    ga.manifest = manifest;
    ga.by_path = by_path;
    ga.images = images;
    ga.total = total;
}

/// Scans newly loaded packed images (frame bounding boxes), within a per-frame pixel budget.
fn scan_loaded(ga: Res<GameAssets>, images: Res<Assets<Image>>, mut build: ResMut<AtlasBuild>) {
    if build.done {
        return;
    }
    let mut budget = SCAN_BUDGET;
    let mut i = 0;
    while i < build.todo.len() && budget > 0 {
        let mi = build.todo[i];
        let Some(img) = images.get(&ga.images[mi]) else {
            i += 1;
            continue;
        };
        let e = &ga.manifest[mi];
        let (w, h) = (img.width(), img.height());
        let sc = match rgba8(img) {
            Some(data) if data.len() >= (w * h * 4) as usize => scan_image(e, &data, w, h),
            _ => {
                warn!("immagine non RGBA8: {}", e.rel());
                Scanned { bboxes: Vec::new() }
            }
        };
        budget = budget.saturating_sub(w as u64 * h as u64);
        build.scanned[mi] = Some(sc);
        build.todo.swap_remove(i);
    }
}

/// Packs every scanned image + overlay regions + generated sprites into the atlas pages.
fn pack_atlas(
    ga: &GameAssets,
    images: &mut Assets<Image>,
    build: &AtlasBuild,
    atlas: &mut SpriteAtlas,
) {
    enum Src {
        Frame { mi: usize, f: usize, bb: [u32; 4] },
        Region { mi: usize, rect: [u32; 4] },
        Gen { gi: usize },
    }
    struct Item {
        frame_slot: usize,
        src: Src,
        w: u32,
        h: u32,
    }
    let generated = generated_sprites();
    let mut items: Vec<Item> = Vec::new();
    let mut frames: Vec<AtlasFrame> = Vec::new();
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut by_name: HashMap<String, SheetId> = HashMap::default();

    // sheets of packed images
    for (mi, e) in ga.manifest.iter().enumerate() {
        let Some(sc) = &build.scanned[mi] else {
            continue;
        };
        if sc.bboxes.is_empty() {
            continue;
        }
        let first = frames.len() as u32;
        for (f, bb) in sc.bboxes.iter().enumerate() {
            let slot = frames.len();
            frames.push(AtlasFrame::default());
            if let Some(bb) = bb {
                items.push(Item {
                    frame_slot: slot,
                    src: Src::Frame { mi, f, bb: *bb },
                    w: bb[2],
                    h: bb[3],
                });
            }
        }
        let id = sheets.len() as SheetId;
        sheets.push(Sheet {
            fw: e.fw as f32,
            fh: e.fh as f32,
            ax: e.ax.unwrap_or(e.fw as f32 * 0.5),
            ay: e.ay.unwrap_or(e.fh as f32),
            fps: e.fps.unwrap_or(0.0),
            frames: sc.bboxes.len() as u16,
            first,
        });
        by_name.insert(e.rel().to_string(), id);
    }
    // overlay regions
    for (name, rel, rect) in overlay_regions() {
        let Some(&mi) = ga.by_path.get(rel) else {
            continue;
        };
        if images.get(&ga.images[mi]).is_none() {
            continue;
        }
        let slot = frames.len();
        frames.push(AtlasFrame::default());
        items.push(Item {
            frame_slot: slot,
            src: Src::Region { mi, rect },
            w: rect[2],
            h: rect[3],
        });
        let id = sheets.len() as SheetId;
        sheets.push(Sheet {
            fw: rect[2] as f32,
            fh: rect[3] as f32,
            ax: 0.0,
            ay: 0.0,
            fps: 0.0,
            frames: 1,
            first: slot as u32,
        });
        by_name.insert(name, id);
    }
    // generated
    for (gi, (name, w, h, _)) in generated.iter().enumerate() {
        let slot = frames.len();
        frames.push(AtlasFrame::default());
        items.push(Item {
            frame_slot: slot,
            src: Src::Gen { gi },
            w: *w,
            h: *h,
        });
        let id = sheets.len() as SheetId;
        sheets.push(Sheet {
            fw: *w as f32,
            fh: *h as f32,
            ax: *w as f32 * 0.5,
            ay: *h as f32 * 0.5,
            fps: 0.0,
            frames: 1,
            first: slot as u32,
        });
        by_name.insert(name.clone(), id);
    }

    // pack tallest first
    items.sort_by(|a, b| b.h.cmp(&a.h).then(b.w.cmp(&a.w)));
    let mut packer = Packer::new();
    let mut converted: HashMap<usize, Vec<u8>> = HashMap::default();
    for it in &items {
        let (w, h) = (it.w.min(ATLAS_PAGE - 2), it.h.min(ATLAS_PAGE - 2));
        let (page, px, py) = packer.alloc(w, h);
        let (ox, oy) = match it.src {
            Src::Frame { mi, f, bb } => {
                let img = &images.get(&ga.images[mi]);
                if let Some(img) = img {
                    let iw = img.width();
                    let fmt_ok = matches!(
                        img.texture_descriptor.format,
                        TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
                    );
                    let data: Option<&[u8]> = if fmt_ok {
                        img.data.as_deref()
                    } else {
                        if !converted.contains_key(&mi)
                            && let Some(c) = rgba8(img)
                        {
                            converted.insert(mi, c.into_owned());
                        }
                        converted.get(&mi).map(|v| v.as_slice())
                    };
                    if let Some(data) = data {
                        let e = &ga.manifest[mi];
                        packer.blit(
                            page,
                            px,
                            py,
                            data,
                            iw,
                            f as u32 * e.fw + bb[0],
                            bb[1],
                            w,
                            h,
                            strips_shadow(e.rel()),
                        );
                    }
                }
                (bb[0], bb[1])
            }
            Src::Region { mi, rect } => {
                if let Some(img) = images.get(&ga.images[mi])
                    && let Some(data) = rgba8(img)
                {
                    let (iw, ih) = (img.width(), img.height());
                    let w2 = w.min(iw.saturating_sub(rect[0]));
                    let h2 = h.min(ih.saturating_sub(rect[1]));
                    packer.blit(page, px, py, &data, iw, rect[0], rect[1], w2, h2, false);
                }
                (0, 0)
            }
            Src::Gen { gi } => {
                let (_, gw, _, data) = &generated[gi];
                packer.blit(page, px, py, data, *gw, 0, 0, w, h, false);
                (0, 0)
            }
        };
        frames[it.frame_slot] = AtlasFrame {
            page,
            px: px as u16,
            py: py as u16,
            w: w as u16,
            h: h as u16,
            ox: ox as u16,
            oy: oy as u16,
        };
    }

    let n_pages = packer.pages.len();
    let mut pages = Vec::with_capacity(n_pages);
    for data in packer.pages {
        let mut img = Image::new(
            Extent3d {
                width: ATLAS_PAGE,
                height: ATLAS_PAGE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            ..ImageSamplerDescriptor::nearest()
        });
        pages.push(images.add(img));
    }
    // free the CPU-only world sheets
    let mut freed = 0usize;
    for (mi, e) in ga.manifest.iter().enumerate() {
        if use_of(e.rel()) == Use::World && images.remove(&ga.images[mi]).is_some() {
            freed += 1;
        }
    }
    info!(
        "atlante sprite: {} pagine {}², {} fotogrammi, {} fogli, {} immagini liberate",
        n_pages,
        ATLAS_PAGE,
        frames.len(),
        sheets.len(),
        freed
    );
    atlas.pages = pages;
    atlas.frames = frames;
    atlas.sheets = sheets;
    atlas.by_name = by_name;
    atlas.ready = true;
}

#[derive(Component)]
struct LoadingText;

#[derive(Component)]
struct LoadingBar;

fn spawn_loading_screen(mut commands: Commands, ga: Res<GameAssets>) {
    let font = ga.font.clone();
    commands.spawn((
        DespawnOnExit(AppState::Loading),
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: px(16),
            ..default()
        },
        children![
            (
                Text::new("Tiny Swords"),
                TextFont {
                    font: font.clone().into(),
                    font_size: FontSize::Px(48.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ),
            (
                Node {
                    width: px(420),
                    max_width: percent(60),
                    height: px(16),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(0x1b, 0x1f, 0x33)),
                children![(
                    LoadingBar,
                    Node {
                        width: percent(0),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(0xe3, 0xc8, 0x4e)),
                )],
            ),
            (
                LoadingText,
                Text::new(""),
                TextFont {
                    font: font.into(),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ),
        ],
    ));
}

#[allow(clippy::too_many_arguments)]
fn track_loading(
    server: Res<AssetServer>,
    mut ga: ResMut<GameAssets>,
    mut images: ResMut<Assets<Image>>,
    mut build: ResMut<AtlasBuild>,
    mut atlas: ResMut<SpriteAtlas>,
    mut text: Query<&mut Text, With<LoadingText>>,
    mut bar: Query<&mut Node, With<LoadingBar>>,
    dev: Res<DevArgs>,
    lobby: Res<LobbyState>,
    mut req: MessageWriter<LinkRequest>,
    mut next: ResMut<NextState<AppState>>,
) {
    if build.done {
        return;
    }
    let mut loaded = 0;
    let mut failed = 0;
    let mut first_failed: Option<usize> = None;
    for (i, h) in ga
        .images
        .iter()
        .enumerate()
        .filter(|(_, h)| *h != &Handle::<Image>::default())
    {
        match server.load_state(h.id()) {
            LoadState::Loaded => loaded += 1,
            LoadState::Failed(_) => {
                failed += 1;
                first_failed.get_or_insert(i);
            }
            _ => {}
        }
    }
    if failed > ga.failed {
        warn!("{} immagini non caricate", failed);
    }
    ga.loaded = loaded;
    ga.failed = failed;
    let done = loaded + failed;
    // a missing sprite stops the loading like the 2D game (`renderLoading` shows `G.error`, spec 07 §8.2)
    let error = first_failed
        .and_then(|i| ga.manifest.get(i))
        .map(|e| format!("Impossibile caricare assets/{}", e.rel()));
    for mut t in &mut text {
        let s = error
            .clone()
            .unwrap_or_else(|| format!("Caricamento sprite {}/{}", done, ga.total));
        if t.0 != s {
            t.0 = s;
        }
    }
    if error.is_some() {
        return;
    }
    for mut n in &mut bar {
        n.width = percent(100.0 * done as f32 / ga.total.max(1) as f32);
    }
    if done < ga.total {
        return;
    }
    // every image is in: finish scanning (failed ones are dropped from the queue), then pack once
    let pending = build
        .todo
        .iter()
        .any(|&mi| images.get(&ga.images[mi]).is_some());
    if pending {
        return;
    }
    if !build.started {
        // show the full bar for one frame before the (short) packing stall
        build.started = true;
        return;
    }
    pack_atlas(&ga, &mut images, &build, &mut atlas);
    build.done = true;
    build.scanned.clear();
    build.todo.clear();
    info!("asset caricati: {loaded} (falliti {failed})");
    next.set(AppState::Menu);
    if dev.solo {
        req.write(LinkRequest::StartSolo {
            color: lobby.color,
            level: lobby.level,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parses_and_classifies() {
        let m = parse_manifest();
        assert_eq!(m.len(), 588);
        assert_eq!(
            use_of("Units/Blue Units/Lancer/Lancer_Idle.png"),
            Use::World
        );
        assert_eq!(use_of("Buildings/Red Buildings/Castle.png"), Use::Both);
        assert_eq!(use_of("UI Elements/UI Elements/Icons/Icon_01.png"), Use::Ui);
        assert_eq!(
            use_of("UI Elements/UI Elements/Bars/SmallBar_Base.png"),
            Use::Both
        );
        assert_eq!(use_of("Terrain/Tileset/Tilemap_color1.png"), Use::Ui);
    }

    #[test]
    fn frame_sub_rect() {
        let f = AtlasFrame {
            page: 0,
            px: 100,
            py: 200,
            w: 50,
            h: 40,
            ox: 10,
            oy: 20,
        };
        let (r, uv) = f.sub(0.0, 30.0, 1000.0, 1000.0).unwrap();
        assert_eq!(r, [10.0, 30.0, 60.0, 60.0]);
        assert!((uv[1] - 210.0 / ATLAS_PAGE as f32).abs() < 1e-6);
        assert!(f.sub(0.0, 0.0, 5.0, 5.0).is_none());
    }
}
