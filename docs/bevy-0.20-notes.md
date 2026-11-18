# Bevy 0.20.1 — verified API digest for the HD-2D RTS client (WASM / WebGL2)

Source of truth: the Bevy 0.20.1 clone at `C:/Users/User/.bevy-ref/bevy-0.20.1`. Every path below
(`crates/...`, `examples/...`, `_release-content/...`) is relative to that clone. Line numbers are from
that checkout.

**How this was verified.** Every snippet in sections 2–18 was put into one scratch crate
(`bevy = 0.20.1`, the feature list in §1.3 option A plus picking/widgets) and passed `cargo check` on
rustc 1.100.0, native target, with no errors. The WESL shaders in §12 follow the official example
shaders. The shader compiler only runs them when the game runs, so `cargo check` does not test them.
The `wasm32-unknown-unknown` result is in §1.5.

Your memory of Bevy is probably 0.14–0.16. Read §19, the list of renames and traps, before you write
code.

---

## 0. Quick facts

| Item | Value | Where |
|---|---|---|
| bevy version | 0.20.1, edition 2024, `rust-version = "1.97.1"` | `Cargo.toml:3-12` |
| wgpu / wgpu-types | 30 (lock: wgpu 30.0.2) | `crates/bevy_render/Cargo.toml:90-97` |
| winit | 0.30 (lock: 0.30.13) | project `Cargo.lock` |
| Shader language | **WESL** (`.wesl`). naga_oil and GLSL are removed. Plain WGSL with no directives still loads. | `_release-content/migration-guides/shaders_use_wesl.md` |
| Default fixed timestep | 64 Hz (`Duration::from_micros(15625)`) | `crates/bevy_time/src/fixed.rs:76` |
| Max virtual delta | 250 ms | `crates/bevy_time/src/virt.rs:86` |
| WebGL2: directional lights / cascades | max **1** directional light, **1** cascade | `crates/bevy_pbr/src/render/light.rs:229-244` |
| WebGL2: clusterable lights (point+spot) | 204 (uniform-buffer clustering) | `crates/bevy_pbr/src/cluster/mod.rs:31` |
| WebGL2: DepthOfField, SSR, SSAO, TAA | **not available** (DoF and SSR turn themselves off; SSAO is unsupported; TAA needs a depth prepass) | `crates/bevy_core_pipeline/src/core_3d/mod.rs:7-31`, `crates/bevy_pbr/src/ssao/mod.rs:110`, `crates/bevy_anti_alias/src/taa/mod.rs:113` |

---

## 1. Cargo: features, versions, profiles, build

### 1.1 Cargo feature names that matter (from `Cargo.toml:135-760`)

**Profiles and collections:**

- `default = ["2d","3d","ui","audio"]`.
- `3d = ["default_app","default_platform","3d_bevy_render","scene","picking"]`.
- `ui = ["default_app","default_platform","ui_api","ui_bevy_render","scene","picking","bevy_ui_widgets"]`.
- `default_app = ["async_executor","bevy_asset","bevy_log","bevy_state","reflect_auto_register"]`.
- `default_platform = ["std","bevy_gilrs","bevy_winit","bevy_clipboard","default_font","multi_threaded","webgl2","x11","wayland","custom_cursor","sysinfo_plugin"]`.
- `common_api = ["bevy_animation","bevy_camera","bevy_color","bevy_curve","bevy_gizmos","bevy_image","bevy_mesh","bevy_shader","bevy_material","bevy_text","bevy_window","hdr","png"]`.
  Here `hdr` is the .hdr **image format**, not HDR rendering.
- `3d_api = ["common_api","bevy_light","bevy_mikktspace","ktx2","morph_animation","morph","smaa_luts","tonemapping_luts","zstd_rust"]`.
- `3d_bevy_render = ["3d_api","bevy_render","bevy_core_pipeline","bevy_gizmos_render","bevy_anti_alias","bevy_gltf","bevy_pbr","bevy_post_process","gltf_animation"]`.
- `ui_api = ["default_app","common_api","bevy_input_focus","bevy_ui"]`.
- `ui_bevy_render = ["ui_api","bevy_render","bevy_core_pipeline","bevy_ui_render"]`.
- `picking = ["bevy_picking","mesh_picking","sprite_picking","ui_picking"]`.
- `audio = ["bevy_audio","vorbis"]`.

**Single features:**

- Crates: `bevy_render`, `bevy_core_pipeline`, `bevy_pbr`, `bevy_post_process` (bloom, DoF, effect stack), `bevy_anti_alias` (FXAA/SMAA/TAA), `bevy_light`, `bevy_camera`, `bevy_mesh`, `bevy_image`, `bevy_shader`, `bevy_material`, `bevy_color`, `bevy_text`, `bevy_ui`, `bevy_ui_render`, `bevy_ui_widgets`, `bevy_input_focus`, `bevy_picking`, `ui_picking`, `mesh_picking`, `bevy_sprite`, `bevy_sprite_render`, `bevy_window`, `bevy_winit`, `bevy_asset`, `bevy_state`, `bevy_log`, `bevy_audio`, `bevy_gizmos`, `bevy_gizmos_render`.
- Platform: `std`, `async_executor`, `multi_threaded` (on wasm it changes nothing, because wasm stays single-threaded), `web` (enabled automatically on wasm32 by bevy_winit, bevy_render and bevy_asset, see `crates/bevy_winit/Cargo.toml:76-89`), `webgl2` (→ `bevy_internal/webgl`), `webgpu` (overrides webgl2).
- Images: `png`, `ktx2`, `zstd_rust` / `zstd_c`, `tonemapping_luts`, `smaa_luts`.
- Audio: `wav` (hound), `vorbis` (lewton), `mp3`, `flac`, plus the `symphonia-*` variants.
- Other: `default_font` (an **ASCII-only** Fira Mono subset, +20 kB), `custom_cursor`, `reflect_auto_register`.
- Development only: `bevy_ui_debug`, `dev`, `file_watcher`, `embedded_watcher`, `dynamic_linking` (not on wasm), `trace*`.

### 1.2 Feature traps

- **`tonemapping_luts` needs a zstd backend.** It enables `ktx2` and `bevy_image/zstd` (`crates/bevy_internal/Cargo.toml:80-84`). `bevy_image` throws `compile_error!` when neither `zstd_rust` nor `zstd_c` is on (`crates/bevy_image/src/lib.rs:14-17`). `3d_api` brings `zstd_rust` with it. A hand-trimmed list must add **`zstd_rust`** itself.
- The `TonyMcMapface` (the default), `AgX` and `BlenderFilmic` tonemappers need `tonemapping_luts`. Without it the screen turns pink. `Reinhard`, `ReinhardLuminance`, `AcesFitted`, `SomewhatBoringDisplayTransform` and `Linear` need no LUT (`crates/bevy_core_pipeline/src/tonemapping/mod.rs:218-250`).
- The **new** headless `Button` and the `Hovered`/`Pressed` components (§15.4) need `bevy_picking`, `ui_picking` and `bevy_ui_widgets`. **The client manifest currently in the repo has none of the three.** `3d_bevy_render` does **not** include picking.
- `MeshPickingPlugin` / `MeshRayCast` need `mesh_picking`. They are optional, because cursor→ground can be done with plain math (§14).
- Italian text with accented letters (`è à ù ò ì`) needs a real `.ttf`. `default_font` is ASCII only (`Cargo.toml` comment on `default_font`).

### 1.3 Recommended feature lists

**Option A: what the repo has now (`crates/client/Cargo.toml`) plus picking and widgets.** This exact list
passed `cargo check`:

```toml
bevy = { version = "0.20.1", default-features = false, features = [
  "default_app", "std", "bevy_winit", "webgl2", "multi_threaded", "custom_cursor", "default_font",
  "3d_bevy_render", "ui_api", "ui_bevy_render", "bevy_audio", "wav", "png", "tonemapping_luts",
  "bevy_picking", "ui_picking", "mesh_picking", "bevy_ui_widgets",
] }
```

**Option B: trimmed for a smaller wasm.** It drops gltf, animation, morph, mikktspace, SMAA LUTs,
scene and gilrs. All of them come in through `3d_bevy_render`/`3d_api`, and the game does not use
them. This list (with `mesh_picking` added) passed `cargo check --target wasm32-unknown-unknown` with
every snippet in this file.

```toml
bevy = { version = "0.20.1", default-features = false, features = [
  # core / platform
  "std", "async_executor", "bevy_asset", "bevy_log", "bevy_state",
  "bevy_window", "bevy_winit", "webgl2", "custom_cursor",
  # 3D rendering
  "bevy_render", "bevy_core_pipeline", "bevy_pbr", "bevy_light", "bevy_camera", "bevy_mesh",
  "bevy_image", "bevy_shader", "bevy_material", "bevy_color", "png",
  "bevy_post_process",            # Bloom (works on WebGL2); DoF is ignored on WebGL2
  "bevy_anti_alias",              # FXAA/SMAA (optional; MSAA needs nothing)
  "tonemapping_luts", "ktx2", "zstd_rust",
  # UI + text + widgets + picking
  "bevy_text", "bevy_ui", "bevy_ui_render", "bevy_input_focus", "bevy_ui_widgets",
  "bevy_picking", "ui_picking",   # add "mesh_picking" only if MeshRayCast is used
  # audio
  "bevy_audio", "wav",            # or "vorbis" for .ogg
  # debug (can be cfg'd out for release)
  "bevy_gizmos", "bevy_gizmos_render",
] }
```

Keep `default_font` only if some text is ASCII-only and you load no font. Keep `multi_threaded` for
native builds, where it helps; on wasm it changes nothing.

### 1.4 wasm-bindgen and related versions (bevy 0.20.1 and the project lock)

- Every Bevy crate asks only for `wasm-bindgen = "0.2"`, with no patch pin: `crates/bevy_app/Cargo.toml:104`, `crates/bevy_asset/Cargo.toml:80`, `crates/bevy_render/Cargo.toml:144`, `crates/bevy_winit/Cargo.toml:77`, `crates/bevy_clipboard/Cargo.toml:37`, `crates/bevy_platform/Cargo.toml:91`.
- Other floors Bevy sets:
  - `js-sys = "0.3.83"` in `crates/bevy_render/Cargo.toml:134`. Other crates ask for `"0.3"`.
  - `web-sys = "0.3.67"` in `crates/bevy_render/Cargo.toml:135`. Other crates ask for `"0.3"`.
  - `wasm-bindgen-futures = "0.4.46"` in `crates/bevy_platform/Cargo.toml:88`. bevy_asset and bevy_clipboard ask for `"0.4"`.
