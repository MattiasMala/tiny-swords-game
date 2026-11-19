// OWNER: C4 (control) — spec 08-net §B.2 R3-R5 (session token, reconnection, lobby protocol, messages) + addenda
// N1-N3; 06-ui §4-§5 (menu/lobby notes), §13.3 (network notes); 05 §15.1 (solo pause), §15.3 (beforeunload).
//
// The client always speaks `ts_sim::proto` to a `Room`:
//   * `Link::Local`  — a `Room(RoomKind::Solo)` running inside the client, driven at a fixed 20 Hz here
//                      (accumulator, at most 5 ticks per frame). Works natively and on wasm. Paused (no ticks) while
//                      [`SimPaused`] is set (solo pause menu / help, 05 §4.2).
//   * `Link::Remote` — WebSocket to `ws(s)://<location.host>/ws` (binary postcard frames). wasm only; natively it
//                      logs "unsupported" and the menu shows `proto::NO_SERVER`.
// `Link` is a NonSend resource (web-sys sockets are !Send). Other modules never touch it: they write
// `LinkRequest` messages (start solo / connect / close) and `NetOut` (client messages, commands with seq), and read
// `LobbyState`, `mirror::Mirror` and the messages `GameEventMsg`, `CommandResultMsg`, `MatchOverMsg`.
//
// Session behaviour (08 N1):
//   * the token from `Welcome` is stored in `localStorage["tinyswords.session"]`, sent in every `Hello`;
//   * page (re)load with a stored token → a silent rejoin attempt (`LinkRequest::Rejoin`): if the server answers
//     `Init{rejoin}` the client goes straight into the match; otherwise the link is dropped quietly (menu stays);
//   * socket lost in the lobby or in a match → automatic reconnection with backoff (`LobbyState.reconnecting`;
//     the UI (C3) draws the overlay `Riconnessione in corso…`; Esc gives up); dead sockets detected after `DEAD_AFTER` s of silence
//     (pings every `PING_EVERY` s keep traffic flowing, `LobbyState.ping_ms` = last round trip);
//   * `Close{leave: true}` sends `Leave` and clears the token;
//   * wasm: `beforeunload` asks for confirmation while a match is being played (05 §15.3).

use crate::AppState;
use crate::dev::DevArgs;
use crate::mirror::Mirror;
use bevy::prelude::*;
use std::collections::VecDeque;
use ts_sim::PROTOCOL_VERSION;
use ts_sim::cmd::Command;
use ts_sim::defs::{AiLevel, FactionIdx, KingdomColor};
use ts_sim::events::GameEvent;
use ts_sim::proto::{self, ClientMsg, ConnId, LobbyPlayer, ServerMsg};
use ts_sim::room::{Room, RoomKind};

/// Connection id of the client inside its local solo room.
pub const LOCAL_CONN: ConnId = 1;
/// Max sim ticks run per frame by the local room (catch-up cap).
pub const MAX_TICKS_PER_FRAME: u32 = 5;
/// Keep-alive / latency probe period (s) (08 N1: ping every 5 s).
pub const PING_EVERY: f64 = 5.0;
/// Connection considered lost after this long without any server message (s) (08 N1: 15 s).
pub const DEAD_AFTER: f64 = 15.0;
/// Reconnection backoff: first retry after `RETRY_MIN` s, then +1 s per attempt up to `RETRY_MAX`.
pub const RETRY_MIN: f64 = 1.0;
pub const RETRY_MAX: f64 = 5.0;
/// Lobby reconnection gives up after this long (s) and goes back to the menu with `CONNECTION_LOST`.
pub const LOBBY_RETRY_LIMIT: f64 = 20.0;
/// After `Welcome`, how long to wait for a rejoin `Init` before deciding we are not in a running match (s).
pub const INIT_WAIT: f64 = 1.5;

/// Requests to the link layer (from UI / dev CLI).
#[derive(Message, Clone, Debug)]
pub enum LinkRequest {
    /// Start a solo match against the AI in a local room.
    StartSolo { color: KingdomColor, level: AiLevel },
    /// Connect to the server of this page (multiplayer lobby).
    ConnectRemote,
    /// Silent rejoin attempt with the stored session token (page reload): straight into the match if the server
    /// still has our kingdom, otherwise nothing visible happens.
    Rejoin,
    /// Drop the current link (back to menu). Remote: sends `Leave` first if `leave` (and forgets the token).
    Close { leave: bool },
}

