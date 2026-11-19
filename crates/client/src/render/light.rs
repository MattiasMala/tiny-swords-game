// OWNER: C2 (sprites) — spec 07-light-sfx.md §2 (day/night cycle `DAY_LENGTH`, `DAY_KEYS`, ambient, shade, sun path),
// §2.4 (3D mapping), §4 (point lights + flicker), §5 (night halos), §6 (bloom, vignette), §7 (F2 quality levels,
// auto-degrade + toasts), addenda L2 (smooth sun direction), docs/bevy-0.20-notes.md §8/§9 (WebGL2: 1 directional
// light, 1 cascade, ≤ 204 clustered lights).
//
// * `LightState` = the JS `Light` record: phase, ambient (0..1 rgb), night, shade, smoothed sun direction, quality
//   level (2 complete / 1 light / 0 off, mirrored from C4's `GraphicsQuality`: F2 + auto-degrade live there).
//   `--time T` overrides the day phase.
// * One shadow-casting `DirectionalLight` (sun / moon). Its colour follows the ambient keyframes; its strength vs the
//   `GlobalAmbientLight` follows `shade` so that shadowed ground keeps `1 - shade·k` of the lit brightness.
// * Point lights: every draw system pushes `LightSrc` candidates (buildings, burning buildings, owned capture points,
//   campfires, shrines, gold veins); the `MAX_POINT_LIGHTS` nearest the view centre become pooled `PointLight`s with
//   the JS flicker, intensity ∝ `power_eff·(0.25 + 0.75·night)`; at night additive glow halos (`r·0.62`, alpha
//   `0.3·night·power_eff`) are drawn as sprites.
// * Camera post (level 2): `Hdr` + `Bloom` + `Vignette` (`0.75 + 0.2·night`) + `DioramaPost` (tilt-shift bands and
//   sun shafts, `shaders/light_post.wesl`, after tonemapping); tonemapping always.

use crate::AppState;
use crate::assets::SpriteAtlas;
use crate::dev::DevArgs;
use crate::mirror::Mirror;
use crate::render::anim::WorldClock;
use crate::render::camera::RtsCamera;
use crate::render::sprites::{Basis, Flavor, Layer, Painter, SpriteBatches, SpriteSystems};
use crate::render::sprites_world::{NONE, Sheets};
use crate::control::GraphicsQuality;
use bevy::asset::embedded_asset;
use bevy::camera::Hdr;
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::post_process::bloom::Bloom;
use bevy::post_process::effect_stack::Vignette;
use bevy::prelude::*;
use bevy::render::{RenderApp, extract_component::ExtractComponent, render_resource::ShaderType};
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::BoxedSystem;
use bevy::shader::ShaderRef;
use ts_sim::defs::DAY_KEYS;

/// Day phase 0..1 used by the lighting (kept for other modules).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct DayTime(pub f32);

#[derive(Component)]
pub struct Sun;

#[derive(Component)]
struct PooledLight(usize);

/// Point lights kept at most (WebGL2 clustered forward: cheap enough, but every light costs per fragment).
pub const MAX_POINT_LIGHTS: usize = 24;

/// A light source candidate pushed by the draw systems this frame.
#[derive(Clone, Copy, Debug)]
pub struct LightSrc {
    /// world position
    pub pos: Vec3,
    /// radius in sim px
    pub r: f32,
    pub power: f32,
    pub seed: f32,
    pub flick: f32,
}

#[derive(Resource, Default)]
pub struct LightCandidates {
    pub list: Vec<LightSrc>,
}

impl LightCandidates {
    #[inline]
    pub fn push(&mut self, l: LightSrc) {
        self.list.push(l);
    }
}

/// The JS `Light` state.
#[derive(Resource, Debug, Clone)]
pub struct LightState {
    pub phase: f32,
    /// ambient colour 0..1 (DAY_KEYS interpolation / 255)
    pub ambient: [f32; 3],
    pub night: f32,
    pub shade: f32,
    /// 2D shadow shear (`lx`, `ly`) of the smoothed sun
    pub lx: f32,
    pub ly: f32,
    /// 2 complete, 1 light, 0 off
    pub level: u8,
    applied_level: Option<u8>,
}

