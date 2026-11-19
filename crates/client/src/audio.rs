// OWNER: C4 (control) — spec 07-light-sfx.md §10 (procedural SFX `Sfx`, positional falloff, UI sounds, horn alert,
// audio toggle), docs/bevy-0.20-notes.md §18 (AudioPlayer, PlaybackSettings, browser needs a user gesture).
//
// Every sound of spec 07 §10.2 is synthesized once at startup into a mono 16-bit PCM WAV buffer (no asset files):
// band-limited oscillators (Fourier series like WebAudio's built-in waves, Chebyshev recurrence), exponential
// frequency slides and gain envelopes, white-noise bursts through the WebAudio lowpass biquad. Sounds that contain
// noise get `NOISE_VARIANTS` renders so repeated hits do not sound identical (JS re-randomises per play).
//
// Playing: other modules write [`PlaySfx`] (`PlaySfx::ui(Sound::Click)` full volume, `PlaySfx::at(sound, x, y)`
// positional, sim px). Sim events (`GameEvent::Sound` / `GameEvent::Play`) are consumed here. Rules (07 §10.1):
// muted → nothing; no sound before the first user gesture (browser autoplay policy, JS `ctx.state != 'running'`);
// volume ≤ 0.02 → skipped; per-name 45 ms throttle (only consumed by sounds that pass the volume check);
// positional volume `1 - d / (viewW * 0.75)` with `d` = distance from the view centre in world px.
// UI sounds of the HUD / screens (C3) arrive as `GameEvent::Play` too (`UiState::play`), so every sound path ends here.

use crate::AppState;
use crate::control::view::ViewInfo;
use crate::link::GameEventMsg;
use crate::mirror::Mirror;
use bevy::audio::{PlaybackMode, Volume};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use std::sync::Arc;
use ts_sim::defs::Sound;
use ts_sim::events::GameEvent;

/// Output sample rate of the synthesized buffers.
pub const SAMPLE_RATE: u32 = 44_100;
/// Base voice gain (`v = 0.07 * vol`).
pub const BASE_GAIN: f32 = 0.07;
/// Per-name repeat limit (s).
pub const THROTTLE: f64 = 0.045;
/// Sounds at or below this volume are skipped.
pub const MIN_VOL: f32 = 0.02;
/// Renders per noise-based sound.
pub const NOISE_VARIANTS: usize = 3;

/// Audio on/off (`Sfx.enabled`). Toggled by `M` and the speaker buttons; not persisted.
#[derive(Resource, Debug, Clone, Copy)]
pub struct AudioSettings {
    pub enabled: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings { enabled: true }
    }
}

/// Request to play a sound effect.
#[derive(Message, Clone, Copy, Debug)]
pub struct PlaySfx {
    pub sound: Sound,
    /// `Some(px)` = positional (distance falloff from the view centre), `None` = UI sound at `vol`
    pub at: Option<Vec2>,
    /// volume scalar (1 = spec level)
    pub vol: f32,
}

impl PlaySfx {
    /// Local UI sound at full volume (`Sfx.play(name)`).
    pub fn ui(sound: Sound) -> PlaySfx {
        PlaySfx {
            sound,
            at: None,
            vol: 1.0,
        }
    }

    /// Positional sound at sim px (`Sfx.hear(name, x, y)`).
    pub fn at(sound: Sound, x: f32, y: f32) -> PlaySfx {
        PlaySfx {
            sound,
            at: Some(Vec2::new(x, y)),
            vol: 1.0,
        }
    }
}

/// Synthesized buffers, `bank[sound.index()]` = variants.
#[derive(Resource, Default)]
pub struct SfxBank {
    pub bank: Vec<Vec<Handle<AudioSource>>>,
}

/// Runtime state of the player.
#[derive(Resource)]
struct SfxState {
    /// last real time (s) each sound played
    last: Vec<f64>,
    /// a user gesture happened (WebAudio may run)
    unlocked: bool,
    /// variant picker (presentation randomness)
    rng: u32,
}

