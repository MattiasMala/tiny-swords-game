// OWNER: C1 (terrain) — RTS camera rig. Spec 05-control-loop.md §11 (pan speeds, zoom steps, limits, lookAt — the
// INPUT lives in control/, C4), docs/ARCHITECTURE.md "Rendering HD-2D" (perspective, narrow FOV, fixed pitch,
// looking north).
//
// CONTRACT (other owners code against it — keep stable):
// * `CameraRig` resource: `focus` (sim px, the ground point looked at) and `zoom` (1 = default, > 1 closer) are the
//   TARGETS written by control (keys / edge pan / wheel), the minimap (C3) and `look_at`. This module eases the
//   displayed camera towards them (`cur_focus`, `cur_zoom`), clamps them to the map, and centres the rig on the own
//   castle once per match (or on `--cam X,Y`).
// * Helpers: `world_to_screen`, `px_to_screen`, `cursor_ray`, `ray_ground_hit` (DDA through the terrain heightfield,
//   levels and stair ramps included) and `cursor_ground_hit`; the `RtsView` system param bundles them with the
//   camera, the primary window and the map.
// * `CameraRig::view_corners`: ground points (px) under the 4 screen corners, updated every frame (minimap frame).

use crate::AppState;
use crate::dev::DevArgs;
use crate::mirror::Mirror;
use crate::render::consts::{GROUND_Y, LEVEL_HEIGHT, PX, WATER_Y};
use crate::render::terrain::{MAX_TERRAIN_Y, ground_height};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use ts_sim::TILE;
use ts_sim::defs::BuildingKind;
use ts_sim::map::Map;

/// Distance from the focus point (world units) at zoom 1 (≈ one screen px per sprite px on a 850-px-high view).
pub const BASE_DISTANCE: f32 = 26.0;
/// Camera pitch below the horizon.
pub const PITCH_DEG: f32 = 45.0;
/// Vertical field of view (narrow: diorama look).
pub const FOV_DEG: f32 = 30.0;
/// Discrete zoom steps (spec 05 §11.2: 5 steps, min = ⅓ of max).
pub const ZOOM_STEPS: [f32; 5] = [0.5, 0.75, 1.0, 1.25, 1.5];
/// Hard zoom limits (targets are clamped into this range).
pub const ZOOM_MIN: f32 = 0.4;
pub const ZOOM_MAX: f32 = 2.0;
/// Height the camera aims at (half a level above the lowest ground): keeps plateaus and coast both framed.
pub const FOCUS_Y: f32 = GROUND_Y + LEVEL_HEIGHT * 0.5;
/// Easing rate of the displayed focus / zoom (1/s).
const EASE: f32 = 16.0;
/// Focus may not leave the map by more than this (px) — the border is open sea.
const EDGE_SLACK_PX: f32 = 2.0 * TILE;

/// Marker of the main game camera.
#[derive(Component)]
pub struct RtsCamera;

/// The RTS camera rig (see the module header for who writes what).
#[derive(Resource, Debug, Clone)]
pub struct CameraRig {
    /// TARGET ground point looked at, sim px (x east, y south)
    pub focus: Vec2,
    /// TARGET zoom: 1 = default distance, > 1 closer (clamped to `ZOOM_MIN..=ZOOM_MAX`)
    pub zoom: f32,
    /// the camera always looks north with a fixed yaw (kept for a possible free-rotate mode; always `true`)
    pub yaw_locked: bool,
    /// pitch below the horizon, degrees
    pub pitch_deg: f32,
    /// jump to the targets on the next frame instead of easing (teleports: minimap click, lookAt, match start)
    pub snap: bool,
    /// displayed (eased) focus in px — read-only for other modules
    pub cur_focus: Vec2,
    /// displayed (eased) zoom — read-only for other modules
    pub cur_zoom: f32,
    /// ground points (px) under the screen corners TL, TR, BR, BL (updated every frame; minimap view frame)
    pub view_corners: [Vec2; 4],
    /// map size in px (clamp bounds; set on every match)
    pub map_px: Vec2,
}