impl Default for LightState {
    fn default() -> Self {
        LightState {
            phase: 0.0,
            ambient: [1.0; 3],
            night: 0.0,
            shade: 0.3,
            lx: 0.0,
            ly: 0.4,
            level: 2,
            applied_level: None,
        }
    }
}

/// `Light.tick()` values for a phase: (ambient 0..255 rounded, shade, night, lx, ly) — exact JS formulas.
pub fn day_values(phase: f32) -> ([f32; 3], f32, f32, f32, f32) {
    let mut i = 1;
    while i < DAY_KEYS.len() - 1 && DAY_KEYS[i].0 < phase {
        i += 1;
    }
    let (p0, c0, s0) = DAY_KEYS[i - 1];
    let (p1, c1, s1) = DAY_KEYS[i];
    let k = if p1 > p0 { (phase - p0) / (p1 - p0) } else { 0.0 };
    let mut amb = [0.0f32; 3];
    for j in 0..3 {
        amb[j] = (c0[j] as f32 + (c1[j] as f32 - c0[j] as f32) * k + 0.5).floor();
    }
    let shade = s0 + (s1 - s0) * k;
    let lum = (amb[0] * 0.3 + amb[1] * 0.59 + amb[2] * 0.11) / 255.0;
    let night = ((0.92 - lum) / 0.42).clamp(0.0, 1.0);
    let day = (phase / 0.7).clamp(0.0, 1.0);
    let sun = (day - 0.5) * 2.0;
    let (lx, ly) = if night > 0.9 {
        (-0.55, 0.34)
    } else {
        (sun * 1.15, 0.26 + 0.2 * sun * sun)
    };
    (amb, shade, night, lx, ly)
}

/// Continuous sun shear (addenda L2): the sun path by day, blended into the fixed moon direction as night falls.
pub fn smooth_sun(phase: f32, night: f32) -> (f32, f32) {
    // evening (phase ≥ 0.7) keeps the setting sun, dawn (phase ≥ 0.95) brings back the rising one
    let sun = if phase >= 0.85 {
        -1.0
    } else {
        ((phase / 0.7).clamp(0.0, 1.0) - 0.5) * 2.0
    };
    let (sx, sy) = (sun * 1.15, 0.26 + 0.2 * sun * sun);
    let m = {
        let t = ((night - 0.55) / 0.4).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    (sx + (-0.55 - sx) * m, sy + (0.34 - sy) * m)
}

/// JS flicker: `power * (1 - flick + flick*(0.5 + 0.5*sin(t*9+seed)*sin(t*5.3+seed*1.7)))`.
#[inline]
pub fn power_eff(power: f32, flick: f32, seed: f32, t: f32) -> f32 {
    power
        * (1.0 - flick + flick * (0.5 + 0.5 * (t * 9.0 + seed).sin() * (t * 5.3 + seed * 1.7).sin()))
}

pub struct LightPlugin;

impl Plugin for LightPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/light_post.wesl");
        app.add_plugins(FullscreenMaterialPlugin::<DioramaPost>::default())
            .init_resource::<DayTime>()
            .init_resource::<LightState>()
            .init_resource::<LightCandidates>()
            .insert_resource(GlobalAmbientLight {
                color: Color::WHITE,
                brightness: 600.0,
                ..default()
            })
            .add_systems(Startup, spawn_sun)
            .add_systems(
                Update,
                (
                    (sync_level, update_day, camera_post, update_diorama, shadow_range).chain(),
                    clear_candidates.in_set(SpriteSystems::Prepare),
                    (draw_halos, place_point_lights)
                        .chain()
                        .in_set(crate::render::sprites::DrawStage::Late),
                ),
            );
    }
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        Sun,
        DirectionalLight {
            illuminance: 3_000.0,
            shadow_maps_enabled: true,
            color: Color::srgb(1.0, 0.98, 0.94),
            shadow_depth_bias: 0.03,
            shadow_normal_bias: 1.2,
            ..default()
        },
        Transform::from_xyz(-0.5, 1.0, 0.6).looking_at(Vec3::ZERO, Vec3::Y),
        // layer 1 = shadow-only casters (cloud shadows, fx.rs)
        bevy::camera::visibility::RenderLayers::from_layers(&[0, crate::render::fx::SHADOW_ONLY_LAYER]),
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 1.0,
            maximum_distance: 60.0,
            first_cascade_far_bound: 60.0,
            overlap_proportion: 0.2,
        }
        .build(),
    ));
    for i in 0..MAX_POINT_LIGHTS {
        commands.spawn((
            PooledLight(i),
            PointLight {
                color: Color::srgb(1.0, 0.72, 0.42),
                intensity: 0.0,
                range: 4.0,
                radius: 0.1,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(0.0, -50.0, 0.0),
            Visibility::Hidden,
        ));
    }
}