/// Outgoing client messages, flushed to the link every frame.
#[derive(Resource, Default, Debug)]
pub struct NetOut {
    queue: Vec<ClientMsg>,
    seq: u32,
}

impl NetOut {
    pub fn send(&mut self, m: ClientMsg) {
        self.queue.push(m);
    }

    /// Queues a player command; returns its `seq` (matched by `CommandResultMsg`).
    pub fn command(&mut self, cmd: Command) -> u32 {
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        self.queue.push(ClientMsg::Command { seq, cmd });
        seq
    }
}

/// Lobby / session state for the menu and lobby screens.
#[derive(Resource, Debug, Clone)]
pub struct LobbyState {
    /// chosen kingdom colour in the menu (`Menu.color`)
    pub color: KingdomColor,
    /// chosen AI level in the menu (`Menu.level`)
    pub level: AiLevel,
    /// player name sent in `Hello`
    pub name: String,
    /// our lobby id (from `Welcome`)
    pub you: Option<u32>,
    pub players: Vec<LobbyPlayer>,
    pub lobby_level: AiLevel,
    pub leader: Option<u32>,
    pub urls: Vec<String>,
    /// network note shown under the menu (`Menu.note`)
    pub note: Option<String>,
    /// current link kind
    pub remote: bool,
    pub connected: bool,
    /// the socket dropped and the client is trying to get back (overlay `proto::RECONNECTING`)
    pub reconnecting: bool,
    /// last ping round trip (ms), remote only
    pub ping_ms: Option<u32>,
}

impl Default for LobbyState {
    fn default() -> Self {
        LobbyState {
            color: KingdomColor::Blue,
            level: AiLevel::Normal,
            name: String::new(),
            you: None,
            players: Vec::new(),
            lobby_level: AiLevel::Normal,
            leader: None,
            urls: Vec::new(),
            note: None,
            remote: false,
            connected: false,
            reconnecting: false,
            ping_ms: None,
        }
    }
}

impl LobbyState {
    pub fn is_leader(&self) -> bool {
        self.you.is_some() && self.you == self.leader
    }
}

/// Solo only: when `true` the local room does not tick (pause menu, help, 05 §4.2). Written by control every
/// frame; ignored by remote links (a network match never stops).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct SimPaused(pub bool);

/// A presentation event from the sim (fx / sound / toast / banner …), for audio, ui and fx.
#[derive(Message, Clone, Debug)]
pub struct GameEventMsg(pub GameEvent);

/// Answer to `NetOut::command` (`err = Some(refusal)` → toast + deny sound, done by control).
#[derive(Message, Clone, Debug)]
pub struct CommandResultMsg {
    pub seq: u32,
    pub err: Option<String>,
}

/// The match ended.
#[derive(Message, Clone, Debug)]
pub struct MatchOverMsg {
    pub winner: Option<FactionIdx>,
}

/// What a link reports when polled.
pub enum LinkEvent {
    Opened,
    Msg(ServerMsg),
    /// socket closed (`opened` = it had been open)
    Closed {
        opened: bool,
    },
}

/// Local solo room.
pub struct LocalLink {
    pub room: Room,
    acc: f32,
    incoming: VecDeque<ServerMsg>,
}

impl LocalLink {
    fn new(seed: Option<u32>) -> LocalLink {
        let mut room = Room::new(RoomKind::Solo, Vec::new());
        room.seed = seed;
        room.add_entropy(entropy());
        room.connect(LOCAL_CONN, "solo".into());
        LocalLink {
            room,
            acc: 0.0,
            incoming: VecDeque::new(),
        }
    }

    fn send(&mut self, m: ClientMsg) {
        self.room.receive(LOCAL_CONN, m);
        self.collect();
    }

    fn collect(&mut self) {
        for (c, m) in self.room.drain_outbox() {
            if c == LOCAL_CONN {
                self.incoming.push_back(m);
            }
        }
        self.room.drain_closed();
    }

