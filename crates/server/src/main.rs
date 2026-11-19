// OWNER: S6 (net) — spec 08-net.md §A.2.1 (CLI, bind, printed addresses, per-connection logs), §B.2 R1 (static files,
// /ws on the same port, application/wasm, no-cache, Origin check 403 `Origine non ammessa`, 400 `WebSocket
// richiesto`), R2-R7 (one authoritative Room, tokens, snapshots) + addenda N1-N4.
//
//   ts-server [porta] [--bind|-b INDIRIZZO]        defaults 8000 and 0.0.0.0 (always reachable on the LAN)
// * `/`        → `web/` (index.html, pkg/*.js, pkg/*.wasm as application/wasm), `Cache-Control: no-cache`, gzip/br
// * `/assets`  → `assets/`;  `/fonts` → `fonts/` (the client embeds Fredoka; served anyway for tools/pages)
// * `/ws`      → binary WebSocket, postcard `ts_sim::proto` (ClientMsg in, ServerMsg out)
// * one shared lobby `Room` owned by a dedicated task (room_task.rs) ticking at 20 Hz
//   (`MissedTickBehavior::Skip`); sockets talk to it through mpsc channels; per-connection ordered outbound queue
//   with latest-wins snapshots (the room never blocks on a socket).
// * sockets: client messages ≤ `MAX_CLIENT_MESSAGE` (64 KiB), WS ping every 5 s, dropped after 15 s of silence,
//   more than `MAX_MSGS_PER_SEC` frames in a second → dropped; commands are also rate-limited by the room.
// * Ctrl+C: every client gets `No{Il server ha chiuso la partita.}`, then the process exits.
// Repo root: `--root DIR`, else the current dir if it has `web/` and `assets/`, else the source checkout
// (CARGO_MANIFEST_DIR/../..).