/// The light level follows `control::GraphicsQuality` (C4 owns the F2 key, the slow-frame watchdog and their toasts,
/// spec 07 §7). Dev screenshots render their first frames as fast as possible while pipelines compile: there an
/// automatic drop is ignored (a level chosen with F2 still applies).
fn sync_level(dev: Res<DevArgs>, q: Res<GraphicsQuality>, mut ls: ResMut<LightState>) {
    let level = if dev.screenshot.is_some() && q.lights_auto {
        2
    } else {
        q.lights.min(2)
    };
    if ls.level != level {
        ls.level = level;
    }
}

/// Brightness of fully lit flat ground (linear, before tonemapping) — calibrated so day art keeps its colours.
const EXPOSURE_LIT: f32 = 1.0;
/// Ambient-light brightness that yields albedo × 1.0 (Bevy: radiance × exposure, default EV100 9.7 ≈ 1/998).
const AMBIENT_UNIT: f32 = 998.0;
/// Directional illuminance that yields albedo × 1.0 on a surface facing the light (Lambert ÷ π).
const SUN_UNIT: f32 = 998.0 * std::f32::consts::PI;
/// HD-2D shadows are a bit stronger than the 2D alpha (`shade`) for readability.
const SHADE_GAIN: f32 = 1.35;

#[allow(clippy::too_many_arguments)]
fn update_day(
    dev: Res<DevArgs>,
    mirror: Res<Mirror>,
    time: Res<Time<Real>>,
    mut day: ResMut<DayTime>,
    mut ls: ResMut<LightState>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    basis: Res<Basis>,
) {
    let phase = dev
        .time
        .map(|t| t.rem_euclid(1.0))
        .unwrap_or_else(|| ts_sim::defs::day_phase(mirror.view.time));
    if (day.0 - phase).abs() > 1e-5 {
        day.0 = phase;
    }
    let off = ls.level == 0;
    let (amb, shade, night, _, _) = if off {
        day_values(0.3)
    } else {
        day_values(phase)
    };
    let (tlx, tly) = if off { (0.0, 0.26) } else { smooth_sun(phase, night) };
    // ease towards the target direction (covers --time jumps and the remaining kinks)
    let k = 1.0 - (-time.delta_secs() * 3.0).exp();
    if ls.applied_level.is_none() {
        ls.lx = tlx;
        ls.ly = tly;
    } else {
        ls.lx += (tlx - ls.lx) * k;
        ls.ly += (tly - ls.ly) * k;
    }
    ls.phase = phase;
    ls.ambient = [amb[0] / 255.0, amb[1] / 255.0, amb[2] / 255.0];
    ls.night = night;
    ls.shade = shade;

    // toward-sun direction (spec 07 §2.4): normalize(-lx, 1, -ly)
    let to_sun = Vec3::new(-ls.lx, 1.0, -ls.ly).normalize();
    let ndotl = to_sun.y.max(0.05);
    // shadowed ground keeps (1 - shade·gain) of the lit brightness
    let keep = (1.0 - shade * SHADE_GAIN).clamp(0.3, 0.95);
    let lit = EXPOSURE_LIT * if off { 1.0 } else { 1.0 };
    let a = lit * keep;
    let s = (lit - a) / ndotl;
    let col = Color::srgb(ls.ambient[0], ls.ambient[1], ls.ambient[2]).to_linear();
    // keep the ambient colour's luminance in the light colour (night stays dark & blue)
    ambient.color = Color::LinearRgba(LinearRgba::new(col.red, col.green, col.blue, 1.0));
    ambient.brightness = a * AMBIENT_UNIT;
    for (mut l, mut tf) in &mut sun {
        // moonlight is cooler; the sun warms with a low elevation
        let tint = if night > 0.5 {
            LinearRgba::new(col.red * 0.9, col.green * 0.95, col.blue * 1.1, 1.0)
        } else {
            LinearRgba::new(col.red, col.green, col.blue, 1.0)
        };
        l.color = Color::LinearRgba(tint);
        l.illuminance = s * SUN_UNIT;
        l.shadow_maps_enabled = !off;
        let focus = if basis.valid {
            basis.eye
        } else {
            Vec3::ZERO
        };
        *tf = Transform::from_translation(focus + to_sun * 50.0).looking_at(focus, Vec3::Y);
    }
    if ls.applied_level.is_none() {
        ls.applied_level = Some(ls.level);
    }
}

