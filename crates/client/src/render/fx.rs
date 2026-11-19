// OWNER: C2 (sprites) — sprite FX (`GameEvent::Fx`, spec 07 §8.4 / 01 §8.2), event props are in sprites_world,
// floating "+N" (`GameEvent::Float`, 07 §8.5), ambient water splashes (07 §8.4, `open_water`), drifting cloud
// shadows (`Clouds_01..08`, 07 §8.4 / §3.3 step 3, addenda L2: real projected shadows).
//
// * FX instances live in presentation time (`WorldClock`: frozen while the sim is paused) and are removed when
//   `t·fps ≥ frames`. Events off screen (margin 80 px, 07 addenda L1) or not in sight are dropped. `follow` tracks a
//   unit. The 2D draw-order shift `dy` becomes a foot `y + dy` lifted by `dy` px (same screen position).
// * Flames, explosions and heals are unlit (bright at night, light sources); dust and splashes are lit.
// * Clouds: 32 drifting horizontal quads high above the map, on render layer 1 — seen only by the sun's shadow pass,
//   never by the camera — with a checker-dithered alpha so their shadow is ~half as dark as a solid one.

use crate::AppState;
use crate::assets::{GameAssets, SpriteAtlas};
use crate::link::GameEventMsg;
use crate::mirror::Mirror;
use crate::render::anim::WorldClock;
use crate::render::consts::PX;
use crate::render::light::{LightCandidates, LightSrc};
use crate::render::sprites::{
    Basis, DrawStage, Flavor, Layer, Painter, SpriteBatches, SpriteOpts, drawable, foot_world,
};
use crate::render::sprites_overlay::LabelRequests;
use crate::render::sprites_world::{Fog, NONE, Sheets};
use crate::render::terrain::FogTexture;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowReceiver;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ts_sim::TILE;
use ts_sim::defs::{FX_INFO, FxKind, Res as ResKind};
use ts_sim::events::GameEvent;
use ts_sim::slab::UnitId;

/// Render layer seen only by the sun (cloud shadow casters).
pub const SHADOW_ONLY_LAYER: usize = 1;
/// Height of the cloud layer above the water (world units).
const CLOUD_HEIGHT: f32 = 11.0;
/// Margin (px) outside the view inside which FX events are still spawned.
const FX_MARGIN: f32 = 80.0;

#[derive(Clone, Copy, Debug)]
struct FxInst {
    kind: FxKind,
    sheet: u32,
    x: f32,
    y: f32,
    scale: f32,
    dy: f32,
    follow: Option<UnitId>,
    t: f32,
    fps: f32,
    frames: u16,
}

#[derive(Clone, Copy, Debug)]
struct Float {
    x: f32,
    y: f32,
    text_amount: i32,
    res: ResKind,
    t: f32,
}

#[derive(Resource, Default)]
struct FxState {
    generation: u32,
    list: Vec<FxInst>,
    floats: Vec<Float>,
    splash_t: f32,
    rng: u32,
}

