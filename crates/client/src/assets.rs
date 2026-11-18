// OWNER: C2 (sprites) — spec 01-defs-assets.md (folder rules, sheets), 09-asset-inventory.md §1.2 (assets.json format),
// 05-control-loop.md §2 / 06-ui.md §11.5 (loading screen: `Caricamento sprite ${loaded}/${total}`).
//
// DECISIONS (foundation):
// * Manifest = `docs/spec/assets.json` embedded with `include_str!` (588 PNGs with frame size / anchors / fps).
//   Paths in it start with `assets/`; handles are keyed by the path WITHOUT that prefix (the asset root).
// * Asset root: native = `<repo>/assets` (absolute, set in lib.rs from CARGO_MANIFEST_DIR); web = `assets` relative
//   to the page → `GET /assets/...`, served by ts-server from the repo's `assets/`.
// * Font: `fonts/Fredoka.ttf` lives outside `assets/`, so it is EMBEDDED in the binary (`include_bytes!`, 159 kB) and
//   added with `Font::from_bytes` at startup: identical on native and web, no extra HTTP path, and the loading
//   screen itself can use it. (ts-server still serves `/fonts/` for completeness.)
// * For now every manifest PNG is loaded (as the JS did). C2 may restrict `wanted()` to what the 3D renderer
//   really needs (e.g. skip UI-only pieces drawn differently) — keep `GameAssets` lookups by path.

use crate::AppState;
use crate::dev::DevArgs;
use crate::link::{LinkRequest, LobbyState};
use bevy::asset::LoadState;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use serde::Deserialize;

/// Embedded manifest (docs/spec/assets.json).
pub const MANIFEST_JSON: &str = include_str!("../../../docs/spec/assets.json");
/// Embedded UI font (fonts/Fredoka.ttf, weight 600 is synthesised by the renderer as needed).
pub const FONT_BYTES: &[u8] = include_bytes!("../../../fonts/Fredoka.ttf");

/// One `assets.json` row.
#[derive(Deserialize, Clone, Debug)]
pub struct AssetEntry {
    /// repo-relative path (`assets/...`)
    pub path: String,
    pub w: u32,
    pub h: u32,
    pub fw: u32,
    pub fh: u32,
    pub frames: u32,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub ax: Option<f32>,
    #[serde(default)]
    pub ay: Option<f32>,
    #[serde(default)]
    pub fps: Option<f32>,
}

impl AssetEntry {
    /// Path relative to the asset root (no `assets/` prefix).
    pub fn rel(&self) -> &str {
        self.path.strip_prefix("assets/").unwrap_or(&self.path)
    }
}

/// Parsed manifest + loaded handles + the UI font.
#[derive(Resource, Default)]
pub struct GameAssets {
    pub manifest: Vec<AssetEntry>,
    /// rel path → index in `manifest` / `images`
    pub by_path: HashMap<String, usize>,
    /// one handle per manifest row (`Handle::default()` when not wanted)
    pub images: Vec<Handle<Image>>,
    pub font: Handle<Font>,
    pub loaded: usize,
    pub total: usize,
    pub failed: usize,
}

impl GameAssets {
    /// Image by path relative to `assets/` (e.g. `Terrain/Tileset/Tilemap_color1.png`).
    pub fn image(&self, rel: &str) -> Option<Handle<Image>> {
        self.by_path.get(rel).map(|&i| self.images[i].clone())
    }

    pub fn entry(&self, rel: &str) -> Option<&AssetEntry> {
        self.by_path.get(rel).map(|&i| &self.manifest[i])
    }
}

/// Which manifest rows are loaded at start-up.
fn wanted(_e: &AssetEntry) -> bool {
    true
}

pub fn parse_manifest() -> Vec<AssetEntry> {
    match serde_json::from_str(MANIFEST_JSON) {
        Ok(v) => v,
        Err(e) => {
            error!("assets.json: {e}");
            Vec::new()
        }
    }
}

pub struct AssetsPlugin;

impl Plugin for AssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameAssets>()
            .add_systems(Startup, start_loading)
            .add_systems(OnEnter(AppState::Loading), spawn_loading_screen)
            .add_systems(Update, track_loading.run_if(in_state(AppState::Loading)));
    }
}

fn start_loading(
    server: Res<AssetServer>,
    mut fonts: ResMut<Assets<Font>>,
    mut ga: ResMut<GameAssets>,
) {
    ga.font = fonts.add(Font::from_bytes(FONT_BYTES.to_vec()));
    let manifest = parse_manifest();
    let mut images = Vec::with_capacity(manifest.len());
    let mut by_path = HashMap::default();
    let mut total = 0;
    for (i, e) in manifest.iter().enumerate() {
        by_path.insert(e.rel().to_string(), i);
        if wanted(e) {
            images.push(server.load::<Image>(e.rel().to_string()));
            total += 1;
        } else {
            images.push(Handle::default());
        }
    }
    ga.manifest = manifest;
    ga.by_path = by_path;
    ga.images = images;
    ga.total = total;
}

#[derive(Component)]
struct LoadingText;

#[derive(Component)]
struct LoadingBar;

fn spawn_loading_screen(mut commands: Commands, ga: Res<GameAssets>) {
    let font = ga.font.clone();
    commands.spawn((
        DespawnOnExit(AppState::Loading),
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: px(16),
            ..default()
        },
        children![
            (
                Text::new("Tiny Swords"),
                TextFont {
                    font: font.clone().into(),
                    font_size: FontSize::Px(48.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ),
            (
                Node {
                    width: px(420),
                    max_width: percent(60),
                    height: px(16),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(0x1b, 0x1f, 0x33)),
                children![(
                    LoadingBar,
                    Node {
                        width: percent(0),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(0xe3, 0xc8, 0x4e)),
                )],
            ),
            (
                LoadingText,
                Text::new(""),
                TextFont {
                    font: font.into(),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ),
        ],
    ));
}

#[allow(clippy::too_many_arguments)]
fn track_loading(
    server: Res<AssetServer>,
    mut ga: ResMut<GameAssets>,
    mut text: Query<&mut Text, With<LoadingText>>,
    mut bar: Query<&mut Node, With<LoadingBar>>,
    dev: Res<DevArgs>,
    lobby: Res<LobbyState>,
    mut req: MessageWriter<LinkRequest>,
    mut next: ResMut<NextState<AppState>>,
) {
    let mut loaded = 0;
    let mut failed = 0;
    for h in ga
        .images
        .iter()
        .filter(|h| *h != &Handle::<Image>::default())
    {
        match server.load_state(h.id()) {
            LoadState::Loaded => loaded += 1,
            LoadState::Failed(_) => failed += 1,
            _ => {}
        }
    }
    if failed > ga.failed {
        warn!("{} immagini non caricate", failed);
    }
    ga.loaded = loaded;
    ga.failed = failed;
    let done = loaded + failed;
    for mut t in &mut text {
        t.0 = format!("Caricamento sprite {}/{}", done, ga.total);
    }
    for mut n in &mut bar {
        n.width = percent(100.0 * done as f32 / ga.total.max(1) as f32);
    }
    if done >= ga.total {
        info!("asset caricati: {loaded} (falliti {failed})");
        next.set(AppState::Menu);
        if dev.solo {
            req.write(LinkRequest::StartSolo {
                color: lobby.color,
                level: lobby.level,
            });
        }
    }
}