impl Default for CameraRig {
    fn default() -> Self {
        let map_px = Vec2::new(ts_sim::MAP_W as f32 * TILE, ts_sim::MAP_H as f32 * TILE);
        let c = map_px * 0.5;
        CameraRig {
            focus: c,
            zoom: 1.0,
            yaw_locked: true,
            pitch_deg: PITCH_DEG,
            snap: true,
            cur_focus: c,
            cur_zoom: 1.0,
            view_corners: [c; 4],
            map_px,
        }
    }
}

impl CameraRig {
    /// Distance eye → focus for a zoom value.
    pub fn distance_for(zoom: f32) -> f32 {
        BASE_DISTANCE / zoom.clamp(ZOOM_MIN, ZOOM_MAX)
    }

    /// Displayed distance eye → focus (world units).
    pub fn distance(&self) -> f32 {
        Self::distance_for(self.cur_zoom)
    }

    /// Camera transform for a focus (px) and zoom.
    pub fn transform_for(&self, focus_px: Vec2, zoom: f32) -> Transform {
        let p = self.pitch_deg.to_radians();
        let target = Vec3::new(focus_px.x * PX, FOCUS_Y, focus_px.y * PX);
        let d = Self::distance_for(zoom);
        let eye = target + Vec3::new(0.0, p.sin() * d, p.cos() * d);
        Transform::from_translation(eye).looking_at(target, Vec3::Y)
    }

    /// Transform of the displayed state.
    pub fn transform(&self) -> Transform {
        self.transform_for(self.cur_focus, self.cur_zoom)
    }

    /// Clamps the targets into the map / zoom limits.
    pub fn clamp(&mut self) {
        if !self.focus.is_finite() {
            self.focus = self.map_px * 0.5;
        }
        if !self.zoom.is_finite() {
            self.zoom = 1.0;
        }
        self.focus = self.focus.clamp(
            Vec2::splat(-EDGE_SLACK_PX),
            self.map_px + Vec2::splat(EDGE_SLACK_PX),
        );
        self.zoom = self.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    }

    /// `lookAt(x, y)` (spec 05 §11.4): centre the view on a point (px) immediately.
    pub fn look_at(&mut self, px: Vec2) {
        self.focus = px;
        self.snap = true;
        self.clamp();
    }

    /// Index of the zoom step closest to the target zoom.
    pub fn step_index(&self) -> usize {
        let mut best = 0;
        for (i, z) in ZOOM_STEPS.iter().enumerate() {
            if (z - self.zoom).abs() < (ZOOM_STEPS[best] - self.zoom).abs() {
                best = i;
            }
        }
        best
    }

    /// One discrete zoom step (`dir` > 0 = closer), keeping the ground point `anchor_px` (e.g. under the mouse)
    /// fixed on screen (spec 05 §11.2 `setZoom`); `None` = around the screen centre. Returns false at the limit.
    pub fn zoom_step(&mut self, dir: i32, anchor_px: Option<Vec2>) -> bool {
        let i = self.step_index() as i32;
        let j = (i + dir.signum()).clamp(0, ZOOM_STEPS.len() as i32 - 1);
        if i == j && (ZOOM_STEPS[i as usize] - self.zoom).abs() < 1e-3 {
            return false;
        }
        self.zoom_to(ZOOM_STEPS[j as usize], anchor_px);
        true
    }

    /// Sets the target zoom keeping `anchor_px` fixed on screen (exact for an orthographic view, very close for
    /// the narrow perspective used here).
    pub fn zoom_to(&mut self, zoom: f32, anchor_px: Option<Vec2>) {
        let old = self.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        let new = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        if let Some(a) = anchor_px.filter(|a| a.is_finite()) {
            self.focus = a + (self.focus - a) * (old / new);
        }
        self.zoom = new;
        self.clamp();
    }
}

/// Sim px of the centre of tile `(cx, cy)`.
#[inline]
fn tile_center_px(cx: f32, cy: f32) -> Vec2 {
    Vec2::new((cx + 0.5) * TILE, (cy + 0.5) * TILE)
}

// ---------------------------------------------------------------------------------------------------------------
// Projection helpers
// ---------------------------------------------------------------------------------------------------------------

