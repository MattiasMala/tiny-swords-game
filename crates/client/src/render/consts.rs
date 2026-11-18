//! World-space conventions of the 3D client (docs/IMPLEMENTATION-PLAN.md "Units and coordinates (client 3D)").
//! OWNER: foundation; C1 tunes the heights.
//!
//! 1 tile = 1.0 world unit. Sim `(x, y)` px → Bevy `(x / 64, height, y / 64)`: +X east, +Z south, +Y up.
//! Sprite pixels: 1 px = 1/64 world unit.

use bevy::prelude::*;
use ts_sim::TILE;
use ts_sim::map::Map;

/// World units per sim pixel.
pub const PX: f32 = 1.0 / TILE;
/// World units per terrain level (cliff height between tiers).
pub const LEVEL_HEIGHT: f32 = 1.0;
/// Height of the water surface.
pub const WATER_Y: f32 = 0.0;
/// Height of level-0 land (shore lip above the water).
pub const GROUND_Y: f32 = 0.25;
/// Depth of the sea floor (bottom of shore columns).
pub const SEABED_Y: f32 = -0.6;

/// Sim px → world XZ (no height).
#[inline]
pub fn px_to_xz(x: f32, y: f32) -> Vec2 {
    Vec2::new(x * PX, y * PX)
}

/// Sim px + height → world position.
#[inline]
pub fn px_to_world(x: f32, y: f32, h: f32) -> Vec3 {
    Vec3::new(x * PX, h, y * PX)
}

/// World position → sim px (ignores height).
#[inline]
pub fn world_to_px(p: Vec3) -> Vec2 {
    Vec2::new(p.x * TILE, p.z * TILE)
}

/// Tile `(cx, cy)` → world XZ of its centre.
#[inline]
pub fn tile_center(cx: f32, cy: f32) -> Vec2 {
    Vec2::new(cx + 0.5, cy + 0.5)
}

/// Height of the top of a terrain level.
#[inline]
pub fn level_y(level: u8) -> f32 {
    GROUND_Y + level as f32 * LEVEL_HEIGHT
}

/// Ground height at a cell index (water → `WATER_Y`).
pub fn cell_y(map: &Map, c: i32) -> f32 {
    if !map.valid(c) || !map.is_land(c) {
        WATER_Y
    } else {
        level_y(map.level_at(c))
    }
}

/// Ground height under a sim point (px).
pub fn ground_y_px(map: &Map, x: f32, y: f32) -> f32 {
    cell_y(map, map.cell_at(x, y))
}
