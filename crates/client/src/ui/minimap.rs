//! Minimap and the big map of the Invia screen (spec 06 §9, world.js:863-889 for the terrain image).
//!
//! Layers (textures, rebuilt on the CPU): terrain (static per match, 2 px per cell), objects (buildings + units,
//! 3 px per cell, refreshed ~8 times a second), fog (1 px per cell, alpha chased per cell, sampled bilinearly).
//! Capture-point diamonds, the camera frame and attack pings are UI primitives.

use super::kit::{DARK, Ui, faction_color, panel};
use super::{Env, Screen, UiGfx, UiState, View};
use crate::control::Selection;
use crate::mirror::Mirror;
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashSet;
use ts_sim::TILE;
use ts_sim::defs::{BUILDING_SIGHT_DY, BuildingKind, KingdomColor, NodeKind, POINT_SIGHT, SITE_SIGHT};
use ts_sim::snapshot_types::UF_INSIDE;

/// Texture pixels per cell of the objects layer.
const OBJ_PX: usize = 3;
/// Texture pixels per cell of the terrain layer.
const TER_PX: usize = 2;

#[derive(Default)]
pub struct MiniMapGfx {
    pub terrain: Handle<Image>,
    /// buildings + units for the HUD minimap (marker sizes of §9.1)
    pub objects: Handle<Image>,
    /// same for the Invia big map (markers 1.5× in screen units)
    pub objects_big: Handle<Image>,
    pub fog: Handle<Image>,
    pub generation: Option<u32>,
    pub w: usize,
    pub h: usize,
}

/// Fog alpha per cell (chased), visibility scratch, refresh timers.
#[derive(Resource, Default)]
pub struct FogCache {
    alpha: Vec<f32>,
    vis: Vec<u8>,
    t: f32,
    big_was_open: bool,
}

/// White circle (disc, or ring whose stroke is `ratio` of the outer radius), antialiased.
pub fn circle_image(size: u32, ratio: Option<f32>) -> Image {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let r = size as f32 / 2.0;
    let inner = ratio.map(|k| r * (1.0 - k.clamp(0.01, 1.0)));
    for y in 0..size {
        for x in 0..size {
            let d = Vec2::new(x as f32 + 0.5 - r, y as f32 + 0.5 - r).length();
            let mut a = (r - d + 0.5).clamp(0.0, 1.0);
            if let Some(ir) = inner {
                a *= (d - ir + 0.5).clamp(0.0, 1.0);
            }
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a * 255.0) as u8;
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::linear();
    img
}

fn new_image(w: usize, h: usize, linear: bool) -> Image {
    let mut img = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0u8; w * h * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = if linear {
        ImageSampler::linear()
    } else {
        ImageSampler::nearest()
    };
    img
}

fn hex(c: u32) -> [u8; 4] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]
}

/// Minimap terrain image (world.js:863-889 colours).
fn paint_terrain(mirror: &Mirror, data: &mut [u8], tw: usize) {
    let Some(g) = mirror.world.as_ref() else {
        return;
    };
    let m = &g.map;
    let water = hex(0x47abA9);
    for cy in 0..m.h {
        for cx in 0..m.w {
            let c = m.idx(cx, cy);
            let col = if m.land[c as usize] == 0 && m.cliff[c as usize] == 0 {
                water
            } else if m.cliff[c as usize] != 0 {
                hex(0x5c8f92)
            } else if m.is_stair(c) {
                hex(0xc9b87a)
            } else {
                match m.level_at(c) {
                    0 => hex(0x8fb24c),
                    1 => hex(0xb9cf62),
                    _ => hex(0xd9e58a),
                }
            };
            fill_cell(data, tw, cx as usize, cy as usize, TER_PX, col);
        }
    }
    for t in &g.trees {
        let (cx, cy) = m.xy(t.cell);
        fill_cell(data, tw, cx as usize, cy as usize, TER_PX, hex(0x4f7f3a));
    }
    for d in &g.golds {
        let (cx, cy) = m.xy(d.cell);
        fill_cell(data, tw, cx as usize, cy as usize, TER_PX, hex(0xf2d24b));
    }
    for d in &g.stones {
        let (cx, cy) = m.xy(d.cell);
        fill_cell(data, tw, cx as usize, cy as usize, TER_PX, hex(0x9aa6ad));
    }
    for (k, d) in &g.bigs {
        let (cx, cy) = m.xy(d.cell);
        let fill = if *k == NodeKind::Gold {
            hex(0xf2d24b)
        } else {
            hex(0xc3ccd1)
        };
        let (x0, y0) = (cx as usize * TER_PX, cy as usize * TER_PX);
        let n = 2 * TER_PX;
        for y in 0..n {
            for x in 0..n {
                let border = x == 0 || y == 0 || x == n - 1 || y == n - 1;
                put(data, tw, x0 + x, y0 + y, if border { hex(0x1b1f33) } else { fill });
            }
        }
    }
}