impl FxState {
    fn rnd(&mut self) -> f32 {
        // xorshift32 (presentation randomness, client-side)
        let mut x = self.rng.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[derive(Component)]
struct CloudShadow(usize);

#[derive(Resource, Default)]
struct Clouds {
    generation: Option<u32>,
    /// (top-left x, y px, speed px/s, variant)
    list: Vec<(f32, f32, f32, u8)>,
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<StandardMaterial>>,
    rng: u32,
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FxState>()
            .init_resource::<Clouds>()
            .add_systems(
                Update,
                (
                    (consume_events, tick_fx)
                        .chain()
                        .in_set(crate::render::sprites::SpriteSystems::Prepare),
                    draw_fx.in_set(DrawStage::Fill),
                    draw_floats.in_set(DrawStage::Late),
                    clouds,
                )
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(OnExit(AppState::InGame), clear_fx);
    }
}

fn clear_fx(mut st: ResMut<FxState>) {
    st.list.clear();
    st.floats.clear();
}

#[allow(clippy::too_many_arguments)]
fn consume_events(
    mut events: MessageReader<GameEventMsg>,
    mirror: Res<Mirror>,
    atlas: Res<SpriteAtlas>,
    sheets: Res<Sheets>,
    basis: Res<Basis>,
    fog: Option<Res<FogTexture>>,
    mut st: ResMut<FxState>,
) {
    if st.generation != mirror.generation {
        st.generation = mirror.generation;
        st.list.clear();
        st.floats.clear();
        st.splash_t = 2.0;
        st.rng = mirror.seed ^ 0x5eed_f00d;
    }
    let fog = Fog(fog.as_deref());
    for GameEventMsg(e) in events.read() {
        match e {
            GameEvent::Fx {
                fx,
                x,
                y,
                scale,
                dy,
                follow,
                color,
            } => {
                if !atlas.ready || !sheets.ready {
                    continue;
                }
                if basis.valid && !basis.in_view(*x, *y, FX_MARGIN) {
                    continue;
                }
                if !fog.sees(*x, *y) {
                    continue;
                }
                let sheet = if *fx == FxKind::Heal {
                    let c = color.unwrap_or(ts_sim::defs::KingdomColor::Blue);
                    sheets.heal[c.index()]
                } else {
                    sheets.fx[fx.index()]
                };
                if sheet == NONE {
                    continue;
                }
                let info = FX_INFO[fx.index()];
                st.list.push(FxInst {
                    kind: *fx,
                    sheet,
                    x: *x,
                    y: *y,
                    scale: if *scale > 0.0 { *scale } else { 1.0 },
                    dy: *dy,
                    follow: *follow,
                    t: 0.0,
                    fps: info.4,
                    frames: info.5 as u16,
                });
            }
            GameEvent::Float {
                to,
                x,
                y,
                amount,
                res,
            } => {
                if Some(*to) == mirror.you {
                    st.floats.push(Float {
                        x: *x,
                        y: *y,
                        text_amount: *amount,
                        res: *res,
                        t: 0.0,
                    });
                }
            }
            _ => {}
        }
    }
}

/// Ages FX / floats and spawns the ambient water splashes.
fn tick_fx(
    mirror: Res<Mirror>,
    clock: Res<WorldClock>,
    sheets: Res<Sheets>,
    time: Res<Time<Real>>,
    mut st: ResMut<FxState>,
) {
    let dt = clock.dt;
    // FX positions follow their unit while it exists
    let now = time.elapsed_secs_f64();
    for f in st.list.iter_mut() {
        f.t += dt;
        if let Some(id) = f.follow
            && let Some(p) = mirror.unit_pos_interp(id, now)
        {
            f.x = p.x;
            f.y = p.y;
        }
    }
    st.list.retain(|f| f.fps <= 0.0 || f.t * f.fps < f.frames as f32);
    for f in st.floats.iter_mut() {
        f.t += time.delta_secs();
        f.y -= 28.0 * time.delta_secs();
    }
    st.floats.retain(|f| f.t < 1.2);
    // ambient splashes on open water (main.js:320-326)
    if dt > 0.0 && sheets.ready && sheets.fx[FxKind::Splash.index()] != NONE {
        st.splash_t -= dt;
        if st.splash_t <= 0.0 {
            let r = st.rnd();
            st.splash_t = 0.4 + r * 0.8;
            if let Some(g) = mirror.world.as_ref()
                && !g.open_water.is_empty()
            {
                let i = ((st.rnd() * g.open_water.len() as f32) as usize).min(g.open_water.len() - 1);
                let c = g.open_water[i];
                let p = g.map.center(c);
                let info = FX_INFO[FxKind::Splash.index()];
                st.list.push(FxInst {
                    kind: FxKind::Splash,
                    sheet: sheets.fx[FxKind::Splash.index()],
                    x: p.x,
                    y: p.y,
                    scale: 1.0,
                    dy: 0.0,
                    follow: None,
                    t: 0.0,
                    fps: info.4,
                    frames: info.5 as u16,
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_fx(
    mirror: Res<Mirror>,
    atlas: Res<SpriteAtlas>,
    basis: Res<Basis>,
    fog: Option<Res<FogTexture>>,
    st: Res<FxState>,
    mut batches: ResMut<SpriteBatches>,
    mut lights: ResMut<LightCandidates>,
) {
    let Some(map) = drawable(&mirror, &atlas, &basis) else {
        return;
    };
    if batches.pages == 0 {
        return;
    }
    let fog = Fog(fog.as_deref());
    let mut p = Painter {
        batches: &mut batches,
        basis: &basis,
        atlas: &atlas,
    };
    for f in &st.list {
        if !basis.in_view(f.x, f.y, 200.0) {
            continue;
        }
        let fr = (f.t * f.fps).floor() as i64;
        let water = f.kind == FxKind::Splash;
        let foot = if water {
            Vec3::new(f.x * PX, crate::render::consts::WATER_Y, f.y * PX)
        } else {
            foot_world(map, f.x, f.y + f.dy)
        };
        let lift = if water { 0.0 } else { f.dy.max(0.0) };
        let (flavor, color) = match f.kind {
            FxKind::Dust1 | FxKind::Dust2 | FxKind::Splash => (Flavor::LitFlat, fog.tint(f.x, f.y)),
            _ => (Flavor::Overlay, [1.0; 4]),
        };
        p.sprite(Layer::Dynamic, flavor, f.sheet, fr, foot, SpriteOpts {
            scale: f.scale,
            lift,
            color,
            fold: lift <= 0.0,
            pull: if flavor == Flavor::Overlay { 0.2 } else { 0.05 },
            ..default()
        });
        if matches!(f.kind, FxKind::Boom1 | FxKind::Boom2) {
            let k = 1.0 - (f.t * f.fps / f.frames.max(1) as f32).clamp(0.0, 1.0);
            lights.push(LightSrc {
                pos: foot + basis.up * (lift + 40.0),
                r: 260.0 * f.scale,
                power: 1.6 * k,
                seed: f.x,
                flick: 0.2,
            });
        }
    }
}

/// Floating "+N" texts: resource icon + label rising 28 px/s for 1.2 s (07 §8.5).
fn draw_floats(
    mirror: Res<Mirror>,
    atlas: Res<SpriteAtlas>,
    basis: Res<Basis>,
    ga: Res<GameAssets>,
    st: Res<FxState>,
    mut labels: ResMut<LabelRequests>,
) {
    let Some(map) = drawable(&mirror, &atlas, &basis) else {
        return;
    };
    for f in &st.floats {
        if !basis.in_view(f.x, f.y, 100.0) {
            continue;
        }
        let a = (1.6 - f.t * 1.4).clamp(0.0, 1.0);
        if a <= 0.0 {
            continue;
        }
        // (x, y) is a 2D point: a ground point lifted by the rise so far
        let rise = f.t * 28.0;
        let foot = foot_world(map, f.x, f.y + rise);
        let w = foot + basis.up * rise;
        let Some((s, _)) = basis.project(w) else {
            continue;
        };
        let k = match basis.project(w + basis.right * 100.0) {
            Some((b, _)) => (b - s).length() / 100.0,
            None => 1.0,
        };
        let icon = match f.res {
            ResKind::Wood => ga.image("UI Elements/UI Elements/Icons/Icon_02.png"),
            ResKind::Gold => ga.image("UI Elements/UI Elements/Icons/Icon_03.png"),
            ResKind::Meat => ga.image("UI Elements/UI Elements/Icons/Icon_04.png"),
            ResKind::Stone => ga.image("Extra/Icons/stone.png"),
        };
        let text = if f.text_amount >= 0 {
            format!("+{}", f.text_amount)
        } else {
            format!("{}", f.text_amount)
        };
        labels.list.push((
            s - Vec2::new(33.0 * k, 0.0),
            text,
            18.0 * k,
            Color::srgba_u8(0xff, 0xf7, 0xc9, (a * 255.0) as u8),
            icon,
            false,
        ));
    }
}

/// Drifting cloud shadows.
#[allow(clippy::too_many_arguments)]
fn clouds(
    mut commands: Commands,
    mirror: Res<Mirror>,
    ga: Res<GameAssets>,
    time: Res<Time<Real>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut st: ResMut<Clouds>,
    mut q: Query<(&CloudShadow, &mut Transform)>,
) {
    let Some(g) = mirror.world.as_ref() else {
        return;
    };
    if st.materials.is_empty() {
        // one dithered silhouette per cloud image (half-resolution checkerboard ⇒ ~50 % shadow after PCF)
        for v in 0..8 {
            let rel = format!("Terrain/Decorations/Clouds/Clouds_0{}.png", v + 1);
            let Some(src) = ga.image(&rel).and_then(|h| images.get(&h).cloned()) else {
                return;
            };
            let (w, h) = (src.width(), src.height());
            let Some(data) = src.data.as_ref() else {
                return;
            };
            if data.len() < (w * h * 4) as usize {
                return;
            }
            let (dw, dh) = (w / 2, h / 2);
            let mut out = vec![0u8; (dw * dh * 4) as usize];
            for y in 0..dh {
                for x in 0..dw {
                    let si = (((y * 2) * w + x * 2) * 4) as usize;
                    let solid = data[si + 3] == 255;
                    let i = ((y * dw + x) * 4) as usize;
                    if solid && (x + y) % 2 == 0 {
                        out[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
            let mut img = Image::new(
                Extent3d {
                    width: dw,
                    height: dh,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                out,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
            let tex = images.add(img);
            st.materials.push(materials.add(StandardMaterial {
                base_color_texture: Some(tex),
                alpha_mode: AlphaMode::Mask(0.5),
                unlit: true,
                cull_mode: None,
                ..default()
            }));
            // horizontal quad 576×256 px (image top = north)
            let (hw, hh) = (288.0 * PX, 128.0 * PX);
            let m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_POSITION,
                    vec![[-hw, 0.0, hh], [hw, 0.0, hh], [hw, 0.0, -hh], [-hw, 0.0, -hh]],
                )
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 4])
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_UV_0,
                    vec![[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                )
                .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
            st.meshes.push(meshes.add(m));
        }
    }
    if st.generation != Some(mirror.generation) {
        st.generation = Some(mirror.generation);
        st.rng = mirror.seed ^ 0xc10d_5eed;
        st.list = g
            .map
            .decor
            .iter()
            .filter(|d| d.kind == ts_sim::map::DecorKind::Cloud)
            .map(|d| (d.x, d.y, d.scale.clamp(7.0, 16.0), d.variant % 8))
            .collect();
        for (i, &(_, _, _, v)) in st.list.iter().enumerate() {
            let exists = q.iter().any(|(c, _)| c.0 == i);
            if !exists {
                commands.spawn((
                    CloudShadow(i),
                    Mesh3d(st.meshes[v as usize].clone()),
                    MeshMaterial3d(st.materials[v as usize].clone()),
                    Transform::from_xyz(0.0, -100.0, 0.0),
                    RenderLayers::layer(SHADOW_ONLY_LAYER),
                    NotShadowReceiver,
                    DespawnOnExit(AppState::InGame),
                ));
            }
        }
    }
    let dt = time.delta_secs().min(0.1);
    let mw = g.map.w as f32 * TILE;
    let mh = g.map.h as f32 * TILE;
    let mut rng = st.rng;
    let mut rnd = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        (rng >> 8) as f32 / (1u32 << 24) as f32
    };
    for c in st.list.iter_mut() {
        c.0 += c.2 * dt;
        if c.0 > mw + 100.0 {
            c.0 = -600.0;
            c.1 = -100.0 + rnd() * (mh - 50.0);
        }
    }
    st.rng = rng;
    for (cs, mut tf) in &mut q {
        if let Some(&(x, y, _, _)) = st.list.get(cs.0) {
            tf.translation = Vec3::new((x + 288.0) * PX, CLOUD_HEIGHT, (y + 128.0) * PX);
        }
    }
}