impl Default for SfxState {
    fn default() -> Self {
        SfxState {
            last: vec![-1.0; Sound::COUNT],
            unlocked: false,
            rng: 0x9e37_79b9,
        }
    }
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioSettings>()
            .init_resource::<SfxState>()
            .init_resource::<SfxBank>()
            .add_message::<PlaySfx>()
            .add_systems(Startup, synthesize_bank)
            .add_systems(
                Update,
                (
                    unlock_on_gesture,
                    sim_sounds,
                    play_requests,
                )
                    .chain(),
            );
    }
}

fn synthesize_bank(mut sources: ResMut<Assets<AudioSource>>, mut bank: ResMut<SfxBank>) {
    bank.bank = Sound::ALL
        .iter()
        .map(|&s| {
            let n = if recipe(s).iter().any(|v| matches!(v, Voice::Noise { .. })) {
                NOISE_VARIANTS
            } else {
                1
            };
            (0..n)
                .map(|k| {
                    let pcm = render(s, SAMPLE_RATE, 0x5eed_0000 ^ ((s.index() as u32) << 8) ^ k as u32);
                    sources.add(AudioSource {
                        bytes: Arc::from(wav_bytes(&pcm, SAMPLE_RATE)),
                    })
                })
                .collect()
        })
        .collect();
}

/// `Sfx.unlock()` on every key / mouse press.
fn unlock_on_gesture(
    mut keys: MessageReader<KeyboardInput>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut st: ResMut<SfxState>,
) {
    let any = keys.read().count() > 0 || buttons.read().count() > 0;
    if any && !st.unlocked {
        st.unlocked = true;
    }
}

/// Sim presentation events → sounds.
fn sim_sounds(
    mut events: MessageReader<GameEventMsg>,
    mut out: MessageWriter<PlaySfx>,
    view: Res<ViewInfo>,
    mirror: Res<Mirror>,
) {
    for GameEventMsg(e) in events.read() {
        match e {
            GameEvent::Sound { sound, x, y } => {
                // 07 addenda L1: the JS gated `chop` with `onScreen(x, y, 80)`; the sim always emits it, the client culls
                if *sound == Sound::Chop {
                    let map = mirror.map();
                    let k = view.scale_at(map, *x, *y) / ts_sim::TILE;
                    if !view.on_screen(map, *x, *y, 80.0 * k) {
                        continue;
                    }
                }
                out.write(PlaySfx::at(*sound, *x, *y));
            }
            // private: the room routes it only to its recipient; the UI also emits its sounds this way
            GameEvent::Play { sound, .. } => {
                out.write(PlaySfx::ui(*sound));
            }
            _ => {}
        }
    }
}

fn play_requests(
    mut commands: Commands,
    mut reqs: MessageReader<PlaySfx>,
    settings: Res<AudioSettings>,
    bank: Res<SfxBank>,
    view: Res<ViewInfo>,
    time: Res<Time<Real>>,
    state: Res<State<AppState>>,
    mut st: ResMut<SfxState>,
) {
    if !settings.enabled || !st.unlocked || bank.bank.is_empty() {
        reqs.clear();
        return;
    }
    let now = time.elapsed_secs_f64();
    for r in reqs.read() {
        let mut vol = r.vol;
        if let Some(p) = r.at {
            // positional sounds exist only in a match (the JS menu demo battle is silent)
            if *state.get() != AppState::InGame || !view.ok {
                continue;
            }
            vol *= positional_volume(p, view.center_px, view.width_px);
        }
        if vol <= MIN_VOL {
            continue;
        }
        let i = r.sound.index();
        if now - st.last[i] < THROTTLE {
            continue;
        }
        st.last[i] = now;
        let Some(variants) = bank.bank.get(i).filter(|v| !v.is_empty()) else {
            continue;
        };
        st.rng = st.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let h = variants[(st.rng >> 16) as usize % variants.len()].clone();
        commands.spawn((
            AudioPlayer::new(h),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(vol.min(1.0)),
                ..PlaybackSettings::DESPAWN
            },
        ));
    }
}