#[inline]
fn put(data: &mut [u8], tw: usize, x: usize, y: usize, c: [u8; 4]) {
    let i = (y * tw + x) * 4;
    if i + 3 < data.len() && x < tw {
        data[i..i + 4].copy_from_slice(&c);
    }
}

fn fill_cell(data: &mut [u8], tw: usize, cx: usize, cy: usize, px: usize, c: [u8; 4]) {
    for y in 0..px {
        for x in 0..px {
            put(data, tw, cx * px + x, cy * px + y, c);
        }
    }
}

/// Filled square (texture px), clipped.
fn square(data: &mut [u8], tw: usize, th: usize, x: f32, y: f32, size: f32, c: [u8; 4]) {
    let x0 = x.round().max(0.0) as usize;
    let y0 = y.round().max(0.0) as usize;
    let x1 = ((x + size).round().max(0.0) as usize).min(tw);
    let y1 = ((y + size).round().max(0.0) as usize).min(th);
    for yy in y0..y1 {
        for xx in x0..x1 {
            put(data, tw, xx, yy, c);
        }
    }
}

fn rgba(c: KingdomColor) -> [u8; 4] {
    let [r, g, b] = c.rgb();
    [r, g, b, 255]
}

/// Buildings and units into an objects layer; `k` = texture px per screen design unit `s` of the markers.
fn paint_objects(mirror: &Mirror, sel: &HashSet<ts_sim::slab::UnitId>, data: &mut [u8], tw: usize, th: usize, k: f32) {
    data.fill(0);
    let tex = OBJ_PX as f32 / TILE;
    let dark = [0x1b, 0x1f, 0x33, 255];
    for b in mirror.view.buildings.values() {
        let d = b.kind.def();
        let bx = (b.cx as f32 + d.w as f32 * 0.5) * TILE;
        let by1 = (b.cy + d.h) as f32 * TILE;
        let r = if b.kind == BuildingKind::Castle {
            9.0
        } else if d.w > 1 {
            5.0
        } else {
            3.0
        } * k;
        let c = mirror
            .colors
            .get(b.faction as usize)
            .copied()
            .unwrap_or(KingdomColor::Blue);
        let (x, y) = (bx * tex - r / 2.0, by1 * tex - r);
        let bd = (k * 0.4).max(1.0);
        square(data, tw, th, x - bd, y - bd, r + 2.0 * bd, dark);
        square(data, tw, th, x, y, r, rgba(c));
    }
    let us = 2.5 * k;
    for u in mirror.view.units.values() {
        if u.flags & UF_INSIDE != 0 {
            continue;
        }
        let c = if sel.contains(&u.id) {
            [255, 255, 255, 255]
        } else {
            rgba(
                mirror
                    .colors
                    .get(u.faction as usize)
                    .copied()
                    .unwrap_or(KingdomColor::Blue),
            )
        };
        square(data, tw, th, u.x * tex - us / 2.0, u.y * tex - us / 2.0, us, c);
    }
}

/// Stamps a disc of `r` cells around `(x, y)` px into the visibility grid.
fn stamp(vis: &mut [u8], w: i32, h: i32, x: f32, y: f32, r: f32) {
    let (cx, cy) = (x / TILE, y / TILE);
    let x0 = (cx - r).floor().max(0.0) as i32;
    let x1 = ((cx + r).ceil() as i32).min(w - 1);
    let y0 = (cy - r).floor().max(0.0) as i32;
    let y1 = ((cy + r).ceil() as i32).min(h - 1);
    let r2 = r * r;
    for yy in y0..=y1 {
        let dy = yy as f32 + 0.5 - cy;
        for xx in x0..=x1 {
            let dx = xx as f32 + 0.5 - cx;
            if dx * dx + dy * dy <= r2 {
                vis[(yy * w + xx) as usize] = 1;
            }
        }
    }
}

