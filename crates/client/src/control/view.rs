//! Screen ↔ world helpers for input (OWNER: C4).
//!
//! [`ViewInfo`] is a per-frame copy of the game camera's matrices (refreshed in `PostUpdate` after transform
//! propagation, so `Update` systems of the next frame use a consistent camera). It projects sim points to window
//! logical pixels, casts the cursor ray onto the terrain heightfield (levels included, 05 §17.5) and answers
//! `on_screen` (05 addenda C1: always the local camera). Audio uses `center_px` / `width_px` for the positional
//! falloff (07 §10.1: `viewW` = ground width of the view).

use crate::mirror::Mirror;
use crate::render::camera::{RtsCamera, ray_ground_hit};
use crate::render::consts::{GROUND_Y, PX};
use crate::render::terrain::ground_height;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use ts_sim::TILE;
use ts_sim::map::Map;

/// Camera snapshot for projections.
#[derive(Resource, Debug, Clone, Default)]
pub struct ViewInfo {
    /// a camera exists and the matrices are valid
    pub ok: bool,
    pub clip_from_world: Mat4,
    pub world_from_clip: Mat4,
    /// viewport rect in window logical pixels
    pub viewport: Rect,
    /// ground point under the viewport centre (sim px)
    pub center_px: Vec2,
    /// ground width of the view through its centre row (sim px)
    pub width_px: f32,
    /// ground height used for `center_px`
    pub center_y: f32,
}

impl ViewInfo {
    /// World position → window logical px (`None` behind the camera).
    pub fn project(&self, p: Vec3) -> Option<Vec2> {
        if !self.ok {
            return None;
        }
        let c = self.clip_from_world * p.extend(1.0);
        if c.w <= 1e-5 {
            return None;
        }
        let ndc = c.truncate() / c.w;
        let size = self.viewport.size();
        Some(Vec2::new(
            self.viewport.min.x + (ndc.x * 0.5 + 0.5) * size.x,
            self.viewport.min.y + (0.5 - ndc.y * 0.5) * size.y,
        ))
    }

    /// Sim point (px) on the ground, lifted by `lift_px` sprite pixels, → window px.
    pub fn project_px(&self, map: Option<&Map>, x: f32, y: f32, lift_px: f32) -> Option<Vec2> {
        let h = ground_h(map, x, y);
        self.project(Vec3::new(x * PX, h + lift_px * PX, y * PX))
    }

    /// Ray through a window logical point.
    pub fn ray(&self, screen: Vec2) -> Option<Ray3d> {
        if !self.ok {
            return None;
        }
        let size = self.viewport.size();
        if size.x <= 0.0 || size.y <= 0.0 {
            return None;
        }
        let n = (screen - self.viewport.min) / size;
        let ndc = Vec2::new(n.x * 2.0 - 1.0, 1.0 - n.y * 2.0);
        // reverse-z: near = 1, far → 0
        let near = self.world_from_clip.project_point3(ndc.extend(1.0));
        let far = self.world_from_clip.project_point3(ndc.extend(0.01));
        let dir = Dir3::new(far - near).ok()?;
        Some(Ray3d::new(near, dir))
    }

    /// Ground point (sim px) under a window point: ray marched against the terrain heights; outside the map the
    /// ray meets the level-0 plane. `None` if the ray never goes down to the ground.
    pub fn ground_at(&self, map: Option<&Map>, screen: Vec2) -> Option<Vec2> {
        let ray = self.ray(screen)?;
        ground_hit(map, ray)
    }

    /// `onScreen(x, y, m)`: the sim point projects inside the viewport grown by `m` px.
    pub fn on_screen(&self, map: Option<&Map>, x: f32, y: f32, m: f32) -> bool {
        match self.project_px(map, x, y, 0.0) {
            Some(p) => {
                p.x >= self.viewport.min.x - m
                    && p.x <= self.viewport.max.x + m
                    && p.y >= self.viewport.min.y - m
                    && p.y <= self.viewport.max.y + m
            }
            None => false,
        }
    }

    /// Screen px per world unit at a given ground point (horizontal scale of sprites there).
    pub fn scale_at(&self, map: Option<&Map>, x: f32, y: f32) -> f32 {
        match (
            self.project_px(map, x, y, 0.0),
            self.project_px(map, x + TILE, y, 0.0),
        ) {
            (Some(a), Some(b)) => a.distance(b),
            _ => 0.0,
        }
    }
}

/// Ground height (world units) under a sim point (C1's terrain heights, ramps included).
pub fn ground_h(map: Option<&Map>, x: f32, y: f32) -> f32 {
    match map {
        Some(m) => ground_height(m, x * PX, y * PX),
        None => GROUND_Y,
    }
}

/// Ray vs terrain (C1's DDA `ray_ground_hit`). Returns sim px.
pub fn ground_hit(map: Option<&Map>, ray: Ray3d) -> Option<Vec2> {
    let p = ray_ground_hit(map, ray)?;
    Some(Vec2::new(p.x, p.z) * TILE)
}

/// Refreshes [`ViewInfo`] from the game camera.
pub fn update_view_info(
    cam: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mirror: Res<Mirror>,
    mut view: ResMut<ViewInfo>,
) {
    let Ok((camera, gt)) = cam.single() else {
        view.ok = false;
        return;
    };
    let rect = camera.logical_viewport_rect().or_else(|| {
        window
            .single()
            .ok()
            .map(|w| Rect::new(0.0, 0.0, w.width(), w.height()))
    });
    let Some(rect) = rect else {
        view.ok = false;
        return;
    };
    let clip_from_world = camera.clip_from_view() * gt.to_matrix().inverse();
    if !clip_from_world.is_finite() || rect.width() <= 0.0 {
        view.ok = false;
        return;
    }
    view.ok = true;
    view.clip_from_world = clip_from_world;
    view.world_from_clip = clip_from_world.inverse();
    view.viewport = rect;
    let map = mirror.map();
    let mid = rect.center();
    if let Some(c) = view.ground_at(map, mid) {
        view.center_px = c;
        view.center_y = ground_h(map, c.x, c.y);
        // width: rays through the left / right edge of the centre row meet the plane of the centre point
        let h = view.center_y;
        let at = |s: Vec2| -> Option<Vec2> {
            let r = view.ray(s)?;
            let t = (h - r.origin.y) / r.direction.y;
            (t > 0.0).then(|| {
                let p = r.origin + *r.direction * t;
                Vec2::new(p.x, p.z) * TILE
            })
        };
        if let (Some(a), Some(b)) = (
            at(Vec2::new(rect.min.x, mid.y)),
            at(Vec2::new(rect.max.x, mid.y)),
        ) {
            view.width_px = a.distance(b);
        }
    }
}
