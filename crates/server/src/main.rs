// OWNER: S6 (net) — spec 08-net.md §A.2.1 (CLI, bind, printed addresses, per-connection logs), §B.2 R1 (static files,
// /ws on the same port, application/wasm, no-cache, Origin check 403 `Origine non ammessa`, 400 `WebSocket
// richiesto`), R2-R7 (one authoritative Room, tokens, snapshots) + addenda N1-N4.
//
// SKELETON (foundation):
//   ts-server [porta] [--bind|-b INDIRIZZO]        defaults 8000 and 0.0.0.0
// * `/`        → `web/` (index.html, pkg/*.js, pkg/*.wasm as application/wasm), `Cache-Control: no-cache`, gzip/br
// * `/assets`  → `assets/`;  `/fonts` → `fonts/` (the client embeds Fredoka; served anyway for tools/pages)
// * `/ws`      → binary WebSocket, postcard `ts_sim::proto` (ClientMsg in, ServerMsg out)
// * one shared lobby `Room` owned by a dedicated task (room_task.rs) ticking at 20 Hz
//   (`MissedTickBehavior::Skip`); sockets talk to it through mpsc channels.
// Repo root: `--root DIR`, else the current dir if it has `web/` and `assets/`, else the source checkout
// (CARGO_MANIFEST_DIR/../..).
//
// S6 TODO: per-connection backpressure (latest-wins snapshots), rate limiting, graceful `SERVER_CLOSED` notice,
// logging roles (`+ {who} {id} da {ip}` with lobby roles), IPv6 LAN urls.

mod lan;
mod room_task;

use axum::{
    Router,
    body::Body,
    extract::{
        ConnectInfo, State,
        ws::{Message, WebSocket, WebSocketUpgrade, rejection::WebSocketUpgradeRejection},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use room_task::{Out, RoomEvent};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
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

#[tokio::main]
async fn main() {
    let args = parse_args();
    let root = repo_root(args.root);
    let urls = lan::lan_urls(&args.bind, args.port);

    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(room_task::run(rx, urls.clone()));

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
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        println!();
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
    ws.on_upgrade(move |socket| client(socket, peer, ctx))
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
    if ctx
        .room
        .send(RoomEvent::Connect {
            conn: id,
            token: new_token(),
            out: out_tx,
        })
        .is_err()
    {
        return;
    }
    let writer = tokio::spawn(async move {
        while let Some(o) = out_rx.recv().await {
            match o {
                Out::Bin(b) => {
                    if sink.send(Message::Binary(b.into())).await.is_err() {
                        break;
                    }
                }
                Out::Close => {
                    let _ = sink.close().await;
                    break;
                }
            }
        }
    });
    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            // malformed frames are dropped (08 R7)
            Message::Binary(b) => {
                if let Ok(m) = proto::decode::<ClientMsg>(&b)
                    && ctx.room.send(RoomEvent::Msg { conn: id, msg: m }).is_err()
                {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    let _ = ctx.room.send(RoomEvent::Disconnect { conn: id });
    writer.abort();
    println!("- giocatore {id}");
}
