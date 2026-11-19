// OWNER: C2 (sprites) — docs/ARCHITECTURE.md (2D sprites standing in the 3D world, alpha-masked camera-facing quads that
// cast shadows, batched per texture), spec 01 (sheets, anchors, folder rules), 03a §anims, 07 §3/§8 (draw rules,
// shadows, overlays), 09 (inventory, anchors, baked shadows), `defs.rs` render anchors/crops.
//
// DESIGN
// * Every world sprite (units, buildings, trees, deposits, decor, FX, overlays) is a quad in ONE dynamic mesh per
//   (layer, atlas page, flavour) — see `assets::SpriteAtlas` (frames cropped + packed into a few pages). A frame is a
//   handful of draw calls for the whole world. Meshes are re-uploaded only when their content changed (hash).
//   * `Layer::Static` = scenery (trees, stumps, gold/stone, big deposits, decor, capture points): rebuilt only when
//     the visible chunk set, an animation tick (8/10 Hz), the fog or a node amount changes.
//   * `Layer::Dynamic` = everything that moves (units, buildings, sheep, projectiles, pickups, FX, overlays): rebuilt
//     every frame for what is in view.
//   * `Flavor`: `Lit` alpha mask, lit, casts shadows · `LitFlat` same without shadow casting (flat props, decals) ·
//     `Translucent` lit alpha blend (construction ghosts, depleted nodes, placement ghost) · `Overlay` unlit blend
//     (rings, bars, brackets, pennants) · `Glow` additive (night halos).
// * Billboards (Octopath style): quads facing the camera yaw, leaning back by `LEAN` × the camera pitch and stretched
//   vertically by `1 / cos(pitch − lean)` so the projected pixel art keeps its native proportions (not foreshortened)
//   while staying upright enough not to sink into cliffs behind them. Content BELOW the anchor (ripples, feet) is
//   folded onto the ground towards the camera. Normals point up: sprites are lit like the ground they stand on and
//   receive the shadows of other sprites.
// * Pixel (ax, ay) of a frame lands on the foot point; 1 sprite px = 1/64 world unit (`consts::PX`).
// * Picking: every pickable sprite in view is projected to a screen rect (`SpriteHits`, rebuilt each frame) using the
//   JS hit boxes of `control.js pick()` (units first, then buildings / points, sheep, nodes, trees).

use crate::AppState;
use crate::assets::{AtlasFrame, SheetId, SpriteAtlas};
use crate::mirror::Mirror;
use crate::render::camera::{CameraRig, RtsCamera};
use crate::render::consts::PX;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::hash::{Hash, Hasher};
use ts_sim::TILE;
use ts_sim::defs::BuildingKind;
use ts_sim::map::Map;
use ts_sim::slab::{BuildingId, UnitId};

// ---------------------------------------------------------------------------------------------------------------
// Cross-module contract (C2): picking hits + placement ghost.
// ---------------------------------------------------------------------------------------------------------------

/// What a sprite on screen stands for (picking).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HitTarget {
    Unit(UnitId),
    Building(BuildingId),
    /// tree / gold / stone (small or big)
    Node(ts_sim::slab::NodeId),
    Point(ts_sim::slab::PointId),
    Sheep(ts_sim::slab::SheepId),
    Pickup(ts_sim::slab::PickupId),
}

impl HitTarget {
    /// The sim `EntityRef` (pickups have none).
    pub fn entity_ref(self) -> Option<ts_sim::cmd::EntityRef> {
        use ts_sim::cmd::EntityRef as E;
        Some(match self {
            HitTarget::Unit(i) => E::Unit(i),
            HitTarget::Building(i) => E::Building(i),
            HitTarget::Node(i) => E::Node(i),
            HitTarget::Point(i) => E::Point(i),
            HitTarget::Sheep(i) => E::Sheep(i),
            HitTarget::Pickup(_) => return None,
        })
    }
}

