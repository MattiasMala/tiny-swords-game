// OWNER: C2 (sprites) — sprite FX (`GameEvent::Fx`, `GameEvent::Prop`), floating "+N" (`GameEvent::Float`), spec
// 03a/03b FX names, 01 (Particle FX sheets), 07 (lights of fires/explosions).
//
// SKELETON: reads `GameEventMsg` and counts FX events (debug log). C2 TODO: billboard FX instances, floats, props.

use crate::link::GameEventMsg;
use bevy::prelude::*;
use ts_sim::events::GameEvent;

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, consume_fx);
    }
}

fn consume_fx(mut events: MessageReader<GameEventMsg>, mut n: Local<u64>) {
    for GameEventMsg(e) in events.read() {
        if matches!(e, GameEvent::Fx { .. } | GameEvent::Prop { .. }) {
            *n += 1;
            trace!("fx #{}: {:?}", *n, e);
        }
    }
}
