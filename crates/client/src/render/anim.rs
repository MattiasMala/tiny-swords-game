// OWNER: C2 (sprites) — sprite animation rules: spec 01 §2.5 (`UNIT_ANIM`, `unitSheet` folder rules, pawn tool/load
// variants, lancer 5 directions, siege idle = frame 0 of the run sheet, attack frames clamp), 07 §8.3 (`drawSheet` frame
// wrap), 09 §12 (world animation clocks).
//
// * `WorldClock` = presentation time following the sim time (`G.time`): advances with real time while snapshots keep
//   coming, freezes when the sim is paused / frozen (solo pause, test-mode freeze, network stall). All world
//   animations (trees, bushes, water rocks, fires, ducks, FX) run on it.
// * `UnitAnims` keeps a local `animT` per unit: reset to the snapshot's `anim_t` whenever the server anim changes
//   (the server only re-sends `anim_t` on anim changes), advanced locally in between.
// * `UnitSheetCache` maps (colour, kind, anim, dir, carry, tool) → atlas sheet, building the file name with the
//   folder rules of spec 01 §2.5 once per key.

use crate::assets::{SheetId, SpriteAtlas};
use crate::mirror::Mirror;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use ts_sim::defs::{AnimKind, Dir5, KingdomColor, PawnTool, Res as ResKind, UnitKind};
use ts_sim::slab::UnitId;
use ts_sim::snapshot_types::UnitView;

/// Presentation clock (seconds) tied to the sim clock.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct WorldClock {
    /// presentation time (≈ sim `time`, smoothed)
    pub t: f64,
    /// advance of `t` this frame (0 while the sim is paused)
    pub dt: f32,
    /// real seconds since start (for effects that ignore pauses)
    pub real: f64,
    last_sim: f32,
    /// real time of the last sim-time change
    last_change: f64,
}

/// Local animation state of one unit.
#[derive(Clone, Copy, Debug)]
pub struct UnitAnim {
    pub key: AnimKey,
    /// last `anim_t` received from the server
    pub server_t: f32,
    /// local `animT`
    pub t: f32,
}

/// Everything that selects a unit sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimKey {
    pub color: KingdomColor,
    pub kind: UnitKind,
    pub anim: AnimKind,
    pub dir: Dir5,
    pub carry: Option<ResKind>,
    pub tool: Option<PawnTool>,
}

impl AnimKey {
    pub fn of(u: &UnitView, color: KingdomColor) -> AnimKey {
        let lancer = u.kind == UnitKind::Lancer;
        let pawn = u.kind == UnitKind::Pawn;
        AnimKey {
            color,
            kind: u.kind,
            anim: u.anim,
            // the direction only matters for lancer attack/block sheets
            dir: if lancer { u.dir } else { Dir5::Right },
            carry: if pawn { u.carry } else { None },
            tool: if pawn { u.tool } else { None },
        }
    }

    /// Attack animations do not loop (frame clamped to `n - 1`).
    pub fn clamps(&self) -> bool {
        matches!(self.anim, AnimKind::Attack | AnimKind::Attack2)
    }
}

#[derive(Resource, Default)]
pub struct UnitAnims {
    pub map: HashMap<UnitId, UnitAnim>,
    generation: u32,
}

impl UnitAnims {
    pub fn get(&self, id: UnitId) -> Option<&UnitAnim> {
        self.map.get(&id)
    }
}

/// Cached sheet lookups (`None` = no such sheet in the atlas).
#[derive(Resource, Default)]
pub struct UnitSheetCache {
    map: HashMap<AnimKey, Option<(SheetId, f32)>>,
}