/// The single shadow cascade covers what the camera sees: its far bound follows the zoom (eye distance), so zoomed
/// out views keep their shadows and zoomed in views get sharper ones.
fn shadow_range(
    rig: Res<crate::render::camera::CameraRig>,
    mut q: Query<&mut bevy::light::CascadeShadowConfig, With<Sun>>,
) {
    let far = (rig.distance() * 2.6).clamp(30.0, 180.0);
    for mut c in &mut q {
        let cur = c.bounds.last().copied().unwrap_or(0.0);
        if (cur - far).abs() > far * 0.08 {
            *c = CascadeShadowConfigBuilder {
                num_cascades: 1,
                minimum_distance: 1.0,
                maximum_distance: far,
                first_cascade_far_bound: far,
                overlap_proportion: 0.2,
            }
            .build();
        }
    }
}

/// Camera post-processing per level (Hdr + Bloom + Vignette at level 2, tonemapping always).
fn camera_post(
    mut commands: Commands,
    mut ls: ResMut<LightState>,
    mut cams: Query<(Entity, Has<Bloom>, Option<&mut Vignette>), With<RtsCamera>>,
) {
    let Ok((e, has_bloom, vignette)) = cams.single_mut() else {
        return;
    };
    let full = ls.level >= 2;
    let level_changed = ls.applied_level != Some(ls.level) || (full != has_bloom);
    if level_changed {
        let mut ec = commands.entity(e);
        ec.insert((Hdr, Tonemapping::Linear));
        if full {
            ec.insert((
                Bloom {
                    intensity: 0.12,
                    ..Bloom::NATURAL
                },
                Vignette {
                    intensity: 0.75,
                    radius: 0.85,
                    smoothness: 2.0,
                    roundness: 1.0,
                    color: Color::srgb_u8(8, 10, 28),
                    ..default()
                },
            ));
        } else {
            ec.remove::<(Bloom, Vignette)>();
        }
        ls.applied_level = Some(ls.level);
    } else if let Some(mut v) = vignette {
        let want = (0.75 + 0.2 * ls.night) * 0.6;
        if (v.intensity - want).abs() > 0.01 {
            v.intensity = want;
        }
    }
}

/// Level-2 "diorama" post pass (spec 07 §6): tilt-shift bands + sun shafts (`shaders/light_post.wesl`), on the game
/// camera only while the light level is 2.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default, Debug)]
#[extract_app(RenderApp)]
pub struct DioramaPost {
    /// top band fraction, bottom band fraction, blur radius (px at 720 rows), band strength
    pub band: Vec4,
    /// sun strength, lean, time (s), unused
    pub shafts: Vec4,
    /// linear shaft colour × gain
    pub shaft_color: Vec4,
}

impl FullscreenMaterial for DioramaPost {
    fn fragment_shader() -> ShaderRef {
        "embedded://ts_client/render/shaders/light_post.wesl".into()
    }

