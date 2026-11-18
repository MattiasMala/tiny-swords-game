# Implementation plan & conventions (contract for every implementer)

Read first: `docs/ARCHITECTURE.md`, then the spec chapters relevant to your module (`docs/spec/*.md`),
and for Bevy APIs `docs/bevy-0.20-notes.md` + the real source at `C:/Users/User/.bevy-ref/bevy-0.20.1`
(crates/ and examples/). Your memory of Bevy is from older versions: VERIFY every Bevy API in that source.
The old JS game is still in `js/` for double-checking behaviour (it will be deleted at the end).

## Toolchain

- Rust 1.100.0 pinned by `rust-toolchain.toml` (target `wasm32-unknown-unknown` installed), Bevy 0.20.1,
  wasm-bindgen(-cli) 0.2.130. Dependencies are prebuilt in `target/` — do not run `cargo clean`, do not change
  the `[profile.*]` sections, do not add heavy dependencies without need.
- Check with `cargo check -p ts-sim`, `cargo check -p ts-server`, `cargo check -p ts-client` (native) and
  `cargo check -p ts-client --target wasm32-unknown-unknown`. Tests: `cargo test -p ts-sim`.
- Several agents work in the same tree at the same time. Cargo serialises builds with a lock ("Blocking waiting
  for file lock") — that is normal, just wait. If `cargo check` shows errors ONLY in files you do not own,
  another agent is mid-edit: re-run a bit later; never "fix" files you do not own (except trivial,
  obviously-needed signature adaptations that you then report). Keep your own files compiling at all times:
  make small, compiling increments.

## Units and coordinates (sim)

- **Sim units are the JS pixels**: 1 tile = `TILE = 64.0` units. All constants from the spec (speeds px/s, ranges
  px, radii px) are used verbatim. Cells are `(cx, cy)` with `cx in 0..W`, `cy in 0..H`, cell index `cy*W+cx`.
- `x` grows east, `y` grows south. An entity's `(x, y)` is its foot point.
- Fixed tick `TICK_DT = 0.05` (20 Hz). All randomness goes through `World.rng` (seeded `Rng`), never `std` random.
- `f32` everywhere in the sim. No `HashMap` iteration order dependence in gameplay (use `Vec`/`BTreeMap`).

## Units and coordinates (client 3D)

- 1 tile = 1.0 world unit. Sim `(x, y)` px → Bevy `Vec3::new(x / 64.0, height, y / 64.0)`
  (Bevy +X = east, +Z = south, +Y = up). `LEVEL_HEIGHT` (world units per terrain level) lives in
  `client/src/render/consts.rs`.
- Camera looks north (towards -Z), pitched down; south faces of cliffs are the visible ones, as in the 2D art.
- Sprite pixels → world: `1 px = 1/64` world units (a 192-px unit frame is 3 tiles wide, like the 2D game).

## Ownership map (who writes which file)

| Owner | Files |
|---|---|
| foundation | `sim/src/{lib,rng,math,slab,defs,world,cmd,proto,snapshot_types}.rs` (types + stubs), client/server skeletons |
| S1 map | `sim/src/map.rs`, `sim/src/mapgen/**`, `sim/src/path.rs`, `sim/examples/mapgen_png.rs` |
| S2 units | `sim/src/unit.rs`, `sim/src/combat.rs`, `sim/src/formation.rs`, `sim/src/projectile.rs` |
| S3 eco | `sim/src/gather.rs`, `sim/src/building.rs`, `sim/src/point.rs`, `sim/src/sheep.rs` |
| S4 faction | `sim/src/faction.rs`, `sim/src/command.rs` (execution of `cmd::Command`), `sim/src/fog.rs` |
| S5 ai | `sim/src/ai/**`, `sim/examples/simbench.rs`, `sim/tests/ai_*.rs` |
| S6 net | `sim/src/snapshot.rs`, `sim/src/room.rs`, `server/**` |
| C1 terrain | `client/src/render/terrain*.rs`, `client/src/render/water.rs`, `client/src/render/camera.rs`, `client/src/render/sky.rs` |
| C2 sprites | `client/src/render/sprites*.rs`, `client/src/render/anim.rs`, `client/src/render/light.rs`, `client/src/render/fx.rs`, `client/src/assets.rs` |
| C3 ui | `client/src/ui/**` |
| C4 control | `client/src/control/**`, `client/src/link.rs`, `client/src/audio.rs`, `web/**`, `build-web.*` |

`world.rs`, `lib.rs`, `client/src/lib.rs` are shared: add only what you need (a field, a `mod` line, a plugin)
with minimal, surgical edits and re-read the file right before editing.

## Language

Code identifiers and comments in English. **Every user-visible string in Italian**, copied verbatim from the spec.
