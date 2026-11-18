//! `ts_client` — Bevy client of Tiny Swords — La Guerra dei Cinque Regni (wasm32 in the browser, native for dev).
//!
//! SHARED FILE (foundation): owners add only their `mod` line / plugin with minimal, surgical edits.
//!
//! Module ownership (docs/IMPLEMENTATION-PLAN.md):
//! * C1 terrain: `render::{terrain, water, camera, sky}`
//! * C2 sprites: `render::{sprites, light, fx}`, `assets`
//! * C3 ui: `ui`
//! * C4 control: `control`, `link`, `audio`, `dev` (native CLI)
//! * foundation: `lib`, `main`, `mirror`, `render::{mod, consts}`
//!
//! Data flow: `link` talks `ts_sim::proto` to a `Room` (local for solo, remote over WebSocket), its pump applies
//! `ServerMsg`s to `mirror::Mirror` / `link::LobbyState` and re-emits presentation events as Bevy messages
//! (`link::GameEventMsg`); renderers read `Mirror`; UI/control write `link::NetOut` / `link::LinkRequest`.

use bevy::{asset::AssetMetaCheck, prelude::*, window::WindowResolution};

pub mod assets;
pub mod audio;
pub mod control;
pub mod dev;
pub mod link;
pub mod mirror;
pub mod render;
pub mod ui;

/// Top-level client state.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// sprites + font loading screen (`Caricamento sprite n/m`)
    #[default]
    Loading,
    /// main menu
    Menu,
    /// multiplayer lobby (remote link)
    Lobby,
    /// a match is running (solo or network); entered on `ServerMsg::Init`
    InGame,
}

/// Window title.
pub const TITLE: &str = "Tiny Swords — La Guerra dei Cinque Regni";
/// Background / water colour `#47aba9`.
pub const WATER_COLOR: Color = Color::srgb(
    0x47 as f32 / 255.0,
    0xab as f32 / 255.0,
    0xa9 as f32 / 255.0,
);

/// Asset root. Native: the repo's `assets/` (absolute, so `cargo run` works from any cwd). Web: `assets`,
/// i.e. `GET /assets/...` relative to the page, served by `ts-server`.
fn asset_root() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").to_string()
    }
    #[cfg(target_arch = "wasm32")]
    {
        "assets".to_string()
    }
}

/// Builds and runs the app (returns immediately on the web).
pub fn run() {
    let dev = dev::DevArgs::from_env();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: TITLE.into(),
                    canvas: Some("#game".into()),
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: true,
                    resolution: WindowResolution::new(1280, 720),
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_root(),
                meta_check: AssetMetaCheck::Never,
                ..default()
            })
            .set(ImagePlugin::default_nearest()),
    )
    .insert_resource(ClearColor(WATER_COLOR))
    .insert_resource(dev)
    .init_state::<AppState>()
    .add_plugins((
        assets::AssetsPlugin,
        link::LinkPlugin,
        mirror::MirrorPlugin,
        render::RenderPlugin,
        ui::UiPlugin,
        control::ControlPlugin,
        audio::AudioPlugin,
        dev::DevPlugin,
    ));
    app.run();
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    run();
}