/// Rebuilds the minimap layers (terrain per match, objects and fog at ~8 Hz).
#[allow(clippy::too_many_arguments)]
pub fn update_layers(
    mut gfx: ResMut<UiGfx>,
    mut cache: ResMut<FogCache>,
    mut images: ResMut<Assets<Image>>,
    mirror: Res<Mirror>,
    time: Res<Time<Real>>,
    st: Res<UiState>,
    sel: Res<Selection>,
) {
    let Some(map) = mirror.map() else {
        return;
    };
    let (w, h) = (map.w as usize, map.h as usize);
    let gfx = &mut *gfx;
    let mm = &mut gfx.minimap;
    if mm.generation != Some(mirror.generation) || mm.w != w || mm.h != h {
        let mut ter = new_image(w * TER_PX, h * TER_PX, false);
        if let Some(d) = ter.data.as_mut() {
            paint_terrain(&mirror, d, w * TER_PX);
        }
        mm.terrain = images.add(ter);
        mm.objects = images.add(new_image(w * OBJ_PX, h * OBJ_PX, false));
        mm.objects_big = images.add(new_image(w * OBJ_PX, h * OBJ_PX, false));
        mm.fog = images.add(new_image(w, h, true));
        mm.generation = Some(mirror.generation);
        mm.w = w;
        mm.h = h;
        cache.alpha = vec![1.0; w * h];
        cache.vis = vec![0; w * h];
        cache.t = 1.0;
    }
    let dt = time.delta_secs();
    cache.t += dt;
    let big_open = st.screen == Some(Screen::Invia);
    if cache.t < 0.12 && !(big_open && !cache.big_was_open) {
        return;
    }
    let step = cache.t.min(0.5);
    cache.t = 0.0;
    cache.big_was_open = big_open;
    let selset: HashSet<ts_sim::slab::UnitId> = sel.units.iter().copied().collect();
    let (tw, th) = (w * OBJ_PX, h * OBJ_PX);
    // marker size in texture px per `s`: minimap is 270 s wide for w cells → OBJ_PX*w/270 tex px per s
    let k_small = OBJ_PX as f32 * w as f32 / 270.0;
    if let Some(mut img) = images.get_mut(&mm.objects)
        && let Some(d) = img.data.as_mut()
    {
        paint_objects(&mirror, &selset, d, tw, th, k_small);
    }
    if big_open {
        let k_big = OBJ_PX as f32 * w as f32 / 900.0 * 1.5;
        if let Some(mut img) = images.get_mut(&mm.objects_big)
            && let Some(d) = img.data.as_mut()
        {
            paint_objects(&mirror, &selset, d, tw, th, k_big);
        }
    }
    // fog: visible now from own units / buildings / points; explored from the mirror
    let all_visible = super::fog_off(&mirror);
    let cache = &mut *cache;
    let vis = &mut cache.vis;
    let alphas = &mut cache.alpha;
    vis.fill(0);
    if !all_visible && let Some(you) = mirror.you {
        let mods = mirror
            .view
            .economy
            .as_ref()
            .map(|e| e.mods)
            .unwrap_or_default();
        let (wi, hi) = (w as i32, h as i32);
        for u in mirror.view.units.values() {
            if u.faction == you && u.flags & UF_INSIDE == 0 {
                stamp(vis, wi, hi, u.x, u.y, mods.unit_sight_px(u.kind) / TILE);
            }
        }
        for b in mirror.view.buildings.values() {
            if b.faction != you || mirror.view.ghosts.contains(&b.id) {
                continue;
            }
            let d = b.kind.def();
            let x = (b.cx as f32 + d.w as f32 * 0.5) * TILE;
            let y = (b.cy + d.h) as f32 * TILE - BUILDING_SIGHT_DY;
            let r = if b.done { d.sight + mods.sight } else { SITE_SIGHT };
            stamp(vis, wi, hi, x, y, r);
        }
        if let Some(g) = mirror.world.as_ref() {
            for p in &mirror.view.points {
                if p.owner == Some(you)
                    && let Some(gp) = g.points.get(p.id.0 as usize)
                {
                    let (cx, cy) = map.xy(gp.cell);
                    let (px, py) = ((cx + 1) as f32 * TILE, (cy + 2) as f32 * TILE - 4.0);
                    stamp(vis, wi, hi, px, py - TILE, POINT_SIGHT);
                }
            }
        }
    }
    let kk = (step * 6.0).min(1.0);
    if let Some(mut img) = images.get_mut(&mm.fog)
        && let Some(d) = img.data.as_mut()
    {
        for i in 0..w * h {
            let target = if all_visible || vis[i] != 0 {
                0.0
            } else if mirror.view.explored.get(i) {
                0.42
            } else {
                1.0
            };
            let a = &mut alphas[i];
            *a += (target - *a) * kk;
            if (*a - target).abs() < 0.01 {
                *a = target;
            }
            let o = i * 4;
            d[o] = 10;
            d[o + 1] = 14;
            d[o + 2] = 26;
            d[o + 3] = (*a * 255.0) as u8;
        }
    }
}

/// Foot point (px) of capture point `i` of the generated world.
pub fn point_pos(mirror: &Mirror, i: usize) -> Option<Vec2> {
    let g = mirror.world.as_ref()?;
    let gp = g.points.get(i)?;
    let (cx, cy) = g.map.xy(gp.cell);
    Some(Vec2::new((cx + 1) as f32 * TILE, (cy + 2) as f32 * TILE - 4.0))
}

