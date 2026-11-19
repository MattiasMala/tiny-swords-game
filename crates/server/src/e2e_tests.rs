// OWNER: S6 (net) — end-to-end test of the real server: the axum router + the room task, driven by real WebSocket
// clients (tokio-tungstenite). Covers the reconnect bug fix (08 R3 / addenda N1): join + start, one player's socket
// closes (AI takes over, the other player sees it), the same token reclaims the kingdom (Init rejoin + full
// snapshot), a second socket with the same token replaces the first, commands work again; plus the HTTP refusals
// `WebSocket richiesto` (400) and `Origine non ammessa` (403).

use super::{repo_root, room_task, router};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{self, Message, client::IntoClientRequest};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use ts_sim::cmd::Command;
use ts_sim::defs::{FactionIdx, KingdomColor};
use ts_sim::proto::{self, ClientMsg, ServerMsg};
use ts_sim::snapshot_types::{Control, Snapshot};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Map generation in a debug build can take a while.
const SLOW: Duration = Duration::from_secs(120);
const FAST: Duration = Duration::from_secs(20);

async fn start_server() -> SocketAddr {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(room_task::run(rx, vec!["http://test:8000".into()]));
    let app = router(&repo_root(None), tx);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    addr
}

async fn open(addr: SocketAddr) -> Ws {
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("websocket handshake");
    ws
}

async fn send(ws: &mut Ws, m: ClientMsg) {
    ws.send(Message::Binary(proto::encode(&m).into()))
        .await
        .expect("send");
}

async fn hello(ws: &mut Ws, token: Option<&str>, name: &str) {
    send(
        ws,
        ClientMsg::Hello {
            protocol: ts_sim::PROTOCOL_VERSION,
            token: token.map(str::to_string),
            name: name.into(),
        },
    )
    .await;
}

/// Next server message (`None` = socket closed).
async fn next_msg(ws: &mut Ws) -> Option<ServerMsg> {
    loop {
        match ws.next().await? {
            Ok(Message::Binary(b)) => return Some(proto::decode(&b).expect("valid ServerMsg")),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {} // ping / pong (answered by tungstenite)
        }
    }
}

/// Reads until `f` returns `Some` (panics on close or timeout).
async fn wait_for<T>(ws: &mut Ws, within: Duration, what: &str, mut f: impl FnMut(&ServerMsg) -> Option<T>) -> T {
    let fut = async {
        loop {
            let Some(m) = next_msg(ws).await else {
                panic!("socket closed while waiting for {what}");
            };
            if let Some(t) = f(&m) {
                return t;
            }
        }
    };
    tokio::time::timeout(within, fut)
        .await
        .unwrap_or_else(|_| panic!("timeout waiting for {what}"))
}