- The **effective** floor comes from wgpu 30.0.2 and wgpu-hal 30.0.2: `wasm-bindgen >= 0.2.127`, `js-sys/web-sys >= 0.3.104`. winit 0.30.13 needs `wasm-bindgen >= 0.2.93`.
- **What the project `Cargo.lock` has now:** `wasm-bindgen 0.2.130`, `js-sys 0.3.107`, `web-sys 0.3.107`, `wasm-bindgen-futures 0.4.80`, `wgpu 30.0.2`, `winit 0.30.13`.
- **wasm-bindgen-cli must be exactly 0.2.130:** `cargo install wasm-bindgen-cli --version 0.2.130 --locked`. Neither the CLI nor `wasm-opt` is installed on this machine yet.
- getrandom: the lock has 0.3.4 (through `rand_core 0.9` ← `rand 0.9` ← `tungstenite`, server only) and 0.4.3. Neither gets the `wasm_js` feature from Bevy. With both feature lists in §1.3, the wasm32 `cargo check` passed with no getrandom error. If a crate added later pulls getrandom into the **client** wasm build, add `getrandom = { version = "0.4", features = ["wasm_js"] }` (and/or `"0.3"`) under `[target.'cfg(target_arch = "wasm32")'.dependencies]`. getrandom 0.3.4's README says that since this version the RUSTFLAGS `--cfg getrandom_backend="wasm_js"` is no longer needed. The sim uses its own PCG32, so this only matters if something else pulls `rand`.

### 1.5 wasm build check

`cargo check --target wasm32-unknown-unknown` (rustc 1.100.0) of the scratch crate passed with **both**
feature lists A and B. B was checked with `mesh_picking` added. It was run with every snippet from this
file. A full wasm **link**, `wasm-bindgen` and a run in a browser were **not** done here.

### 1.6 Release profile and build pipeline

Bevy's own wasm profile (`Cargo.toml:5314-5318`):

```toml
[profile.wasm-release]
inherits = "release"
opt-level = "z"     # Bevy uses "z" (size). For max speed use 3 or "s"; measure both.
lto = "fat"
codegen-units = 1
```

Recommendation for this game. Speed matters, but so does download size:

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
strip = true

[profile.wasm-release]
inherits = "release"
opt-level = "s"     # good size/speed balance on wasm; try 3 if the frame time is CPU-bound
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
debug = false

[profile.dev.package."*"]
opt-level = 3       # bevy itself optimized in dev
```

Pipeline (`examples/README.md:839-933`):

```sh
cargo build --profile wasm-release --target wasm32-unknown-unknown -p ts-client
wasm-bindgen --target web --no-typescript --out-dir web/pkg --out-name ts_client \
  target/wasm32-unknown-unknown/wasm-release/ts_client.wasm
wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int -o web/pkg/ts_client_bg.wasm web/pkg/ts_client_bg.wasm   # optional, from binaryen
```

Bevy's table (July 2022): "z"+fat+cgu1 gives 4.8 MB after wasm-opt, against 13 MB for the default
build. Serve the `.wasm` with `Content-Type: application/wasm` and compress it (brotli or gzip).

Loading on the page (`examples/wasm/index.html`):

```html
<canvas id="game"></canvas>
<script type="module">
  import init from './pkg/ts_client.js';
  init();   // runs the #[wasm_bindgen(start)] fn
</script>
```

On web, `App::run()` returns right away. winit uses `EventLoop::spawn_app`
(`crates/bevy_winit/src/state.rs:1041-1049`). Do not write code after `run()` that expects the game to
have ended.

Browser audio cannot start until the user interacts with the page (`examples/README.md`, "Audio in the
browsers").

---

## 2. App and plugin setup

`DefaultPlugins` is in `crates/bevy_internal/src/default_plugins.rs:3-104`. The plugin order, each
behind its feature:

- Core: PanicHandler, Log, TaskPool, FrameCount, Time, Transform, Diagnostics, Input, InputFocus+InputDispatch, Window, Accessibility.
- Assets and window: (WebAsset), Asset, WorldSerialization, Scene, Winit.
- Rendering: Render, Image, Mesh, Camera, Light, (PipelinedRendering, not on wasm), CorePipeline, PostProcess, AntiAlias, Sprite, SpriteRender, Clipboard, Text, Ui, UiRender, Gltf, Pbr.
- Other: Audio, Gilrs, Animation, Gizmo, GizmoRender, States, UiWidgetsPlugins (group), DefaultPickingPlugins (group).

`FrameTimeDiagnosticsPlugin` and `MeshPickingPlugin` are **not** in `DefaultPlugins`.

```rust
use bevy::{
    asset::AssetMetaCheck,
    log::LogPlugin,
    prelude::*,
    window::{CursorOptions, WindowResolution},
    winit::WinitSettings,
};

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Tiny Swords".into(),
                        canvas: Some("#game".into()),          // CSS selector; web only
                        fit_canvas_to_parent: true,            // parent must NOT size itself from children
                        prevent_default_event_handling: true,  // default true: blocks F5/Ctrl+R/Tab while focused
                        resolution: WindowResolution::new(1280, 720), // PHYSICAL px (ignored by fit_canvas_to_parent)
                        ..default()
                    }),
                    primary_cursor_options: Some(CursorOptions::default()),
                    ..default()
                })
                .set(AssetPlugin {
                    meta_check: AssetMetaCheck::Never,  // web: avoid 404 requests for *.meta files
                    ..default()                          // file_path default "assets"
                })
                .set(ImagePlugin::default_nearest())      // pixel-art default sampler
                .set(LogPlugin {
                    filter: "wgpu=error,naga=warn,ts_client=debug".into(),
                    level: bevy::log::Level::INFO,
                    ..default()
                }),
        )
        .insert_resource(WinitSettings::game())   // Continuous update while focused
        .insert_resource(ClearColor(Color::srgb(0.1, 0.12, 0.15)))
        .run();
}
```

Facts behind this setup:

- **Window** (`crates/bevy_window/src/window.rs:164`): `canvas: Option<String>` (:251), `fit_canvas_to_parent: bool` (:259, default `false`), `prevent_default_event_handling: bool` (:266, default **true**, :501). `Window` has `#[require(CursorOptions)]`. Cursor visibility and grab moved to the separate **`CursorOptions`** component (:752: `visible`, `grab_mode`, `hit_test`). It is set through `WindowPlugin::primary_cursor_options` (`crates/bevy_window/src/lib.rs:81`).
- `WindowResolution::new(physical_width: u32, physical_height: u32)`. The arguments are **physical u32** pixels (`window.rs:923`). Logical size is read with `window.width()` / `window.height()`. `window.scale_factor()` gives the device pixel ratio.
- **AssetPlugin** (`crates/bevy_asset/src/lib.rs:235`): fields `file_path` (default `"assets"`), `processed_file_path`, `watch_for_changes_override`, `use_asset_processor_override`, `mode: AssetMode`, `meta_check: AssetMetaCheck`, `unapproved_path_mode`. `AssetMetaCheck` (:321) has the variants `Always` (default), `Paths(HashSet<AssetPath>)` and `Never`.
- On wasm, `HttpWasmAssetReader` fetches `<file_path>/<asset path>` relative to the page URL (`crates/bevy_asset/src/io/wasm.rs:39-124`). With the default settings, `asset_server.load("x/y.png")` turns into `GET assets/x/y.png`.
- **LogPlugin** (`crates/bevy_log/src/lib.rs:218`): `filter: String`, `level: Level`, `custom_layer`, `fmt_layer`. `DEFAULT_FILTER` (:278) is `"wgpu=error,naga=warn,..."`. On wasm, logs go to the browser console through `tracing-wasm` (:389-392).
- **ImagePlugin** (`crates/bevy_image/src/image.rs:163`): `default_sampler: ImageSamplerDescriptor`, with the constructors `ImagePlugin::default_nearest()` (:209) and `default_linear()` (:204).
- **WinitSettings** (`crates/bevy_winit/src/winit_config.rs:6`): `focused_mode`, `unfocused_mode: UpdateMode`, with `WinitSettings::game()` and `desktop_app()`. `UpdateMode` is `Continuous` or `Reactive{wait, react_to_*}`.
- Diagnostics: `.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())`, see §17.
- `App::set_error_handler(handler)` (`crates/bevy_app/src/app.rs:1493`). Systems may return `Result` (`fn sys(...) -> Result { ...; Ok(()) }`). In 0.20, panics inside systems also go to the fallback handler, which re-panics by default (`_release-content/release-notes/panics-to-errors.md`).

---

## 3. ECS essentials in 0.20

- The prelude (`crates/bevy_ecs/src/lib.rs:68-104`) exports:
  - Components and entities: `Component`, `Bundle`, `Entity`, `Name`, `ChildOf`, `Children`, `children!`, `related!`.
  - Queries: `Query`, `Single`, `Populated`, `Has`, `With`, `Without`, `Or`, `Added`, `Changed`, `Ref`, `Mut`, `RemovedComponents`.
  - System params: `Res`, `ResMut`, `Local`, `Commands`, `EntityCommands`, `ParamSet`, `In`, `If`.
  - Messages: `Message`, `MessageReader`, `MessageWriter`, `Messages`.
  - Events and observers: `Event`, `EntityEvent`, `EventPattern`, `On`, `Observer`.
  - Lifecycle events: `Add`, `Insert`, `Discard` (the old `Replace`), `Remove`, `Despawn`.
  - Scheduling: `SystemSet`, `IntoScheduleConfigs`, `ApplyDeferred`, and all common run conditions.
  - Templates: `template`, `FromTemplate`, `Template` (BSN).