/// Map size in px.
pub fn map_px(mirror: &Mirror) -> Vec2 {
    mirror.map().map_or(
        Vec2::new(ts_sim::MAP_W as f32 * TILE, ts_sim::MAP_H as f32 * TILE),
        |m| Vec2::new(m.w as f32 * TILE, m.h as f32 * TILE),
    )
}

/// `drawMap(x, y, mw, mh, s)` → px per world px (`kx`, `ky`).
#[allow(clippy::too_many_arguments)]
pub fn draw_map(
    u: &mut Ui,
    st: &UiState,
    env: &Env,
    x: f32,
    y: f32,
    mw: f32,
    mh: f32,
    s: f32,
    big: bool,
) -> Vec2 {
    let mp = map_px(&env.mirror);
    let k = Vec2::new(mw / mp.x, mh / mp.y);
    let gfx: &UiGfx = &env.gfx;
    let mm = &gfx.minimap;
    u.p.image(&mm.terrain, None, x, y, mw, mh, Color::WHITE);
    let obj = if big { &mm.objects_big } else { &mm.objects };
    u.p.image(obj, None, x, y, mw, mh, Color::WHITE);
    // capture points (diamonds)
    let mirror = &env.mirror;
    for p in &mirror.view.points {
        let Some(pos) = point_pos(mirror, p.id.0 as usize) else {
            continue;
        };
        if !super::knows(mirror, pos.x, pos.y - TILE) {
            continue;
        }
        let c = Vec2::new(x + pos.x * k.x, y + (pos.y - TILE) * k.y);
        let fill = match p.owner {
            Some(o) => faction_color(env.color_of(o)),
            None => Color::srgb_u8(0xf4, 0xec, 0xd2),
        };
        diamond(u, c, 6.5 * s, DARK);
        diamond(u, c, 4.5 * s, fill);
    }
    u.p.image(&mm.fog, None, x, y, mw, mh, Color::WHITE);
    // camera frame
    let cw = Color::srgba(1.0, 1.0, 1.0, 0.9);
    let corners = env.rig.view_corners;
    let pts: Vec<Vec2> = corners
        .iter()
        .map(|c| {
            Vec2::new(
                x + (c.x * k.x).clamp(0.0, mw),
                y + (c.y * k.y).clamp(0.0, mh),
            )
        })
        .collect();
    if !big {
        for i in 0..4 {
            u.p.line(pts[i], pts[(i + 1) % 4], 1.5 * s, cw);
        }
    }
    // attack pings
    for &(px, py, t) in &st.pings {
        let r = (5.0 + (t * 14.0) % 16.0) * s;
        let a = (2.0 - t / 2.0).clamp(0.0, 1.0);
        let lw = 2.5 * s;
        let outer = r + lw / 2.0;
        let ring = env.gfx.ring(lw / outer);
        u.ring(
            &ring,
            x + px * k.x,
            y + py * k.y,
            outer,
            Color::srgba(1.0, 0x4a as f32 / 255.0, 0x3d as f32 / 255.0, a),
        );
    }
    k
}

fn diamond(u: &mut Ui, c: Vec2, r: f32, color: Color) {
    let side = r * std::f32::consts::SQRT_2;
    u.p.image_rot(
        &Handle::default(),
        None,
        c.x - side / 2.0,
        c.y - side / 2.0,
        side,
        side,
        color,
        std::f32::consts::FRAC_PI_4,
        false,
    );
}

/// `drawMinimap(s)` (HUD) → left edge of the frame (`x - 10s`).
pub fn draw_minimap(u: &mut Ui, st: &mut UiState, env: &mut Env, _v: &View, s: f32) -> f32 {
    let (w, h) = (u.w(), u.h());
    let mp = map_px(&env.mirror);
    let mw = 270.0 * s;
    let mh = mw * mp.y / mp.x;
    let x = w - mw - 24.0 * s;
    let y = h - mh - 26.0 * s;
    u.panel(&panel::PAPER, x - 10.0 * s, y - 10.0 * s, mw + 20.0 * s, mh + 20.0 * s, 0.5 * s);
    let k = draw_map(u, st, env, x, y, mw, mh, s, false);
    let m = u.p.io.mouse;
    let wx = ((m.x - x) / k.x).clamp(0.0, mp.x);
    let wy = ((m.y - y) / k.y).clamp(0.0, mp.y);
    if u.p.over(x - 10.0 * s, y - 10.0 * s, mw + 20.0 * s, mh + 20.0 * s) {
        u.p.set_hot();
        if u.p.io.pressed {
            st.map_drag = true;
        }
        if u.p.io.rpressed && !st.spectate {
            super::selection::order_at(st, env, Vec2::new(wx, wy));
        }
    }
    if !u.p.io.lmb {
        st.map_drag = false;
    }
    if st.map_drag && !u.p.locked {
        env.rig.look_at(Vec2::new(wx, wy));
        u.p.set_hot();
    }
    x - 10.0 * s
}