/// Pick priority classes of `control.js pick()` (lower wins).
pub mod prio {
    pub const UNIT: u8 = 0;
    pub const BUILDING: u8 = 1;
    pub const POINT: u8 = 2;
    pub const SHEEP: u8 = 3;
    pub const DEPOSIT: u8 = 4;
    pub const TREE: u8 = 5;
    pub const PICKUP: u8 = 6;
}

/// One pickable sprite drawn this frame.
#[derive(Clone, Copy, Debug)]
pub struct SpriteHit {
    pub target: HitTarget,
    /// hit box in window logical pixels (origin top-left, like `Window::cursor_position`)
    pub screen_rect: Rect,
    /// distance from the camera (smaller = in front)
    pub depth: f32,
    /// pick class (`prio::*`): `SpriteHits::at` prefers the lowest class, then the front-most sprite
    pub priority: u8,
    /// screen point of the entity used by box selection (`(x, y - 26)` for units, spec 05 `boxSelect`)
    pub anchor: Vec2,
    /// sim position (px) of the entity foot
    pub foot: Vec2,
}

/// Sprites drawn this frame, rebuilt every frame.
#[derive(Resource, Default, Debug)]
pub struct SpriteHits {
    pub hits: Vec<SpriteHit>,
}

impl SpriteHits {
    /// Best hit under `p` (logical window px): lowest `priority` class, then front-most.
    pub fn at(&self, p: Vec2) -> Option<&SpriteHit> {
        self.hits
            .iter()
            .filter(|h| h.screen_rect.contains(p))
            .min_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then(a.depth.total_cmp(&b.depth))
            })
    }

    /// Every hit whose `anchor` lies inside `r` (box selection helper).
    pub fn inside(&self, r: Rect) -> impl Iterator<Item = &SpriteHit> {
        self.hits.iter().filter(move |h| r.contains(h.anchor))
    }

    /// Screen rect of a target drawn this frame.
    pub fn rect_of(&self, t: HitTarget) -> Option<Rect> {
        self.hits
            .iter()
            .find(|h| h.target == t)
            .map(|h| h.screen_rect)
    }
}

/// Placement / move preview set by control (C4) and drawn by the sprite renderer.
#[derive(Resource, Debug, Clone, Default)]
pub struct Ghost {
    /// `None` = no ghost
    pub kind: Option<BuildingKind>,
    /// top-left footprint cell
    pub cell: IVec2,
    /// placement allowed (green) or refused (red)
    pub valid: bool,
    /// sprite variant (houses 0..2)
    pub variant: u8,
    /// building being moved (its sprite is hidden while the ghost shows), if any
    pub moving: Option<BuildingId>,
    /// extra footprints (shift + drag palisade rows), each `(cell, valid)`
    pub chain: Vec<(IVec2, bool)>,
}

#[derive(Component)]
pub struct UnitVisual(pub UnitId);

#[derive(Component)]
pub struct BuildingVisual(pub BuildingId);

// ---------------------------------------------------------------------------------------------------------------
// Batching
// ---------------------------------------------------------------------------------------------------------------

/// Leaning of billboards, as a fraction of the camera pitch from vertical (1 = fully camera-facing).
pub const LEAN: f32 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Layer {
    Static = 0,
    Dynamic = 1,
}
pub const LAYERS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Flavor {
    /// alpha mask, lit, casts shadows
    Lit = 0,
    /// alpha mask, lit, no shadow casting
    LitFlat = 1,
    /// alpha blend, lit
    Translucent = 2,
    /// alpha blend, unlit, not fogged (overlays)
    Overlay = 3,
    /// additive, unlit (halos)
    Glow = 4,
}
pub const FLAVORS: usize = 5;

/// System sets of the sprite pipeline (Update): prepare (clock, camera basis) → draw (fill batches) → upload.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpriteSystems {
    Prepare,
    Draw,
    Upload,
}

#[derive(Default)]
pub struct QuadBuf {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub col: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
}

impl QuadBuf {
    fn clear(&mut self) {
        self.pos.clear();
        self.nrm.clear();
        self.uv.clear();
        self.col.clear();
        self.idx.clear();
    }

