// OWNER: S6 (net) — the task that owns the shared lobby `Room` (spec 08-net §B.2 R2: authoritative fixed tick, R6
// backpressure, R7 robustness).
//
// Every socket task sends `RoomEvent`s here; the room answers through a per-connection `ConnHandle`: one ordered
// unbounded queue of encoded frames plus two counters shared with the socket writer:
//   * `snaps` = snapshots queued and not yet written. Before every tick the room is told which sockets still have
//     one (`Room::set_congested`): those get no new snapshot (latest-wins: the next one is built from the newest
//     state, deltas stay consistent, private events are held back by the room). The room task never blocks.
//   * `bytes` = bytes queued; above `MAX_QUEUED_BYTES` the client is too slow even for control messages and is
//     dropped (it reconnects with its token and gets a full snapshot).
// The room ticks at 20 Hz (`TICK_DT`, `MissedTickBehavior::Skip`), also in the lobby (lobby timers); the outbox is
// flushed after every event and every tick so lobby replies are immediate.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use ts_sim::proto::{self, ClientMsg, ConnId, ServerMsg};
use ts_sim::room::{Room, RoomKind};

/// Queued bytes above which a connection is considered dead (slow consumer).
pub const MAX_QUEUED_BYTES: usize = 4 << 20;

/// To a socket writer.
pub enum Out {
    /// control message (lobby, init, command result, …)
    Bin(Vec<u8>),
    /// snapshot (counted in `ConnHandle::snaps`)
    Snap(Vec<u8>),
    /// flush and close
    Close,
}

impl Out {
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        match self {
            Out::Bin(b) | Out::Snap(b) => b.len(),
            Out::Close => 0,
        }
    }
}

/// The room's handle on one socket writer.
#[derive(Clone)]
pub struct ConnHandle {
    pub tx: mpsc::UnboundedSender<Out>,
    /// snapshots queued, not written yet
    pub snaps: Arc<AtomicUsize>,
    /// bytes queued, not written yet
    pub bytes: Arc<AtomicUsize>,
    /// set by the room when it drops the connection (slow consumer): the writer stops at once
    pub kill: Arc<AtomicBool>,
}

impl ConnHandle {
    pub fn new(tx: mpsc::UnboundedSender<Out>) -> ConnHandle {
        ConnHandle {
            tx,
            snaps: Arc::new(AtomicUsize::new(0)),
            bytes: Arc::new(AtomicUsize::new(0)),
            kill: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Queue a frame; `false` if the writer is gone or the queue overflowed.
    fn push(&self, o: Out) -> bool {
        let n = o.len();
        if self.bytes.load(Ordering::Relaxed) + n > MAX_QUEUED_BYTES {
            return false;
        }
        let snap = matches!(o, Out::Snap(_));
        if snap {
            self.snaps.fetch_add(1, Ordering::Relaxed);
        }
        self.bytes.fetch_add(n, Ordering::Relaxed);
        if self.tx.send(o).is_err() {
            if snap {
                self.snaps.fetch_sub(1, Ordering::Relaxed);
            }
            self.bytes.fetch_sub(n, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// The room dropped this connection: the writer must stop without flushing.
    pub fn killed(&self) -> bool {
        self.kill.load(Ordering::Relaxed)
    }

    /// Writer side: a frame of `len` bytes was written (or dropped); `snap` = it was a snapshot.
    pub fn written(&self, snap: bool, len: usize) {
        if snap {
            self.snaps.fetch_sub(1, Ordering::Relaxed);
        }
        self.bytes.fetch_sub(len, Ordering::Relaxed);
    }
}

/// From a socket task.
pub enum RoomEvent {
    Connect {
        conn: ConnId,
        token: String,
        out: ConnHandle,
    },
    Msg {
        conn: ConnId,
        msg: ClientMsg,
    },
    Disconnect {
        conn: ConnId,
    },
    /// Ctrl+C: tell everybody (`Il server ha chiuso la partita.`) and close.
    Shutdown {
        done: tokio::sync::oneshot::Sender<()>,
    },
}

pub async fn run(mut rx: mpsc::UnboundedReceiver<RoomEvent>, urls: Vec<String>) {
    let mut room = Room::new(RoomKind::Lobby, urls);
    let mut seed = [0u8; 8];
    let _ = getrandom::fill(&mut seed);
    room.add_entropy(u64::from_le_bytes(seed));
    let mut conns: HashMap<ConnId, ConnHandle> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_secs_f32(ts_sim::TICK_DT));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut stats = SnapStats::default();
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
                    RoomEvent::Shutdown { done } => {
                        let bye = proto::encode(&ServerMsg::No { why: proto::SERVER_CLOSED.to_string() });
                        for h in conns.values() {
                            h.push(Out::Bin(bye.clone()));
                            h.push(Out::Close);
                        }
                        let _ = done.send(());
                        break;
                    }
                }
            }
            _ = tick.tick() => {
                for (c, h) in &conns {
                    room.set_congested(*c, h.snaps.load(Ordering::Relaxed) > 0);
                }
                room.tick();
            }
        }
        flush(&mut room, &mut conns, &mut stats);
    }
}

/// Snapshot traffic statistics, logged every minute while a match runs.
#[derive(Default)]
struct SnapStats {
    n: u64,
    bytes: u64,
    max: usize,
}

fn flush(room: &mut Room, conns: &mut HashMap<ConnId, ConnHandle>, stats: &mut SnapStats) {
    let mut dead = Vec::new();
    for (c, m) in room.drain_outbox() {
        let Some(h) = conns.get(&c) else { continue };
        let snap = matches!(m, ServerMsg::Snapshot(_));
        let bytes = proto::encode(&m);
        if snap {
            stats.n += 1;
            stats.bytes += bytes.len() as u64;
            stats.max = stats.max.max(bytes.len());
        }
        let ok = h.push(if snap { Out::Snap(bytes) } else { Out::Bin(bytes) });
        if !ok {
            dead.push(c);
        }
    }
    for c in room.drain_closed() {
        if let Some(h) = conns.remove(&c) {
            h.push(Out::Close);
        }
    }
    for c in dead {
        if let Some(h) = conns.remove(&c) {
            println!("giocatore {c} troppo lento: connessione chiusa");
            h.kill.store(true, Ordering::Relaxed);
            let _ = h.tx.send(Out::Close);
            room.disconnect(c);
        }
    }
    for line in room.drain_log() {
        println!("{line}");
    }
    if stats.n >= 20 * 60 * conns.len().max(1) as u64 {
        println!(
            "istantanee: {} inviate, media {} byte, massimo {} byte",
            stats.n,
            stats.bytes / stats.n.max(1),
            stats.max
        );
        *stats = SnapStats::default();
    }
}
