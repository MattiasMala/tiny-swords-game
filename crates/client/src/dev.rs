// OWNER: C4 (control) — native development CLI (not in the specs; tooling for implementers).
//
//   cargo run -p ts-client -- --solo --seed 7 --screenshot shot.png [--frames 120] [--cam X,Y] [--zoom Z] [--time T]
//
// * `--solo`          skip the menu: start a solo match as soon as loading ends
// * `--seed N`        forced match seed (solo room `Room::seed`)
// * `--screenshot P`  after `--frames N` frames (default 120) of the match (or of the menu without `--solo`) save the
//                     primary window to P (PNG) and exit
// * `--cam X,Y`       centre the camera on tile (X, Y) instead of the own castle
// * `--zoom Z`        camera zoom factor (1 = default distance, 2 = twice as close)
// * `--time T`        day time 0..1 (0 = dawn), read by `render::light` (overrides the sim day phase)
// * `--place KIND`    (C4 input testing) start placing building KIND (`house`, `wall`, `tower` …) once the match runs
// * `--select WHAT`   (C4 input testing) select `pawns` / `army` / `castle` once the match runs
//
// On wasm everything stays at its default (the browser has no argv).

use crate::AppState;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

/// Parsed CLI options (resource).
#[derive(Resource, Clone, Debug, Default)]
pub struct DevArgs {
    pub solo: bool,
    pub seed: Option<u32>,
    pub screenshot: Option<String>,
    pub frames: u32,
    /// tile coordinates
    pub cam: Option<Vec2>,
    pub zoom: Option<f32>,
    pub time: Option<f32>,
    /// building key to start placing (input testing)
    pub place: Option<String>,
    /// `pawns` / `army` / `castle`: initial selection (input testing)
    pub select: Option<String>,
}

impl DevArgs {
    /// Reads `std::env::args()` (native only).
    pub fn from_env() -> DevArgs {
        #[cfg(not(target_arch = "wasm32"))]
        {
            DevArgs::parse(std::env::args().skip(1))
        }
        #[cfg(target_arch = "wasm32")]
        {
            DevArgs {
                frames: 120,
                ..default()
            }
        }
    }

    pub fn parse(args: impl IntoIterator<Item = String>) -> DevArgs {
        let mut d = DevArgs {
            frames: 120,
            ..default()
        };
        let mut it = args.into_iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--solo" => d.solo = true,
                "--seed" => d.seed = it.next().and_then(|v| v.parse().ok()),
                "--screenshot" => d.screenshot = it.next(),
                "--frames" => d.frames = it.next().and_then(|v| v.parse().ok()).unwrap_or(120),
                "--cam" => {
                    d.cam = it.next().and_then(|v| {
                        let (x, y) = v.split_once(',')?;
                        Some(Vec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
                    })
                }
                "--zoom" => d.zoom = it.next().and_then(|v| v.parse().ok()),
                "--time" => d.time = it.next().and_then(|v| v.parse().ok()),
                "--place" => d.place = it.next(),
                "--select" => d.select = it.next(),
                other => warn!("argomento sconosciuto: {other}"),
            }
        }
        d
    }
}

pub struct DevPlugin;

impl Plugin for DevPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, screenshot_countdown);
    }
}

/// Counts frames of the target state, then captures the window once and exits after the PNG is written.
fn screenshot_countdown(
    mut commands: Commands,
    dev: Res<DevArgs>,
    state: Res<State<AppState>>,
    mut frames: Local<u32>,
    mut done: Local<bool>,
) {
    let Some(path) = dev.screenshot.clone() else {
        return;
    };
    if *done {
        return;
    }
    let target = if dev.solo {
        AppState::InGame
    } else {
        AppState::Menu
    };
    if *state.get() != target {
        return;
    }
    *frames += 1;
    if *frames < dev.frames.max(1) {
        return;
    }
    *done = true;
    info!("screenshot -> {path}");
    let mut save = save_to_disk(path);
    commands.spawn(Screenshot::primary_window()).observe(
        move |ev: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            save(ev);
            exit.write(AppExit::Success);
        },
    );
}