- `Query::single()` / `single_mut()` return `Result`. A `Single<D, F>` parameter **skips the system** unless exactly one entity matches. Use `Option<Single<..>>` to accept zero.
- **Required components:** spawning `Camera3d` also inserts `Camera` and `Projection`. `Camera` then inserts `Frustum`, `CameraMainTextureUsages`, `VisibleEntities`, `Transform`, `Visibility` and `RenderTarget` (`crates/bevy_camera/src/components.rs:25`, `crates/bevy_camera/src/camera.rs:376-385`). `Mesh3d` requires `Transform` (`crates/bevy_mesh/src/components.rs:103`). `DirectionalLight` and `PointLight` require `Transform` and `Visibility`. `Node` requires `BackgroundColor`, `BorderColor`, `FocusPolicy`, `ZIndex`, `Visibility`, `UiTransform`, `EmSize` and more (`crates/bevy_ui/src/ui_node.rs:463-485`). There are **no bundles**: spawn tuples of components.
- `commands.entity(e).despawn()` is recursive: it removes the `Children` too. Hierarchies: `children![a, b]` inside a spawn tuple, `.with_children(|p| { p.spawn(..); })`, `.add_children(&[..])`, `ChildOf(parent)`.
- `Assets::get_mut(id)` returns `Option<AssetMut<'_, A>>`, a guard that derefs to `&mut A` and marks the asset changed (`crates/bevy_asset/src/assets.rs:440,628`). Use `get_mut_untracked` to skip change detection.

---

## 4. Schedules, system sets, run conditions

Main schedules (`crates/bevy_app/src/main_schedule.rs:57-196`):

- Startup: `PreStartup`, `Startup`, `PostStartup`.
- Every frame: `First`, `PreUpdate`, `RunFixedMainLoop`, then the fixed loop (`FixedFirst`, `FixedPreUpdate`, **`FixedUpdate`**, `FixedPostUpdate`, `FixedLast`), then `Update`, `SpawnScene`, `PostUpdate`, `Last`.
- State transitions run in the `StateTransition` schedule (after `PreUpdate`).

```rust
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
enum GameSets { Input, Render }

app.configure_sets(Update, (GameSets::Input, GameSets::Render).chain())
   .add_systems(Update, (
        (camera_controls, cursor_pick, read_ticks).chain().in_set(GameSets::Input),
        interpolate.in_set(GameSets::Render),
   ).run_if(in_state(AppState::InGame)));
```

`RunFixedMainLoopSystems::{BeforeFixedMainLoop, FixedMainLoop, AfterFixedMainLoop}` are the sets for
ordering around the fixed loop (`main_schedule.rs:414`).

---

## 5. States

`crates/bevy_state/src/*`. `StatesPlugin` is part of `DefaultPlugins`.

```rust
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
enum AppState { #[default] Loading, Menu, InGame }

#[derive(SubStates, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[source(AppState = AppState::InGame)]
enum InGamePhase { #[default] Playing, Paused }

app.init_state::<AppState>()            // or .insert_state(AppState::Menu)
   .add_sub_state::<InGamePhase>()
   .add_systems(OnEnter(AppState::Menu), spawn_menu)
   .add_systems(OnExit(AppState::Menu), || info!("leaving menu"))
   .add_systems(FixedUpdate, sim_tick.run_if(in_state(AppState::InGame)));

fn go_to_game(mut next: ResMut<NextState<AppState>>) { next.set(AppState::InGame); }
fn go_via_commands(mut commands: Commands) { commands.set_state(AppState::Menu); }

// Entity cleanup tied to state (always enabled; no #[states(scoped_entities)] needed):
commands.spawn((DespawnOnExit(AppState::Menu), Node::default()));
```

- `NextState<S>` (`crates/bevy_state/src/state/resources.rs:181`) is an enum with `Unchanged`, `Pending(S)` and `PendingIfDifferent(S)`. Methods: `set(s)` (:198), which always runs the exit/enter schedules even when the state is the same; `set_if_different(s)`, the renamed `set_if_neq`, see `_release-content/migration-guides/next_state_set_if_neq_rename.md`; `reset()`.
- The state is read with `Res<State<S>>` and `state.get()`.
- `OnEnter(S)` and `OnExit(S)` are in `crates/bevy_state/src/state/transitions.rs:19,25`. `OnTransition{exited, entered}` is there too.
- `in_state(s)` is in `crates/bevy_state/src/condition.rs:103`, `state_changed::<S>` at :165.
- `DespawnOnExit<S>` / `DespawnOnEnter<S>` are in `crates/bevy_state/src/state_scoped.rs:145,220`. `init_state` turns them on itself (`crates/bevy_state/src/app.rs:112,244`). `DisableOnExit` and `EnableOnEnter` also exist.

---

## 6. FixedUpdate and `Time<Fixed>`

```rust
app.insert_resource(Time::<Fixed>::from_hz(20.0));   // sim TICK_DT = 0.05 s
fn sim_tick(time: Res<Time<Fixed>>) { let dt = time.delta_secs(); /* == 0.05 */ }
fn interpolate(fixed: Res<Time<Fixed>>) { let alpha: f32 = fixed.overstep_fraction(); }
```

- `Time::<Fixed>` (`crates/bevy_time/src/fixed.rs`) has `from_hz(f64)` (:105), `from_seconds(f64)` (:94), `from_duration` (:83), `timestep()`, `set_timestep*`, `overstep()`, `overstep_fraction()` (:205), `discard_overstep()` and `accumulate_overstep()`.
- Inside `FixedUpdate`, plain `Res<Time>` is the fixed clock. In `Update`, `Res<Time>` is the virtual clock. `Time<Virtual>` has `pause()`, `set_relative_speed()` and `set_max_delta()` (`crates/bevy_time/src/virt.rs:192-231`).
- The fixed loop catches up at most `max_delta` (250 ms) of virtual time per frame. A backgrounded browser tab therefore does **not** fast-forward the sim. For network play the authoritative server owns the ticks. The client should feed `ts_sim` from server snapshots, not from `FixedUpdate`, or should drive the local `Room` with its own accumulator.

---

## 7. Messages vs Events/Observers (what is current)

The old buffered `Event`/`EventReader`/`EventWriter` is now called **Message**. The name **Event** now
means observer-triggered events.

**Messages** (buffered, pull-based, double-buffered every frame):

```rust
#[derive(Message, Debug, Clone, Copy)]
struct SimTicked(u64);

app.add_message::<SimTicked>();                         // crates/bevy_app/src/app.rs:427
fn writer(mut w: MessageWriter<SimTicked>) { w.write(SimTicked(1)); }      // message_writer.rs:74 (also write_batch, write_default)
fn reader(mut r: MessageReader<SimTicked>) { for m in r.read() { /* .. */ } } // message_reader.rs:44
```

- `Message` is a marker trait, `pub trait Message: Send + Sync + 'static {}` (`crates/bevy_ecs/src/message/mod.rs:100`).
- Engine input streams are Messages: `MouseWheel`, `KeyboardInput`, `CursorMoved`, `WindowResized` and the rest. `WindowPlugin` registers them with `app.add_message::<WindowResized>()` and similar calls.

**Events and Observers** (pushed, run immediately when triggered):

```rust
#[derive(Event, Debug)]
struct GameOver { winner: u8 }

#[derive(EntityEvent, Debug)]
struct UnitDied { entity: Entity }          // field named `entity` (or mark one with #[event_target])

app.add_observer(on_unit_died);             // global observer, app.rs:1459
fn on_unit_died(ev: On<UnitDied>, mut commands: Commands) {
    commands.entity(ev.entity).despawn();   // On<E> derefs to E
    commands.trigger(GameOver { winner: 0 });
}
// Lifecycle observers: bundle is now INSIDE the event type
fn on_add_unit(ev: On<Add<UnitMarker>>) { info!("added {:?}", ev.entity); }
// Entity-scoped observer:
commands.spawn(UnitMarker).observe(|ev: On<UnitDied>| { /* .. */ });
```

- The traits are in `crates/bevy_ecs/src/event/mod.rs`: `Event` (:89, it has `type Trigger`), `EventPattern` (:118) and `EntityEvent` (:369, `fn event_target(&self) -> Entity`).
- `On<'w,'t,E: EventPattern>` (`crates/bevy_ecs/src/observer/system_param.rs:25`) has `event()`, `event_mut()`, `observer()`, `original_event_target()`, `propagate(bool)` and `get_propagate()`.
- Propagating events use `#[entity_event(propagate)]`.
- **There is no `Trigger<E>` system parameter.** The old `Trigger<E, B>` became `On<E>`. As of 0.20 `On` has no second bundle parameter: write `On<Add<A>>`, not `On<Add, A>` (`_release-content/migration-guides/observer_event_matching.md`). `Trigger` is now an internal trait (`event/trigger.rs`).
- Which to use: messages for per-frame streams (sim output, network packets to systems), observers for UI callbacks and one-off reactions.

---

## 8. Camera, projection, HDR, post-processing

```rust
use bevy::{camera::Hdr, core_pipeline::tonemapping::Tonemapping,
           post_process::{bloom::Bloom, dof::DepthOfField}};

commands.spawn((
    Camera3d::default(),
    Projection::Perspective(PerspectiveProjection {
        fov: 30f32.to_radians(),   // vertical FOV in radians; default PI/4
        near: 0.5, far: 400.0,
        ..default()                // REQUIRED: there is a new field `near_clip_plane: Vec4`
    }),
    Transform::from_xyz(0.0, 30.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    Msaa::Off,                     // component on the camera; default Sample4
    Hdr,                           // marker component (Camera.hdr field is gone)
    Tonemapping::TonyMcMapface,    // default; needs `tonemapping_luts`
    Bloom::NATURAL,                // #[require(Hdr)]
    DistanceFog {
        color: Color::srgba(0.55, 0.65, 0.8, 1.0),
        directional_light_color: Color::srgba(1.0, 0.9, 0.7, 0.4),
        directional_light_exponent: 30.0,
        falloff: FogFalloff::Linear { start: 60.0, end: 160.0 },
    },
    AmbientLight { color: Color::WHITE, brightness: 200.0, ..default() }, // per-camera override
));
```