fn control_of(s: &Snapshot, f: FactionIdx) -> Option<Control> {
    s.factions.get(f as usize).map(|p| p.control)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_with_same_token_reclaims_kingdom() {
    let addr = start_server().await;

    // ---- lobby: Ada (leader) + Bob ----
    let mut ada = open(addr).await;
    hello(&mut ada, None, "Ada").await;
    let (ada_id, token) = wait_for(&mut ada, FAST, "Welcome (Ada)", |m| match m {
        ServerMsg::Welcome { you, token, urls } => {
            assert_eq!(urls, &vec!["http://test:8000".to_string()]);
            Some((*you, token.clone()))
        }
        _ => None,
    })
    .await;
    assert_eq!(token.len(), 32, "128-bit hex token");
    send(&mut ada, ClientMsg::Join { color: Some(KingdomColor::Purple) }).await;
    wait_for(&mut ada, FAST, "Lobby (Ada)", |m| match m {
        ServerMsg::Lobby { players, leader, .. } if players.len() == 1 => {
            assert_eq!(*leader, Some(ada_id));
            assert_eq!(players[0].color, KingdomColor::Purple);
            Some(())
        }
        _ => None,
    })
    .await;

    let mut bob = open(addr).await;
    hello(&mut bob, None, "Bob").await;
    let bob_token = wait_for(&mut bob, FAST, "Welcome (Bob)", |m| match m {
        ServerMsg::Welcome { token, .. } => Some(token.clone()),
        _ => None,
    })
    .await;
    assert_ne!(bob_token, token);
    send(&mut bob, ClientMsg::Join { color: Some(KingdomColor::Purple) }).await; // taken → first free (Blue)
    wait_for(&mut bob, FAST, "Lobby (Bob)", |m| match m {
        ServerMsg::Lobby { players, .. } if players.len() == 2 => {
            assert_eq!(players[1].color, KingdomColor::Blue);
            Some(())
        }
        _ => None,
    })
    .await;

    // ---- start ----
    send(&mut ada, ClientMsg::Start).await;
    let fa = wait_for(&mut ada, SLOW, "Init (Ada)", |m| match m {
        ServerMsg::Init { you, rejoin, colors, .. } => {
            assert!(!rejoin);
            let f = you.expect("Ada plays");
            assert_eq!(colors[f as usize], KingdomColor::Purple);
            Some(f)
        }
        _ => None,
    })
    .await;
    wait_for(&mut ada, FAST, "first snapshot (Ada)", |m| match m {
        ServerMsg::Snapshot(s) => {
            assert!(s.full && s.you == Some(fa) && s.economy.is_some());
            Some(())
        }
        _ => None,
    })
    .await;
    let fb = wait_for(&mut bob, SLOW, "Init (Bob)", |m| match m {
        ServerMsg::Init { you, .. } => Some(you.expect("Bob plays")),
        _ => None,
    })
    .await;
    assert_ne!(fa, fb);

    // ---- a newcomer cannot join the running match ----
    let mut eve = open(addr).await;
    hello(&mut eve, None, "Eve").await;
    let why = wait_for(&mut eve, FAST, "refusal (Eve)", |m| match m {
        ServerMsg::No { why } => Some(why.clone()),
        _ => None,
    })
    .await;
    assert_eq!(why, proto::NO_PLAYING);
    assert!(next_msg(&mut eve).await.is_none(), "refused socket is closed");

    // ---- Ada's page closes: the AI takes over, the match goes on, Bob sees it ----
    ada.close(None).await.expect("close");
    drop(ada);
    wait_for(&mut bob, FAST, "Ada's kingdom marked disconnected", |m| match m {
        ServerMsg::Snapshot(s) => (control_of(s, fa) == Some(Control::Disconnected)).then_some(()),
        _ => None,
    })
    .await;

    // ---- page reload: same token → the same kingdom back (Init rejoin + full snapshot) ----
    let mut ada2 = open(addr).await;
    hello(&mut ada2, Some(&token), "Ada").await;
    wait_for(&mut ada2, FAST, "Welcome (reload)", |m| match m {
        ServerMsg::Welcome { you, token: t, .. } => {
            assert_eq!(*you, ada_id, "same lobby id");
            assert_eq!(t, &token, "same token");
            Some(())
        }
        _ => None,
    })
    .await;
    wait_for(&mut ada2, FAST, "Init rejoin", |m| match m {
        ServerMsg::Init { you, rejoin, .. } => {
            assert!(*rejoin);
            assert_eq!(*you, Some(fa), "kingdom reclaimed");
            Some(())
        }
        ServerMsg::No { why } => panic!("refused: {why}"),
        _ => None,
    })
    .await;
    wait_for(&mut ada2, FAST, "full snapshot after rejoin", |m| match m {
        ServerMsg::Snapshot(s) => {
            assert!(s.full && s.you == Some(fa));
            assert!(s.economy.is_some());
            assert_eq!(control_of(s, fa), Some(Control::Human));
            assert!(s.units.iter().any(|u| u.faction == fa), "own units resent");
            Some(())
        }
        _ => None,
    })
    .await;
    wait_for(&mut bob, FAST, "Ada's kingdom human again", |m| match m {
        ServerMsg::Snapshot(s) => (control_of(s, fa) == Some(Control::Human)).then_some(()),
        _ => None,
    })
    .await;

    // ---- a second socket with the same token replaces the first ----
    let mut ada3 = open(addr).await;
    hello(&mut ada3, Some(&token), "Ada").await;
    wait_for(&mut ada3, FAST, "Init rejoin (second socket)", |m| match m {
        ServerMsg::Init { you, rejoin: true, .. } => {
            assert_eq!(*you, Some(fa));
            Some(())
        }
        _ => None,
    })
    .await;
    let closed = tokio::time::timeout(FAST, async {
        while next_msg(&mut ada2).await.is_some() {}
    })
    .await;
    assert!(closed.is_ok(), "the replaced socket is closed");

    // ---- commands work on the reclaimed kingdom ----
    send(
        &mut ada3,
        ClientMsg::Command {
            seq: 7,
            cmd: Command::Recall { units: vec![] },
        },
    )
    .await;
    wait_for(&mut ada3, FAST, "CommandResult", |m| match m {
        ServerMsg::CommandResult { seq: 7, err } => {
            assert!(err.is_none());
            Some(())
        }
        _ => None,
    })
    .await;
    // test mode is refused in a network match
    send(
        &mut ada3,
        ClientMsg::Command {
            seq: 8,
            cmd: Command::DebugFreeze { on: true },
        },
    )
    .await;
    wait_for(&mut ada3, FAST, "test-mode refusal", |m| match m {
        ServerMsg::CommandResult { seq: 8, err } => {
            assert_eq!(err.as_deref(), Some("La modalità test c'è solo nelle partite da soli"));
            Some(())
        }
        _ => None,
    })
    .await;
    // the replacement did not hand the kingdom to the AI
    wait_for(&mut bob, FAST, "a snapshot", |m| match m {
        ServerMsg::Snapshot(_) => Some(()),
        _ => None,
    })
    .await;
    let mut ok = true;
    let _ = tokio::time::timeout(Duration::from_millis(500), async {
        while let Some(m) = next_msg(&mut bob).await {
            if let ServerMsg::Snapshot(s) = m
                && control_of(&s, fa).is_some_and(|c| c != Control::Human)
            {
                ok = false;
            }
        }
    })
    .await;
    assert!(ok, "kingdom stays human across the socket replacement");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_refusals() {
    let addr = start_server().await;
    // plain GET /ws → 400 `WebSocket richiesto`
    let mut s = TcpStream::connect(addr).await.expect("tcp");
    s.write_all(b"GET /ws HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write");
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(FAST, s.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 400"), "{text}");
    assert!(text.contains("WebSocket richiesto"), "{text}");
    assert!(text.to_ascii_lowercase().contains("cache-control: no-cache"), "{text}");
    // foreign Origin → 403 `Origine non ammessa`
    let mut req = format!("ws://{addr}/ws").into_client_request().expect("request");
    req.headers_mut()
        .insert("Origin", "http://evil.example".parse().expect("header"));
    match tokio_tungstenite::connect_async(req).await {
        Err(tungstenite::Error::Http(resp)) => {
            assert_eq!(resp.status().as_u16(), 403);
            let body = resp.body().as_deref().unwrap_or_default();
            assert_eq!(String::from_utf8_lossy(body), "Origine non ammessa");
        }
        other => panic!("expected 403, got {:?}", other.map(|_| ())),
    }
    // same-origin page → accepted
    let mut req = format!("ws://{addr}/ws").into_client_request().expect("request");
    req.headers_mut()
        .insert("Origin", format!("http://{addr}").parse().expect("header"));
    assert!(tokio_tungstenite::connect_async(req).await.is_ok());
}