/// World position → logical screen px (origin top-left); `None` behind the camera / outside the viewport math.
pub fn world_to_screen(camera: &Camera, cam_tf: &GlobalTransform, p: Vec3) -> Option<Vec2> {
    camera.world_to_viewport(cam_tf, p).ok()
}

/// Sim point (px) `lift` world units above the terrain → screen px.
pub fn px_to_screen(
    camera: &Camera,
    cam_tf: &GlobalTransform,
    map: Option<&Map>,
    x: f32,
    y: f32,
    lift: f32,
) -> Option<Vec2> {
    let wx = x * PX;
    let wz = y * PX;
    let h = map.map_or(GROUND_Y, |m| ground_height(m, wx, wz));
    world_to_screen(camera, cam_tf, Vec3::new(wx, h + lift, wz))
}

/// Ray through a screen point (logical px).
pub fn cursor_ray(camera: &Camera, cam_tf: &GlobalTransform, screen: Vec2) -> Option<Ray3d> {
    camera.viewport_to_world(cam_tf, screen).ok()
}

/// First hit of a ray with the terrain (columns, ramps) or the water plane, in world units. Exact DDA over the
/// cell grid between the top of the highest level and the water plane; outside the map the water plane is hit.
pub fn ray_ground_hit(map: Option<&Map>, ray: Ray3d) -> Option<Vec3> {
    let o = ray.origin;
    let d: Vec3 = *ray.direction;
    if d.y >= -1e-5 {
        return None;
    }
    let water_t = (WATER_Y - o.y) / d.y;
    let Some(map) = map else {
        return Some(o + d * water_t);
    };
    // segment of the ray between y = top and y = water
    let top = MAX_TERRAIN_Y + 0.01;
    let t_start = ((top - o.y) / d.y).max(0.0);
    let t_end = water_t.max(t_start);
    let h_at = |t: f32| -> f32 {
        let p = o + d * t;
        ground_height(map, p.x, p.z)
    };
    let f = |t: f32| -> f32 { o.y + d.y * t - h_at(t) };
    // DDA over cells in XZ
    let p0 = o + d * t_start;
    let (mut cx, mut cz) = (p0.x.floor() as i32, p0.z.floor() as i32);
    let step_x = if d.x > 0.0 { 1 } else { -1 };
    let step_z = if d.z > 0.0 { 1 } else { -1 };
    let inv_x = if d.x.abs() > 1e-9 { 1.0 / d.x } else { f32::INFINITY };
    let inv_z = if d.z.abs() > 1e-9 { 1.0 / d.z } else { f32::INFINITY };
    let next_bx = if step_x > 0 { cx as f32 + 1.0 } else { cx as f32 };
    let next_bz = if step_z > 0 { cz as f32 + 1.0 } else { cz as f32 };
    let mut t_max_x = if inv_x.is_finite() { t_start + (next_bx - p0.x) * inv_x } else { f32::INFINITY };
    let mut t_max_z = if inv_z.is_finite() { t_start + (next_bz - p0.z) * inv_z } else { f32::INFINITY };
    let t_delta_x = inv_x.abs();
    let t_delta_z = inv_z.abs();
    let mut t0 = t_start;
    let max_steps = (map.w + map.h) as usize * 4 + 16;
    for _ in 0..max_steps {
        let t1 = t_max_x.min(t_max_z).min(t_end);
        // inside cell (cx, cz) for t in [t0, t1]
        let inside = cx >= 0 && cz >= 0 && cx < map.w && cz < map.h;
        if inside || cx.abs() < 4 * map.w {
            let f0 = f(t0 + 1e-4);
            if f0 <= 0.0 {
                // entered through a side face (cliff wall): hit at the entry point
                return Some(o + d * t0);
            }
            let f1 = f(t1 - 1e-4);
            if f1 <= 0.0 {
                // bisection between t0 (above) and t1 (below)
                let (mut a, mut b) = (t0, t1);
                for _ in 0..14 {
                    let m = 0.5 * (a + b);
                    if f(m) > 0.0 {
                        a = m;
                    } else {
                        b = m;
                    }
                }
                return Some(o + d * b);
            }
        }
        if t1 >= t_end {
            break;
        }
        t0 = t1;
        if t_max_x < t_max_z {
            cx += step_x;
            t_max_x += t_delta_x;
        } else {
            cz += step_z;
            t_max_z += t_delta_z;
        }
    }
    Some(o + d * water_t)
}