| Type | Path / import | Fields and defaults | WebGL2 |
|---|---|---|---|
| `Camera3d` | prelude; `crates/bevy_camera/src/components.rs:25` | `depth_load_op`, `depth_texture_usages`; requires `Camera`, `Projection` | ok |
| `Camera` | prelude; `crates/bevy_camera/src/camera.rs:385` | `viewport`, `order: isize`, `is_active`, `clear_color: ClearColorConfig`, `msaa_writeback`, `invert_culling`, `sub_camera_view`. **No `hdr` field.** | ok |
| `Hdr` | `bevy::camera::Hdr` (`components.rs:89`) | marker | ok (Rgba16Float target) |
| `Projection` | prelude; `projection.rs:217` | enum `Perspective(..)`, `Orthographic(..)`, `Custom(..)` | ok |
| `PerspectiveProjection` | prelude; `projection.rs:284` | `fov` (rad), `aspect_ratio` (auto), `near` 0.1, `far` 1000, `near_clip_plane` | ok |
| `Msaa` | prelude (`bevy::render::view::Msaa`, `crates/bevy_render/src/view/mod.rs:248`) | `Off`, `Sample2`, **`Sample4`** (default), `Sample8` | ok (2/8 depend on the GPU) |
| `Tonemapping` | `bevy::core_pipeline::tonemapping::Tonemapping`, re-exported from `bevy::render::view` (`view/mod.rs:283`) | `None`, `Linear`, `Reinhard`, `ReinhardLuminance`, `AcesFitted`, `AgX`, `SomewhatBoringDisplayTransform`, **`TonyMcMapface`** (default), `BlenderFilmic` | ok |
| `Bloom` | `bevy::post_process::bloom::Bloom` (`crates/bevy_post_process/src/bloom/settings.rs:33`) | `intensity` (NATURAL 0.15), `low_frequency_boost*`, `high_pass_frequency`, `prefilter`, `composite_mode`, `max_mip_dimension`; consts `NATURAL`, `ANAMORPHIC`, `OLD_SCHOOL`, `SCREEN_BLUR`; `Default = NATURAL` | **ok**, separate-texture fallback (`bloom/mod.rs:286`) |
| `DistanceFog` | prelude (bevy_pbr), `crates/bevy_pbr/src/fog.rs:56` | `color`, `directional_light_color`, `directional_light_exponent`, `falloff: FogFalloff`; default Linear 0..100, white | ok |
| `FogFalloff` | `fog.rs:103` | `Linear{start,end}`, `Exponential{density}`, `ExponentialSquared{density}`, `Atmospheric{extinction,inscattering}`; helpers `from_visibility(_squared/_contrast/_color...)` | ok |
| `DepthOfField` | `bevy::post_process::dof::DepthOfField` (`dof/mod.rs:78`) | `mode: DepthOfFieldMode{Gaussian(default), Bokeh}`, `focal_distance`, `sensor_height`, `aperture_f_stops`, `max_circle_of_confusion_diameter`, `max_depth` | **NO**: logged and ignored when `DEPTH_PREPASS_TEXTURE_SUPPORTED == false` (`dof/mod.rs:669`) |
| `Fxaa`, `Smaa` | `bevy::anti_alias::{fxaa::Fxaa, smaa::Smaa}` | components on the camera | ok |
| `TemporalAntiAliasing` | `bevy::anti_alias::taa` | requires `DepthPrepass`, `MotionVectorPrepass` | **no** |
| `ScreenSpaceAmbientOcclusion` | bevy_pbr | | **no** (`ssao/mod.rs:110`) |
| `ChromaticAberration`, `Vignette` | `bevy::post_process::effect_stack` | | probably fine (fullscreen pass); not checked on WebGL2 |
| `ClearColor` (resource), `ClearColorConfig` | prelude | `Default`, `Custom(Color)`, `None` | ok |

Notes:

- Tilt-shift and "diorama" blur, Octopath style: true DoF is not available on WebGL2. Fake it with a custom fullscreen pass that blurs by **screen Y** (top and bottom bands). It needs no depth, so it works on WebGL2. Or ship DoF only for `webgpu` builds.
- `Tonemapping::None` is now a full passthrough. Use `Tonemapping::Linear` for "no curve, keep grading and dither" (`_release-content/migration-guides/tonemapping_none_passthrough.md`).
- `ScreenSpaceTransmission` is now opt-in (`transmission_opt_in.md`). It does not matter for this game.

---

## 9. Lights and shadows

```rust
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, NotShadowCaster};

app.insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 300.0, ..default() })
   .insert_resource(DirectionalLightShadowMap { size: 2048 });   // power of two; default 2048

commands.spawn((
    DirectionalLight {
        illuminance: light_consts::lux::OVERCAST_DAY * 8.0,   // default AMBIENT_DAYLIGHT = 10_000
        shadow_maps_enabled: true,                          // RENAMED (was `shadows_enabled`)
        color: Color::srgb(1.0, 0.95, 0.85),
        ..default()
    },
    Transform::from_xyz(-30.0, 60.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
    CascadeShadowConfigBuilder {
        num_cascades: 1,                 // WebGL2 supports only 1
        minimum_distance: 1.0,
        maximum_distance: 120.0,         // keep tight: the whole visible map area
        first_cascade_far_bound: 120.0,
        overlap_proportion: 0.2,
    }.build(),                           // -> CascadeShadowConfig component
));
commands.spawn((
    PointLight { color: Color::srgb(1.0, 0.6, 0.2), intensity: 20_000.0, range: 8.0,
                 shadow_maps_enabled: false, ..default() },
    Transform::from_xyz(3.0, 2.0, 3.0),
));
```

- **Ambient light** is both a **resource and a component** now:
  - `GlobalAmbientLight` is a `Resource` with `color`, `brightness` (default 80.0) and `affects_lightmapped_meshes`, plus the const `GlobalAmbientLight::NONE` (`crates/bevy_light/src/ambient_light.rs:62`).
  - `AmbientLight` is a **Component**, `#[require(Camera)]`, and overrides the global value per camera (:12).
- `DirectionalLight` (`crates/bevy_light/src/directional_light.rs:73`):
  - Fields: `color`, `illuminance`, `shadow_maps_enabled`, `contact_shadows_enabled`, `affects_lightmapped_mesh_diffuse`, `shadow_depth_bias` (default 0.02), `shadow_normal_bias` (default 1.8).
  - It requires `Cascades`, `CascadeShadowConfig`, `Transform` and `Visibility`.
  - The light points along the entity's **forward (−Z)**, so use `looking_at`.
- `PointLight` (`crates/bevy_light/src/point_light.rs:49`):
  - Fields: `color`, `intensity` (lumens, default 1_000_000), `range` (20), `radius`, `shadow_maps_enabled`, `contact_shadows_enabled`, `shadow_depth_bias` (0.08), `shadow_normal_bias` (0.6), `shadow_map_near_z` (0.1).
  - Point-light shadows are expensive (6 cube faces). Keep them off for torches.
- `CascadeShadowConfigBuilder` (`crates/bevy_light/src/cascade.rs:60`):
  - Fields: `num_cascades`, `minimum_distance`, `maximum_distance`, `first_cascade_far_bound`, `overlap_proportion`.
  - Defaults: cascades 1 on webgl+wasm and 4 elsewhere, min 0.1, max 150, first bound 10, overlap 0.2 (`cascade.rs:142-155`).
  - `build()` asserts its inputs. With `num_cascades == 1` the `minimum_distance < first_cascade_far_bound` check is skipped.
- WebGL2 limits: `MAX_DIRECTIONAL_LIGHTS = 1`, `MAX_CASCADES_PER_LIGHT = 1` (`crates/bevy_pbr/src/render/light.rs:229-244`). Point and spot lights together are capped at 204 per view (`crates/bevy_pbr/src/cluster/mod.rs:31`).
- Shadow marker components live in `bevy::light`: `NotShadowCaster`, `NotShadowReceiver`, `TransmittedShadowReceiver` (`crates/bevy_light/src/lib.rs:264-282`). The `ShadowFilteringMethod` component sets PCF quality.
- Alpha-masked billboards (`AlphaMode::Mask`) cast shadows with the right cut-out shape. The shadow pass discards pixels by alpha.
- `light_consts::lux::*` (prelude) gives reference levels: `AMBIENT_DAYLIGHT` 10_000, `OVERCAST_DAY` 1000, `CLEAR_SUNRISE` 400, `FULL_DAYLIGHT` 20_000, `DIRECT_SUNLIGHT` 100_000.

---

## 10. Building meshes by hand (voxel terrain chunks)

```rust
use bevy::{asset::RenderAssetUsages, mesh::{Indices, PrimitiveTopology}};

fn build_quad_mesh() -> Mesh {
    let positions: Vec<[f32; 3]> = vec![[0.,0.,0.],[1.,0.,0.],[1.,0.,1.],[0.,0.,1.]];
    let normals:   Vec<[f32; 3]> = vec![[0.,1.,0.]; 4];
    let uvs:       Vec<[f32; 2]> = vec![[0.,0.],[1.,0.],[1.,1.],[0.,1.]];
    let colors:    Vec<[f32; 4]> = vec![[1.,1.,1.,1.],[0.8,0.8,0.8,1.],[1.,1.,1.,1.],[0.6,0.6,0.6,1.]]; // per-vertex AO
    // Front faces are COUNTER-CLOCKWISE seen from the viewer (StandardMaterial culls Back by default).
    let indices = Indices::U32(vec![0, 3, 2, 0, 2, 1]);  // CCW seen from +Y
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(indices)
}
// mutable form:
let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0f32,0.,0.],[1.,0.,0.],[0.,0.,1.]]);
m.insert_indices(Indices::U16(vec![0, 2, 1]));
commands.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(material), Transform::IDENTITY));
```

- `Mesh::new(primitive_topology: PrimitiveTopology, asset_usage: RenderAssetUsages)` is at `crates/bevy_mesh/src/mesh.rs:412`.
- The insert methods:
  - `insert_attribute(attr, impl Into<VertexAttributeValues>)` (:445) panics after the mesh has been extracted to the render world. `try_insert_attribute` returns an error instead.
  - `with_inserted_attribute` is at :497. `insert_indices(Indices)` is at :757, `with_inserted_indices` at :785.
  - `compute_normals()`, `compute_flat_normals()` and `duplicate_vertices()` also exist.
- Attribute constants (`mesh.rs:339-388`):
  - `ATTRIBUTE_POSITION` Float32x3 (location 0)
  - `ATTRIBUTE_NORMAL` Float32x3 (1)
  - `ATTRIBUTE_UV_0` Float32x2 (2)
  - `ATTRIBUTE_UV_1` (3)
  - `ATTRIBUTE_TANGENT` Float32x4 (4)
  - `ATTRIBUTE_COLOR` Float32x4 (5)