    /// Advances the room by `dt` seconds of real time at the fixed tick.
    fn advance(&mut self, dt: f32) {
        self.acc += dt;
        let mut n = 0;
        while self.acc >= ts_sim::TICK_DT && n < MAX_TICKS_PER_FRAME {
            self.acc -= ts_sim::TICK_DT;
            self.room.tick();
            n += 1;
        }
        if self.acc >= ts_sim::TICK_DT {
            // too far behind (slow frame / background tab): drop the backlog instead of fast-forwarding
            self.acc = 0.0;
        }
        self.collect();
    }
}

/// The current connection (NonSend resource).
#[derive(Default)]
pub enum Link {
    #[default]
    Idle,
    Local(Box<LocalLink>),
    Remote(remote::RemoteLink),
}

impl Link {
    pub fn send(&mut self, m: ClientMsg) {
        match self {
            Link::Idle => {}
            Link::Local(l) => l.send(m),
            Link::Remote(r) => r.send(&proto::encode(&m)),
        }
    }

    fn poll(&mut self) -> Vec<LinkEvent> {
        match self {
            Link::Idle => Vec::new(),
            Link::Local(l) => l.incoming.drain(..).map(LinkEvent::Msg).collect(),
            Link::Remote(r) => r.poll(),
        }
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, Link::Remote(_))
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Link::Local(_))
    }
}

/// Session bookkeeping of the remote link (private to this module).
#[derive(Resource, Debug, Default)]
struct Session {
    /// silent rejoin attempt (page reload): no lobby, no notes
    auto: bool,
    /// a `Welcome` was received on this logical session (reconnect only makes sense after it)
    welcomed: bool,
    /// reconnect loop: real time of the next attempt / when it started / attempts so far
    next_try: f64,
    since: f64,
    attempts: u32,
    /// state we were in when the socket dropped (InGame or Lobby)
    resume: Option<AppState>,
    /// waiting for a rejoin `Init` until this time
    init_deadline: Option<f64>,
    /// `Join` scheduled (sent unless an `Init` arrives first)
    join_at: Option<f64>,
    /// last time anything arrived from the server
    last_rx: f64,
    next_ping: f64,
    /// one-shot startup rejoin done
    startup_done: bool,
}

/// The link systems (Update): pump the connection, then everything that reads the mirror runs after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LinkSet;

pub struct LinkPlugin;

impl Plugin for LinkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send(Link::Idle)
            .init_resource::<NetOut>()
            .init_resource::<LobbyState>()
            .init_resource::<SimPaused>()
            .init_resource::<Session>()
            .add_message::<LinkRequest>()
            .add_message::<GameEventMsg>()
            .add_message::<CommandResultMsg>()
            .add_message::<MatchOverMsg>()
            .add_systems(OnEnter(AppState::Menu), startup_rejoin)
            .add_systems(
                Update,
                (
                    handle_requests,
                    flush_out,
                    drive_local,
                    pump,
                    keepalive,
                    reconnect_loop,
                    before_unload_flag,
                )
                    .chain()
                    .in_set(LinkSet),
            );
        web::install_before_unload();
    }
}