/// `1 - d / (viewW * 0.75)` (linear falloff, audible within 0.75 view widths of the view centre).
pub fn positional_volume(p: Vec2, center: Vec2, view_w: f32) -> f32 {
    if view_w <= 0.0 {
        return 0.0;
    }
    1.0 - p.distance(center) / (view_w * 0.75)
}

// ---------------------------------------------------------------------------------------------------------------
// Synthesis (spec 07 §10.1 / §10.2)
// ---------------------------------------------------------------------------------------------------------------

/// WebAudio oscillator types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wave {
    Sine,
    Square,
    Triangle,
    Sawtooth,
}

/// One voice of a sound (`tone` / `noise`), gains already multiplied by the base `v` factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Voice {
    /// `tone(freq, dur, type, vol, slideTo?)` starting at `at` seconds
    Tone {
        at: f32,
        freq: f32,
        dur: f32,
        wave: Wave,
        vol: f32,
        slide: Option<f32>,
    },
    /// `noise(dur, vol, cutoff)`
    Noise {
        at: f32,
        dur: f32,
        vol: f32,
        cutoff: f32,
    },
}

fn tone(freq: f32, dur: f32, wave: Wave, k: f32, slide: Option<f32>) -> Voice {
    Voice::Tone {
        at: 0.0,
        freq,
        dur,
        wave,
        vol: k * BASE_GAIN,
        slide,
    }
}

fn tone_at(at: f32, freq: f32, dur: f32, wave: Wave, k: f32) -> Voice {
    Voice::Tone {
        at,
        freq,
        dur,
        wave,
        vol: k * BASE_GAIN,
        slide: None,
    }
}

fn noise(dur: f32, k: f32, cutoff: f32) -> Voice {
    Voice::Noise {
        at: 0.0,
        dur,
        vol: k * BASE_GAIN,
        cutoff,
    }
}

/// The sound table of spec 07 §10.2 (`v = 0.07`).
pub fn recipe(s: Sound) -> Vec<Voice> {
    use Wave::*;
    match s {
        Sound::Hit => vec![
            noise(0.09, 2.2, 1800.0),
            tone(180.0, 0.08, Square, 0.6, Some(90.0)),
        ],
        Sound::Block => vec![
            tone(900.0, 0.07, Square, 0.6, Some(500.0)),
            noise(0.05, 1.5, 5000.0),
        ],
        Sound::Shoot => vec![tone(700.0, 0.12, Triangle, 1.0, Some(250.0))],
        Sound::Chop => vec![noise(0.06, 1.6, 900.0)],
        Sound::Coin => vec![
            tone(990.0, 0.07, Square, 0.7, None),
            tone(1480.0, 0.12, Square, 0.6, None),
        ],
        Sound::Heal => vec![tone(520.0, 0.25, Sine, 1.4, Some(1040.0))],
        Sound::Build => vec![
            tone(330.0, 0.1, Triangle, 1.4, None),
            tone(495.0, 0.18, Triangle, 1.2, None),
        ],
        Sound::Death => vec![tone(260.0, 0.22, Sawtooth, 0.7, Some(70.0))],
        Sound::Boom => vec![
            noise(0.5, 3.5, 500.0),
            tone(90.0, 0.4, Sine, 2.5, Some(40.0)),
        ],
        Sound::Click => vec![tone(600.0, 0.05, Square, 0.7, None)],
        Sound::Select => vec![tone(520.0, 0.04, Triangle, 0.9, None)],
        Sound::Ack => vec![
            tone(440.0, 0.05, Triangle, 1.0, None),
            tone(660.0, 0.07, Triangle, 0.8, None),
        ],
        Sound::Research => [660.0, 880.0, 1100.0]
            .iter()
            .enumerate()
            .map(|(i, &f)| tone_at(i as f32 * 0.09, f, 0.16, Triangle, 1.3))
            .collect(),
        Sound::Age => [392.0, 523.0, 659.0, 784.0, 1047.0]
            .iter()
            .enumerate()
            .map(|(i, &f)| tone_at(i as f32 * 0.14, f, 0.35, Sawtooth, 1.1))
            .collect(),
        Sound::Deny => vec![tone(160.0, 0.14, Square, 0.8, Some(110.0))],
        Sound::Horn => vec![
            tone(196.0, 0.5, Sawtooth, 0.9, None),
            tone(294.0, 0.6, Sawtooth, 0.6, None),
        ],
        Sound::Win => [523.0, 659.0, 784.0, 1047.0]
            .iter()
            .enumerate()
            .map(|(i, &f)| tone_at(i as f32 * 0.13, f, 0.3, Triangle, 1.6))
            .collect(),
        Sound::Lose => [392.0, 330.0, 262.0, 196.0]
            .iter()
            .enumerate()
            .map(|(i, &f)| tone_at(i as f32 * 0.18, f, 0.4, Sawtooth, 1.0))
            .collect(),
    }
}