- `Indices::{U16(Vec<u16>), U32(Vec<u32>)}` (`crates/bevy_mesh/src/index.rs:86`). Use U16 when the chunk has ≤ 65535 vertices.
- `PrimitiveTopology` is importable as `bevy::mesh::PrimitiveTopology` or `bevy::render::render_resource::PrimitiveTopology`. `RenderAssetUsages` is `bevy::asset::RenderAssetUsages`, a bitflag with `MAIN_WORLD` and `RENDER_WORLD`; the default is both (`crates/bevy_asset/src/render_asset.rs:32`).
  - `RENDER_WORLD` only: the CPU copy is dropped after upload, which saves RAM. The mesh can then no longer be edited; replace it with a new asset instead.
  - Keep `MAIN_WORLD` as well if you plan to edit it in place.
- **Vertex colors multiply the material:** the PBR shader sets `base_color = vertex_color` and then `*= material.base_color`, then `*= texture` (`crates/bevy_pbr/src/render/pbr_fragment.wesl:51-52,101`). Per-vertex AO and tint come out of `ATTRIBUTE_COLOR` with no custom shader.
- **Aabb is recomputed automatically** when the `Mesh3d` handle changes **or** the mesh asset changes. `calculate_bounds` filters on `Or<(AssetChanged<Mesh3d>, Changed<Mesh3d>)>` (`crates/bevy_camera/src/visibility/mod.rs:557-575`). The doc comment in `mesh.rs:438` that says otherwise is out of date. Use `NoFrustumCulling` for meshes whose vertices move in a shader.
- Built-in shapes: `meshes.add(Rectangle::new(w, h))` is a quad in the XY plane facing +Z, good for billboards. `Plane3d::default().mesh().size(x, z)` and `Cuboid::new(..)` also exist. Shapes now live in `bevy::shape` (re-exported in the prelude; `_release-content/migration-guides/bevy_shape.md`).

---

## 11. StandardMaterial for terrain and billboard sprites; samplers; atlases

```rust
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};

let tileset: Handle<Image> = asset_server
    .load_builder()                                       // `load_with_settings` is GONE
    .with_settings(|s: &mut ImageLoaderSettings| {
        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            ..ImageSamplerDescriptor::nearest()
        });
    })
    .load("tiny_swords/Terrain/Tileset/Tilemap_color1.png");

let terrain = materials.add(StandardMaterial {
    base_color: Color::WHITE,
    base_color_texture: Some(tileset.clone()),
    perceptual_roughness: 0.95, metallic: 0.0, reflectance: 0.1,
    ..default()
});
let sprite = materials.add(StandardMaterial {
    base_color_texture: Some(asset_server.load("tiny_swords/Units/Blue/Warrior.png")),
    alpha_mode: AlphaMode::Mask(0.5),     // cut-out; depth-sorted like opaque; casts shaped shadows
    double_sided: true,
    cull_mode: None,                      // Option<Face>; default Some(Face::Back)
    unlit: false,                         // true = ignore lighting/shadows (flat sprite look)
    perceptual_roughness: 1.0, reflectance: 0.0,
    ..default()
});
```

- `StandardMaterial` (`crates/bevy_pbr/src/pbr_material.rs:27`). The fields that matter here:
  - `base_color: Color` (default WHITE), `base_color_texture: Option<Handle<Image>>` (:58).
  - `emissive: LinearRgba`, `emissive_texture`.
  - `perceptual_roughness` (default 0.5, :134), `metallic` (0.0), `reflectance` (0.5).
  - `normal_map_texture`, `occlusion_texture`.
  - `double_sided` (:650), `cull_mode: Option<Face>` (:671), `unlit` (:677), `fog_enabled` (:680), `alpha_mode` (:685), `depth_bias` (:697).
  - `uv_transform: Affine2` (:790), `opaque_render_method`.
  - `Face` is imported from `bevy::render::render_resource::Face`.
- `AlphaMode` (prelude, `crates/bevy_material/src/alpha.rs:9`): `Opaque` (default), `Mask(f32)`, `Blend`, `Premultiplied`, `AlphaToCoverage`, `Add`, `Multiply`. Prefer `Mask` for sprites. `Blend` needs sorting and does not write depth.
- Samplers (`crates/bevy_image/src/image.rs:697-935`):
  - `ImageSampler::{Default, Descriptor(ImageSamplerDescriptor)}`, with `ImageSampler::nearest()` and `ImageSampler::linear()`.
  - `ImageSamplerDescriptor` fields: `address_mode_u/v/w`, `mag_filter`, `min_filter`, `mipmap_filter`, `lod_min_clamp`, `lod_max_clamp`, `compare`, `anisotropy_clamp`, `border_color`, `label`.
  - `ImageAddressMode`: `ClampToEdge`, `Repeat`, `MirrorRepeat`, `ClampToBorder`.
- `ImageLoaderSettings` (`crates/bevy_image/src/image_loader.rs:134`): `format`, `texture_format`, `is_srgb`, `sampler`, `asset_usage`.
- PNGs load with **one mip level**. Nearest sampling on minified pixel art shimmers. Keep the camera close enough to stay near 1:1, or bake mips offline (KTX2).
- Tile atlases on a mesh: write the atlas UVs straight into `ATTRIBUTE_UV_0` (tile index → UV rect). To avoid bleeding with nearest sampling, inset UVs by half a texel, or pad the tiles.
- **Sprite-sheet animation on billboards.** A material is shared by every mesh that uses it, so per-unit `uv_transform` means one material per unit, which breaks batching. Two better options:
  - (a) A cache of materials keyed by `(sheet, frame)`, swapping the `MeshMaterial3d` handle when the frame changes.
  - (b) A shared quad mesh per `(sheet, frame)`, with the frame UVs baked in, swapping the `Mesh3d` handle.
  - Entities with the same mesh handle and the same material handle are batched together.
- Billboarding: Bevy has no billboard component. Rotate each frame (cylindrical, so sprites stay upright):

```rust
fn face_camera(cam: Single<&GlobalTransform, With<RtsCamera>>, mut q: Query<&mut Transform, With<Billboard>>) {
    let f = cam.forward();
    let yaw = f32::atan2(-f.x, -f.z);
    for mut tf in &mut q { tf.rotation = Quat::from_rotation_y(yaw); }
}
```

With a camera that has a fixed yaw (the RTS case), set this once at spawn and skip the per-frame system.

- `TextureAtlasLayout` (prelude; `crates/bevy_image/src/texture_atlas.rs:104`) has `size: UVec2` and `textures: Vec<URect>`. Constructors: `TextureAtlasLayout::from_grid(tile_size: UVec2, columns: u32, rows: u32, padding: Option<UVec2>, offset: Option<UVec2>)` (:133) and `new_empty(dim)`. Add frames with `add_texture(URect)`.
- `TextureAtlas { layout: Handle<TextureAtlasLayout>, index: usize }` (:214), with `with_index()` and `texture_rect(&Assets<TextureAtlasLayout>) -> Option<URect>`. Use these to compute UVs yourself.

---

## 12. Custom materials, ExtendedMaterial, WESL shaders, embedding for wasm

```rust
use bevy::{asset::embedded_asset, pbr::{ExtendedMaterial, MaterialExtension},
           render::render_resource::AsBindGroup, shader::ShaderRef};

struct ShadersPlugin;
impl Plugin for ShadersPlugin {
    fn build(&self, app: &mut App) {
        // file is <crate>/src/shaders/water.wesl relative to the .rs file calling the macro
        embedded_asset!(app, "shaders/water.wesl");
        embedded_asset!(app, "shaders/terrain.wesl");
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct WaterMaterial {
    #[uniform(0)] color: LinearRgba,
    #[texture(1)] #[sampler(2)] foam: Option<Handle<Image>>,
}
impl Material for WaterMaterial {
    fn fragment_shader() -> ShaderRef { "embedded://ts_client/shaders/water.wesl".into() }
    fn alpha_mode(&self) -> AlphaMode { AlphaMode::Blend }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct TerrainExt { #[uniform(100)] tint: LinearRgba }   // extension bindings start at 100 by convention
impl MaterialExtension for TerrainExt {
    fn fragment_shader() -> ShaderRef { "embedded://ts_client/shaders/terrain.wesl".into() }
    fn deferred_fragment_shader() -> ShaderRef { "embedded://ts_client/shaders/terrain.wesl".into() }
}
type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExt>;

app.add_plugins((ShadersPlugin,
                 MaterialPlugin::<WaterMaterial>::default(),
                 MaterialPlugin::<TerrainMaterial>::default()));
// spawn:
MeshMaterial3d(terrain_mats.add(ExtendedMaterial {
    base: StandardMaterial { base_color_texture: Some(tileset), ..default() },
    extension: TerrainExt { tint: LinearRgba::WHITE },
}))
```

- The `Material` trait is `Asset + AsBindGroup + Clone + Sized` (`crates/bevy_pbr/src/material.rs:147`). Its methods: `vertex_shader`, `fragment_shader`, `alpha_mode(&self)`, `opaque_render_method`, `depth_bias`, `enable_prepass`, `enable_shadows`, `prepass_*_shader`, `deferred_*_shader` and `specialize(...)`. Register each material type with `MaterialPlugin::<M>::default()`.
- `MaterialExtension` (`crates/bevy_pbr/src/extended_material.rs:31`). `ExtendedMaterial<B, E>{ base, extension }` is at :151.
- `ShaderRef` (`crates/bevy_shader/src/shader.rs:357`) has the variants `Default`, `Handle(Handle<Shader>)` and `Path(AssetPath)`. It implements `From<&'static str>`.
- **`embedded_asset!` path rule** (`crates/bevy_asset/src/io/embedded/mod.rs:228-283,352-364`):
  - The crate name comes from `module_path!()`, i.e. the **lib name** with underscores, `ts_client`.
  - The macro walks up from `file!()` to the first parent directory named `src` and keeps the rest of the path.
  - So `embedded_asset!(app, "shaders/water.wesl")` called in `crates/client/src/lib.rs` (or any file directly in `src/`) registers `embedded://ts_client/shaders/water.wesl`. Called from `src/render/mod.rs`, it registers `embedded://ts_client/render/shaders/water.wesl`.
  - `load_embedded_asset!(asset_server, "shaders/x.wesl")` builds the same path for you. `bevy::shader::load_shader_library!(app, "lib.wesl")` embeds a shader library and keeps it loaded, so other shaders can import it (`crates/bevy_shader/src/lib.rs:28`).
  - Embedded shaders are inside the wasm, so there is no HTTP fetch and no 404.