    fn hash(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.pos.len().hash(&mut h);
        let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: f32| {
            acc ^= v.to_bits() as u64;
            acc = acc.wrapping_mul(0x0100_0000_01b3);
        };
        for p in &self.pos {
            mix(p[0]);
            mix(p[1]);
            mix(p[2]);
        }
        for u in &self.uv {
            mix(u[0]);
            mix(u[1]);
        }
        for c in &self.col {
            mix(c[0]);
            mix(c[1]);
            mix(c[2]);
            mix(c[3]);
        }
        acc.hash(&mut h);
        h.finish()
    }

    /// Appends a quad (corners bl, br, tr, tl; uv rect u0,v0 = top-left texel corner).
    #[inline]
    pub fn quad(&mut self, c: [Vec3; 4], uv: [f32; 4], col: [f32; 4], flip: bool) {
        let base = self.pos.len() as u32;
        let (ul, ur) = if flip { (uv[2], uv[0]) } else { (uv[0], uv[2]) };
        self.pos.extend(c.iter().map(|v| v.to_array()));
        self.nrm.extend([[0.0, 1.0, 0.0]; 4]);
        self.uv
            .extend([[ul, uv[3]], [ur, uv[3]], [ur, uv[1]], [ul, uv[1]]]);
        self.col.extend([col; 4]);
        self.idx
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

pub struct Batch {
    pub entity: Entity,
    pub mesh: Handle<Mesh>,
    pub buf: QuadBuf,
    last_hash: u64,
    shown: bool,
}

/// All batches + their materials (created once the atlas is ready).
#[derive(Resource, Default)]
pub struct SpriteBatches {
    pub pages: usize,
    pub batches: Vec<Batch>,
    pub materials: Vec<Handle<StandardMaterial>>,
    /// static layer key of the last rebuild (`None` = rebuild)
    pub static_key: Option<u64>,
    /// whether lighting is enabled (quality level > 0): flips `unlit` on the lit materials
    pub lit: bool,
}

impl SpriteBatches {
    #[inline]
    fn index(&self, layer: Layer, page: usize, flavor: Flavor) -> usize {
        ((layer as usize) * self.pages + page) * FLAVORS + flavor as usize
    }

    #[inline]
    pub fn buf(&mut self, layer: Layer, page: usize, flavor: Flavor) -> Option<&mut QuadBuf> {
        if page >= self.pages {
            return None;
        }
        let i = self.index(layer, page, flavor);
        self.batches.get_mut(i).map(|b| &mut b.buf)
    }

    pub fn clear_layer(&mut self, layer: Layer) {
        let n = self.pages * FLAVORS;
        let start = layer as usize * n;
        for b in self.batches.iter_mut().skip(start).take(n) {
            b.buf.clear();
        }
    }
}

/// Camera-derived billboard basis and projection for this frame.
#[derive(Resource, Clone, Debug)]
pub struct Basis {
    pub valid: bool,
    /// world offset of one sprite px to the right
    pub right: Vec3,
    /// world offset of one sprite px up the billboard (lean + anti-foreshortening stretch included)
    pub up: Vec3,
    /// world offset of one sprite px "down" folded onto the ground (towards the camera)
    pub down_ground: Vec3,
    /// unit vector from the scene towards the camera (horizontal)
    pub toward: Vec3,
    pub eye: Vec3,
    pub clip_from_world: Mat4,
    /// logical viewport size
    pub viewport: Vec2,
    /// visible ground area (sim px), with margins for tall sprites: `[x0, y0, x1, y1]`
    pub cull: [f32; 4],
}

impl Default for Basis {
    fn default() -> Self {
        Basis {
            valid: false,
            right: Vec3::X * PX,
            up: Vec3::Y * PX,
            down_ground: Vec3::Z * PX,
            toward: Vec3::Z,
            eye: Vec3::new(0.0, 30.0, 30.0),
            clip_from_world: Mat4::IDENTITY,
            viewport: Vec2::new(1280.0, 720.0),
            cull: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

impl Basis {
    /// World point → logical screen px + view distance.
    #[inline]
    pub fn project(&self, p: Vec3) -> Option<(Vec2, f32)> {
        let c = self.clip_from_world * p.extend(1.0);
        if c.w <= 1e-4 {
            return None;
        }
        let ndc = c.truncate() / c.w;
        let s = Vec2::new(
            (ndc.x * 0.5 + 0.5) * self.viewport.x,
            (0.5 - ndc.y * 0.5) * self.viewport.y,
        );
        Some((s, c.w))
    }

    /// Whether a sim point (px) is inside the culling rect, with an extra margin (px).
    #[inline]
    pub fn in_view(&self, x: f32, y: f32, margin: f32) -> bool {
        x >= self.cull[0] - margin
            && x <= self.cull[2] + margin
            && y >= self.cull[1] - margin
            && y <= self.cull[3] + margin
    }

    /// Screen rect of a billboard box: foot world position, horizontal half-width (px) and vertical px range
    /// `[bottom, top]` above the foot.
    pub fn box_rect(&self, foot: Vec3, half_w: f32, bottom: f32, top: f32) -> Option<(Rect, f32)> {
        let a = foot + self.right * -half_w + self.up * bottom;
        let b = foot + self.right * half_w + self.up * top;
        let (pa, d) = self.project(a)?;
        let (pb, _) = self.project(b)?;
        Some((Rect::from_corners(pa, pb), d))
    }
}

/// Options of one sprite draw.
#[derive(Clone, Copy, Debug)]
pub struct SpriteOpts {
    pub flip: bool,
    pub scale: f32,
    pub color: [f32; 4],
    /// lift above the foot, sprite px (2D `y - h` offsets)
    pub lift: f32,
    /// only the frame rows `[y0, y1)` (frame px)
    pub rows: Option<(f32, f32)>,
    /// only the frame columns `[x0, x1)` (frame px)
    pub cols: Option<(f32, f32)>,
    /// rotation in the billboard plane (radians, counter-clockwise on screen) around the anchor
    pub rot: f32,
    /// fold the part below the anchor onto the ground
    pub fold: bool,
    /// pull towards the camera (world units) without changing the projection (overlays)
    pub pull: f32,
}

impl Default for SpriteOpts {
    fn default() -> Self {
        SpriteOpts {
            flip: false,
            scale: 1.0,
            color: [1.0; 4],
            lift: 0.0,
            rows: None,
            cols: None,
            rot: 0.0,
            fold: true,
            pull: 0.0,
        }
    }
}

/// Mutable drawing context for one system.
pub struct Painter<'a> {
    pub batches: &'a mut SpriteBatches,
    pub basis: &'a Basis,
    pub atlas: &'a SpriteAtlas,
}

impl Painter<'_> {
    #[inline]
    fn pull_corners(&self, c: &mut [Vec3; 4], k: f32) {
        if k <= 0.0 {
            return;
        }
        for v in c.iter_mut() {
            let d = self.basis.eye - *v;
            let len = d.length();
            if len > k * 1.5 {
                *v += d / len * k;
            }
        }
    }

    /// Draws frame `frame` of `sheet` with its anchor on `foot` (world). Returns the drawn frame-space rect.
    pub fn sprite(
        &mut self,
        layer: Layer,
        flavor: Flavor,
        sheet: SheetId,
        frame: i64,
        foot: Vec3,
        o: SpriteOpts,
    ) -> Option<[f32; 4]> {
        let s = *self.atlas.sheet(sheet);
        let f = *self.atlas.frame(sheet, frame);
        self.frame(layer, flavor, &f, s.ax, s.ay, foot, o)
    }

    /// Draws an atlas frame with anchor `(ax, ay)` on `foot`.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        layer: Layer,
        flavor: Flavor,
        f: &AtlasFrame,
        ax: f32,
        ay: f32,
        foot: Vec3,
        o: SpriteOpts,
    ) -> Option<[f32; 4]> {
        let (rx0, rx1) = o.cols.unwrap_or((0.0, f32::MAX));
        let (ry0, ry1) = o.rows.unwrap_or((0.0, f32::MAX));
        let (r, uv) = f.sub(rx0, ry0, rx1, ry1)?;
        let page = f.page as usize;
        let b = self.basis;
        let sc = o.scale;
        // local px: x right of the anchor, y up from the anchor
        let (mut lx0, mut lx1) = ((r[0] - ax) * sc, (r[2] - ax) * sc);
        if o.flip {
            (lx0, lx1) = (-lx1, -lx0);
        }
        let ytop = (ay - r[1]) * sc;
        let ybot = (ay - r[3]) * sc;
        let lift = b.up * o.lift;
        let base = foot + lift;
        let col = o.color;
        if o.rot != 0.0 {
            let (sn, cs) = o.rot.sin_cos();
            let rot = |x: f32, y: f32| -> Vec3 {
                let (rx, ry) = (x * cs - y * sn, x * sn + y * cs);
                base + b.right * rx + b.up * ry
            };
            let mut c = [rot(lx0, ybot), rot(lx1, ybot), rot(lx1, ytop), rot(lx0, ytop)];
            self.pull_corners(&mut c, o.pull);
            let buf = self.batches.buf(layer, page, flavor)?;
            buf.quad(c, uv, col, o.flip);
            return Some(r);
        }
        if o.fold && ybot < 0.0 && o.lift <= 0.0 {
            // part above the anchor stands up, the rest lies on the ground towards the camera
            if ytop > 0.0 {
                let vsplit = uv[1] + (uv[3] - uv[1]) * (ytop / (ytop - ybot));
                let mut c = [
                    base + b.right * lx0,
                    base + b.right * lx1,
                    base + b.right * lx1 + b.up * ytop,
                    base + b.right * lx0 + b.up * ytop,
                ];
                self.pull_corners(&mut c, o.pull);
                let uv_up = [uv[0], uv[1], uv[2], vsplit];
                let gd = -ybot;
                let g = base + Vec3::Y * 0.004;
                let mut cg = [
                    g + b.right * lx0 + b.down_ground * gd,
                    g + b.right * lx1 + b.down_ground * gd,
                    g + b.right * lx1,
                    g + b.right * lx0,
                ];
                self.pull_corners(&mut cg, o.pull);
                let uv_dn = [uv[0], vsplit, uv[2], uv[3]];
                let buf = self.batches.buf(layer, page, flavor)?;
                buf.quad(c, uv_up, col, o.flip);
                buf.quad(cg, uv_dn, col, o.flip);
            } else {
                let g = base + Vec3::Y * 0.004;
                let mut cg = [
                    g + b.right * lx0 + b.down_ground * (-ybot),
                    g + b.right * lx1 + b.down_ground * (-ybot),
                    g + b.right * lx1 + b.down_ground * (-ytop),
                    g + b.right * lx0 + b.down_ground * (-ytop),
                ];
                self.pull_corners(&mut cg, o.pull);
                let buf = self.batches.buf(layer, page, flavor)?;
                buf.quad(cg, uv, col, o.flip);
            }
            return Some(r);
        }
        let mut c = [
            base + b.right * lx0 + b.up * ybot,
            base + b.right * lx1 + b.up * ybot,
            base + b.right * lx1 + b.up * ytop,
            base + b.right * lx0 + b.up * ytop,
        ];
        self.pull_corners(&mut c, o.pull);
        let buf = self.batches.buf(layer, page, flavor)?;
        buf.quad(c, uv, col, o.flip);
        Some(r)
    }

    /// A flat quad lying on the ground (rings, footprints): centre `c` (world), half sizes in px along X and along
    /// the ground towards the camera (`hz` is the on-screen half height: stretched by the pitch).
    pub fn ground_quad(
        &mut self,
        layer: Layer,
        flavor: Flavor,
        f: &AtlasFrame,
        c: Vec3,
        hx: f32,
        hz: f32,
        color: [f32; 4],
    ) {
        let Some((_, uv)) = f.full() else { return };
        let b = self.basis;
        let r = b.right * hx;
        let d = b.down_ground * hz;
        let c = c + Vec3::Y * 0.02;
        let q = [c - r + d, c + r + d, c + r - d, c - r - d];
        if let Some(buf) = self.batches.buf(layer, f.page as usize, flavor) {
            buf.quad(q, uv, color, false);
        }
    }

    /// A flat world-aligned rect on the ground (footprints): world xz rect at height `y`.
    pub fn ground_rect(
        &mut self,
        layer: Layer,
        flavor: Flavor,
        f: &AtlasFrame,
        x0: f32,
        z0: f32,
        x1: f32,
        z1: f32,
        y: f32,
        color: [f32; 4],
    ) {
        let Some((_, uv)) = f.full() else { return };
        let q = [
            Vec3::new(x0, y, z1),
            Vec3::new(x1, y, z1),
            Vec3::new(x1, y, z0),
            Vec3::new(x0, y, z0),
        ];
        if let Some(buf) = self.batches.buf(layer, f.page as usize, flavor) {
            buf.quad(q, uv, color, false);
        }
    }

    /// Screen-aligned overlay rect in billboard px around `foot`: local px `[x0, x1] × [ybot, ytop]` (y up) of the
    /// frame region `uvsrc` (frame px rect inside `f`), pulled towards the camera.
    #[allow(clippy::too_many_arguments)]
    pub fn overlay_rect(
        &mut self,
        f: &AtlasFrame,
        src: [f32; 4],
        foot: Vec3,
        x0: f32,
        x1: f32,
        ybot: f32,
        ytop: f32,
        color: [f32; 4],
        pull: f32,
    ) {
        if x1 - x0 <= 0.01 || ytop - ybot <= 0.01 {
            return;
        }
        let Some((_, uv)) = f.sub(src[0], src[1], src[2], src[3]) else {
            return;
        };
        let b = self.basis;
        let mut c = [
            foot + b.right * x0 + b.up * ybot,
            foot + b.right * x1 + b.up * ybot,
            foot + b.right * x1 + b.up * ytop,
            foot + b.right * x0 + b.up * ytop,
        ];
        self.pull_corners(&mut c, pull);
        if let Some(buf) = self
            .batches
            .buf(Layer::Dynamic, f.page as usize, Flavor::Overlay)
        {
            buf.quad(c, uv, color, false);
        }
    }
}

/// Fog access for the renderers (explored = known; visible = currently seen).
pub struct FogView<'a> {
    pub explored: &'a ts_sim::math::BitGrid,
    pub visible: Option<&'a [u8]>,
    pub w: i32,
    pub h: i32,
    pub off: bool,
}

impl FogView<'_> {
    #[inline]
    pub fn cell(&self, x: f32, y: f32) -> Option<usize> {
        let cx = (x / TILE).floor() as i32;
        let cy = (y / TILE).floor() as i32;
        if cx < 0 || cy < 0 || cx >= self.w || cy >= self.h {
            return None;
        }
        Some((cy * self.w + cx) as usize)
    }

