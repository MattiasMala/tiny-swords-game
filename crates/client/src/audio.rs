// OWNER: C4 (control) — spec 07-light-sfx.md (procedural SFX `Sfx`, positional falloff, UI sounds, horn alert, audio
// toggle), docs/bevy-0.20-notes.md §18 (AudioPlayer, PlaybackSettings, browser needs a user gesture).
//
// SKELETON: consumes `GameEventMsg` sound events (`Sound` positional / `Play` private) and only traces them.
// C4 TODO: synthesize the spec 07 sounds into `AudioSource`s (WAV bytes) at startup, distance falloff from the
// camera focus, `Sfx.enabled` toggle (menu audio button).

use crate::link::GameEventMsg;
use bevy::prelude::*;
use ts_sim::events::GameEvent;

/// Audio on/off (`Sfx.enabled`).
#[derive(Resource, Debug, Clone, Copy)]
pub struct AudioSettings {
    pub enabled: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings { enabled: true }
    }
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioSettings>()
            .add_systems(Update, consume_sounds);
    }
}

fn consume_sounds(mut events: MessageReader<GameEventMsg>, settings: Res<AudioSettings>) {
    for GameEventMsg(e) in events.read() {
        match e {
            GameEvent::Sound { sound, x, y } if settings.enabled => {
                trace!("sound {sound:?} at {x},{y}");
            }
            GameEvent::Play { sound, .. } if settings.enabled => {
                trace!("ui sound {sound:?}");
            }
            _ => {}
        }
    }
}
