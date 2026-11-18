// OWNER: C1 (terrain) — sky / background / distance fog (docs/ARCHITECTURE.md "nebbia di distanza", spec 07-light-sfx
// day colours). SKELETON: the background is the `ClearColor` (water colour) set in lib.rs; nothing else yet.
// C1 TODO: DistanceFog on the camera tinted by the day phase, horizon gradient beyond the map, drifting clouds
// (`DecorKind::Cloud`) shadows.

use bevy::prelude::*;

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, _app: &mut App) {}
}