    /// `Fog.knows`: the cell has been explored.
    #[inline]
    pub fn knows(&self, x: f32, y: f32) -> bool {
        if self.off {
            return true;
        }
        self.cell(x, y).is_some_and(|c| self.explored.get(c))
    }

    /// `Fog.sees`: the cell is currently visible.
    #[inline]
    pub fn sees(&self, x: f32, y: f32) -> bool {
        if self.off {
            return true;
        }
        match (self.cell(x, y), self.visible) {
            (Some(c), Some(v)) => v.get(c).is_some_and(|&s| s != 0),
            (Some(c), None) => self.explored.get(c),
            _ => false,
        }
    }

    /// Colour multiplier: full in sight, dimmed under fog (`shade 0.42` towards navy, spec 07 §8.2 step 14).
    #[inline]
    pub fn tint(&self, x: f32, y: f32) -> [f32; 4] {
        if self.sees(x, y) {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            FOG_TINT
        }
    }
}

/// Fogged (explored, not visible) colour multiplier.
pub const FOG_TINT: [f32; 4] = [0.6, 0.62, 0.7, 1.0];

/// Ground height (world units) under a sim point (px).
#[inline]
pub fn ground_at(map: &Map, x: f32, y: f32) -> f32 {
    crate::render::terrain::ground_height(map, x * PX, y * PX)
}