/// Renders a sound at full volume into mono f32 samples (mixed voices, not clipped).
pub fn render(s: Sound, sr: u32, seed: u32) -> Vec<f32> {
    let voices = recipe(s);
    let end = voices
        .iter()
        .map(|v| match *v {
            Voice::Tone { at, dur, .. } | Voice::Noise { at, dur, .. } => at + dur,
        })
        .fold(0.0f32, f32::max);
    let len = (end * sr as f32).ceil() as usize + 1;
    let mut out = vec![0.0f32; len];
    let mut rng = seed | 1;
    for v in voices {
        match v {
            Voice::Tone {
                at,
                freq,
                dur,
                wave,
                vol,
                slide,
            } => add_tone(&mut out, sr, at, freq, dur, wave, vol, slide),
            Voice::Noise {
                at,
                dur,
                vol,
                cutoff,
            } => add_noise(&mut out, sr, at, dur, vol, cutoff, &mut rng),
        }
    }
    out
}

/// Fourier coefficient of harmonic `n` (WebAudio built-in waves, normalised to a peak of ~1).
#[inline]
fn coeff(wave: Wave, n: u32) -> f32 {
    let nf = n as f32;
    match wave {
        Wave::Sine => {
            if n == 1 {
                1.0
            } else {
                0.0
            }
        }
        Wave::Square => {
            if n % 2 == 1 {
                4.0 / (nf * std::f32::consts::PI)
            } else {
                0.0
            }
        }
        Wave::Sawtooth => {
            let s = if n % 2 == 1 { 1.0 } else { -1.0 };
            s * 2.0 / (nf * std::f32::consts::PI)
        }
        Wave::Triangle => {
            // 8 sin(nπ/2) / (nπ)²: +, 0, -, 0 …
            let s = match n % 4 {
                1 => 1.0,
                3 => -1.0,
                _ => 0.0,
            };
            s * 8.0 / (nf * std::f32::consts::PI).powi(2)
        }
    }
}

/// Max harmonics per voice (keeps startup synthesis cheap; inaudible above).
const MAX_HARMONICS: u32 = 160;

#[allow(clippy::too_many_arguments)]
fn add_tone(
    out: &mut [f32],
    sr: u32,
    at: f32,
    freq: f32,
    dur: f32,
    wave: Wave,
    vol: f32,
    slide: Option<f32>,
) {
    if dur <= 0.0 || freq <= 0.0 || vol <= 0.0 {
        return;
    }
    let srf = sr as f32;
    let start = (at * srf).round() as usize;
    let n = (dur * srf) as usize;
    let nyq = srf * 0.5;
    let ratio = slide.map(|t| (t / freq).max(1e-6)).unwrap_or(1.0);
    let floor = 0.0001f32 / vol;
    let coeffs: Vec<f32> = (1..=MAX_HARMONICS).map(|k| coeff(wave, k)).collect();
    let mut phase = 0.0f64;
    for i in 0..n {
        let k = i as f32 / n as f32;
        let f = freq * ratio.powf(k);
        let g = vol * floor.powf(k);
        let harmonics = ((nyq / f) as u32).clamp(1, MAX_HARMONICS);
        // Σ b_h sin(h φ) with the Chebyshev recurrence sin(hφ) = 2cos φ sin((h-1)φ) − sin((h-2)φ)
        let ph = phase as f32;
        let (s1, c1) = ph.sin_cos();
        let two_c = 2.0 * c1;
        let (mut s_prev, mut s_cur) = (0.0f32, s1);
        let mut acc = 0.0f32;
        for h in 1..=harmonics {
            let b = coeffs[(h - 1) as usize];
            if b != 0.0 {
                acc += b * s_cur;
            }
            let next = two_c * s_cur - s_prev;
            s_prev = s_cur;
            s_cur = next;
        }
        if let Some(o) = out.get_mut(start + i) {
            *o += acc * g;
        }
        phase += std::f64::consts::TAU * f as f64 / sr as f64;
        if phase > std::f64::consts::TAU {
            phase -= std::f64::consts::TAU;
        }
    }
}

