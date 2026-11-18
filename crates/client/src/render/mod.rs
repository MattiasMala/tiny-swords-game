//! HD-2D rendering. OWNER: foundation (module list only); each submodule has its own owner header.
//!
//! * C1: `terrain` (voxel-ish terrain chunks), `water`, `camera` (RTS camera rig), `sky`
//! * C2: `sprites` (billboard units/buildings/scenery), `light` (day/night), `fx`
//! * shared: `consts` (world scale, heights, px → world helpers)

use bevy::prelude::*;

pub mod camera;
pub mod consts;
pub mod fx;
pub mod light;
pub mod sky;
pub mod sprites;
pub mod terrain;
pub mod water;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            camera::CameraPlugin,
            terrain::TerrainPlugin,
            water::WaterPlugin,
            sky::SkyPlugin,
            sprites::SpritesPlugin,
            light::LightPlugin,
            fx::FxPlugin,
        ));
    }
}