- **WESL syntax** (`_release-content/migration-guides/shaders_use_wesl.md`, example shaders in `assets/shaders/*.wesl`):
  - `import a::b::{c, d};` statements come first and end with `;`.
  - `#ifdef X` became `@if(X)` / `@elif` / `@else`, attached to a declaration, statement or block.
  - Binding groups and indices are written `@group(constants::MATERIAL_BIND_GROUP) @binding(0)`.
  - Bevy module paths changed:
    - `bevy_pbr::forward_io` → `bevy_pbr::render::forward_io`
    - `bevy_pbr::mesh_view_bindings` → `bevy_pbr::render::mesh_view_bindings`
    - `bevy_pbr::pbr_fragment` → `bevy_pbr::render::pbr_fragment`
    - `bevy_pbr::pbr_functions` → `bevy_pbr::render::pbr_functions`
    - `bevy_pbr::prepass_io` → `bevy_pbr::prepass::io`
    - `bevy_pbr::pbr_deferred_functions` → `bevy_pbr::deferred::functions`
  - Embedded shaders are importable as `ts_client::shaders::water`. Relative imports use `super::file::item`.
- **WebGL2 uniform alignment:** a uniform struct must be 16-byte aligned. Pad `u32`/`f32` scalars up to `vec4`. Bevy sets the `SIXTEEN_BYTE_ALIGNMENT` flag on WebGL2 (`assets/shaders/extended_material.wesl:18-26`, `examples/shader/extended_material.rs` `_webgl2_padding_*` fields). Using `LinearRgba`/`Vec4` fields avoids the problem.

Plain material shader (`src/shaders/water.wesl`):

```wgsl
import bevy_pbr::render::{
    mesh_view_bindings::globals,
    forward_io::VertexOutput,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> water_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var foam_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var foam_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv + vec2<f32>(globals.time * 0.02, 0.0);   // globals.time = seconds since startup
    let foam = textureSample(foam_texture, foam_sampler, uv);
    return mix(water_color, foam, foam.a);
}
```

Extended `StandardMaterial` shader that keeps PBR lighting, shadows and fog (`src/shaders/terrain.wesl`):

```wgsl
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct TerrainExt { tint: vec4<f32>, }
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> terrain_ext: TerrainExt;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags, pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color * terrain_ext.tint);
    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color); // fog + tonemap(if not HDR)
}
    return out;
}
```

---

## 13. Images made at runtime (fog-of-war texture, minimap)

```rust
use bevy::{asset::RenderAssetUsages,
           render::render_resource::{Extent3d, TextureDimension, TextureFormat}};

let mut img = Image::new_fill(
    Extent3d { width: 128, height: 128, depth_or_array_layers: 1 },
    TextureDimension::D2,
    &[0, 0, 0, 255],                         // one pixel, repeated
    TextureFormat::Rgba8Unorm,               // linear data (fog mask); Rgba8UnormSrgb for colors
    RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD, // keep CPU copy to edit
);
img.sampler = ImageSampler::linear();
if let Some(data) = img.data.as_mut() { data[3] = 0; }      // data: Option<Vec<u8>>, row-major RGBA
let handle = images.add(img);
// later, per update (mutable guard marks the asset changed -> re-upload):
if let Some(mut img) = images.get_mut(&handle) { let _ = img.set_color_at(0, 0, Color::WHITE); }
// exact bytes:
let raw = Image::new(Extent3d { width: 2, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2,
                     vec![255,0,0,255, 0,255,0,255], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
```

- `Image::new(size, dimension, data: Vec<u8>, format, asset_usage)` is at `crates/bevy_image/src/image.rs:1123`. A debug assert checks that the data length equals pixel count × pixel size.
- `Image::new_fill(size, dimension, pixel: &[u8], format, asset_usage)` is at :1206. `new_uninit` and `new_target_texture(w, h, format, view_format)` also exist.
- Field `pub data: Option<Vec<u8>>` (:639). Per-pixel access: `pixel_bytes_mut(UVec3)`, `get_color_at(x, y)` and `set_color_at(x, y, Color)` (:1732-1863).
- Every mutable access re-uploads the **whole** texture. Update a fog texture at the sim's 5 Hz fog rate, not every frame. Write `img.data` in bulk instead of calling `set_color_at` for each pixel.
- Example: `examples/2d/cpu_draw.rs`.

---

## 14. From cursor to world (ray casting) and optional mesh picking

```rust
use bevy::window::PrimaryWindow;

fn cursor_pick(window: Single<&Window, With<PrimaryWindow>>,
               camera: Single<(&Camera, &GlobalTransform), With<RtsCamera>>) {
    let (camera, cam_tf) = *camera;
    let Some(cursor) = window.cursor_position() else { return };          // logical px, origin top-left
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else { return };  // -> Result<Ray3d, _>
    if let Some(p) = ray.plane_intersection_point(Vec3::new(0.0, 1.0, 0.0), InfinitePlane3d::new(Vec3::Y)) {
        let tile = (p.x.floor() as i32, p.z.floor() as i32);
    }
    let screen: Result<Vec2, _> = camera.world_to_viewport(cam_tf, Vec3::ZERO); // for HP bars/labels
}
```

- `Camera::viewport_to_world(&self, &GlobalTransform, Vec2) -> Result<Ray3d, ViewportConversionError>` is at `crates/bevy_camera/src/camera.rs:648`. `world_to_viewport` is at :580, `viewport_to_world_2d` at :710.
- `Ray3d { origin: Vec3, direction: Dir3 }` (`crates/bevy_shape/src/ray.rs:78`) has these methods:
  - `get_point(d)`
  - `intersect_plane(origin, InfinitePlane3d) -> Option<f32>`, which returns only distances `> EPSILON`
  - `plane_intersection_point(..) -> Option<Vec3>`
- `InfinitePlane3d { normal: Dir3 }` (`crates/bevy_shape/src/dim3.rs:187`) is built with `InfinitePlane3d::new(Vec3::Y)`. These types moved to `bevy_shape` but stay in the prelude.
- Terrain with height levels: intersect the plane at the highest level first and walk down. Or DDA-march the ray through the height grid, which is exact and cheap.
- Optional mesh picking (feature `mesh_picking`; `MeshPickingPlugin` must be added by hand, see `crates/bevy_picking/src/mesh_picking/mod.rs:67`):

```rust
app.add_plugins(MeshPickingPlugin);
fn mesh_pick(mut ray_cast: MeshRayCast, /* camera, window */) {
    // ray from viewport_to_world as above
    if let Some((entity, hit)) = ray_cast.cast_ray(ray, &MeshRayCastSettings::default()).first() {
        let _ = (entity, hit.point, hit.distance);
    }
}
```

`MeshRayCast::cast_ray(&mut self, Ray3d, &MeshRayCastSettings) -> &[(Entity, RayMeshHit)]`, sorted
nearest first (`crates/bevy_picking/src/mesh_picking/ray_cast/mod.rs:214`). The settings fields are
`visibility`, `filter` and `early_exit_test`. It is CPU work. An RTS with hundreds of units should do
its own spatial queries on sim data instead.

---

## 15. UI (bevy_ui) and text

### 15.1 Node and Val

```rust
commands.spawn((
    DespawnOnExit(AppState::Menu),
    Node {
        width: percent(100), height: percent(100),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center, justify_content: JustifyContent::Center,
        row_gap: px(12),
        padding: UiRect::axes(px(16), px(8)),
        border: UiRect::all(px(2)),
        border_radius: BorderRadius::all(px(6)),   // now a Node FIELD (no separate component)
        position_type: PositionType::Absolute, left: px(10), top: px(10),
        ..default()
    },
    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
    BorderColor::all(Color::BLACK),                // struct with per-side colors; ::all(..), set_all(..)
    GlobalZIndex(10),                              // global stacking; ZIndex(i32) = among siblings
    Visibility::Hidden,
));
```

- `Node` (`crates/bevy_ui/src/ui_node.rs:485`) fields:
  - `display`, `box_sizing`, `position_type`, `overflow`, `scrollbar_width`, `overflow_clip_margin`
  - `left`, `right`, `top`, `bottom`
  - `width`, `height`, `min_*`, `max_*`, `aspect_ratio`
  - `align_items`, `justify_items`, `align_self`, `justify_self`, `align_content`, `justify_content`, `direction`
  - `margin`, `padding`, `border`, **`border_radius`**
  - `flex_direction`, `flex_wrap`, `flex_grow`, `flex_shrink`, `flex_basis`, `row_gap`, `column_gap`
  - `grid_*`
- `Val` variants: `Auto`, `Px`, `Percent`, `Vw`, `Vh`, `VMin`, `VMax`, and the **new `Em`, `Rem`** (`crates/bevy_ui/src/geometry.rs`). Helper functions: `px(n)`, `percent(n)`, `vw(n)`, `vh(n)`, `auto()`, `em(n)`, `rem(n)`. `px(5).all()` returns a `UiRect`.
- `ZIndex(i32)` and `GlobalZIndex(i32)` are at `ui_node.rs:2868,2886`. `BackgroundColor` is at :2576, `BorderColor` at :2603, `BoxShadow` and `Outline` exist too.
- `FocusPolicy` (`crates/bevy_ui/src/focus.rs:121`) has `Block` and `Pass`. **The default is `Pass`.**
- **UI scale:** the resource `UiScale(f32)` (default 1.0, `crates/bevy_ui/src/lib.rs:132`) scales `Val::Px`. `RemSize` (bevy_text) sets what `rem` means; the default is 20 px.
- UI renders on the camera marked `IsDefaultUiCamera`. Without that marker it uses the single active camera. `UiTargetCamera(entity)` overrides it per root (`ui_node.rs:3408,3450`).

### 15.2 Text and fonts

```rust
let font: Handle<Font> = asset_server.load("fonts/Cinzel.ttf");     // .ttf/.otf asset
p.spawn((
    Text::new("La Guerra dei Cinque Regni"),
    TextFont { font: font.clone().into(),               // FontSource (From<Handle<Font>>, From<&str> family)
               font_size: FontSize::Px(32.0),           // FontSize enum, NOT f32
               ..default() },
    TextColor(Color::srgb(0.95, 0.9, 0.75)),
    TextLayout::justify(Justify::Center),               // or TextLayout::new(Justify, LineBreak), ::no_wrap()
));
// Mixed styles: children with TextSpan("..") + their own TextFont/TextColor.
// Font from bytes (embedded with include_bytes!):
let h = fonts.add(Font::from_bytes(include_bytes!("../assets/fonts/x.ttf").to_vec())); // no family-name arg now
```