fn add_noise(out: &mut [f32], sr: u32, at: f32, dur: f32, vol: f32, cutoff: f32, rng: &mut u32) {
    let srf = sr as f32;
    let start = (at * srf).round() as usize;
    let len = ((srf * dur).floor() as usize).max(1);
    // WebAudio lowpass biquad, Q = 1 dB: alpha = sin(w0) / (2·10^(Q/20))
    let w0 = std::f32::consts::TAU * cutoff.min(nyquist_guard(srf)) / srf;
    let (sw, cw) = w0.sin_cos();
    let alpha = sw / (2.0 * 10f32.powf(1.0 / 20.0));
    let a0 = 1.0 + alpha;
    let b0 = (1.0 - cw) / 2.0 / a0;
    let b1 = (1.0 - cw) / a0;
    let b2 = b0;
    let a1 = -2.0 * cw / a0;
    let a2 = (1.0 - alpha) / a0;
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for i in 0..len {
        // xorshift32
        let mut x = *rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *rng = x;
        let r = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let s = r * (1.0 - i as f32 / len as f32);
        let y = b0 * s + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        x2 = x1;
        x1 = s;
        y2 = y1;
        y1 = y;
        if let Some(o) = out.get_mut(start + i) {
            *o += y * vol;
        }
    }
}

#[inline]
fn nyquist_guard(sr: f32) -> f32 {
    sr * 0.49
}

/// Mono 16-bit PCM RIFF/WAVE file (clipped to ±1 like the WebAudio output).
pub fn wav_bytes(samples: &[f32], sr: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&sr.to_le_bytes());
    b.extend_from_slice(&(sr * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_renders_with_expected_length_and_level() {
        for &s in Sound::ALL {
            let pcm = render(s, 22_050, 7);
            assert!(!pcm.is_empty(), "{s:?}");
            let peak = pcm.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.005 && peak < 0.6, "{s:?} peak {peak}");
            assert!(pcm.iter().all(|v| v.is_finite()));
        }
        // research: 3 notes at 0/90/180 ms, 0.16 s each → 0.34 s
        let n = render(Sound::Research, 1000, 1).len();
        assert!((340..=342).contains(&n), "{n}");
        // age: 0.56 + 0.35 = 0.91 s
        let n = render(Sound::Age, 1000, 1).len();
        assert!((910..=912).contains(&n), "{n}");
    }

    #[test]
    fn wav_header() {
        let w = wav_bytes(&[0.0, 1.0, -2.0], 8000);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(i16::from_le_bytes([w[46], w[47]]), 32767);
        assert_eq!(i16::from_le_bytes([w[48], w[49]]), -32767);
    }

    #[test]
    fn falloff() {
        let c = Vec2::new(1000.0, 1000.0);
        assert!((positional_volume(c, c, 1000.0) - 1.0).abs() < 1e-6);
        assert!(positional_volume(Vec2::new(1750.0, 1000.0), c, 1000.0).abs() < 1e-6);
        assert!(positional_volume(Vec2::new(5000.0, 1000.0), c, 1000.0) < 0.0);
    }
}