/// Ground point (sim px) under a screen point; `None` when the ray misses (looking up).
pub fn cursor_ground_hit(
    camera: &Camera,
    cam_tf: &GlobalTransform,
    map: Option<&Map>,
    screen: Vec2,
) -> Option<Vec2> {
    let ray = cursor_ray(camera, cam_tf, screen)?;
    let p = ray_ground_hit(map, ray)?;
    Some(Vec2::new(p.x * TILE, p.z * TILE))
}

/// System param bundling the camera, the primary window and the map for picking / overlays.
#[derive(SystemParam)]
pub struct RtsView<'w, 's> {
    cams: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<RtsCamera>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    mirror: Res<'w, Mirror>,
}

impl RtsView<'_, '_> {
    /// Cursor position in logical px (top-left origin), if inside the window.
    pub fn cursor(&self) -> Option<Vec2> {
        self.windows.single().ok()?.cursor_position()
    }

    /// Ground point (px) under the cursor.
    pub fn cursor_ground(&self) -> Option<Vec2> {
        let c = self.cursor()?;
        self.ground_at_screen(c)
    }

    /// Ground point (px) under a screen point.
    pub fn ground_at_screen(&self, screen: Vec2) -> Option<Vec2> {
        let (cam, tf) = self.cams.single().ok()?;
        cursor_ground_hit(cam, tf, self.mirror.map(), screen)
    }

    /// Sim point (px) lifted `lift` world units above the terrain → screen px.
    pub fn px_to_screen(&self, x: f32, y: f32, lift: f32) -> Option<Vec2> {
        let (cam, tf) = self.cams.single().ok()?;
        px_to_screen(cam, tf, self.mirror.map(), x, y, lift)
    }

    /// World position → screen px.
    pub fn world_to_screen(&self, p: Vec3) -> Option<Vec2> {
        let (cam, tf) = self.cams.single().ok()?;
        world_to_screen(cam, tf, p)
    }

    /// Terrain height (world units) under a sim point (px).
    pub fn ground_y_px(&self, x: f32, y: f32) -> f32 {
        self.mirror
            .map()
            .map_or(GROUND_Y, |m| ground_height(m, x * PX, y * PX))
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------------------------------------------

/// Deprecated alias of the skeleton name (control/ skeleton); use `CameraRig`.
pub type CamRig = CameraRig;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraRig>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                PostUpdate,
                (
                    center_on_match.run_if(in_state(AppState::InGame)),
                    apply_rig,
                )
                    .chain()
                    .before(TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                update_view_corners.after(TransformSystems::Propagate),
            );
    }
}

fn spawn_camera(mut commands: Commands, mut rig: ResMut<CameraRig>, dev: Res<DevArgs>) {
    if let Some(z) = dev.zoom {
        rig.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
        rig.cur_zoom = rig.zoom;
    }
    commands.spawn((
        RtsCamera,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEG.to_radians(),
            near: 0.5,
            far: 400.0,
            ..default()
        }),
        rig.transform(),
    ));
}

/// Centres the rig once per match (after the first snapshot): `--cam`, else own castle, else an own unit.
fn center_on_match(
    mirror: Res<Mirror>,
    dev: Res<DevArgs>,
    mut rig: ResMut<CameraRig>,
    mut done_gen: Local<Option<u32>>,
) {
    if !mirror.active || mirror.snapshots == 0 || *done_gen == Some(mirror.generation) {
        return;
    }
    *done_gen = Some(mirror.generation);
    if let Some(m) = mirror.map() {
        rig.map_px = Vec2::new(m.w as f32 * TILE, m.h as f32 * TILE);
    }
    if let Some(c) = dev.cam {
        rig.look_at(tile_center_px(c.x, c.y));
        return;
    }
    let castle = mirror
        .view
        .buildings
        .values()
        .find(|b| Some(b.faction) == mirror.you && b.kind == BuildingKind::Castle);
    if let Some(b) = castle {
        // footprint with top-left (cx, cy): look at its centre (JS: castle y - 80 px)
        let d = b.kind.def();
        rig.look_at(Vec2::new(
            (b.cx as f32 + d.w as f32 * 0.5) * TILE,
            (b.cy as f32 + d.h as f32 * 0.5) * TILE,
        ));
    } else if let Some(u) = mirror
        .view
        .units
        .values()
        .find(|u| Some(u.faction) == mirror.you)
    {
        rig.look_at(Vec2::new(u.x, u.y));
    } else if let Some(m) = mirror.map() {
        rig.look_at(Vec2::new(m.w as f32, m.h as f32) * TILE * 0.5);
    }
}