/// World position of a sim foot point.
#[inline]
pub fn foot_world(map: &Map, x: f32, y: f32) -> Vec3 {
    Vec3::new(x * PX, ground_at(map, x, y), y * PX)
}

// ---------------------------------------------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------------------------------------------

pub struct SpritesPlugin;

impl Plugin for SpritesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpriteHits>()
            .init_resource::<Ghost>()
            .init_resource::<SpriteBatches>()
            .init_resource::<Basis>()
            .configure_sets(
                Update,
                (
                    SpriteSystems::Prepare,
                    SpriteSystems::Draw,
                    SpriteSystems::Upload,
                )
                    .chain(),
            )
            .add_plugins((
                super::anim::AnimPlugin,
                super::sprites_world::SpritesWorldPlugin,
                super::sprites_overlay::SpritesOverlayPlugin,
            ))
            .add_systems(
                Update,
                (
                    (ensure_batches, compute_basis)
                        .chain()
                        .in_set(SpriteSystems::Prepare),
                    begin_frame
                        .in_set(SpriteSystems::Draw)
                        .before(DrawStage::Fill),
                    upload_batches.in_set(SpriteSystems::Upload),
                )
                    .run_if(in_state(AppState::InGame)),
            )
            .configure_sets(
                Update,
                (
                    DrawStage::Fill.in_set(SpriteSystems::Draw),
                    DrawStage::Late.in_set(SpriteSystems::Draw).after(DrawStage::Fill),
                ),
            )
            .add_systems(OnExit(AppState::InGame), hide_all);
    }
}

