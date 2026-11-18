// OWNER: C4 (control) — spec 08-net §B.2 R3-R5 (session token, reconnection, lobby protocol, messages) + addenda
// N1-N3; 06-ui §4-§5 (menu/lobby notes), §13.3 (network notes).
//
// The client always speaks `ts_sim::proto` to a `Room`:
//   * `Link::Local`  — a `Room(RoomKind::Solo)` running inside the client, driven at a fixed 20 Hz here
//                      (accumulator, at most 5 ticks per frame). Works natively and on wasm.
//   * `Link::Remote` — WebSocket to `ws(s)://<location.host>/ws` (binary postcard frames). wasm only; natively it
//                      logs "unsupported" and the menu shows `proto::NO_SERVER`.
// `Link` is a NonSend resource (web-sys sockets are !Send). Other modules never touch it: they write
// `LinkRequest` messages (start solo / connect / close) and `NetOut` (client messages, commands with seq), and read
// `LobbyState`, `mirror::Mirror` and the messages `GameEventMsg`, `CommandResultMsg`, `MatchOverMsg`.
//
// C4 TODO: reconnect loop with `proto::RECONNECTING` overlay (08 N1), ping/latency, rejoin UX, beforeunload, toasts
// for `CommandResult.err`, leave on back button (`ClientMsg::Leave` clears the stored token).

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

/// Requests to the link layer (from UI / dev CLI).
#[derive(Message, Clone, Debug)]
pub enum LinkRequest {
    /// Start a solo match against the AI in a local room.
    StartSolo { color: KingdomColor, level: AiLevel },
    /// Connect to the server of this page (multiplayer lobby).
    ConnectRemote,
    /// Drop the current link (back to menu). Remote: sends `Leave` first if `leave`.
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
        }
    }
}

impl LobbyState {
    pub fn is_leader(&self) -> bool {
        self.you.is_some() && self.you == self.leader
    }
}

/// A presentation event from the sim (fx / sound / toast / banner …), for audio, ui and fx.
#[derive(Message, Clone, Debug)]
pub struct GameEventMsg(pub GameEvent);

/// Answer to `NetOut::command` (`err = Some(refusal)` → toast + deny sound).
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
}

pub struct LinkPlugin;

impl Plugin for LinkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send(Link::Idle)
            .init_resource::<NetOut>()
            .init_resource::<LobbyState>()
            .add_message::<LinkRequest>()
            .add_message::<GameEventMsg>()
            .add_message::<CommandResultMsg>()
            .add_message::<MatchOverMsg>()
            .add_systems(
                Update,
                (handle_requests, flush_out, drive_local, pump).chain(),
            );
    }
}