/// Eases the displayed state towards the targets and writes the camera transform.
fn apply_rig(
    time: Res<Time<Real>>,
    mut rig: ResMut<CameraRig>,
    mut q: Query<&mut Transform, With<RtsCamera>>,
) {
    let r = rig.bypass_change_detection();
    r.clamp();
    let dt = time.delta_secs().min(0.1);
    let before = (r.cur_focus, r.cur_zoom);
    if r.snap {
        r.snap = false;
        r.cur_focus = r.focus;
        r.cur_zoom = r.zoom;
    } else {
        let k = 1.0 - (-EASE * dt).exp();
        r.cur_focus += (r.focus - r.cur_focus) * k;
        // ease zoom in log space (uniform feel)
        let lz = r.cur_zoom.ln() + (r.zoom.ln() - r.cur_zoom.ln()) * k;
        r.cur_zoom = lz.exp();
        if r.cur_focus.distance_squared(r.focus) < 0.01 {
            r.cur_focus = r.focus;
        }
        if (r.cur_zoom - r.zoom).abs() < 1e-4 {
            r.cur_zoom = r.zoom;
        }
    }
    let changed = before != (r.cur_focus, r.cur_zoom);
    let tf = r.transform();
    for mut t in &mut q {
        if changed || *t != tf {
            *t = tf;
        }
    }
}

/// Ground points under the 4 screen corners (for the minimap frame).
fn update_view_corners(
    mut rig: ResMut<CameraRig>,
    mirror: Res<Mirror>,
    cams: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
) {
    let Ok((cam, tf)) = cams.single() else { return };
    let Some(size) = cam.logical_viewport_size() else {
        return;
    };
    let map = mirror.map();
    let pts = [
        Vec2::ZERO,
        Vec2::new(size.x, 0.0),
        size,
        Vec2::new(0.0, size.y),
    ];
    let mut out = rig.view_corners;
    for (i, s) in pts.iter().enumerate() {
        // corners: water plane is enough (and cheaper) for the frame
        if let Some(ray) = cursor_ray(cam, tf, *s) {
            let p = ray_ground_hit(None, ray).or_else(|| ray_ground_hit(map, ray));
            if let Some(p) = p {
                out[i] = Vec2::new(p.x * TILE, p.z * TILE);
            }
        }
    }
    if out != rig.view_corners {
        rig.bypass_change_detection().view_corners = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_and_anchor() {
        let mut r = CameraRig::default();
        r.zoom = 1.0;
        r.focus = Vec2::new(1000.0, 1000.0);
        assert!(r.zoom_step(1, Some(Vec2::new(1200.0, 1000.0))));
        assert!((r.zoom - 1.25).abs() < 1e-6);
        // anchor stays: focus moved toward it by 1 - 1/1.25
        assert!((r.focus.x - (1200.0 - 200.0 * 0.8)).abs() < 1e-3);
        r.zoom = 1.5;
        assert!(!r.zoom_step(1, None));
    }

    #[test]
    fn ray_hits_water_without_map() {
        let ray = Ray3d::new(Vec3::new(5.0, 10.0, 5.0), Dir3::new(Vec3::new(0.0, -1.0, 0.0)).unwrap());
        let p = ray_ground_hit(None, ray).unwrap();
        assert!((p.y - WATER_Y).abs() < 1e-5 && (p.x - 5.0).abs() < 1e-5);
    }
}