    // After tonemapping: the default slot (before tonemapping) is unordered with respect to Bloom and the effect stack
    // (vignette) and their main-texture ping-pong, which garbles the frame. Here it runs strictly after both.
    fn schedule_configs(system: ScheduleConfigs<BoxedSystem>) -> ScheduleConfigs<BoxedSystem> {
        system
            .in_set(bevy::core_pipeline::Core3dSystems::PostProcess)
            .after(bevy::core_pipeline::tonemapping::tonemapping)
    }
}

/// Tilt-shift bands of `Light.post` step 4: `top = 22 %`, `bottom = 18 %` of the screen height.
const BAND_TOP: f32 = 0.22;
const BAND_BOTTOM: f32 = 0.18;
/// Blur radius of the bands (px at 720 rows; the 2D game blurs a quarter-resolution copy ≈ 3-4 px).
const BAND_BLUR: f32 = 4.5;
/// Shaft brightness (added to the tonemapped frame like the 2D shafts: net alpha `0.085·sun·(0.6..1)`).
const SHAFT_GAIN: f32 = 1.0;

/// Adds / removes / updates the diorama pass (`Light.shafts`: `sun = (1 - night)·(0.45 + 0.55·|lx|)`, lean `-lx/2`,
/// presentation time so the shafts stop while the game is paused, addenda L3).
fn update_diorama(
    mut commands: Commands,
    ls: Res<LightState>,
    clock: Res<WorldClock>,
    state: Res<State<AppState>>,
    mut cams: Query<(Entity, Option<&mut DioramaPost>), With<RtsCamera>>,
) {
    let Ok((e, post)) = cams.single_mut() else {
        return;
    };
    let want = ls.level >= 2 && matches!(state.get(), AppState::InGame | AppState::Menu);
    if !want {
        if post.is_some() {
            commands.entity(e).remove::<DioramaPost>();
        }
        return;
    }
    let sun = (1.0 - ls.night) * (0.45 + 0.55 * ls.lx.abs());
    let c = Color::srgb_u8(255, 244, 210).to_linear();
    let v = DioramaPost {
        band: Vec4::new(BAND_TOP, BAND_BOTTOM, BAND_BLUR, 1.0),
        shafts: Vec4::new(
            if sun > 0.05 { sun } else { 0.0 },
            -ls.lx * 0.5,
            (clock.t % 100_000.0) as f32,
            0.0,
        ),
        shaft_color: Vec4::new(c.red, c.green, c.blue, 0.0) * SHAFT_GAIN,
    };
    match post {
        Some(mut p) => {
            if p.band != v.band || p.shafts != v.shafts || p.shaft_color != v.shaft_color {
                *p = v;
            }
        }
        None => {
            commands.entity(e).insert(v);
        }
    }
}

fn clear_candidates(mut c: ResMut<LightCandidates>) {
    c.list.clear();
}

/// Night halos: additive glow sprites at the light sources (`r·0.62`, alpha `0.3·night·power_eff`).
fn draw_halos(
    atlas: Res<SpriteAtlas>,
    sheets: Res<Sheets>,
    basis: Res<Basis>,
    ls: Res<LightState>,
    clock: Res<WorldClock>,
    cands: Res<LightCandidates>,
    mut batches: ResMut<SpriteBatches>,
) {
    // `Light.apply` step 2: halos belong to light levels 1 and 2 (spec 07 §5, §7)
    if !atlas.ready || !basis.valid || ls.level < 1 || ls.night <= 0.02 || sheets.glow == NONE {
        return;
    }
    let t = clock.t as f32;
    let f = *atlas.frame(sheets.glow, 0);
    let mut p = Painter {
        batches: &mut batches,
        basis: &basis,
        atlas: &atlas,
    };
    for l in &cands.list {
        let pe = power_eff(l.power, l.flick, l.seed, t);
        let a = (0.3 * ls.night * pe).clamp(0.0, 1.0);
        if a < 0.01 {
            continue;
        }
        let size = l.r * 0.62 * 2.0;
        let scale = size / f.w.max(1) as f32;
        p.frame(
            Layer::Dynamic,
            Flavor::Glow,
            &f,
            f.w as f32 * 0.5,
            f.h as f32 * 0.5,
            l.pos,
            crate::render::sprites::SpriteOpts {
                scale,
                color: [a, a, a, a],
                fold: false,
                pull: 1.0,
                ..default()
            },
        );
    }
}