/// Inner ordering of the draw set: everything that fills batches runs in `Fill`, after `begin_frame`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum DrawStage {
    Fill,
    /// after every world draw (overlays, halos, point lights)
    Late,
}

/// Creates the batch meshes / materials once the atlas is packed.
fn ensure_batches(
    mut commands: Commands,
    atlas: Res<SpriteAtlas>,
    mut batches: ResMut<SpriteBatches>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !atlas.ready || !batches.batches.is_empty() {
        return;
    }
    let pages = atlas.pages.len();
    batches.pages = pages;
    batches.lit = true;
    let mut mats = Vec::with_capacity(pages * FLAVORS);
    for page in &atlas.pages {
        for fl in 0..FLAVORS {
            let (alpha_mode, unlit) = match fl {
                0 | 1 => (AlphaMode::Mask(0.5), false),
                2 => (AlphaMode::Blend, false),
                3 => (AlphaMode::Blend, true),
                _ => (AlphaMode::Add, true),
            };
            mats.push(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(page.clone()),
                alpha_mode,
                unlit,
                cull_mode: None,
                double_sided: false,
                perceptual_roughness: 1.0,
                metallic: 0.0,
                reflectance: 0.0,
                fog_enabled: fl != 3 && fl != 4,
                ..default()
            }));
        }
    }
    for layer in 0..LAYERS {
        for page in 0..pages {
            for fl in 0..FLAVORS {
                let mesh = meshes.add(empty_mesh());
                let mut e = commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(mats[page * FLAVORS + fl].clone()),
                    Transform::IDENTITY,
                    Visibility::Hidden,
                    NoFrustumCulling,
                    Name::new(format!("sprites l{layer} p{page} f{fl}")),
                ));
                if fl != 0 {
                    e.insert(NotShadowCaster);
                }
                let entity = e.id();
                batches.batches.push(Batch {
                    entity,
                    mesh,
                    buf: QuadBuf::default(),
                    last_hash: 0,
                    shown: false,
                });
            }
        }
    }
    batches.materials = mats;
    batches.static_key = None;
}

