// OWNER: C1 (terrain) — client fog of war (spec 05 §12.1-§12.2 + addenda C3): `FogTexture`.
//
// CONTRACT: `FogTexture` resource (read by C2 sprites to hide/dim billboards, C3 minimap, C4 picking):
// * `image`: R8 texture, one texel per cell, linearly filtered; value = current fog opacity `shade` × 255
//   (0 = visible, 0.42 = explored but not visible, 1 = never explored) exactly as the JS `Fog.shade`; the terrain,
//   water and foam shaders darken / desaturate by it.
// * `visible` (client-computed every 0.2 s from own units — sheltered ones too —, own buildings (sites: 2 cells,
//   from `(x, y-30)`) and own capture points (from `(x, y-64)`), radii + `mods.sight`), `explored` (snapshot
//   memory OR visible), `on` (false for spectators, fog-off test mode, match over), and `sees_px` / `knows_px` /
//   `shade_px` queries with the JS semantics.
// * `shade` chases its target with `k = min(1, dt*5)` per frame (fade in/out like the JS).

use crate::AppState;
use crate::mirror::Mirror;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ts_sim::TILE;
use ts_sim::defs::{BUILDING_SIGHT_DY, POINT_SIGHT, SITE_SIGHT, VISIBLE_PERIOD};
use ts_sim::math::{BitGrid, V2};

/// Opacity of explored-but-not-visible cells (`Math.fround(0.42)`).
pub const EXPLORED_SHADE: f32 = 0.42;
/// Fog colour rgb(10, 14, 26).
pub const FOG_RGB: [u8; 3] = [10, 14, 26];

#[derive(Resource)]
pub struct FogTexture {
    /// R8Unorm `w × h`, texel = shade × 255
    pub image: Handle<Image>,
    pub w: i32,
    pub h: i32,
    /// fog applies (a kingdom's player, fog not switched off, match not over)
    pub on: bool,
    /// cells currently seen
    pub visible: BitGrid,
    /// cells ever seen (snapshot memory ∪ visible)
    pub explored: BitGrid,
    /// smoothed opacity per cell (0 visible … 1 unexplored)
    pub shade: Vec<f32>,
    timer: f32,
    settled: bool,
    /// texel bytes last uploaded (`shade × 255`): the texture is re-uploaded only when one of them changes
    bytes: Vec<u8>,
    generation: Option<u32>,
    /// dev switch (native env `TS_NOFOG=1`): render without fog of war
    dev_off: bool,
}

impl FogTexture {
    #[inline]
    fn cell(&self, x: f32, y: f32) -> Option<usize> {
        let cx = (x / TILE).floor() as i32;
        let cy = (y / TILE).floor() as i32;
        if cx < 0 || cy < 0 || cx >= self.w || cy >= self.h || !x.is_finite() || !y.is_finite() {
            return None;
        }
        Some((cy * self.w + cx) as usize)
    }

    /// `Fog.sees(x, y)`: currently visible (outside the map → false while the fog is on).
    pub fn sees_px(&self, x: f32, y: f32) -> bool {
        !self.on || self.cell(x, y).is_some_and(|c| self.visible.get(c))
    }

    /// `Fog.knows(x, y)`: explored.
    pub fn knows_px(&self, x: f32, y: f32) -> bool {
        !self.on || self.cell(x, y).is_some_and(|c| self.explored.get(c))
    }

