//! F2 lights / F3 render resolution and the slow-frame watchdog (05 §14.1-§14.2, 07 §7). OWNER: C4.
//!
//! The renderers read [`GraphicsQuality`]: `lights` 2 = full (shadows, point lights, post pass), 1 = light (shadows
//! and lights, no post pass / shafts), 0 = off; `res_scale()` = render-resolution factor (1, 0.75, 0.5).
//! Watchdog (real, unclamped frame time): slow frame = dt > 0.03 s, accumulator `+dt` when slow, `-2·dt` otherwise,
//! frames with dt > 0.25 s ignored; at 3 s the lights drop one level (auto mode only); resolution drops only once the
//! auto lights reached 0 (or the lights were set by hand).

use crate::AppState;
use crate::ui::UiState;
use bevy::prelude::*;

/// Render-resolution steps (`Quality.steps`).
pub const RES_STEPS: [f32; 3] = [1.0, 0.75, 0.5];
/// Slow-frame threshold (s).
pub const SLOW_DT: f32 = 0.03;
/// Accumulated slowness (s) that triggers a step down.
pub const SLOW_LIMIT: f32 = 3.0;
/// Frames longer than this are ignored (tab switches, loading hitches).
pub const IGNORE_DT: f32 = 0.25;

#[derive(Resource, Debug, Clone)]
pub struct GraphicsQuality {
    /// 2 full, 1 light, 0 off (`Light.level`)
    pub lights: u8,
    /// lights still under automatic control (`Light.auto`)
    pub lights_auto: bool,
    /// index into `RES_STEPS` (`Quality.i`)
    pub res_index: usize,
    /// resolution under automatic control (`Quality.auto`)
    pub res_auto: bool,
    slow_lights: f32,
    slow_res: f32,
}

impl Default for GraphicsQuality {
    fn default() -> Self {
        GraphicsQuality {
            lights: 2,
            lights_auto: true,
            res_index: 0,
            res_auto: true,
            slow_lights: 0.0,
            slow_res: 0.0,
        }
    }
}

impl GraphicsQuality {
    /// Render-resolution factor.
    pub fn res_scale(&self) -> f32 {
        RES_STEPS[self.res_index.min(RES_STEPS.len() - 1)]
    }

    /// F2: 2 → 1 → 0 → 2, manual from now on; returns the toast.
    pub fn cycle_lights(&mut self) -> &'static str {
        self.lights = (self.lights + 2) % 3;
        self.lights_auto = false;
        ["Luci spente", "Luci leggere: senza sfocatura, bagliore e raggi", "Luci complete"]
            [self.lights as usize]
    }

    /// F3: next resolution step, manual from now on; returns the toast.
    pub fn cycle_resolution(&mut self) -> &'static str {
        self.res_auto = false;
        self.res_index = (self.res_index + 1) % RES_STEPS.len();
        self.slow_res = 0.0;
        ["Risoluzione piena", "Risoluzione al 75%", "Risoluzione al 50%"][self.res_index]
    }

    /// One frame of the watchdog; returns a toast when something was lowered.
    pub fn watch(&mut self, dt: f32) -> Option<&'static str> {
        if dt > IGNORE_DT {
            return None;
        }
        let acc = |slow: f32| {
            if dt > SLOW_DT {
                slow + dt
            } else {
                (slow - 2.0 * dt).max(0.0)
            }
        };
        let mut out = None;
        // `Light.watch`
        if self.lights_auto && self.lights > 0 {
            self.slow_lights = acc(self.slow_lights);
            if self.slow_lights >= SLOW_LIMIT {
                self.lights -= 1;
                self.slow_lights = 0.0;
                out = Some(if self.lights > 0 {
                    "Il gioco rallentava: luci alleggerite (F2 per cambiarle)"
                } else {
                    "Il gioco rallentava: luci spente (F2 per riaccenderle)"
                });
            }
        }
        // `Quality.watch`
        let lights_busy = self.lights_auto && self.lights > 0;
        if self.res_auto && self.res_index < RES_STEPS.len() - 1 && !lights_busy {
            self.slow_res = acc(self.slow_res);
            if self.slow_res >= SLOW_LIMIT {
                self.res_index += 1;
                self.slow_res = 0.0;
                out = Some("Il gioco rallentava: risoluzione ridotta (F3 per cambiarla)");
            }
        }
        out
    }
}

/// Watchdog system: runs during a match only (the menu has no demo battle to measure).
pub fn watch(
    time: Res<Time<Real>>,
    state: Res<State<AppState>>,
    dev: Res<crate::dev::DevArgs>,
    mut q: ResMut<GraphicsQuality>,
    mut ui: ResMut<UiState>,
) {
    // dev screenshots render their first frames slowly while pipelines compile: never degrade (or toast) there
    if *state.get() != AppState::InGame || dev.screenshot.is_some() {
        return;
    }
    if let Some(t) = q.watch(time.delta_secs()) {
        ui.toast(t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_and_watchdog() {
        let mut q = GraphicsQuality::default();
        assert_eq!(q.cycle_lights(), "Luci leggere: senza sfocatura, bagliore e raggi");
        assert_eq!(q.cycle_lights(), "Luci spente");
        assert_eq!(q.cycle_lights(), "Luci complete");
        assert!(!q.lights_auto);
        let mut q = GraphicsQuality::default();
        let mut msgs = Vec::new();
        for _ in 0..400 {
            if let Some(m) = q.watch(0.05) {
                msgs.push(m);
            }
        }
        // 3 s of slow frames per step: lights 2 → 1 → 0, then the resolution
        assert_eq!(q.lights, 0);
        assert!(q.res_index >= 1);
        assert_eq!(msgs[0], "Il gioco rallentava: luci alleggerite (F2 per cambiarle)");
        assert_eq!(msgs[1], "Il gioco rallentava: luci spente (F2 per riaccenderle)");
        // ignored hitches
        let mut q = GraphicsQuality::default();
        for _ in 0..100 {
            q.watch(0.5);
        }
        assert_eq!(q.lights, 2);
    }
}