fn empty_mesh() -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0f32, 0.0, 0.0]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0f32, 1.0, 0.0]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0f32, 0.0]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1f32, 1.0, 1.0, 1.0]; 3])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2]))
}

/// Billboard basis, projection and culling rect from the camera.
fn compute_basis(
    cams: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    rig: Res<CameraRig>,
    mut basis: ResMut<Basis>,
) {
    let Ok((cam, tf)) = cams.single() else {
        basis.valid = false;
        return;
    };
    let fwd: Vec3 = *tf.forward();
    let mut fh = Vec3::new(fwd.x, 0.0, fwd.z);
    if fh.length_squared() < 1e-6 {
        fh = Vec3::NEG_Z;
    }
    let fh = fh.normalize();
    let right = Vec3::new(-fh.z, 0.0, fh.x);
    // camera pitch below the horizon
    let pitch = (-fwd.y).clamp(-1.0, 1.0).asin().max(0.05);
    let lean = LEAN * pitch;
    let up_dir = (Vec3::Y * lean.cos() + fh * lean.sin()).normalize();
    let stretch = 1.0 / (pitch - lean).cos().max(0.2);
    let ground_stretch = 1.0 / pitch.sin().max(0.2);
    basis.right = right * PX;
    basis.up = up_dir * (PX * stretch);
    basis.down_ground = -fh * (PX * ground_stretch);
    basis.toward = -fh;
    basis.eye = tf.translation();
    let view = tf.to_matrix().inverse();
    basis.clip_from_world = cam.clip_from_view() * view;
    basis.viewport = cam.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
    // culling rect from the ground points under the screen corners (+ room for tall sprites to the north)
    let c = rig.view_corners;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in c {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    if !(x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()) || x1 <= x0 {
        let f = rig.cur_focus;
        (x0, y0, x1, y1) = (f.x - 1500.0, f.y - 1000.0, f.x + 1500.0, f.y + 1000.0);
    }
    basis.cull = [x0 - 96.0, y0 - 64.0, x1 + 96.0, y1 + 6.0 * TILE];
    basis.valid = true;
}

/// Clears the dynamic layer (the static one is cleared by its owner when rebuilt) and the hits.
fn begin_frame(mut batches: ResMut<SpriteBatches>, mut hits: ResMut<SpriteHits>) {
    batches.clear_layer(Layer::Dynamic);
    hits.hits.clear();
}

/// Uploads changed batches.
fn upload_batches(
    mut batches: ResMut<SpriteBatches>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut vis: Query<&mut Visibility>,
) {
    for b in batches.batches.iter_mut() {
        let empty = b.buf.idx.is_empty();
        if empty {
            if b.shown {
                b.shown = false;
                b.last_hash = 0;
                if let Ok(mut v) = vis.get_mut(b.entity) {
                    *v = Visibility::Hidden;
                }
            }
            continue;
        }
        let h = b.buf.hash();
        if h != b.last_hash {
            b.last_hash = h;
            if let Some(mut m) = meshes.get_mut(&b.mesh) {
                m.insert_attribute(Mesh::ATTRIBUTE_POSITION, b.buf.pos.clone());
                m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, b.buf.nrm.clone());
                m.insert_attribute(Mesh::ATTRIBUTE_UV_0, b.buf.uv.clone());
                m.insert_attribute(Mesh::ATTRIBUTE_COLOR, b.buf.col.clone());
                m.insert_indices(Indices::U32(b.buf.idx.clone()));
            }
        }
        if !b.shown {
            b.shown = true;
            if let Ok(mut v) = vis.get_mut(b.entity) {
                *v = Visibility::Inherited;
            }
        }
    }
}

/// Leaving the match: hide every batch, forget the static layer.
fn hide_all(
    mut batches: ResMut<SpriteBatches>,
    mut vis: Query<&mut Visibility>,
    mut hits: ResMut<SpriteHits>,
) {
    for b in batches.batches.iter_mut() {
        b.buf.clear();
        b.shown = false;
        b.last_hash = 0;
        if let Ok(mut v) = vis.get_mut(b.entity) {
            *v = Visibility::Hidden;
        }
    }
    batches.static_key = None;
    hits.hits.clear();
}

/// Convenience: is the mirror in a drawable state?
pub fn drawable<'a>(mirror: &'a Mirror, atlas: &SpriteAtlas, basis: &Basis) -> Option<&'a Map> {
    if !atlas.ready || !basis.valid {
        return None;
    }
    mirror.map()
}