- `TextFont` (`crates/bevy_text/src/text.rs:671`) fields: `font: FontSource`, `font_size: FontSize`, `weight`, `width`, `style`, `font_smoothing`, `font_features`, `font_variations`.
- **The default size is `FontSize::Rem(1.0)`**, which is 20 px at the default `RemSize` (`_release-content/migration-guides/default_font_size_is_rem.md`).
- `FontSize` (:783) variants: `Px`, `Vw`, `Vh`, `VMin`, `VMax`, `Rem`. It implements `From<f32>`, which means px.
- `FontSource` (:284) variants: `Handle`, `Family(SmolStr)`, `Families`, `List`, `Generic`.
- `FontSmoothing` has `AntiAliased` (default) and `None` (for pixel fonts).
- `Justify`: `Left` (default), `Center`, `Right`, `Justified`, `Start`, … `LineBreak`: `WordBoundary`, `AnyCharacter`, `WordOrCharacter`, `NoWrap`.
- `Font::from_bytes(Vec<u8>)` is at `crates/bevy_text/src/font.rs:39`.
- Text in the world: `Text2d` (bevy_sprite) is for 2D cameras. For 3D, place UI text with absolute `Node` positions from `world_to_viewport` (cheapest for HP numbers). Another way is render-to-texture.
- Text input (player name): `bevy::ui_widgets::TextInput` + `bevy::text::EditableText` + `TabIndex`. Read the value with `editable.value()` (`examples/ui/text/text_input.rs`).

### 15.3 ImageNode, 9-slice, atlas in UI

```rust
let slicer = TextureSlicer {
    border: BorderRect::all(24.0),                         // px insets in the source image
    center_scale_mode: SliceScaleMode::Stretch,
    sides_scale_mode: SliceScaleMode::Tile { stretch_value: 1.0 },
    max_corner_scale: 1.0,
};
p.spawn((
    ImageNode { image: panel.clone(), image_mode: NodeImageMode::Sliced(slicer), ..default() },
    Node { width: px(260), height: px(64), ..default() },
));
let layout = layouts.add(TextureAtlasLayout::from_grid(UVec2::splat(64), 4, 2, None, None));
p.spawn((ImageNode::from_atlas_image(icons, TextureAtlas { layout, index: 3 }),
         Node { width: px(32), height: px(32), ..default() }));
```

- `ImageNode` (`crates/bevy_ui/src/widget/image.rs:18`) fields: `color`, `image`, `texture_atlas: Option<TextureAtlas>`, `flip_x`, `flip_y`, `rect: Option<Rect>`, `image_mode`, `visual_box`.
- Constructors and builders: `ImageNode::new(handle)`, `solid_color`, `from_atlas_image`, and `.with_color()`, `.with_flip_x()`, `.with_rect()`, `.with_mode()`.
- `NodeImageMode` (:215) variants: `Auto`, `Stretch`, `Sliced(TextureSlicer)`, `Tiled{tile_x, tile_y, stretch_value}`.
- `TextureSlicer` (`crates/bevy_sprite/src/texture_slice/slicer.rs:14`) fields: `border: BorderRect`, `center_scale_mode`, `sides_scale_mode`, `max_corner_scale` (default 1.0).
- `BorderRect` (`crates/bevy_sprite/src/texture_slice/border_rect.rs:10`) now has the fields `{ min_inset: Vec2, max_inset: Vec2 }`, not left/right/top/bottom. Build it with `BorderRect::all(f)`, `::axes(h, v)`, or `From<[min_x, max_x, min_y, max_y]>`.
- `SliceScaleMode` has `Stretch` and `Tile{stretch_value}`.
- 9-slice with an atlas: set both `texture_atlas` and `image_mode: Sliced` (`examples/ui/images/ui_texture_atlas_slice.rs`).
- Pixel-perfect UI: `ImagePlugin::default_nearest()` covers UI images too.

### 15.4 Buttons: the new API, and the deprecated one

`bevy::ui::Button` / `Interaction` are **deprecated** (`_release-content/migration-guides/deprecate_button_and_interaction.md`).
**`bevy::prelude::Button` cannot be used:** it is a type alias of a `pub(crate)` struct, and using it in
`With<Button>` fails to compile with the error "type `DeprecatedButton` is private". This was confirmed
by compiling it. Use the new headless button (features `bevy_ui_widgets` + `bevy_picking` + `ui_picking`):

```rust
use bevy::{picking::hover::Hovered, ui::Pressed, ui_widgets::{observe, Activate, Button as WButton}};

p.spawn((
    WButton,                      // bevy::ui_widgets::Button (crates/bevy_ui_widgets/src/button.rs:31)
    Hovered::default(),           // bevy::picking::hover::Hovered(pub bool), .get()
    ImageNode { image: panel.clone(), image_mode: NodeImageMode::Sliced(slicer.clone()), ..default() },
    Node { width: px(260), height: px(64), justify_content: JustifyContent::Center,
           align_items: AlignItems::Center, ..default() },
    observe(|_a: On<Activate>, mut next: ResMut<NextState<AppState>>| next.set(AppState::InGame)),
    children![(Text::new("Gioca da solo"), TextFont { font: font.clone().into(), font_size: FontSize::Px(22.0), ..default() })],
));

fn button_feedback(mut q: Query<(&Hovered, Has<Pressed>, &mut ImageNode),
                                (With<WButton>, Or<(Changed<Hovered>, Changed<ImageNode>)>)>) {
    for (hovered, pressed, mut img) in &mut q {
        img.color = match (pressed, hovered.get()) {
            (true, _) => Color::srgb(0.8, 0.8, 0.8),
            (false, true) => Color::srgb(1.1, 1.1, 1.1),
            _ => Color::WHITE,
        };
    }
}
```

- `Activate { entity }` is an `EntityEvent` (`crates/bevy_ui_widgets/src/lib.rs:114`). It fires on click release while `Pressed`, and on Enter/Space while the button has focus.
- `Pressed` (`crates/bevy_ui/src/interaction_states.rs:54`) is inserted on pointer down and removed on up, cancel or drag end. To react on press instead of release, add `ActivateOnPress`. `InteractionDisabled` disables the button.
- `observe(..)` (`crates/bevy_ui_widgets/src/observe.rs:82`) is a spawn-time helper that attaches an entity observer.
- Removal of `Pressed` is not a change of a component that still exists. Watch it with `RemovedComponents<Pressed>`, as in `examples/ui/images/ui_texture_slice.rs`.
- If picking cannot be enabled, the deprecated `Interaction` still works on its own: spawn `(MyMarker, Node, Interaction::default(), FocusPolicy::Block)` and read `Query<&Interaction, (Changed<Interaction>, With<MyMarker>)>` with `#[allow(deprecated)]`. `ui_focus_system` still updates it (`crates/bevy_ui/src/lib.rs:185`). It will be removed after 0.20.

### 15.5 Is the pointer over the UI? (block world clicks)

```rust
fn pointer_over_ui(hover_map: Res<bevy::picking::hover::HoverMap>, ui: Query<(), With<Node>>) -> bool {
    hover_map.values().any(|hits| hits.keys().any(|e| ui.contains(*e)))
}
// use as a run condition: world_click.run_if(not(pointer_over_ui))
```

`HoverMap(pub HashMap<PointerId, EntityHashMap<HitData>>)` is at `crates/bevy_picking/src/hover.rs:64`.
Give non-interactive overlay nodes `Pickable::IGNORE` (`bevy_picking::Pickable`, which has
`should_block_lower` and `is_hoverable`) so they do not swallow the pointer.

---

## 16. Input

```rust
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit, MouseWheel};

fn camera_controls(keys: Res<ButtonInput<KeyCode>>, mouse: Res<ButtonInput<MouseButton>>,
                   scroll: Res<AccumulatedMouseScroll>, motion: Res<AccumulatedMouseMotion>,
                   mut wheel: MessageReader<MouseWheel>, time: Res<Time>) {
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) { /* pan */ }
    if keys.just_pressed(KeyCode::Escape) {}
    if mouse.just_pressed(MouseButton::Left) {}
    let zoom = match scroll.unit {                      // per-frame sum
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 100.0,
    };
    for ev in wheel.read() { let _ = (ev.unit, ev.x, ev.y, ev.window); }
    let drag: Vec2 = motion.delta;
}
```

- `ButtonInput<T>` (`crates/bevy_input/src/button_input.rs:125`) methods: `pressed`, `just_pressed`, `just_released`, `any_pressed([..])`, `any_just_pressed`, `get_pressed()`, `get_just_pressed()`, `clear_just_pressed`, `reset_all`.
- `KeyCode` uses physical keys: `KeyCode::KeyA`, `Digit1`, `ShiftLeft`, `ControlLeft`, `Space`, `Escape`, `F1`… For layout-aware text, read `KeyboardInput { key_code, logical_key: Key, state, .. }` messages.
- `MouseButton` has `Left`, `Right`, `Middle`, `Back`, `Forward`, `Other(u16)`.
- In `crates/bevy_input/src/mouse.rs`: `AccumulatedMouseScroll { unit, delta: Vec2 }` (:285), `AccumulatedMouseMotion { delta }` (:264), and the `MouseWheel` **Message** `{ unit, x, y, window }` (:214). `MouseScrollUnit` has `Line` and `Pixel`. Browsers usually report `Pixel`.
- Cursor position: `window.cursor_position() -> Option<Vec2>` in logical pixels from the top-left (`crates/bevy_window/src/window.rs:620`). `physical_cursor_position()` is at :631. The `CursorMoved` message also exists.
- On web, a right click opens the browser context menu unless it is suppressed. Call `addEventListener('contextmenu', e => e.preventDefault())` on the canvas in `index.html`. `prevent_default_event_handling: true` handles keys.

---

## 17. Window cursor, diagnostics, gizmos

```rust
use bevy::window::{CursorIcon, CursorOptions, PrimaryWindow, SystemCursorIcon};
fn set_cursor(mut commands: Commands, window: Single<Entity, With<PrimaryWindow>>,
              mut opts: Single<&mut CursorOptions>) {
    commands.entity(*window).insert(CursorIcon::from(SystemCursorIcon::Crosshair));
    opts.visible = true;          // hide: false
}
```

