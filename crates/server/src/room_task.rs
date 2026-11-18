// OWNER: S6 (net) — the task that owns the shared lobby `Room` (spec 08-net §B.2 R2: authoritative fixed tick).
//
// Every socket task sends `RoomEvent`s here; the room answers through per-connection unbounded channels of `Out`.
// The room ticks at 20 Hz (`TICK_DT`) with `MissedTickBehavior::Skip`; the outbox is flushed after every event and
// every tick so lobby replies are immediate.

use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use ts_sim::proto::{self, ClientMsg, ConnId};
use ts_sim::room::{Room, RoomKind};

/// To a socket writer.
pub enum Out {
    Bin(Vec<u8>),
    Close,
}

/// From a socket task.
pub enum RoomEvent {
    Connect {
        conn: ConnId,
        token: String,
        out: mpsc::UnboundedSender<Out>,
    },
    Msg {
        conn: ConnId,
        msg: ClientMsg,
    },
    Disconnect {
        conn: ConnId,
    },
}

pub async fn run(mut rx: mpsc::UnboundedReceiver<RoomEvent>, urls: Vec<String>) {
    let mut room = Room::new(RoomKind::Lobby, urls);
    let mut seed = [0u8; 8];
    let _ = getrandom::fill(&mut seed);
    room.add_entropy(u64::from_le_bytes(seed));
    let mut conns: HashMap<ConnId, mpsc::UnboundedSender<Out>> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_secs_f32(ts_sim::TICK_DT));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            ev = rx.recv() => {
                let Some(ev) = ev else { break };
                match ev {
                    RoomEvent::Connect { conn, token, out } => {
                        conns.insert(conn, out);
                        room.connect(conn, token);
                    }
                    RoomEvent::Msg { conn, msg } => room.receive(conn, msg),
                    RoomEvent::Disconnect { conn } => {
                        conns.remove(&conn);
                        room.disconnect(conn);
                    }
                }
            }
            _ = tick.tick() => room.tick(),
        }
        flush(&mut room, &mut conns);
    }
}

fn flush(room: &mut Room, conns: &mut HashMap<ConnId, mpsc::UnboundedSender<Out>>) {
    for (c, m) in room.drain_outbox() {
        if let Some(tx) = conns.get(&c) {
            let _ = tx.send(Out::Bin(proto::encode(&m)));
        }
    }
    for c in room.drain_closed() {
        if let Some(tx) = conns.remove(&c) {
            let _ = tx.send(Out::Close);
        }
    }
}