mod lan;
mod room_task;

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{
        ConnectInfo, State,
        ws::{Message, WebSocket, WebSocketUpgrade, rejection::WebSocketUpgradeRejection},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use room_task::{ConnHandle, Out, RoomEvent};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tower_http::{
    compression::CompressionLayer, services::ServeDir, set_header::SetResponseHeaderLayer,
};
use ts_sim::proto::{self, ClientMsg};

struct Args {
    port: u16,
    bind: String,
    root: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut a = Args {
        port: 8000,
        bind: "0.0.0.0".to_string(),
        root: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(s) = it.next() {
        match s.as_str() {
            "--bind" | "-b" => {
                if let Some(v) = it.next() {
                    a.bind = v;
                }
            }
            "--root" => a.root = it.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("Tiny Swords: gioco e server per il multigiocatore");
                println!("uso: ts-server [porta] [--bind|-b INDIRIZZO] [--root CARTELLA]");
                std::process::exit(0);
            }
            other => match other.parse::<u16>() {
                Ok(p) => a.port = p,
                Err(_) => {
                    eprintln!("argomento non valido: {other}");
                    std::process::exit(2);
                }
            },
        }
    }
    a
}

fn repo_root(arg: Option<PathBuf>) -> PathBuf {
    if let Some(r) = arg {
        return r;
    }
    if let Ok(cwd) = std::env::current_dir()
        && cwd.join("web").is_dir()
        && cwd.join("assets").is_dir()
    {
        return cwd;
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[derive(Clone)]
struct AppCtx {
    room: mpsc::UnboundedSender<RoomEvent>,
}

static NEXT_CONN: AtomicU32 = AtomicU32::new(1);

/// Max size of one client → server message (08 R7).
const MAX_CLIENT_MESSAGE: usize = 64 << 10;
/// WS ping period.
const PING_EVERY: Duration = Duration::from_secs(5);
/// Connection considered lost after this long without any frame from the client (08 N1).
const SILENCE_TIMEOUT: Duration = Duration::from_secs(15);
/// Frames per second above which a client is disconnected (flood).
const MAX_MSGS_PER_SEC: u32 = 300;

#[tokio::main]
async fn main() {
    let args = parse_args();
    let root = repo_root(args.root);
    let urls = lan::lan_urls(&args.bind, args.port);

    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(room_task::run(rx, urls.clone()));
    let room_tx = tx.clone();

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .nest_service("/assets", ServeDir::new(root.join("assets")))
        .nest_service("/fonts", ServeDir::new(root.join("fonts")))
        .fallback_service(ServeDir::new(root.join("web")))
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
        .with_state(AppCtx { room: tx });

    let addr = format!("{}:{}", args.bind, args.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Impossibile ascoltare su {addr}: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "Tiny Swords su http://localhost:{}  (Ctrl+C per fermare)",
        args.port
    );
    for u in &urls {
        println!("Gli altri giocatori aprono: {u}");
    }
    let shutdown = async move {
        let _ = tokio::signal::ctrl_c().await;
        // tell the players, give the writers a moment to flush, then exit (upgraded sockets would keep the
        // graceful shutdown waiting)
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        if room_tx.send(RoomEvent::Shutdown { done: done_tx }).is_ok() {
            let _ = tokio::time::timeout(Duration::from_secs(1), done_rx).await;
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        println!();
        std::process::exit(0);
    };
    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
    {
        eprintln!("errore del server: {e}");
    }
}

fn plain(status: StatusCode, text: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(text))
        .unwrap_or_default()
}

async fn ws_handler(
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(ctx): State<AppCtx>,
) -> Response {
    // Origin check (08 §A.2): only pages served by this same host may connect; a missing Origin is allowed.
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let o = origin.rsplit("://").next().unwrap_or(origin);
        if o != host {
            return plain(StatusCode::FORBIDDEN, "Origine non ammessa");
        }
    }
    let Ok(ws) = ws else {
        return plain(StatusCode::BAD_REQUEST, "WebSocket richiesto");
    };
    ws.max_message_size(MAX_CLIENT_MESSAGE)
        .max_frame_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| client(socket, peer, ctx))
        .into_response()
}

/// Random 128-bit session token, 32 hex chars.
fn new_token() -> String {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        b = t.to_le_bytes();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

async fn client(socket: WebSocket, peer: SocketAddr, ctx: AppCtx) {
    let id = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    println!("+ giocatore {id} da {}", peer.ip());
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Out>();
    let handle = ConnHandle::new(out_tx);
    if ctx
        .room
        .send(RoomEvent::Connect {
            conn: id,
            token: new_token(),
            out: handle.clone(),
        })
        .is_err()
    {
        return;
    }
    // writer: the room's ordered queue + a ping every PING_EVERY
    let mut writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval(PING_EVERY);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ping.tick().await;
        loop {
            tokio::select! {
                o = out_rx.recv() => {
                    let Some(o) = o else { break };
                    let (snap, len) = (matches!(o, Out::Snap(_)), o.len());
                    let res = match o {
                        Out::Bin(b) | Out::Snap(b) => sink.send(Message::Binary(b.into())).await,
                        Out::Close => {
                            let _ = sink.close().await;
                            break;
                        }
                    };
                    handle.written(snap, len);
                    if res.is_err() {
                        break;
                    }
                }
                _ = ping.tick() => {
                    if sink.send(Message::Ping(Bytes::from_static(b"ts"))).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    // reader: any frame (pong included) proves the client alive
    let room = ctx.room.clone();
    let reader = async move {
        let mut window = tokio::time::Instant::now();
        let mut count = 0u32;
        loop {
            let msg = match tokio::time::timeout(SILENCE_TIMEOUT, stream.next()).await {
                Ok(Some(Ok(m))) => m,
                // silence, socket error, stream end
                _ => break,
            };
            let now = tokio::time::Instant::now();
            if now.duration_since(window) >= Duration::from_secs(1) {
                window = now;
                count = 0;
            }
            count += 1;
            if count > MAX_MSGS_PER_SEC {
                println!("giocatore {id}: troppi messaggi, connessione chiusa");
                break;
            }
            match msg {
                // malformed frames are dropped (08 R7)
                Message::Binary(b) => {
                    if let Ok(m) = proto::decode::<ClientMsg>(&b)
                        && room.send(RoomEvent::Msg { conn: id, msg: m }).is_err()
                    {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    };
    tokio::select! {
        _ = reader => {}
        _ = &mut writer => {}
    }
    let _ = ctx.room.send(RoomEvent::Disconnect { conn: id });
    writer.abort();
    println!("- giocatore {id}");
}
