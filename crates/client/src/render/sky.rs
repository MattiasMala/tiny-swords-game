// OWNER: C1 (terrain) — sky / atmosphere: distance fog for depth (docs/ARCHITECTURE.md "nebbia di distanza"),
// tinted by the day phase (spec 07 §2 DAY_KEYS). WebGL2 has no depth of field: the far (upper) part of the view
// fades into a light haze instead, which reads as the tilt-shift band of the 2D post pass (spec 07 §6). The
// vignette of that pass is a camera post effect owned by `light` (C2).

use crate::render::camera::{CameraRig, RtsCamera};
use crate::render::light::DayTime;
use bevy::prelude::*;

/// `DAY_KEYS` (spec 07 §2.1): phase, ambient RGB.
const DAY_KEYS: [(f32, [f32; 3]); 8] = [
    (0.00, [255.0, 200.0, 172.0]),
    (0.07, [255.0, 251.0, 240.0]),
    (0.50, [255.0, 251.0, 240.0]),
    (0.60, [255.0, 206.0, 150.0]),
    (0.67, [208.0, 142.0, 150.0]),
    (0.73, [108.0, 124.0, 196.0]),
    (0.95, [108.0, 124.0, 196.0]),
    (1.00, [255.0, 200.0, 172.0]),
];

/// Ambient colour (0..255, rounded like the JS) and `night` (0..1) of a day phase (spec 07 §2.2).
pub fn day_ambient(phase: f32) -> ([f32; 3], f32) {
    let p = if phase.is_finite() {
        phase.rem_euclid(1.0)
    } else {
        0.1
    };
    let mut i = 1;
    while i < DAY_KEYS.len() - 1 && DAY_KEYS[i].0 < p {
        i += 1;
    }
    let (p0, c0) = DAY_KEYS[i - 1];
    let (p1, c1) = DAY_KEYS[i];
    let k = if p1 > p0 { (p - p0) / (p1 - p0) } else { 0.0 };
    let amb: [f32; 3] = std::array::from_fn(|j| (c0[j] + (c1[j] - c0[j]) * k).round());
    let lum = (amb[0] * 0.3 + amb[1] * 0.59 + amb[2] * 0.11) / 255.0;
    let night = ((0.92 - lum) / 0.42).clamp(0.0, 1.0);
    (amb, night)
}

/// Distance fog: linear from `FOG_START` to `FOG_END` × the camera distance, at most `FOG_ALPHA` opaque.
const FOG_START: f32 = 1.0;
const FOG_END: f32 = 3.6;
const FOG_ALPHA: f32 = 0.5;

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, update_fog);
    }
}

/// Distance fog on the game camera: a light haze far away (upper part of the view) by day, deep blue at night;
/// range follows the camera distance so every zoom level gets the same depth cue.
fn update_fog(
    mut commands: Commands,
    rig: Res<CameraRig>,
    day: Res<DayTime>,
    mut cams: Query<(Entity, Option<&mut DistanceFog>), With<RtsCamera>>,
) {
    let (amb, night) = day_ambient(day.0);
    let d = rig.distance();
    // haze colour: pale sea-sky by day, tinted by the ambient colour, dark blue at night
    let day_c = Vec3::new(0.80, 0.90, 0.92) * Vec3::new(amb[0], amb[1], amb[2]) / 255.0;
    let night_c = Vec3::new(0.10, 0.12, 0.24);
    let c = day_c.lerp(night_c, night * 0.85);
    let fog = DistanceFog {
        color: Color::srgba(c.x, c.y, c.z, FOG_ALPHA),
        directional_light_color: Color::srgba(1.0, 0.92, 0.78, 0.25 * (1.0 - night)),
        directional_light_exponent: 24.0,
        falloff: FogFalloff::Linear {
            start: d * FOG_START,
            end: d * FOG_END,
        },
    };
    for (e, f) in &mut cams {
        match f {
            Some(mut f) => {
                if f.color != fog.color
                    || f.directional_light_color != fog.directional_light_color
                    || !matches!(f.falloff, FogFalloff::Linear { start, end } if (start - d * FOG_START).abs() < 1e-3 && (end - d * FOG_END).abs() < 1e-3)
                {
                    *f = fog.clone();
                }
            }
            None => {
                commands.entity(e).insert(fog.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_keys_match_spec() {
        let (a, n) = day_ambient(0.1);
        assert_eq!(a, [255.0, 251.0, 240.0]);
        assert_eq!(n, 0.0);
        let (a, n) = day_ambient(0.8);
        assert_eq!(a, [108.0, 124.0, 196.0]);
        assert_eq!(n, 1.0);
        let (_, n) = day_ambient(0.6);
        assert!((n - 0.187).abs() < 0.01);
    }
}