/// First time the menu shows: a stored session token means the page was reloaded during a session → try to get
/// back into the match silently (08 N1, 05 §15.3 note).
fn startup_rejoin(
    mut sess: ResMut<Session>,
    dev: Res<DevArgs>,
    mut req: MessageWriter<LinkRequest>,
) {
    if sess.startup_done {
        return;
    }
    sess.startup_done = true;
    if !dev.solo && remote::load_token().is_some() {
        req.write(LinkRequest::Rejoin);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_requests(
    mut reqs: MessageReader<LinkRequest>,
    mut link: NonSendMut<Link>,
    mut lobby: ResMut<LobbyState>,
    mut mirror: ResMut<Mirror>,
    mut sess: ResMut<Session>,
    mut paused: ResMut<SimPaused>,
    dev: Res<DevArgs>,
    time: Res<Time<Real>>,
    mut next: ResMut<NextState<AppState>>,
) {
    let now = time.elapsed_secs_f64();
    for r in reqs.read() {
        match r.clone() {
            LinkRequest::StartSolo { color, level } => {
                *link = Link::Idle;
                *sess = Session {
                    startup_done: true,
                    ..default()
                };
                paused.0 = false;
                let mut l = LocalLink::new(dev.seed);
                lobby.remote = false;
                lobby.connected = true;
                lobby.reconnecting = false;
                lobby.note = None;
                lobby.ping_ms = None;
                let name = lobby.name.clone();
                for m in [
                    ClientMsg::Hello {
                        protocol: PROTOCOL_VERSION,
                        token: None,
                        name,
                    },
                    ClientMsg::Join { color: Some(color) },
                    ClientMsg::SetLevel { level },
                    ClientMsg::Start,
                ] {
                    l.send(m);
                }
                *link = Link::Local(Box::new(l));
            }
            LinkRequest::ConnectRemote | LinkRequest::Rejoin => {
                let auto = matches!(r, LinkRequest::Rejoin);
                match remote::RemoteLink::connect() {
                    Ok(rl) => {
                        *link = Link::Remote(rl);
                        *sess = Session {
                            auto,
                            last_rx: now,
                            next_ping: now + PING_EVERY,
                            startup_done: true,
                            ..default()
                        };
                        lobby.remote = true;
                        lobby.connected = false;
                        lobby.reconnecting = false;
                        lobby.ping_ms = None;
                        if !auto {
                            lobby.note = None;
                            lobby.players.clear();
                            lobby.you = None;
                            next.set(AppState::Lobby);
                        }
                    }
                    Err(e) => {
                        if !auto {
                            warn!("multigiocatore non disponibile: {e}");
                            lobby.note = Some(proto::NO_SERVER.to_string());
                        }
                    }
                }
            }
            LinkRequest::Close { leave } => {
                if leave {
                    link.send(ClientMsg::Leave);
                    remote::clear_token();
                }
                *link = Link::Idle;
                *sess = Session {
                    startup_done: true,
                    ..default()
                };
                paused.0 = false;
                lobby.connected = false;
                lobby.reconnecting = false;
                lobby.ping_ms = None;
                mirror.clear();
                next.set(AppState::Menu);
            }
        }
    }
}

fn flush_out(mut out: ResMut<NetOut>, mut link: NonSendMut<Link>, lobby: Res<LobbyState>) {
    if lobby.reconnecting {
        // nothing reaches the server while the socket is down; commands would apply to a stale view
        out.queue.clear();
        return;
    }
    for m in out.queue.drain(..) {
        link.send(m);
    }
}

fn drive_local(time: Res<Time<Real>>, paused: Res<SimPaused>, mut link: NonSendMut<Link>) {
    if let Link::Local(l) = &mut *link {
        if paused.0 {
            // solo pause / help: nothing advances (05 §4.2); do not accumulate a backlog
            l.acc = 0.0;
            l.collect();
        } else {
            l.advance(time.delta_secs());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn pump(
    mut link: NonSendMut<Link>,
    mut lobby: ResMut<LobbyState>,
    mut mirror: ResMut<Mirror>,
    mut sess: ResMut<Session>,
    time: Res<Time<Real>>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut ev_out: MessageWriter<GameEventMsg>,
    mut res_out: MessageWriter<CommandResultMsg>,
    mut over_out: MessageWriter<MatchOverMsg>,
) {
    let now = time.elapsed_secs_f64();
    let remote = link.is_remote();
    let events = link.poll();
    let mut drop_link: Option<Option<String>> = None;
    let mut lost = false;
    for e in events {
        sess.last_rx = now;
        match e {
            LinkEvent::Opened => {
                lobby.connected = true;
                let m = ClientMsg::Hello {
                    protocol: PROTOCOL_VERSION,
                    token: remote::load_token(),
                    name: lobby.name.clone(),
                };
                link.send(m);
            }
            LinkEvent::Closed { opened } => {
                lobby.connected = false;
                if sess.auto && !sess.welcomed {
                    // silent rejoin failed: stay in the menu without a note
                    drop_link = Some(None);
                } else if lobby.reconnecting {
                    // a failed attempt: the reconnect loop tries again
                    *link = Link::Idle;
                    return;
                } else if opened && sess.welcomed && is_session_state(*state.get()) {
                    lost = true;
                } else {
                    drop_link = Some(Some(
                        if opened {
                            proto::CONNECTION_LOST
                        } else {
                            proto::NO_SERVER
                        }
                        .to_string(),
                    ));
                }
            }
            LinkEvent::Msg(m) => match m {
                ServerMsg::Welcome { you, token, urls } => {
                    lobby.you = Some(you);
                    lobby.urls = urls;
                    if remote {
                        remote::save_token(&token);
                        sess.welcomed = true;
                        if sess.auto || lobby.reconnecting && sess.resume == Some(AppState::InGame) {
                            // reclaim: the server sends Init{rejoin} right after Welcome if we still own a kingdom
                            sess.init_deadline = Some(now + INIT_WAIT);
                        } else {
                            // a stored token may still belong to a running match → give its Init a moment
                            sess.join_at = Some(now + if remote::had_token() { 0.4 } else { 0.0 });
                        }
                    }
                }
                ServerMsg::Lobby {
                    players,
                    level,
                    leader,
                } => {
                    let in_lobby = lobby.you.is_some_and(|y| players.iter().any(|p| p.id == y));
                    lobby.players = players;
                    lobby.lobby_level = level;
                    lobby.leader = leader;
                    if remote && in_lobby {
                        if lobby.reconnecting {
                            end_reconnect(&mut lobby, &mut sess);
                        }
                        if matches!(*state.get(), AppState::Menu | AppState::InGame)
                            && !sess.auto
                        {
                            next.set(AppState::Lobby);
                        }
                    }
                }
                ServerMsg::No { why } => {
                    warn!("server: {why}");
                    if sess.auto {
                        if why == proto::NO_PLAYING || why == proto::SESSION_EXPIRED {
                            // our token belongs to nothing any more
                            remote::clear_token();
                        }
                        drop_link = Some(None);
                    } else if lobby.reconnecting {
                        if why == proto::NO_PLAYING {
                            remote::clear_token();
                            drop_link = Some(Some(proto::SESSION_EXPIRED.to_string()));
                        } else {
                            drop_link = Some(Some(why));
                        }
                    } else if *state.get() != AppState::InGame {
                        lobby.note = Some(why);
                        if remote {
                            drop_link = Some(lobby.note.clone());
                        }
                    }
                }
                ServerMsg::Init {
                    seed,
                    setup,
                    colors,
                    you,
                    rejoin,
                    map_hash,
                } => {
                    sess.init_deadline = None;
                    sess.join_at = None;
                    sess.auto = false;
                    if lobby.reconnecting {
                        end_reconnect(&mut lobby, &mut sess);
                    }
                    if mirror.active && rejoin && mirror.seed == seed && mirror.hash_ok {
                        // same match after a reconnect: keep the regenerated map, reset the dynamic view
                        mirror.view = ts_sim::snapshot_types::ClientMirror::new(
                            you,
                            mirror.view.node_amounts.clone(),
                        );
                        mirror.you = you;
                        mirror.rejoin = true;
                        mirror.colors = colors;
                        mirror.prev_units.clear();
                        mirror.snapshots = 0;
                        mirror.generation = mirror.generation.wrapping_add(1);
                        let _ = (setup, map_hash);
                    } else {
                        mirror.init(seed, setup, colors, you, rejoin, map_hash);
                    }
                    // already InGame (reconnect into the running match): a plain `set` would run OnExit/OnEnter
                    // (DespawnOnExit terrain/water, selection reset) after the renderers already rebuilt for the
                    // new mirror generation → empty world; they follow `Mirror::generation` instead
                    next.set_if_different(AppState::InGame);
                }
                ServerMsg::Snapshot(s) => {
                    if mirror.active {
                        mirror.apply(&s, now);
                        for ev in mirror.view.drain_events() {
                            ev_out.write(GameEventMsg(ev));
                        }
                    }
                }
                ServerMsg::CommandResult { seq, err } => {
                    res_out.write(CommandResultMsg { seq, err });
                }
                ServerMsg::MatchOver { winner } => {
                    mirror.winner = Some(winner);
                    over_out.write(MatchOverMsg { winner });
                }
                ServerMsg::Pong { t } => {
                    let ms = now_ms(now);
                    lobby.ping_ms = Some(ms.wrapping_sub(t));
                }
            },
        }
    }
    // rejoin Init did not come
    if let Some(d) = sess.init_deadline
        && now >= d
    {
        sess.init_deadline = None;
        if sess.auto {
            drop_link = Some(None);
        } else if lobby.reconnecting {
            // our kingdom's match is gone
            drop_link = Some(Some(proto::SESSION_EXPIRED.to_string()));
        }
    }
    if let Some(t) = sess.join_at
        && now >= t
    {
        sess.join_at = None;
        let color = Some(lobby.color);
        link.send(ClientMsg::Join { color });
    }
    if lost {
        begin_reconnect(&mut link, &mut lobby, &mut sess, *state.get(), now);
    }
    if let Some(note) = drop_link {
        let auto = sess.auto;
        *link = Link::Idle;
        *sess = Session {
            startup_done: true,
            ..default()
        };
        lobby.connected = false;
        lobby.reconnecting = false;
        lobby.ping_ms = None;
        if !auto {
            lobby.note = note;
            mirror.clear();
            next.set(AppState::Menu);
        }
    }
}

fn is_session_state(s: AppState) -> bool {
    matches!(s, AppState::InGame | AppState::Lobby)
}

fn begin_reconnect(
    link: &mut Link,
    lobby: &mut LobbyState,
    sess: &mut Session,
    state: AppState,
    now: f64,
) {
    warn!("{}", proto::CONNECTION_LOST);
    *link = Link::Idle;
    lobby.connected = false;
    lobby.reconnecting = true;
    lobby.ping_ms = None;
    sess.resume = Some(state);
    sess.since = now;
    sess.attempts = 0;
    sess.next_try = now + RETRY_MIN;
    sess.init_deadline = None;
    sess.join_at = None;
}

fn end_reconnect(lobby: &mut LobbyState, sess: &mut Session) {
    lobby.reconnecting = false;
    sess.resume = None;
    sess.attempts = 0;
}

/// Pings every `PING_EVERY` s; a remote link silent for `DEAD_AFTER` s is treated as lost.
fn keepalive(
    mut link: NonSendMut<Link>,
    mut lobby: ResMut<LobbyState>,
    mut sess: ResMut<Session>,
    time: Res<Time<Real>>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut mirror: ResMut<Mirror>,
) {
    if !link.is_remote() || !lobby.connected {
        return;
    }
    let now = time.elapsed_secs_f64();
    if now >= sess.next_ping {
        sess.next_ping = now + PING_EVERY;
        link.send(ClientMsg::Ping { t: now_ms(now) });
    }
    if now - sess.last_rx > DEAD_AFTER {
        if sess.welcomed && is_session_state(*state.get()) && !sess.auto {
            begin_reconnect(&mut link, &mut lobby, &mut sess, *state.get(), now);
        } else {
            let auto = sess.auto;
            *link = Link::Idle;
            lobby.connected = false;
            if !auto {
                lobby.note = Some(proto::CONNECTION_LOST.to_string());
                mirror.clear();
                next.set(AppState::Menu);
            }
        }
    }
}

/// While `reconnecting`: open a new socket every few seconds (Hello carries the stored token → the server
/// re-attaches the session / sends `Init{rejoin}`). Esc gives up (back to the menu, token kept for a reload).
#[allow(clippy::too_many_arguments)]
fn reconnect_loop(
    mut link: NonSendMut<Link>,
    mut lobby: ResMut<LobbyState>,
    mut sess: ResMut<Session>,
    mut mirror: ResMut<Mirror>,
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if !lobby.reconnecting {
        return;
    }
    let now = time.elapsed_secs_f64();
    let give_up = keys.just_pressed(KeyCode::Escape)
        || (sess.resume == Some(AppState::Lobby) && now - sess.since > LOBBY_RETRY_LIMIT);
    if give_up {
        *link = Link::Idle;
        *sess = Session {
            startup_done: true,
            ..default()
        };
        lobby.reconnecting = false;
        lobby.connected = false;
        lobby.note = Some(proto::CONNECTION_LOST.to_string());
        mirror.clear();
        next.set(AppState::Menu);
        return;
    }
    if matches!(*link, Link::Idle) && now >= sess.next_try {
        sess.attempts += 1;
        sess.next_try = now + (RETRY_MIN + sess.attempts as f64).min(RETRY_MAX);
        match remote::RemoteLink::connect() {
            Ok(rl) => {
                *link = Link::Remote(rl);
                sess.last_rx = now;
                sess.next_ping = now + PING_EVERY;
            }
            Err(e) => warn!("riconnessione fallita: {e}"),
        }
    }
    // an attempt that never opens: drop it and retry
    if link.is_remote() && !lobby.connected && now - sess.last_rx > RETRY_MAX + 3.0 {
        *link = Link::Idle;
        sess.last_rx = now;
    }
}

/// Keeps the wasm `beforeunload` guard in sync: on while a match is being played (no result yet).
fn before_unload_flag(state: Res<State<AppState>>, mirror: Res<Mirror>) {
    let me_alive = mirror.you.is_some_and(|me| {
        mirror
            .view
            .factions
            .iter()
            .find(|f| f.idx == me)
            .is_none_or(|f| f.alive)
    });
    let playing = *state.get() == AppState::InGame
        && mirror.active
        && mirror.winner.is_none()
        && mirror.view.outcome.is_none()
        && me_alive;
    web::set_guard(playing);
}

/// Real time → ms (u32, wrapping) for ping stamps.
fn now_ms(now: f64) -> u32 {
    ((now * 1000.0) as u64 & 0xffff_ffff) as u32
}

/// Some external randomness for the solo room seed.
fn entropy() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Math::random() * 9.007_199_254_740_991e15) as u64
            ^ ((js_sys::Date::now() as u64) << 20)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x1234_5678)
    }
}

/// Browser page hooks (wasm): `beforeunload` confirmation during a match.
#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::Cell;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;

    thread_local! {
        static GUARD: Cell<bool> = const { Cell::new(false) };
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }

    pub fn set_guard(on: bool) {
        GUARD.with(|g| g.set(on));
    }

    pub fn install_before_unload() {
        if INSTALLED.with(|i| i.replace(true)) {
            return;
        }
        let Some(win) = web_sys::window() else {
            return;
        };
        let cb = Closure::<dyn FnMut(web_sys::BeforeUnloadEvent)>::new(
            move |e: web_sys::BeforeUnloadEvent| {
                if GUARD.with(|g| g.get()) {
                    // Ctrl+W (fast pan up with WASD) would close the tab: ask first (05 §15.3)
                    e.prevent_default();
                    e.set_return_value("");
                }
            },
        );
        let _ = win
            .add_event_listener_with_callback("beforeunload", cb.as_ref().unchecked_ref());
        // lives for the whole page
        cb.forget();
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod web {
    pub fn set_guard(_on: bool) {}
    pub fn install_before_unload() {}
}

/// WebSocket transport + session token storage.
#[cfg(target_arch = "wasm32")]
pub mod remote {
    use super::LinkEvent;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;
    use ts_sim::proto::{self, ServerMsg};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;
    use web_sys::{BinaryType, CloseEvent, Event, MessageEvent, WebSocket};

    enum Raw {
        Open,
        Bin(Vec<u8>),
        Closed,
    }

    pub struct RemoteLink {
        ws: WebSocket,
        queue: Rc<RefCell<VecDeque<Raw>>>,
        open: bool,
        ever_open: bool,
        closed: bool,
        pending: Vec<Vec<u8>>,
        _on_open: Closure<dyn FnMut(Event)>,
        _on_msg: Closure<dyn FnMut(MessageEvent)>,
        _on_close: Closure<dyn FnMut(CloseEvent)>,
        _on_err: Closure<dyn FnMut(Event)>,
    }

    impl RemoteLink {
        /// Opens `ws(s)://<location.host>/ws` (same host the page came from, 08 N4).
        pub fn connect() -> Result<RemoteLink, String> {
            let win = web_sys::window().ok_or("no window")?;
            let loc = win.location();
            let scheme = match loc.protocol().map_err(|e| format!("{e:?}"))?.as_str() {
                "https:" => "wss",
                _ => "ws",
            };
            let host = loc.host().map_err(|e| format!("{e:?}"))?;
            if host.is_empty() {
                return Err("page not served over http".into());
            }
            let url = format!("{scheme}://{host}/ws");
            let ws = WebSocket::new(&url).map_err(|e| format!("{e:?}"))?;
            ws.set_binary_type(BinaryType::Arraybuffer);
            let queue: Rc<RefCell<VecDeque<Raw>>> = Rc::default();
            let q = queue.clone();
            let on_open = Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
                q.borrow_mut().push_back(Raw::Open);
            });
            let q = queue.clone();
            let on_msg = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
                if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                    q.borrow_mut()
                        .push_back(Raw::Bin(js_sys::Uint8Array::new(&buf).to_vec()));
                }
            });
            let q = queue.clone();
            let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_e: CloseEvent| {
                q.borrow_mut().push_back(Raw::Closed);
            });
            let on_err = Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
                bevy::log::warn!("websocket error");
            });
            ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
            ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
            ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
            ws.set_onerror(Some(on_err.as_ref().unchecked_ref()));
            Ok(RemoteLink {
                ws,
                queue,
                open: false,
                ever_open: false,
                closed: false,
                pending: Vec::new(),
                _on_open: on_open,
                _on_msg: on_msg,
                _on_close: on_close,
                _on_err: on_err,
            })
        }

        pub fn send(&mut self, bytes: &[u8]) {
            if self.open {
                let _ = self.ws.send_with_u8_array(bytes);
            } else if !self.closed {
                self.pending.push(bytes.to_vec());
            }
        }

        pub fn poll(&mut self) -> Vec<LinkEvent> {
            let raw: Vec<Raw> = self.queue.borrow_mut().drain(..).collect();
            let mut out = Vec::new();
            for r in raw {
                match r {
                    Raw::Open => {
                        self.open = true;
                        self.ever_open = true;
                        out.push(LinkEvent::Opened);
                        for b in std::mem::take(&mut self.pending) {
                            let _ = self.ws.send_with_u8_array(&b);
                        }
                    }
                    Raw::Bin(b) => match proto::decode::<ServerMsg>(&b) {
                        Ok(m) => out.push(LinkEvent::Msg(m)),
                        Err(e) => bevy::log::warn!("bad server frame: {e}"),
                    },
                    Raw::Closed => {
                        if !self.closed {
                            self.closed = true;
                            self.open = false;
                            out.push(LinkEvent::Closed {
                                opened: self.ever_open,
                            });
                        }
                    }
                }
            }
            out
        }
    }

    impl Drop for RemoteLink {
        fn drop(&mut self) {
            self.ws.set_onopen(None);
            self.ws.set_onmessage(None);
            self.ws.set_onclose(None);
            self.ws.set_onerror(None);
            let _ = self.ws.close();
        }
    }

    thread_local! {
        /// a token was stored when the page loaded (so a `Welcome` may be followed by a rejoin `Init`)
        static HAD: Cell<Option<bool>> = const { Cell::new(None) };
    }

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok().flatten()
    }

    /// Stored session token (`localStorage["tinyswords.session"]`), errors ignored.
    pub fn load_token() -> Option<String> {
        let t = storage()?
            .get_item(proto::SESSION_KEY)
            .ok()
            .flatten()
            .filter(|t| !t.is_empty());
        HAD.with(|h| {
            if h.get().is_none() {
                h.set(Some(t.is_some()));
            }
        });
        t
    }

    /// Whether a token existed before this session's `Welcome`.
    pub fn had_token() -> bool {
        HAD.with(|h| h.get().unwrap_or(false))
    }

    pub fn save_token(token: &str) {
        HAD.with(|h| h.set(Some(true)));
        if let Some(s) = storage() {
            let _ = s.set_item(proto::SESSION_KEY, token);
        }
    }

    pub fn clear_token() {
        HAD.with(|h| h.set(Some(false)));
        if let Some(s) = storage() {
            let _ = s.remove_item(proto::SESSION_KEY);
        }
    }
}

/// Native: no WebSocket client (multiplayer is browser-only); tokens are not persisted.
#[cfg(not(target_arch = "wasm32"))]
pub mod remote {
    use super::LinkEvent;

    pub struct RemoteLink;

    impl RemoteLink {
        pub fn connect() -> Result<RemoteLink, String> {
            Err("unsupported: the native client has no WebSocket link (use the browser)".into())
        }
        pub fn send(&mut self, _bytes: &[u8]) {}
        pub fn poll(&mut self) -> Vec<LinkEvent> {
            Vec::new()
        }
    }

    pub fn load_token() -> Option<String> {
        None
    }
    pub fn had_token() -> bool {
        false
    }
    pub fn save_token(_token: &str) {}
    pub fn clear_token() {}
}