impl UnitSheetCache {
    /// Sheet + playback fps for an anim key.
    pub fn get(&mut self, atlas: &SpriteAtlas, key: AnimKey) -> Option<(SheetId, f32)> {
        if let Some(v) = self.map.get(&key) {
            return *v;
        }
        let v = resolve_sheet(atlas, key);
        self.map.insert(key, v);
        v
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

/// File base name of a unit animation (`UNIT_ANIM[type][anim][0]`, spec 01 §2.5).
pub fn unit_anim_file(key: AnimKey) -> String {
    use AnimKind as A;
    use UnitKind as U;
    let k = key.kind;
    match k {
        U::Pawn => {
            let adorn = key
                .carry
                .map(|r| r.load_name())
                .or(key.tool.map(|t| t.name()));
            match key.anim {
                A::Work => format!(
                    "Pawn_Interact {}",
                    key.tool.unwrap_or(PawnTool::Hammer).name()
                ),
                A::Run => match adorn {
                    Some(a) => format!("Pawn_Run {a}"),
                    None => "Pawn_Run".to_string(),
                },
                _ => match adorn {
                    Some(a) => format!("Pawn_Idle {a}"),
                    None => "Pawn_Idle".to_string(),
                },
            }
        }
        U::Scout => match key.anim {
            A::Run => "Scout_Run",
            _ => "Scout_Idle",
        }
        .to_string(),
        U::Warrior => match key.anim {
            A::Run => "Warrior_Run",
            A::Attack => "Warrior_Attack1",
            A::Attack2 => "Warrior_Attack2",
            A::Block => "Warrior_Guard",
            _ => "Warrior_Idle",
        }
        .to_string(),
        U::Lancer => match key.anim {
            A::Run => "Lancer_Run".to_string(),
            A::Attack | A::Attack2 => format!("Lancer_{}_Attack", key.dir.name()),
            A::Block => format!("Lancer_{}_Defence", key.dir.name()),
            _ => "Lancer_Idle".to_string(),
        },
        U::Archer => match key.anim {
            A::Run => "Archer_Run",
            A::Attack | A::Attack2 => "Archer_Shoot",
            _ => "Archer_Idle",
        }
        .to_string(),
        U::Monk => match key.anim {
            A::Run => "Run",
            A::Attack | A::Attack2 => "Heal",
            _ => "Idle",
        }
        .to_string(),
        U::Knight => match key.anim {
            A::Run => "Knight_Run",
            A::Attack | A::Attack2 => "Knight_Attack",
            _ => "Knight_Idle",
        }
        .to_string(),
        U::Ram => match key.anim {
            A::Attack | A::Attack2 => "Ram_Attack",
            _ => "Ram_Run",
        }
        .to_string(),
        U::Catapult => match key.anim {
            A::Attack | A::Attack2 => "Catapult_Attack",
            _ => "Catapult_Run",
        }
        .to_string(),
    }
}

/// Asset path (relative to `assets/`) of a unit animation sheet (`unitSheet`, spec 01 §2.5).
pub fn unit_sheet_path(key: AnimKey) -> String {
    let file = unit_anim_file(key);
    let c = key.color.key();
    let extra = file.ends_with(" Stone");
    match key.kind.def().dir {
        Some(dir) if !extra => format!("Units/{c} Units/{dir}/{file}.png"),
        _ => format!("Extra/Units/{c}/{file}.png"),
    }
}

/// Playback fps of an anim (siege idle = 0: frozen on the first marching frame, spec 01 §2.5).
pub fn anim_fps(key: AnimKey, sheet_fps: f32) -> f32 {
    let d = key.kind.def();
    match key.anim {
        AnimKind::Idle => d.fps.idle,
        AnimKind::Run => {
            if sheet_fps > 0.0 {
                sheet_fps
            } else {
                d.fps.run
            }
        }
        _ => {
            if sheet_fps > 0.0 {
                sheet_fps
            } else {
                10.0
            }
        }
    }
}

fn resolve_sheet(atlas: &SpriteAtlas, key: AnimKey) -> Option<(SheetId, f32)> {
    let path = unit_sheet_path(key);
    let id = atlas.sheet_id(&path).or_else(|| {
        // missing adornment sheet: fall back to the plain idle/run sheet
        let plain = AnimKey {
            carry: None,
            tool: None,
            anim: if key.anim == AnimKind::Run {
                AnimKind::Run
            } else {
                AnimKind::Idle
            },
            ..key
        };
        atlas.sheet_id(&unit_sheet_path(plain))
    })?;
    let fps = anim_fps(key, atlas.sheet(id).fps);
    Some((id, fps))
}

/// Frame index of an animation at local time `t` (`floor(animT*fps)`; attacks clamp to the last frame).
pub fn anim_frame(t: f32, fps: f32, frames: u16, clamp: bool) -> i64 {
    if fps <= 0.0 {
        return 0;
    }
    let f = (t.max(0.0) * fps).floor() as i64;
    if clamp {
        f.min(frames.max(1) as i64 - 1)
    } else {
        f
    }
}

/// Deterministic per-entity pseudo-random number in `[0, 1)` (presentation only).
#[inline]
pub fn hash01(a: u32, b: u32) -> f32 {
    let mut x = a
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(b.wrapping_mul(0x85EB_CA6B))
        ^ 0x27d4_eb2f;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

pub struct AnimPlugin;

impl Plugin for AnimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldClock>()
            .init_resource::<UnitAnims>()
            .init_resource::<UnitSheetCache>()
            .add_systems(
                Update,
                (tick_clock, tick_unit_anims)
                    .chain()
                    .in_set(super::sprites::SpriteSystems::Prepare),
            );
    }
}

/// Advances the presentation clock: real time, kept within ±0.15 s of the last snapshot's sim time, stopped when
/// the sim time has not moved for 0.25 s (pause / freeze).
fn tick_clock(time: Res<Time<Real>>, mirror: Res<Mirror>, mut clock: ResMut<WorldClock>) {
    let rdt = time.delta_secs_f64().min(0.1);
    clock.real += rdt;
    let sim = mirror.view.time;
    if !mirror.active {
        clock.t += rdt;
        clock.dt = rdt as f32;
        return;
    }
    if sim != clock.last_sim {
        clock.last_sim = sim;
        clock.last_change = clock.real;
    }
    let stalled = clock.real - clock.last_change > 0.25 || mirror.view.frozen;
    let prev = clock.t;
    let target = sim as f64;
    if (clock.t - target).abs() > 1.0 {
        // new match / big jump: snap
        clock.t = target;
    } else if !stalled {
        clock.t = (clock.t + rdt).clamp(target - 0.15, target + 0.15);
    }
    clock.dt = (clock.t - prev).max(0.0) as f32;
}

/// Keeps one local `animT` per visible unit.
fn tick_unit_anims(mirror: Res<Mirror>, clock: Res<WorldClock>, mut anims: ResMut<UnitAnims>) {
    if anims.generation != mirror.generation {
        anims.generation = mirror.generation;
        anims.map.clear();
    }
    let dt = clock.dt;
    let colors = &mirror.colors;
    anims
        .map
        .retain(|id, _| mirror.view.units.contains_key(id));
    for (id, u) in &mirror.view.units {
        let color = colors
            .get(u.faction as usize)
            .copied()
            .unwrap_or(KingdomColor::Blue);
        let key = AnimKey::of(u, color);
        match anims.map.get_mut(id) {
            Some(a) => {
                if a.key != key || a.server_t != u.anim_t {
                    // anim changed (or restarted): resync with the server's animT
                    if a.key.anim != key.anim || a.server_t != u.anim_t {
                        a.t = u.anim_t;
                    }
                    a.key = key;
                    a.server_t = u.anim_t;
                } else {
                    a.t += dt;
                }
            }
            None => {
                anims.map.insert(
                    *id,
                    UnitAnim {
                        key,
                        server_t: u.anim_t,
                        t: u.anim_t,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(kind: UnitKind, anim: AnimKind) -> AnimKey {
        AnimKey {
            color: KingdomColor::Red,
            kind,
            anim,
            dir: Dir5::UpRight,
            carry: None,
            tool: None,
        }
    }

    #[test]
    fn sheet_paths_follow_folder_rules() {
        assert_eq!(
            unit_sheet_path(key(UnitKind::Warrior, AnimKind::Attack2)),
            "Units/Red Units/Warrior/Warrior_Attack2.png"
        );
        assert_eq!(
            unit_sheet_path(key(UnitKind::Lancer, AnimKind::Block)),
            "Units/Red Units/Lancer/Lancer_UpRight_Defence.png"
        );
        assert_eq!(
            unit_sheet_path(key(UnitKind::Knight, AnimKind::Run)),
            "Extra/Units/Red/Knight_Run.png"
        );
        assert_eq!(
            unit_sheet_path(key(UnitKind::Ram, AnimKind::Idle)),
            "Extra/Units/Red/Ram_Run.png"
        );
        let mut p = key(UnitKind::Pawn, AnimKind::Run);
        p.carry = Some(ResKind::Stone);
        assert_eq!(unit_sheet_path(p), "Extra/Units/Red/Pawn_Run Stone.png");
        p.carry = None;
        p.tool = Some(PawnTool::Pickaxe);
        assert_eq!(
            unit_sheet_path(p),
            "Units/Red Units/Pawn/Pawn_Run Pickaxe.png"
        );
        p.anim = AnimKind::Work;
        assert_eq!(
            unit_sheet_path(p),
            "Units/Red Units/Pawn/Pawn_Interact Pickaxe.png"
        );
        assert_eq!(
            unit_sheet_path(key(UnitKind::Monk, AnimKind::Attack)),
            "Units/Red Units/Monk/Heal.png"
        );
    }

    #[test]
    fn every_unit_sheet_exists_in_manifest() {
        let m = crate::assets::parse_manifest();
        let paths: std::collections::HashSet<&str> = m.iter().map(|e| e.rel()).collect();
        for &kind in UnitKind::ALL {
            for &anim in AnimKind::ALL {
                for &dir in Dir5::ALL {
                    let mut k = key(kind, anim);
                    k.dir = dir;
                    let p = unit_sheet_path(k);
                    let ok = paths.contains(p.as_str());
                    // pawns have no attack/block sheets (fallback to plain idle at runtime)
                    if kind != UnitKind::Pawn
                        || matches!(anim, AnimKind::Idle | AnimKind::Run | AnimKind::Work)
                    {
                        assert!(ok, "missing {p}");
                    }
                }
            }
        }
    }

    #[test]
    fn frames_and_fps() {
        assert_eq!(anim_frame(0.5, 0.0, 4, false), 0);
        assert_eq!(anim_frame(0.26, 12.0, 4, true), 3);
        assert_eq!(anim_frame(5.0, 12.0, 4, true), 3);
        assert_eq!(anim_frame(1.0, 8.0, 8, false), 8);
        assert_eq!(anim_fps(key(UnitKind::Ram, AnimKind::Idle), 8.0), 0.0);
        assert_eq!(anim_fps(key(UnitKind::Ram, AnimKind::Run), 8.0), 8.0);
        let h = hash01(3, 7);
        assert!((0.0..1.0).contains(&h));
    }
}