    /// Current (smoothed) fog opacity at a point: 0 visible, 0.42 explored, 1 unexplored.
    pub fn shade_px(&self, x: f32, y: f32) -> f32 {
        match self.cell(x, y) {
            Some(c) => self.shade.get(c).copied().unwrap_or(1.0),
            None => {
                if self.on {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// `Fog.sees` by cell index.
    pub fn sees_cell(&self, c: i32) -> bool {
        !self.on || (c >= 0 && self.visible.get(c as usize))
    }

    /// `Fog.knows` by cell index.
    pub fn knows_cell(&self, c: i32) -> bool {
        !self.on || (c >= 0 && self.explored.get(c as usize))
    }
}

/// Native dev aid: `TS_NOFOG=1` disables the fog of war (screenshots of the whole map).
fn dev_fog_off() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("TS_NOFOG").is_ok_and(|v| v == "1")
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

/// Creates the fog image + resource (all unexplored).
pub fn init_fog(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let (w, h) = (ts_sim::MAP_W as i32, ts_sim::MAP_H as i32);
    let mut img = Image::new_fill(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255],
        TextureFormat::R8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    let n = (w * h) as usize;
    commands.insert_resource(FogTexture {
        image: images.add(img),
        w,
        h,
        on: true,
        visible: BitGrid::new(n),
        explored: BitGrid::new(n),
        shade: vec![1.0; n],
        timer: 0.0,
        settled: false,
        bytes: vec![255; n],
        generation: None,
        dev_off: dev_fog_off(),
    });
}

/// Recomputes `visible` (5 Hz) and `explored`, fades `shade`, uploads the texture when it changed.
pub fn update_fog(
    time: Res<Time<Real>>,
    mirror: Res<Mirror>,
    mut fog: ResMut<FogTexture>,
    mut images: ResMut<Assets<Image>>,
) {
    let fog = fog.bypass_change_detection();
    // presentation timers use the real frame dt clamped like the JS main loop (spec 05 §17.2: 0.05 s)
    let dt = time.delta_secs().min(0.05);
    let mut dirty = false;
    if fog.generation != Some(mirror.generation) {
        fog.generation = Some(mirror.generation);
        if let Some(m) = mirror.map()
            && (m.w != fog.w || m.h != fog.h)
        {
            // non-standard map size: resize everything
            fog.w = m.w;
            fog.h = m.h;
            let n = (m.w * m.h) as usize;
            fog.visible = BitGrid::new(n);
            fog.explored = BitGrid::new(n);
            if let Some(mut img) = images.get_mut(&fog.image) {
                img.resize(Extent3d {
                    width: m.w as u32,
                    height: m.h as u32,
                    depth_or_array_layers: 1,
                });
            }
        }
        let n = (fog.w * fog.h) as usize;
        fog.shade = vec![1.0; n];
        fog.bytes = vec![255; n];
        fog.visible.clear();
        fog.explored.clear();
        fog.timer = 0.0;
        dirty = true;
    }
    let on = !fog.dev_off
        && mirror.active
        && mirror.you.is_some()
        && !mirror.view.fog_off
        && mirror.view.outcome.is_none();
    if on != fog.on {
        fog.on = on;
        fog.settled = false;
    }
    fog.timer -= dt;
    if fog.timer <= 0.0 {
        fog.timer = VISIBLE_PERIOD;
        recompute_visible(fog, &mirror);
        fog.settled = false;
    }
    if !fog.settled {
        let k = (dt * 5.0).min(1.0);
        let mut all = true;
        for c in 0..fog.shade.len() {
            let want = if !fog.on || fog.visible.get(c) {
                0.0
            } else if fog.explored.get(c) {
                EXPLORED_SHADE
            } else {
                1.0
            };
            let s0 = fog.shade[c];
            if s0 == want {
                continue;
            }
            let mut s = s0 + (want - s0) * k;
            if (want - s).abs() < 0.001 {
                s = want;
            } else {
                all = false;
            }
            fog.shade[c] = s;
            let b = (s * 255.0).round() as u8;
            if let Some(old) = fog.bytes.get_mut(c)
                && *old != b
            {
                *old = b;
                dirty = true;
            }
        }
        fog.settled = all;
    }
    // upload only when a texel byte changed (the asset change re-writes the GPU texture in place)
    if dirty && let Some(mut img) = images.get_mut(&fog.image) {
        let n = fog.bytes.len();
        if let Some(data) = img.data.as_mut() {
            if data.len() != n {
                data.resize(n, 255);
            }
            data.copy_from_slice(&fog.bytes);
        }
    }
}

/// `Fog.reveal(x, y, r)` (spec 05 §12.1): every cell `(i, j)` with `(i-cx)² + (j-cy)² <= r² + r` around the cell of `p`.
pub fn stamp_disc(g: &mut BitGrid, w: i32, h: i32, p: V2, r: i32) {
    if r < 0 || !p.x.is_finite() || !p.y.is_finite() {
        return;
    }
    let cx = (p.x / TILE).floor() as i32;
    let cy = (p.y / TILE).floor() as i32;
    let rr = r * r + r;
    for j in (cy - r).max(0)..=(cy + r).min(h - 1) {
        let dy = j - cy;
        let rem = rr - dy * dy;
        if rem < 0 {
            continue;
        }
        let mut dx = (rem as f64).sqrt() as i32;
        while dx * dx > rem {
            dx -= 1;
        }
        let dx = dx.min(r);
        for i in (cx - dx).max(0)..=(cx + dx).min(w - 1) {
            g.set((j * w + i) as usize);
        }
    }
}

/// Visible cells from own units / buildings / points (same discs as the sim's `explore`), explored = memory ∪ vis.
fn recompute_visible(fog: &mut FogTexture, mirror: &Mirror) {
    fog.visible.clear();
    let (w, h) = (fog.w, fog.h);
    let explored = &mirror.view.explored;
    if explored.len == fog.explored.len {
        fog.explored.words.copy_from_slice(&explored.words);
    }
    let Some(me) = mirror.you else {
        return;
    };
    let sight = mirror.view.economy.as_ref().map_or(0.0, |e| e.mods.sight);
    let cells = |r: f32| r.round() as i32;
    for u in mirror.view.units.values() {
        if u.faction == me {
            stamp_disc(
                &mut fog.visible,
                w,
                h,
                V2::new(u.x, u.y),
                cells(u.kind.def().sight + sight),
            );
        }
    }
    for b in mirror.view.buildings.values() {
        if b.faction != me {
            continue;
        }
        let d = b.kind.def();
        let x = (b.cx as f32 + d.w as f32 / 2.0) * TILE;
        let y = (b.cy + d.h) as f32 * TILE - 4.0 - BUILDING_SIGHT_DY;
        let r = cells(if b.done { d.sight } else { SITE_SIGHT } + sight);
        stamp_disc(&mut fog.visible, w, h, V2::new(x, y), r);
    }
    if let Some(g) = mirror.world.as_ref() {
        for p in &mirror.view.points {
            if p.owner != Some(me) {
                continue;
            }
            let Some(gp) = g.points.get(p.id.0 as usize) else {
                continue;
            };
            let (cx, cy) = (gp.cell % g.map.w, gp.cell / g.map.w);
            let at = V2::new((cx + 1) as f32 * TILE, (cy + 2) as f32 * TILE - 4.0 - TILE);
            stamp_disc(&mut fog.visible, w, h, at, cells(POINT_SIGHT + sight));
        }
    }
    for (a, b) in fog.explored.words.iter_mut().zip(&fog.visible.words) {
        *a |= *b;
    }
}

pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, init_fog)
            .add_systems(Update, update_fog.run_if(in_state(AppState::InGame)));
    }
}