/// Picks the strongest / nearest candidates and updates the pooled point lights.
fn place_point_lights(
    basis: Res<Basis>,
    ls: Res<LightState>,
    clock: Res<WorldClock>,
    mut cands: ResMut<LightCandidates>,
    mut q: Query<(&PooledLight, &mut PointLight, &mut Transform, &mut Visibility)>,
) {
    let t = clock.t as f32;
    let on = ls.level >= 1 && ls.night > 0.05;
    let centre = {
        let c = basis.cull;
        Vec3::new(
            (c[0] + c[2]) * 0.5 / ts_sim::TILE,
            0.0,
            (c[1] + c[3]) * 0.5 / ts_sim::TILE,
        )
    };
    if on {
        cands.list.sort_by(|a, b| {
            let da = (a.pos - centre).with_y(0.0).length_squared() / (a.power * a.r).max(1e-3);
            let db = (b.pos - centre).with_y(0.0).length_squared() / (b.power * b.r).max(1e-3);
            da.total_cmp(&db)
        });
    }
    for (pl, mut light, mut tf, mut vis) in &mut q {
        let src = if on { cands.list.get(pl.0) } else { None };
        match src {
            Some(l) => {
                let pe = power_eff(l.power, l.flick, l.seed, t);
                let k = pe * (0.25 + 0.75 * ls.night);
                light.range = l.r / ts_sim::TILE * 1.1;
                light.intensity = 9_000.0 * k * (l.r / 200.0).powi(2);
                tf.translation = l.pos + Vec3::Y * 0.3 + basis.toward * 0.4;
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
            None => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_keys_match_spec_timeline() {
        // phase 0.1 (match start): full day
        let (a, s, n, _, _) = day_values(0.1);
        assert_eq!(a, [255.0, 251.0, 240.0]);
        assert!((s - 0.3).abs() < 1e-6);
        assert_eq!(n, 0.0);
        // golden hour
        let (a, _, n, _, _) = day_values(0.6);
        assert_eq!(a, [255.0, 206.0, 150.0]);
        assert!((n - 0.187).abs() < 0.01);
        // full night: moon direction
        let (a, s, n, lx, ly) = day_values(0.8);
        assert_eq!(a, [108.0, 124.0, 196.0]);
        assert!((s - 0.14).abs() < 1e-6);
        assert_eq!(n, 1.0);
        assert_eq!((lx, ly), (-0.55, 0.34));
        // phase 0 → k = 0, dawn colour
        let (a, _, _, _, _) = day_values(0.0);
        assert_eq!(a, [255.0, 200.0, 172.0]);
    }

    #[test]
    fn sun_is_continuous() {
        let mut prev = None;
        for i in 0..=1000 {
            let p = i as f32 / 1000.0;
            let (_, _, n, _, _) = day_values(p);
            let (lx, ly) = smooth_sun(p, n);
            if let Some((px, py)) = prev {
                let d: f32 = ((lx - px) as f32).abs() + ((ly - py) as f32).abs();
                // wrap (1.0 → 0.0) stays continuous too: both at the rising sun
                assert!(d < 0.12, "jump at phase {p}: {d}");
            }
            prev = Some((lx, ly));
        }
        let (lx0, _) = smooth_sun(0.0, day_values(0.0).2);
        let (lx1, _) = smooth_sun(0.999, day_values(0.999).2);
        assert!((lx0 - lx1).abs() < 0.2);
    }

    #[test]
    fn flicker_bounds() {
        for i in 0..100 {
            let v = power_eff(1.0, 0.35, 3.0, i as f32 * 0.1);
            assert!((0.65..=1.0001).contains(&v));
        }
    }
}