fn handle_requests(
    mut reqs: MessageReader<LinkRequest>,
    mut link: NonSendMut<Link>,
    mut lobby: ResMut<LobbyState>,
    mut mirror: ResMut<Mirror>,
    dev: Res<DevArgs>,
    mut next: ResMut<NextState<AppState>>,
) {
    for r in reqs.read() {
        match r.clone() {
            LinkRequest::StartSolo { color, level } => {
                let mut l = LocalLink::new(dev.seed);
                lobby.remote = false;
                lobby.connected = true;
                lobby.note = None;
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
            LinkRequest::ConnectRemote => match remote::RemoteLink::connect() {
                Ok(r) => {
                    *link = Link::Remote(r);
                    lobby.remote = true;
                    lobby.connected = false;
                    lobby.note = None;
                    lobby.players.clear();
                    lobby.you = None;
                    next.set(AppState::Lobby);
                }
                Err(e) => {
                    warn!("multigiocatore non disponibile: {e}");
                    lobby.note = Some(proto::NO_SERVER.to_string());
                }
            },
            LinkRequest::Close { leave } => {
                if leave {
                    link.send(ClientMsg::Leave);
                    remote::clear_token();
                }
                *link = Link::Idle;
                lobby.connected = false;
                mirror.clear();
                next.set(AppState::Menu);
            }
        }
    }
}

fn flush_out(mut out: ResMut<NetOut>, mut link: NonSendMut<Link>) {
    for m in out.queue.drain(..) {
        link.send(m);
    }
}

fn drive_local(time: Res<Time<Real>>, mut link: NonSendMut<Link>) {
    if let Link::Local(l) = &mut *link {
        l.advance(time.delta_secs());
    }
}

#[allow(clippy::too_many_arguments)]
fn pump(
    mut link: NonSendMut<Link>,
    mut lobby: ResMut<LobbyState>,
    mut mirror: ResMut<Mirror>,
    time: Res<Time<Real>>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut ev_out: MessageWriter<GameEventMsg>,
    mut res_out: MessageWriter<CommandResultMsg>,
    mut over_out: MessageWriter<MatchOverMsg>,
) {
    let events = link.poll();
    if events.is_empty() {
        return;
    }
    let now = time.elapsed_secs_f64();
    let remote = link.is_remote();
    let mut want_join = false;
    let mut got_init = false;
    let mut drop_link = false;
    for e in events {
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
                lobby.note = Some(
                    if opened {
                        proto::CONNECTION_LOST
                    } else {
                        proto::NO_SERVER
                    }
                    .to_string(),
                );
                drop_link = true;
            }
            LinkEvent::Msg(m) => match m {
                ServerMsg::Welcome { you, token, urls } => {
                    lobby.you = Some(you);
                    lobby.urls = urls;
                    if remote {
                        remote::save_token(&token);
                        want_join = true;
                    }
                }
                ServerMsg::Lobby {
                    players,
                    level,
                    leader,
                } => {
                    lobby.players = players;
                    lobby.lobby_level = level;
                    lobby.leader = leader;
                    if remote && *state.get() == AppState::Menu {
                        next.set(AppState::Lobby);
                    }
                }
                ServerMsg::No { why } => {
                    warn!("server: {why}");
                    if *state.get() != AppState::InGame {
                        lobby.note = Some(why);
                        if remote {
                            drop_link = true;
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
                    got_init = true;
                    mirror.init(seed, setup, colors, you, rejoin, map_hash);
                    next.set(AppState::InGame);
                }
                ServerMsg::Snapshot(s) => {
                    mirror.apply(&s, now);
                    for ev in mirror.view.drain_events() {
                        ev_out.write(GameEventMsg(ev));
                    }
                }
                ServerMsg::CommandResult { seq, err } => {
                    res_out.write(CommandResultMsg { seq, err });
                }
                ServerMsg::MatchOver { winner } => {
                    mirror.winner = Some(winner);
                    over_out.write(MatchOverMsg { winner });
                }
                ServerMsg::Pong { .. } => {}
            },
        }
    }
    if want_join && !got_init {
        let color = Some(lobby.color);
        link.send(ClientMsg::Join { color });
    }
    if drop_link {
        *link = Link::Idle;
        mirror.clear();
        next.set(AppState::Menu);
    }
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

/// WebSocket transport + session token storage.
#[cfg(target_arch = "wasm32")]
pub mod remote {
    use super::LinkEvent;
    use std::cell::RefCell;
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
        /// Opens `ws(s)://<location.host>/ws`.
        pub fn connect() -> Result<RemoteLink, String> {
            let win = web_sys::window().ok_or("no window")?;
            let loc = win.location();
            let scheme = match loc.protocol().map_err(|e| format!("{e:?}"))?.as_str() {
                "https:" => "wss",
                _ => "ws",
            };
            let host = loc.host().map_err(|e| format!("{e:?}"))?;
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

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok().flatten()
    }

    /// Stored session token (`localStorage["tinyswords.session"]`), errors ignored.
    pub fn load_token() -> Option<String> {
        storage()?.get_item(proto::SESSION_KEY).ok().flatten()
    }

    pub fn save_token(token: &str) {
        if let Some(s) = storage() {
            let _ = s.set_item(proto::SESSION_KEY, token);
        }
    }

    pub fn clear_token() {
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
    pub fn save_token(_token: &str) {}
    pub fn clear_token() {}
}