- `CursorIcon` is a **component on the window entity** (`crates/bevy_window/src/cursor/mod.rs:45`). Its variants are `System(SystemCursorIcon)` and `Custom(CustomCursor)`, the latter with feature `custom_cursor`.
- `SystemCursorIcon` has `Default`, `Pointer`, `Crosshair`, `Move`, `Grab`, `Grabbing`, `NotAllowed`, `Wait` and the resize icons.
- `CustomCursor` (`cursor/custom_cursor.rs:74`) has two variants:
  - `Image(CustomCursorImage { handle, texture_atlas, flip_x, flip_y, rect, hotspot: (u16, u16) })`
  - `Url(CustomCursorUrl { url, hotspot })`, **web only**. It is the cheapest way to get a pixel-art cursor in the browser.
- To make a cursor change take effect in the same frame, order the system `.before(bevy::window::CursorSystems::Update)`.

```rust
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
app.add_plugins(FrameTimeDiagnosticsPlugin::default());
fn show_fps(d: Res<DiagnosticsStore>) {
    if let Some(fps) = d.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|x| x.smoothed()) { /* f64 */ }
}
```

`FrameTimeDiagnosticsPlugin { max_history_length, smoothing_factor }`. Its diagnostic paths are `FPS`,
`FRAME_TIME` (ms) and `FRAME_COUNT` (`crates/bevy_diagnostic/src/frame_time_diagnostics_plugin.rs:14,64-70`).
`Diagnostic` has `value()`, `average()` and `smoothed()`. `LogDiagnosticsPlugin` prints them to the log.

```rust
fn debug_gizmos(mut gizmos: Gizmos) {
    gizmos.line(Vec3::ZERO, Vec3::Y, Color::WHITE);
    gizmos.circle(Isometry3d::new(Vec3::ZERO, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), 1.0, Color::srgb(1.0,0.0,0.0));
    gizmos.rect(Isometry3d::IDENTITY, Vec2::ONE, Color::BLACK);
}
```

- `Gizmos<'w,'s, Config = DefaultGizmoConfigGroup, Clear = ()>` is at `crates/bevy_gizmos/src/gizmos.rs:143`.
- Signatures: `line(start, end, color)` (:403), `ray(start, vec, color)`, `rect(isometry: impl Into<Isometry3d>, size: Vec2, color)` (:610), `circle(isometry, radius, color)` and `sphere(...)` (`circles.rs:128,207`).
- A circle lies in the isometry's XY plane. Rotate −90° about X to lay it on the ground.
- Gizmos are immediate mode: redraw them every frame. They need `bevy_gizmos` and `bevy_gizmos_render`. Possible uses are selection rings and rally lines, but sprites look better for those.

---

## 18. Audio

```rust
#[derive(Resource)] struct Sfx { click: Handle<AudioSource> }
// load: asset_server.load("audio/click.wav")   (feature "wav"; "vorbis" for .ogg)
// from bytes (e.g. include_bytes!): AudioSource { bytes: Arc<[u8]> }
fn load_sfx(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let bytes: &'static [u8] = include_bytes!("../assets/audio/click.wav");
    let click = sources.add(AudioSource { bytes: bytes.into() });
    commands.insert_resource(Sfx { click });
}
fn play_click(mut commands: Commands, sfx: Res<Sfx>) {
    commands.spawn((
        AudioPlayer::new(sfx.click.clone()),
        PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(0.5)).with_speed(1.1),
    ));
}
fn music_control(mut sink: Single<&mut AudioSink>) {   // sink exists once playback started
    sink.set_volume(bevy::audio::Volume::Linear(0.3));
    sink.pause();
}
```

- `AudioPlayer<Source = AudioSource>(pub Handle<Source>)` has `#[require(PlaybackSettings)]` (`crates/bevy_audio/src/audio.rs:251`).
- `PlaybackSettings` (:35) fields: `mode: PlaybackMode{Once, Loop, Despawn, Remove}`, `volume: Volume`, `speed: f32` (pitch shift through playback rate), `paused`, `muted`, `spatial`, `spatial_scale`, `start_position`, `duration`.
- The `PlaybackSettings` consts are `ONCE`, `LOOP`, `DESPAWN` and `REMOVE`. The builders are `.with_volume()`, `.with_speed()`, `.paused()`, `.muted()` and `.with_start_position()`. Editing `PlaybackSettings` after spawn does nothing; use `AudioSink`.
- `Volume` has the variants `Linear(f32)` and `Decibels(f32)` (`volume.rs:64,93`). `GlobalVolume` is a resource; set it through `AudioPlugin { global_volume, .. }`.
- `AudioSink` implements the `AudioSinkPlayback` trait (`sinks.rs:10`): `play`, `pause`, `stop`, `set_volume(&mut self, Volume)`, `set_speed`, `is_paused`, `empty`, `mute`.
- **`Pitch`** is a sine-wave generator asset, `Pitch::new(frequency_hz, Duration)` (`crates/bevy_audio/src/pitch.rs:11`), played with `AudioPlayer::<Pitch>(handle)`. It is **not** a pitch-shift control. For that, use `speed`.
- The backend is rodio 0.22 with cpal on WebAudio for wasm (`crates/bevy_audio/Cargo.toml`). Audio starts only after a user gesture. On wasm it is single-threaded and may stutter under load. Keep SFX short and WAV, or use OGG with `vorbis` for music.

---

## 19. Renames and traps (0.14–0.16 memory → 0.20)

| Old | 0.20 |
|---|---|
| `Event` / `EventReader` / `EventWriter` / `app.add_event` / `events.send()` | `Message` / `MessageReader` / `MessageWriter` / `app.add_message` / `w.write()` |
| `Trigger<E>` in observers; `On<Add, A>` | `On<E>`; `On<Add<A>>` (lifecycle: `Add`, `Insert`, `Discard`, `Remove`, `Despawn`) |
| `OnAdd` / `OnInsert` / `OnReplace` / `OnRemove` | `Add` / `Insert` / `Discard` / `Remove` (generic over the bundle) |
| `NextState::set_if_neq` | `set_if_different` (old name deprecated) |
| `StateScoped(S)` | `DespawnOnExit(S)` / `DespawnOnEnter(S)` (always enabled) |
| `Camera { hdr: true, .. }` | the `Hdr` marker component (`bevy::camera::Hdr`) |
| `Msaa` resource | `Msaa` component on the camera |
| `AmbientLight` resource | `GlobalAmbientLight` resource **and** per-camera `AmbientLight` component |
| `DirectionalLight/PointLight { shadows_enabled }` | `shadow_maps_enabled` (plus `contact_shadows_enabled`) |
| `PerspectiveProjection { fov, near, far, aspect_ratio }` literal | add `..default()` (new `near_clip_plane`) |
| `Window { cursor: Cursor {..} }` / `window.cursor.visible` | separate `CursorOptions` component; `CursorIcon` component on the window |
| `asset_server.load_with_settings(path, f)` | `asset_server.load_builder().with_settings(f).load(path)` |
| `assets.get_mut(id) -> Option<&mut A>` | `Option<AssetMut<A>>` (derefs); `get_mut_untracked` |
| `TextFont { font: handle, font_size: 20.0 }` | `font: handle.into()` (FontSource), `font_size: FontSize::Px(20.0)` (default `Rem(1.0)`) |
| `Font::from_bytes(bytes, name)` | `Font::from_bytes(bytes)` |
| `BorderRadius` component | `Node.border_radius` field |
| `BorderColor(color)` | `BorderColor::all(color)` (per-side struct) |
| `BorderRect { left, right, top, bottom }` | `BorderRect { min_inset, max_inset }`, `BorderRect::all/axes` |
| `Button` + `Interaction` | `ui_widgets::Button` + `Hovered` + `Pressed` + `On<Activate>` (`bevy::prelude::Button` cannot be used) |
| `Val::Px(..)` everywhere | still valid; helper functions `px()`, `percent()`, `vw()`, `vh()`, `em()`, `rem()` |
| `bevy::math::primitives::*`, `Ray3d` in bevy_math | moved to `bevy::shape` (still in the prelude) |
| `#import`, `#ifdef`, `#{MATERIAL_BIND_GROUP}`, `.wgsl` with naga_oil | WESL: `import ...;`, `@if(..)`, `constants::MATERIAL_BIND_GROUP`, `.wesl`; `bevy_pbr::render::forward_io` etc. |
| `Tonemapping::None` = "no curve" | now a full passthrough; use `Tonemapping::Linear` |
| `bevy_core_pipeline::tonemapping::Tonemapping` | still re-exported; it lives in `bevy_render::view` |
| `MeshPickingPlugin` in DefaultPlugins | not included: add it by hand (feature `mesh_picking`) |
| `Query::single()` panics | returns `Result`; `Single<..>` skips the system |
| `despawn_recursive()` | `despawn()` (recursive) |
| Mesh `Aabb` not refreshed after asset edits | refreshed on `AssetChanged<Mesh3d>` |

### WebGL2 summary (feature `webgl2`, no `webgpu`)

| Feature | WebGL2 |
|---|---|
| PBR `StandardMaterial`, alpha mask, vertex colors, fog, custom/extended materials | yes |
| Directional shadows | yes, **1 light, 1 cascade** |
| Point/spot lights | yes (≤ 204 clustered); point shadows are expensive |
| HDR + Bloom | yes |
| MSAA | yes (Sample4 safe); FXAA/SMAA yes |
| DepthOfField / SSR / SSAO / TAA / depth prepass reads | **no** |
| Compute shaders, storage buffers, bindless | **no** (Bevy falls back to uniform buffers on its own) |
| Uniform struct alignment | 16 bytes; pad scalars |
| Threads | single thread (`multi_threaded` does nothing) |

---

## Verification footer

- Native `cargo check` (rustc 1.100.0, x86_64-pc-windows-msvc) of the scratch crate with every snippet above: **passed**, after two fixes that are already in this file:
  - `Hdr` is `bevy::camera::Hdr`, not `bevy::render::view::Hdr`.
  - The deprecated `bevy::prelude::Button` cannot be used.
- `cargo check --target wasm32-unknown-unknown` of the same crate: **passed** with feature list A, and with feature list B plus `mesh_picking`.
- Not verified here: that the WESL shaders compile at runtime (they follow the official 0.20 example shaders), the visual result of each WebGL2 effect, and the final wasm size.
