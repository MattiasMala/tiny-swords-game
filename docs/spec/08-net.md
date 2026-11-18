# 08 — Networking (lobby, relay server, host snapshots, client replication)

Source files covered: `js/net.js` (all, 333 lines), `server.py` (all, 259 lines), README sections
"Avvio" (README.md:8-21) and "Multigiocatore: come funziona" (README.md:136-143). Network-related hooks in other files
are listed with file:line so implementers can cross-check (main.js, entities.js, factions.js, ui.js, world.js, control.js).

Scope reminder for the Rust rewrite: the JS architecture is *host-authoritative peer simulation through a dumb relay*
(the host's browser runs the sim, `server.py` only forwards text). The Rust rewrite replaces this with a
*server-authoritative* model (native Rust server runs the pure sim crate at fixed ticks). This document therefore has
two parts:

* Part A — exact description of the current behavior (what must be preserved at the *user-visible* level: lobby
  flow, rules, Italian strings, what each client sees/controls).
* Part B — "Problems / requirements for the Rust rewrite", including the reconnect-with-session-token fix.

---

## Part A — Current behavior

### A.1 Topology and roles

* One process `server.py` = static HTTP file server for the game + a single WebSocket relay endpoint `/ws`
  (server.py:1-11, 137-145).
* The server hosts exactly **one room** (`Room`, server.py:82-134): one host + N clients. There is no room id.
* **The first WebSocket connection when no host exists becomes the host** (server.py:95-97). All later connections
  are clients. The role is told to the browser in the `hello` message.
* The host browser runs the full simulation (`updateWorld`) and broadcasts snapshots at ~20 Hz. Clients run no
  gameplay simulation; they only interpolate and render replicated state and send commands (net.js:2-7, README:139-140).
* Selection, camera and fog-of-war are **local** to every player (net.js:3, README:140).
* Max players: `COLORS.length` = 5 (`['Blue','Red','Yellow','Purple','Black']`, assets.js:5). Kingdoms not taken by
  a human are AI-controlled.
* Solo play does not touch the network at all (ui.js:612-614 `GIOCA` → `G.start({players:[{color, local:true}], level})`).

### A.2 Server (`server.py`)

#### A.2.1 Command line, bind, printed addresses

```
python3 server.py [porta] [--bind INDIRIZZO]      (default: 8000 and 0.0.0.0)
```
* `port`: positional, optional, int, default `8000` (server.py:242).
* `--bind` / `-b`: default `"0.0.0.0"`, metavar `INDIRIZZO` (server.py:243).
* argparse description: `"Tiny Swords: gioco e relay per il multigiocatore"` (server.py:241).
* Server class: `ThreadingHTTPServer((bind, port), Handler(directory=ROOT))`, ROOT = directory of server.py
  (server.py:24, 246). One thread per connection.
* Startup prints (exact strings, server.py:248-250):
  * `Tiny Swords su http://localhost:{port}  (Ctrl+C per fermare)` (two spaces before the parenthesis)
  * for each LAN url: `Gli altri giocatori aprono: {url}`
* Ctrl+C: `KeyboardInterrupt` caught, prints an empty line, exits (server.py:251-254).
* Per-connection log lines (flush=True, server.py:174-175, 186):
  * on connect: `+ {who} {id} da {client_ip}` where who = `host` or `giocatore`
  * on disconnect: `- {who} {id}`
* `log_request` is overridden to print nothing (no line per served file); errors still print (server.py:152-153).

**LAN address discovery** `lan_urls(bind, port)` (server.py:222-237):
1. If `bind` is not `""` and not `"0.0.0.0"` → return `[f"http://{bind}:{port}"]` only.
2. Else: open a UDP socket, `connect(("10.255.255.255", 1))` (sends nothing; picks the outgoing interface),
   take `getsockname()[0]` as first IP (ignore OSError).
3. Append `sorted(set(gethostbyname_ex(gethostname())[2]) - set(ips))` (ignore OSError).
4. Return `http://{ip}:{port}` for every ip **not starting with `127.`**. (IPv4 only; may be empty.)
* The list is stored on the server (`server.urls`) and sent to every peer in `hello.urls`. The host lobby shows
  `urls[0]`, or the fallback `http://<IP di questo PC>:${location.port}` when empty (ui.js:642-643):
  label text `'Gli altri giocatori aprono  ' + where` (two spaces after "aprono").

#### A.2.2 HTTP

* `Handler(SimpleHTTPRequestHandler)`, `protocol_version = "HTTP/1.1"` (needed for the WS handshake) (server.py:137-138).
* `GET` with path (query stripped) == `/ws` → WebSocket upgrade; everything else → static file (server.py:141-145).
* Every response gets `Cache-Control: no-cache` so host and clients always revalidate and load the same version
  after an update (server.py:147-150).

#### A.2.3 WebSocket handshake (server.py:155-171)

* Reject with HTTP **400 `"WebSocket richiesto"`** unless `Upgrade` header (lowercased) == `websocket` and
  `Sec-WebSocket-Key` present.
* **Origin check**: if an `Origin` header exists and `origin.split("://",1)[-1] != Host header` → HTTP **403
  `"Origine non ammessa"`** (only pages served by this same server may connect). Missing Origin is allowed.
* Accept = base64(sha1(key + `"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"`)). Response 101 `Switching Protocols`,
  headers `Upgrade: websocket`, `Connection: Upgrade`, `Sec-WebSocket-Accept`. No subprotocol, no extensions
  (no permessage-deflate → all traffic uncompressed).
* After the handshake: `close_connection = True`, socket read timeout = `IDLE_TIMEOUT` = **90 s**.
* `room.join(sock)` → `Peer`; then the server immediately sends
  `{"t":"hello","id":<peer id>,"host":<bool>,"urls":[...]}` (server.py:176-177).

#### A.2.4 Constants (server.py:25-28)

| name | value | meaning |
|---|---|---|
| `WS_GUID` | `258EAFA5-E914-47DA-95CA-C5AB0DC85B11` | RFC 6455 GUID |
| `MAX_MESSAGE` | `4 << 20` = 4 MiB | max size of one *incoming* (browser→server) message, fragments summed |
| `IDLE_TIMEOUT` | 90 s | read timeout; no traffic for 90 s ⇒ connection considered dead |
| `BACKLOG` | 400 | max queued outgoing frames per peer; overflow ⇒ peer dropped |

#### A.2.5 Frame I/O

**Outgoing** `frame(payload, opcode=1)` (server.py:31-39): FIN=1, unmasked, length encoding 7-bit / 16-bit (126) /
64-bit (127). All relayed data is sent as **text frames (opcode 1)**, even if it arrived as binary.

**Incoming** `messages(peer)` generator (server.py:188-219):
* Reads 2-byte header; opcode = b0 & 15; len = b1 & 127, extended with 2 or 8 bytes.
* Error (`ValueError("frame non valido")` → connection closed) if the MASK bit is not set **or**
  `len(buf) + n > MAX_MESSAGE`.
* Unmask with 4-byte key.
* opcode 8 (close) → generator returns (connection closed, **no close frame is echoed**).
* opcode 9 (ping) → reply pong (opcode 10) with same payload.
* opcode 0/1/2 → append to `buf`; when FIN bit set, yield the whole message and reset buf.
* other opcodes (e.g. 10 pong) ignored.
* Caller: empty messages are **heartbeats** and are not routed (server.py:180-181). `OSError`, `EOFError`,
  `ValueError` (incl. socket timeout) end the loop; `finally: room.leave(peer)`.

#### A.2.6 `Peer` (server.py:42-79)

* Fields: `sock`, `id`, `out = Queue(BACKLOG)`; a daemon writer thread per peer.
* `send(payload, opcode)`: `put_nowait(frame(...))`; queue full ⇒ `drop()` (slow consumer is disconnected).
* `close()`: enqueue sentinel `None` (flush then close); queue full ⇒ `drop()`.
* `drop()`: `sock.shutdown(SHUT_RDWR)` ignoring OSError (this makes the reader loop fail → `leave`).
* writer: `sendall` each frame; on `None` or OSError stop and `drop()`.

#### A.2.7 `Room` and routing (server.py:82-134)

State: `lock`, `host: Peer|None`, `clients: {id: Peer}`, `next_id` starting at **1**, incremented for every
connection (ids are never reused during the server's lifetime; the host consumes ids too).

`join(sock)`: under lock create Peer with `next_id++`; if `host is None` it becomes host, else goes to `clients`.

`route(peer, msg)` — wire format (server.py:6-10, 117-134):
* **From host**: `msg = "<dest>|<body>"`. No `|` ⇒ dropped. `dest == "*"` ⇒ send body to all clients;
  `dest` all digits ⇒ send body to that client if it exists; otherwise dropped. The **body is forwarded without the
  prefix**. The host never receives its own broadcasts.
* **From a client** (only if its id is still in `clients`): forwarded to the host as `"<client id>|<msg>"`.
* Server-originated message to host: `"0|<json>"` (id 0 = the server).

`leave(peer)` (server.py:102-115):
* If the peer was the **host**: `host = None`, every client is removed from the room, receives
  `{"t":"hostleft"}` and is then closed. The next connection to arrive becomes the new host.
* Else if it was a client: host receives `0|{"t":"leave","id":<id>}`.
* The leaving peer is closed in any case.

### A.3 Browser-side `Net` object (net.js)

#### A.3.1 Constants (net.js:9-12)

| name | value | meaning |
|---|---|---|
| `SNAP_DT` | `0.05` s | interval between host snapshots (20 Hz) |
| `SLOW_EVERY` | `5` | buildings/resource nodes/points/stumps are included every 5th snapshot (every 250 ms), or immediately when the map changed |
| `MAX_BUFFERED` | `1 << 18` = 262144 bytes | if `ws.bufferedAmount` exceeds this the host skips a snapshot |
| `NO_SERVER` | `Multigiocatore: avvia "python3 server.py" e apri l'indirizzo che stampa.` | note shown when there is no relay |

Heartbeat: every **5000 ms** each browser sends an empty text frame if the socket is open (net.js:42).

#### A.3.2 `Net` fields (net.js:14-28)

| field | meaning |
|---|---|
| `role` | `null` / `'host'` / `'client'`, set on `hello` |
| `playing` | a network match is in progress (set at `startMatch` / `loadMatch`) |
| `ws`, `id`, `urls`, `pingT` | socket, own peer id from server, LAN urls from server, heartbeat interval handle |
| `lobby` | `[{id, color}]`; **`lobby[0]` is always the host** |
| `level` | AI difficulty (`'easy'|'normal'|'hard'`), chosen by host |
| host: `events` | presentation events queued for the next snapshot |
| host: `snapT`, `snapN`, `mapV`, `clock` | snapshot accumulator, snapshot counter, last `World.version` sent, Web Worker timer |
| client: `maps` | per entity list name → `Map(id → local object)` |
| client: `ready` | first snapshot received |
| getter `hosting` | `role==='host' && playing` |
| getter `remote` | `role==='client' && playing` |

#### A.3.3 Connection lifecycle

`connect()` (net.js:32-43), called by the main-menu button **MULTIGIOCATORE** (ui.js:615):
1. If `ws` already exists: no-op. Clear `Menu.note`.
2. If page loaded from `file:` → `Menu.note = NO_SERVER`, return.
3. Open `ws(s)://<location.host>/ws` (`wss` if page is https).
4. `onmessage` → `receive(data)` (ignored if the socket was replaced).
5. `onclose` (if still current socket) → `quit(note)` with:
   * role client: `Connessione con l'host persa.`
   * role host: `Connessione col server persa.`
   * role null (never got hello, e.g. served by `python -m http.server`): `NO_SERVER`.
6. Start 5 s heartbeat.

`leave()` (net.js:46-53): voluntary exit. Clear heartbeat, terminate worker clock, reset
`ws, role, playing, ready, clock, lobby, events, maps` (note: `id`, `urls`, `level` are **not** reset), close socket,
`Menu.screen = 'main'`.

`quit(note)` (net.js:56-59): involuntary exit. If `playing` → `G.toMenu()` (which calls `Net.leave()` and restarts the
menu demo battle, main.js:97) else `leave()`; then `Menu.note = note` (shown under the main menu, ui.js:617).

`send(m, to='*')` (net.js:62-66): no-op unless socket open. Host sends `to + '|' + JSON`; client sends bare JSON.

`receive(data)` (net.js:68-82):
* Host: split on the first `|`; `from = +prefix`. `from != 0` → `fromClient(from, m)`;
  `from == 0 && m.t === 'leave'` → `playerLeft(m.id)`.
* Otherwise (client, or host before `hello` sets the role): parse JSON and dispatch on `m.t`:
  `hello` → `hello(m)`; `lobby` → `lobby = m.players; level = m.level`; `no` → `quit(m.why)`;
  `hostleft` → `quit("L'host ha chiuso la partita.")`; `init` → `loadMatch(m)`; `s` (only if `playing`) →
  `applySnapshot(m)`.

`hello(m)` (net.js:85-92): store `id`, `urls`; role from `m.host`. Host: `lobby = [{id, color: Menu.color}]`,
`level = Menu.level` (the choices made on the main menu). Client: sends `{t:'join', color: Menu.color}`.
Both: `Menu.screen = 'lobby'`.

### A.4 Message catalogue

All payloads are JSON objects with a type field `t`. Routing prefixes (`<id>|`) are described in A.2.7.

| `t` | direction | payload | handler |
|---|---|---|---|
| `hello` | server → any (unprefixed, even to host) | `{id:int, host:bool, urls:[string]}` | net.js:85 |
| `leave` | server → host (`0|`) | `{id:int}` | `playerLeft` net.js:137 |
| `hostleft` | server → clients | `{}` | quit `L'host ha chiuso la partita.` |
| `join` | client → host | `{color:string}` (preferred kingdom = client's `Menu.color`) | `fromClient` net.js:119 |
| `pick` | client → host | `{color:string}` | `pick` net.js:104 |
| `lobby` | host → all clients (`*`) | `{players:[{id,color}], level:string}` | client sets `lobby`, `level` |
| `no` | host → one client | `{why:string}`: `Partita già in corso.` or `Partita al completo.` | client `quit(why)` |
| `init` | host → all clients | `{world, level, factions:[[color, human0/1, conn]]}` | `loadMatch` net.js:206 |
| `s` | host → all clients, ~20 Hz | snapshot, see A.7 | `applySnapshot` net.js:234 |
| `c` | client → host | `{c:string, a:array}` command, see A.8 | `fromClient` net.js:128 |
| `""` (empty frame) | any → server | heartbeat, never routed | server.py:180 |

### A.5 Lobby protocol

UI: `drawLobby` (ui.js:621-647). Screen state `Menu.screen ∈ {'main','lobby'}` (ui.js:555-556).

**Join** (host, `fromClient` 'join', net.js:119-125):
1. If a lobby entry with this id already exists → ignore.
2. Rejection reason: `playing` → `Partita già in corso.`; else `lobby.length >= 5` → `Partita al completo.`.
   If rejected: send `{t:'no', why}` to that client only, return. (The client then quits; its socket close produces a
   `leave` that the host ignores because the id is not in the lobby.)
3. Color assignment: the requested color if it is a valid `COLORS` entry and not taken; otherwise the **first free
   color in COLORS order** (Blue, Red, Yellow, Purple, Black).
4. Push `{id, color}`, `shareLobby()` (broadcast `{t:'lobby', players, level}`).

**Pick** (`choose(color)` from clicking a kingdom card, net.js:99-109): host calls `pick(ownId, color)` locally;
client sends `{t:'pick', color}`. Host `pick(id, color)` ignores the request when: player not in lobby, match
`playing`, color not in `COLORS`, or **any** lobby member (including the requester) already has that color.
Otherwise sets the color and `shareLobby()`. No "swap"; taken kingdoms cannot be chosen; there is no error message.

**Level** (`setLevel`, net.js:110-114): host only; ignored if `AI_LEVEL[level]` undefined. Sets and broadcasts.
Clients see the level row disabled (`levelRow(..., enabled=host)`).

**Leave in lobby** (`playerLeft`, net.js:137-141): remove entry, `shareLobby()`.

**Lobby screen strings** (ui.js:621-647, verbatim):
* Ribbon title: `MULTIGIOCATORE` (color = own kingdom color, or `Menu.color` before the first lobby message).
* Round back button (top-left) → `Net.leave()`.
* While own entry not yet in `lobby`: `Ingresso nella partita…`
* `Giocatori: ${n} su 5 · i regni liberi li guida l'IA`
* `Scegli il tuo regno`
* Kingdom card labels: `Tu` (own), `Host` (`lobby[0]`), `Giocatore` (other human), `IA` (free; drawn dimmed with a
  pawn portrait; humans use a warrior portrait).
* `Difficoltà dell'IA` (host) / `Difficoltà dell'IA (la sceglie l'host)` (clients); level labels from `AI_LEVEL`:
  `Facile`, `Normale`, `Difficile` (factions.js:20-25); portraits pawn/warrior/knight (ui.js:557).
* When `Net.playing` (client received `init`, waiting first snapshot): `Avvio della partita…`
* Host: big button `INIZIA` → `startMatch()`; plus the LAN address line (A.2.1).
* Client: `In attesa che l'host avvii la partita…`
* Help button top-right (opens help overlay).

There is **no minimum player count**: the host can start alone (then it is effectively solo vs 4 AI, but networked,
unpausable).

### A.6 Match start

**Host `startMatch()`** (net.js:150-157):
1. `playing = true; events = []; snapT = 0; snapN = 0; mapV = -1`.
2. `G.start({players: lobby.map(p => ({color: p.color, conn: p.id, local: p.id === Net.id})), level})`.
   In `G.start` (main.js:68-95): new random world seed `(Math.random()*1e9)|0`; `COLORS` shuffled
   (`sort(() => Math.random()-0.5)`) and assigned to `World.bases[i]` → **starting positions are random** (README:138);
   for each faction: `human = (a lobby player has this color)`, `f.idx = i`, `f.conn = p.conn || 0`,
   `G.player = f` if `p.local`. Factions are stored in base order, so `f.idx` = base index.
3. Broadcast `{t:'init', world: World.serialize(), level, factions: G.factions.map(f => [f.color, f.human?1:0, f.conn])}`.
4. Start a Web Worker that posts every **50 ms**; each tick calls `stepUnseen()` (main.js:481-486): if no animation
   frame painted in the last **150 ms** (background tab / hidden window) it runs `step(now)` so the host keeps
   simulating and broadcasting. (Rendering-only concern; disappears with a server-side sim.)

**World serialization** (world.js:906-913), needed because clients cannot regenerate an identical world:
* `cells[c] = land | level<<2 | cliff<<4 | noBuild<<5 | stair<<6 | plateau<<9` for every cell
  (`land` 2 bits, `level` 2 bits, `cliff` 1, `noBuild` 1, `stair` 3 bits, `plateau` rest).
* Plus lists verbatim: `bases, plateaus, ponds, trees, golds, stones, props, meadows, bushes, rocks, waterRocks, ducks,
  foam, openWater, points, bigs`.
* `World.load` (world.js:915-930) decodes, sets `treasures = []` (treasure chests arrive as pickups instead),
  allocates `block/wallOf/ownOf`, `resetBlock()`, clears path fields, `version++`, rebuilds tiles and minimap.
* For 288×192 = 55 296 cells this message is large (hundreds of KB of JSON).

**Client `loadMatch(m)`** (net.js:206-218):
1. `playing = true; ready = false; maps = {}; level = m.level`.
2. `G.reset()` — note `NEXT_ID = 1` (main.js:40): host and client number trees, gold, stone and control points
   identically because `G.scenery()` creates them in the same order right after reset. Commands reference these by id.
3. `World.load(m.world)`, `G.scenery()`, `G.demo = false`, `G.setup = null`.
4. Factions: `makeFaction(color, World.bases[i], !!human, m.level)` with `idx = i`, `conn` (no `setupFaction`:
   castles/units come from snapshots).
5. `G.player = faction whose conn === Net.id`. If none → `quit('Partita già in corso.')` (a client that was not in
   the lobby when the host pressed INIZIA, but still received the broadcast `init`).
6. `Ctl.reset()`, `Fog.reset(true)`.
7. `G.state` is still the menu; the lobby shows `Avvio della partita…` until the first snapshot.

On the **first snapshot** (net.js:281-286): `ready = true`, `G.state = 'play'`, camera centered on own castle,
`Fog.reveal(castle, START_SIGHT=15)`. The first sent snapshot is always "slow" (contains buildings), because
`mapV = -1` and `snapN = 0`, so `G.player.castle` exists.

### A.7 Host snapshot (`t:'s'`)

**Cadence** `hostTick(dt)` (net.js:159-166), called every frame after `update` when hosting (main.js:344):
```
snapT += dt; if snapT < 0.05 return
snapT = 0                       // reset, not subtract → effective rate slightly < 20 Hz
if ws.bufferedAmount > 262144 return   // congestion: skip; events keep accumulating
slow = (snapN++ % 5 == 0) || (mapV != World.version)
mapV = World.version
send(snapshot(slow)) to '*'
```
`World.version` is bumped by `setBlock`, `setOwner`, `resetBlock`, `generate`, `load` (world.js:87, 99, 325, 900, 927),
i.e. building placed/destroyed/moved, walls, etc. Tree stump changes do **not** bump it (only sent on the 250 ms cadence).

Rounding helpers: `r = Math.round`, `r2(v) = Math.round(v*100)/100`. `bits(set)` = bitmask over `TECH_IDS` order
(bit i set if `set[TECH_IDS[i]]` truthy).

Enumerations used as indices (order defined in assets.js):
* `UNIT_TYPES` (assets.js:170): `pawn, scout, warrior, lancer, archer, knight, monk, ram, catapult` (0..8).
* `BUILD_KINDS` (assets.js:297): `castle, house, camp, depot, farm, barracks, wall, archery, stable, smith, market,
  tower, monastery, siege` (0..13).
* `TECH_IDS` (assets.js:396): `age2, age3, carts, masonry, axes, picks, breeding, boots, horseshoes, blades, shields,
  fletching, armor, maps, herbs, scriptures, engineering` (bits 0..16).
* `ACT_LABEL` (entities.js:34-35, `act` index): `Inattivo, In marcia, In combattimento, Taglia legna, Estrae oro,
  Macella pecore, Costruisce, Ripara, Porta il carico, Cura i feriti, Di guardia, Estrae pietra, Al riparo, In fuga,
  In esplorazione`.
* `RES_KINDS` (assets.js:415): `wood, gold, meat, stone` (job index).

**Always-present fields** (net.js:172-187):

`time`: `r2(G.time)`.

`u` — units, only `!dead` (includes units sheltered inside buildings). Row:

| idx | value |
|---|---|
| 0 | `id` |
| 1 | faction idx |
| 2 | `UNIT_TYPES.indexOf(type)` |
| 3, 4 | `round(x)`, `round(y)` (world px) |
| 5 | `ceil(hp)` |
| 6 | `maxHp` |
| 7 | `anim` (animation name string, e.g. `idle`, `run`, `attack1`, `attackUpRight`, `blockDown`…) |
| 8 | `r2(animT)` |
| 9 | flags: bit0 `flip`; bit1 `hurtT > 0`; bit2 `inside` (sheltered); bits3-5 `job + 1` (job = `jobOf(u)` ∈ -1..3, entities.js:994-997) |
| 10 | `act` (index into ACT_LABEL) |
| 11 | `floor(faith || 0)` (monk healing reserve) |

`sh` — sheep (`!dead`): `[id, round(x), round(y), state(string), flip?1:0]`.

`a` — arrows/rocks (all in `G.arrows`): `[id, round(x), round(y), r2(ang), faction idx, rock?1:0]`.

`p` — pickups (all): `[id, round(x), round(y - hop), kind, life < 5 ? 1 : 0 (fading), chest?1:0]`.

`f` — factions in idx order: 

| idx | value |
|---|---|
| 0 | alive 1/0 |
| 1 | human 1/0 |
| 2-5 | `floor(res.wood)`, `floor(res.gold)`, `floor(res.meat)`, `floor(res.stone)` |
| 6, 7 | `pawnCap`, `troopCap` |
| 8 | `age` (1..3) |
| 9 | `bits(tech)` researched |
| 10 | `bits(busy)` researching now |
| 11, 12 | `kills`, `razed` |
| 13-15 | `r2(trades.wood)`, `r2(trades.meat)`, `r2(trades.stone)` (market pressure counters) |
| 16-19 | `rate.wood/gold/meat/stone` (unrounded; per-minute income estimate, only recomputed for human factions, factions.js:898) |

`e` — the presentation events emitted since the last **sent** snapshot (then cleared). See A.9.

**Slow-only fields** (net.js:189-200, when `slow`):

`b` — buildings (`!dead`). Row:
`[id, faction idx, BUILD_KINDS index, cx, cy, variant, round(hp), maxHp, done?1:0, r2(progress), queue, round(rally.x),
round(rally.y), rallyTargetId (0 if none or dead)]` where `queue = [[code, r2(t), total], ...]`,
`code = UNIT_TYPES.indexOf(unit)` for a unit or `100 + TECH_IDS.indexOf(tech)` for research.

`g` — `G.golds.map(g => g.amount)` (by array index; array never shrinks).
`q` — `G.stones.map(s => s.amount)` (by index).
`cp` — control points: `[owner idx or -1, capturing faction idx or -1, t]` by index.
`st` — indices into `G.trees` with `wood <= 0` (stumps).

**Not replicated at all** (clients don't have it): unit orders/targets/paths, building `inside` lists, faction `mods`
(rebuilt client-side from tech bits), `has`, `sites`, `houses`, `pawnN`, `troopN`, `seen`, `alarm`, `threat`, AI state,
sheep owner/home, tree regrow timers, gold/stone regen timers, arrow velocities/targets, pickup `amount`.

### A.8 Client replication (`applySnapshot`, net.js:234-306)

1. Clock: `if |s.time - G.time| > 0.25 → G.time = s.time` (otherwise the client advances `G.time` by its own dt).
2. Factions: assign fields from row (res, caps, age, kills, razed, trades, rate, alive, human); reset `pawns=[]`,
   `soldiers=[]` (refilled from units below). For each tech bit: newly set → `f.tech[id]=true; techMods(f.mods, id)`
   (displayed stats follow research; never removed); `busy` bits set/cleared.
3. `mirror(name, rows, create, update)` (net.js:222-232) for `units`, `sheep`, `arrows`, `pickups`: keeps a
   `Map(id → obj)`; existing objects are updated, new ones created and put into `G.index`, objects missing from the
   snapshot get `dead = true` and leave `G.index`. `G[name]` is replaced by the new ordered array.
   * Unit create: `{id, isUnit:true, faction, type, def: UNIT_DEF[type], x, y, sy:y, anim:'idle', hostAnim:'', animT:0,
     state:'idle', flip:false, hurtT:0, hp:1, maxHp:1, act:0, dead:false}`. Update: `glideTo(x,y)`; hp, maxHp, act,
     faith; **if `anim` differs from last host anim** → set `anim`, `animT` from host and `state = anim.startsWith('attack')
     ? 'attack' : 'idle'` (otherwise the local animT keeps running); `flip`, `hurtT = flag?1:0`, `inside`, `job`;
     push into `faction.pawns` (type pawn) or `faction.soldiers`.
   * Sheep: glide; on state change reset `animT = 0`; flip.
   * Arrows: glide; `ang`, faction (rock fixed at create).
   * Pickups: glide; if fading flag and `life > 5` → `life = 5` (client-side fade-out).
4. If slow fields present → `applyMap` (net.js:291-306): mirror buildings (create from `buildingShape(kind, cx, cy)`;
   if cx/cy changed → `buildingPlace` (building moved by its owner)); set hp, maxHp, done, progress, rally (target looked
   up in `G.index`), queue. Re-link `faction.castle` for every castle. Apply gold/stone amounts by index, control
   points, stumps (`wood = 0` for listed, `TREE_WOOD = 40` otherwise). `World.resetBlock()` and `blockBuilding(b, true)`
   for all buildings (rebuilds occupancy so placement previews are right).
5. `showEvent` for every event in `e`.
6. First snapshot handling (A.6), then `G.checkEnd()` every snapshot (win/lose computed from replicated `alive`).

**Client per-frame update** `clientUpdate(dt)` (net.js:308-326), used instead of `updateWorld` (main.js:339):
* only when `ready`. `G.time += dt`.
* Interpolation: `k = dt / (SNAP_DT * 1.25)` = dt/0.0625 (slightly slower than the snapshot period to absorb late
  snapshots). For units, sheep, arrows, pickups: `glide = min(1, glide + k)`; `x = fromX + (toX-fromX)*glide`, same y.
  `glideTo(o,x,y)`: `from = current; to = new; glide = 0` (net.js:330-332). Newly created objects glide from their own
  position (no motion).
* `animT += dt` for units and sheep; pickups `t += dt; life -= dt`.
* For each completed building, front queue item `t = min(total, t + dt)` (progress bar advances between snapshots).
* `twinkle(dt)`, `updateFx(dt)`, `updateView(dt)` (camera, controls, local fog, clouds, ambient).

Fog on clients is computed locally from replicated units/buildings/points every 0.2 s (control.js:51-59), using
`def.sight + f.mods.sight`.

### A.9 Presentation events

`emit(ev)` (entities.js:58-61): shows locally (`showEvent`) and, when hosting, appends to `Net.events`. When hosting,
`onScreen()` always returns true (entities.js:44-47) so effects/sounds outside the host's view are still generated for
the other players. Events are broadcast to everybody; **filtering by recipient happens on receive** (`mine(i)`:
`i < 0 || G.player.idx === i`, entities.js:63).

| event | fields | client behavior (entities.js:62-81) |
|---|---|---|
| `['fx', name, x, y, scale, dy, followId, color]` | FX key, rounded pos, scale (2 decimals), draw-order offset, unit id to follow (0 none), team color | spawn FX |
| `['prop', name, x, y]` | prop sprite name (e.g. `chest_open`) | add static decor |
| `['snd', name, x, y]` | positional sound (`Sfx.at`) | `Sfx.hear`: volume `1 - d/(0.75*viewW)` from camera center |
| `['play', fi, sound]` | UI sound for faction fi (`age`, `research`, `coin`, `horn`, `win`, `build`) | only if mine |
| `['float', fi, x, y, str, resKind]` | floating "+N" resource text | only if mine |
| `['toast', fi, text]` | short message (Italian, pre-formatted on host) | only if mine |
| `['alert', fi, x, y]` | "under attack" minimap ping (`Ctl.alert`) | only if `G.player.idx === fi` |
| `['say', to, text, colorFi, dur]` | banner (`G.announce`, main.js:33-35); `to=-1` = everyone; colorFi -1 = reader's color | if mine and fewer than 4 banners queued |

Italian strings carried by events (pre-formatted by the host; must be kept verbatim; full lists belong to the
gameplay specs):
* `Distruggi i castelli nemici!` (match start, main.js:94)
* `${f.name}: ${AGE_NAME[f.age]}` (factions.js:106), `Ricerca completata: ${label}` (factions.js:122),
  `${f.name} ti attacca!` (factions.js:745), `${f.name} è caduto!` (factions.js:910)
* `Hai conquistato: ${label}`, `Hai perso: ${label}` (entities.js:209-210), `Costruzione finita: ${label}` (entities.js:1134)
* toasts: `Esplorazione finita: a piedi non resta nulla da scoprire` (entities.js:729),
  `Pedoni al massimo` / `Servono più case` / `Truppe al massimo` / `Servono più accampamenti` (entities.js:1195),
  `Forziere trovato: +${amount} ${RES_LABEL[kind].toLowerCase()}` (entities.js:1287)
* network-specific: `${f.name}: il giocatore è uscito, ora lo guida l'IA` (net.js:147, banner, dur 4, colored with f)
* command rejections returned by `execCommand` (sent back as `['toast', f.idx, why]`, see A.10).

### A.10 Client commands (`t:'c'`)

Every local player action goes through `command(cmd, ...args)` (factions.js:425-428): solo/host → `execCommand(G.player,
...)` immediately and the returned rejection string is shown locally (`G.toast(why)` + `Sfx.play('deny')` in the
callers); **client → `Net.send({t:'c', c: cmd, a: args})` and returns `null`** (no local feedback; the rejection
arrives later as a `toast` event, *without* the `deny` sound).

Host validation (net.js:128-134): requires `playing`, `typeof m.c === 'string'`, `Array.isArray(m.a)`; faction =
`G.factions.find(f => f.conn === fromId)`; ignored if none or `!f.alive`. Calls `execCommand(f, m.c, ...m.a.slice(0,5))`
inside try/catch (`console.warn('comando non valido da', id, m, err)`); a non-null result → `emit(['toast', f.idx, why])`.

Command set (`execCommand`, factions.js:337-422; senders listed for reference):

| cmd | args (a,b,c,d,e) | validation / effect | sent from |
|---|---|---|---|
| `order` | unitIds[], kind, x, y, targetId | ids array ≤ 200; `orderUnits` (entities.js:353-406). kinds: `stop`, `explore` (scouts only), `move`, `amove` (finite x,y; formation), `attack`, `gather`, `build` (also repair), `drop`, `heal`. Units not owned/dead are filtered out. | control.js:184, 290, 293; panels.js:148, 193 |
| `place` | kind, cx, cy, builderIds[] | kind ∈ `BUILD_MENU`, integer cell; `placeBlock` reason; pays cost; `sites[kind]++`; dust FX; builders ≤ 200 get `build` | control.js:428, 441 |
| `jobs` | res, n | res ∈ RES_KINDS, integer ≥ 0 → `setWorkers` | panels.js:218 |
| `job` | pawnIds[], res | ≤ 200 ids; sends each owned pawn to nearest free node in explored territory; reject `${RES_LABEL[res]}: nessun posto libero nel territorio esplorato` if none sent | panels.js:146 |
| `recall` | soldierIds[] | ≤ 200; move non-pawn units to `(castle.x, castle.y1 + 130)` | control.js:351 |
| `train` | buildingId, unitType | own building; `trainBlock`; pay; push queue `{unit, t:0, total: UNIT_DEF.time}` | panels.js:115 |
| `research` | buildingId, techId | `techBlock`; pay; `busy[id]=true`; push queue | panels.js:280 |
| `cancel` | buildingId, queueIndex, expectedUnitOrTech | removed only if `queue[i]` still matches the expected item (network race guard); full refund | ui.js:412 |
| `rally` | buildingId, x, y, targetId | finite x,y; target kept if resource node or own building | control.js:208 |
| `move` | buildingId, cx, cy | integers, different cell; `moveBlock`; `relocateBuilding` | control.js:428 |
| `demolish` | buildingId | castle → `Il castello non si demolisce`; site refunds full cost; finished building no refund | control.js:305 |
| `trade` | marketId, `'sell'|'buy'`, res | finished market; sell needs ≥100 (`Risorse insufficienti`), buy needs gold ≥ price (`Oro insufficiente`); `trades[res]++`; coin sound | panels.js:370, 373 |

Note: only the first 5 args are used; ids are entity ids shared with the host (G.index).

### A.11 Disconnect, host leaving, spectators, mid-game join

**Client disconnects mid-game** (`playerLeft`, net.js:137-148), triggered by the server `leave` message (tab closed,
reload, network loss, 90 s idle timeout, slow-consumer drop):
1. Remove from `lobby`.
2. If the faction exists and is alive: `f.human = false; f.conn = 0`; every soldier `idle(u)` (guards); banner
   `${f.name}: il giocatore è uscito, ora lo guida l'IA` (dur 4).
3. From then on the AI runs the faction (`updateFaction`, factions.js:891-902: AI trickle income, `aiThink` every
   `level.think` s; `explored()` stops applying fog for it; `rate` no longer recomputed). The AI state is the one
   created at `makeFaction` (plan step 0, `waveT = rand(first)`, etc.), untouched while the faction was human.
4. **There is no way back**: the reloaded page gets a new peer id, sends `join`, and the host answers
   `Partita già in corso.` → the player is kicked to the main menu. **The AI keeps the kingdom forever. This is the bug
   to fix** (Part B).

**Host leaves** (closes tab, presses Esci, loses connection): the server drops every client with `hostleft`
→ `L'host ha chiuso la partita.`; the match is gone (README:142). UI warnings: pause menu note
`La partita continua. Se esci finisce per tutti.` (host) / `La partita continua mentre il menu è aperto.` (client)
(ui.js:524-526); game-over screen `Sei l'host: se esci la partita finisce per tutti.` (ui.js:550). The host tab may be
in background (Worker clock) but must not be closed.

**No pause in network games** (main.js:338-344, ui.js:516-527): the pause panel title becomes `MENU` instead of
`PAUSA`, buttons `Riprendi` / `Esci` (instead of `Menu`); the host keeps simulating while its menu/help is open.
Test mode is refused: toast `La modalità test c'è solo nelle partite da soli` + deny sound (main.js:258).

**Game over / spectators** (ui.js:530-551, 483-490; main.js:100-113):
* Each machine computes its own result from replicated `alive` flags: `!G.player.alive` → `lose`; only one alive →
  `win`. Then `endT = 2.5` s, selection cleared, `Fog.on = false` (whole map visible), result sound.
* Game-over panel: `VITTORIA!` / `SCONFITTA`; line `I cinque regni sono tuoi.` / `Vince il ${name}.` /
  `Il ${name} è caduto.`; stats `Tempo m:ss    Nemici abbattuti N    Edifici distrutti N    ${AGE_NAME}`.
  In network games: `Guarda` (only while >1 kingdom alive) → `G.spectate = true; G.state = 'play'`, and `Esci`
  (`G.toMenu()`). Solo: `Rigioca` and `Menu`.
* Spectator HUD: menu + sound buttons, minimap, announcements, ribbon
  `Spettatore · frecce o minimappa per guardare`. All orders are disabled (control.js:379, ui.js:318).
  When ≤1 kingdom remains: `spectate = false; state = 'over'`.
* Spectating is only for eliminated players; there are no pure spectators (a non-lobby connection is rejected).

**Mid-game join forbidden**: `join` while `playing` → `Partita già in corso.`; lobby full → `Partita al completo.`;
a client whose `join` was not processed before INIZIA receives `init` without its conn → `Partita già in corso.`
(net.js:121, 215; README:143).

### A.12 Rendering vs simulation boundaries

* Pure simulation (moves to the sim crate): everything executed in `updateWorld` and `execCommand`, faction `seen`
  exploration (used by `job`/`jobs` command validation), per-faction alerts/announcements decisions, ids.
* Presentation only: `fx`, `prop`, `snd`, `float` events, `anim`/`animT`/`flip`/`hurtT` (although `anim` is driven by
  sim state), interpolation, local fog shading, camera, banners/toasts display, Worker clock, `onScreen` culling.
* The JS sim is variable-timestep: `dt = min(0.05, frameDelta)` (main.js:462-466) — not deterministic.

---

## Part B — Problems / requirements for the Rust rewrite

### B.1 Weaknesses found in the current implementation

1. **Reconnect impossible; AI takes over forever** (net.js:137-148, 119-122). A reload gets a new server id; identity
   is the transport connection. Any transient Wi-Fi drop, tab reload, or browser crash permanently loses the kingdom.
2. **Host is a single point of failure**: the whole match lives in one browser tab; host leaving / sleeping laptop /
   closed tab ends the game for everyone (server.py:105-112). Background-tab throttling had to be patched with a
   Worker clock; at <20 steps/s the `dt` clamp of 0.05 s makes game time run slower than real time.
3. **Variable-timestep, non-deterministic sim** (main.js:462-466, `Math.random` everywhere, `sort(() => Math.random()-0.5)`
   shuffle). Snapshot cadence uses `snapT = 0` reset (drifts below 20 Hz at high refresh rates).
4. **No fog-of-war filtering — total information leak**: every client receives every unit, building, resource count,
   research and event of every faction. A modified client is a perfect map hack.
5. **Bandwidth**: full-state JSON every 50 ms (no deltas, no compression, no permessage-deflate, unit anim names as
   strings); `init` sends the full 55 296-cell world as a JSON number array. Backpressure handling only skips whole
   snapshots on the host (262 144 B threshold) and the server disconnects a peer after 400 queued frames.
6. **Events can be lost or flood**: events accumulate while snapshots are skipped (unbounded), are broadcast to all
   clients (private toasts/alerts of other players included), and carry pre-formatted Italian text.
7. **No command acknowledgement or sequence numbers**: client actions return `null` locally; rejections come back as
   a generic toast without the deny sound; commands can apply to a changed state (only `cancel` guards against it).
8. **Weak validation / no rate limiting**: arbitrary JSON from clients, only light type checks, exceptions caught and
   logged; no per-client command rate limit; arrays capped at 200 ids but message size up to 4 MiB.
9. **Server protocol issues**: no close frame echoed; binary frames relayed as text; `int(dest)` routing by
   string prefix; ids never reused; single room only; no TLS; IPv4-only LAN discovery; heartbeats are empty data
   frames rather than WS ping/pong; idle timeout 90 s means a dead client keeps its human faction idle for up to 90 s
   before AI takeover.
10. **Join race**: a client that connected but whose `join` arrives after INIZIA is kicked with a misleading
    `Partita già in corso.`; a client receives the large `init` even if it is not in the match.
11. **Lobby leadership not transferable**: if the host leaves the lobby everyone is kicked; the next connection becomes
    host only after everyone reconnects.
12. **AI takeover quality**: AI resumes with a fresh plan (`step 0`, initial `waveT`), human `rate` stops updating;
    soldiers are reset to idle, losing their orders.
13. **Client-side derived data drift**: client `mods` only rebuilt from tech bits; `has`/`sites`/caps partially missing
    → UI on clients may disagree with host validation (check panels/UI specs).
14. **Clock sync** is a crude 0.25 s snap; interpolation uses a fixed 62.5 ms blend instead of a timestamped buffer.
15. **No versioning**: no protocol/build version check between host and clients (relies on `Cache-Control: no-cache`).

### B.2 Requirements for the Rust architecture

**R1 — Server binary and bind.**
* Native Rust server (e.g. tokio + axum/hyper + tungstenite) that serves the static WASM client bundle **and** the
  WebSocket endpoint `/ws` on the same port, so the page and socket share an origin.
* CLI compatible with today: `tiny-swords-server [port] [--bind|-b ADDR]`, defaults **port 8000, bind 0.0.0.0**.
  It must always be reachable through 0.0.0.0 by default (user requirement).
* Print exactly the same lines: `Tiny Swords su http://localhost:{port}  (Ctrl+C per fermare)` and
  `Gli altri giocatori aprono: {url}` for each LAN address, using the same discovery algorithm (A.2.1; optionally also
  IPv6 non-link-local). Per-connection logs `+ {who} {id} da {ip}` / `- {who} {id}` (adapt `who` to new roles).
* Serve `.wasm` as `application/wasm`; keep `Cache-Control: no-cache` (or content-hashed filenames); gzip/brotli for
  the bundle. Keep the Origin check (403 `Origine non ammessa`) and 400 `WebSocket richiesto`.
* Send a `hello` that includes `urls` (lobby shows `urls[0]`, fallback `http://<IP di questo PC>:{port}`).

**R2 — Authoritative server simulation.**
* The server owns the match and runs the pure sim crate at a **fixed tick** (recommend 20 Hz sim = snapshot rate, or
  30/60 Hz sim with 20 Hz snapshots); `dt` constant; seeded deterministic RNG (no `Math.random`). The match continues
  regardless of any browser. The "host" becomes only the **lobby leader** (first player in the lobby; leadership passes
  to the next lobby member if the leader leaves before start; after start the leader has no special power except as
  noted below). Remove the "if the host leaves the game ends" rule and its warning strings, or keep the strings unused
  (decide: see open questions).
* Solo play runs the same sim crate inside WASM in the browser (no server needed), same fixed tick.
* No pause in network games (unchanged), test mode still solo-only.

**R3 — Session tokens and reconnection (the bug fix).**
* On `join`, the server creates a random **session token** (≥128-bit, e.g. 32 hex chars) bound to
  `(match id, faction/lobby slot)` and returns it in the `lobby`/`welcome` reply. The client stores it in
  `localStorage` (key per server origin, e.g. `tinyswords.session`) together with the match id. Wrap storage in
  try/catch; if storage is unavailable, reconnection by reload is impossible but nothing else breaks.
* On every connect, the client sends `hello`/`join` **with the stored token if any**. Server logic:
  * token valid and its match is in the lobby → re-attach to the same lobby slot (same color).
  * token valid and match running and the faction is alive → **reclaim**: bind the faction to the new connection,
    set `human = true`, send full `init` (seed/world + faction table + own faction idx) and an immediate full
    snapshot; client goes straight into play.
  * token valid but faction eliminated → join as spectator of that match (eliminated player view).
  * no/invalid token and match running → `no` with `Partita già in corso.` (mid-game join stays forbidden for new
    players; see open question about spectators).
* On disconnect during a match: mark the slot `disconnected` (keep `human` ownership + token). Two acceptable policies
  (pick one, make the constant configurable):
  1. *Grace then AI*: after `RECONNECT_GRACE` (suggest 10–30 s) the AI drives the kingdom **temporarily**, announced with
     the existing banner `${f.name}: il giocatore è uscito, ora lo guida l'IA`; on reclaim, AI hands control back.
  2. *AI immediately but reversible*: same as today's behavior but reclaimable at any time.
  In both cases the reclaim must be possible **for the whole match**, not only within the grace window.
* On reclaim: `human = true`, `conn = new connection`, soldiers keep their current orders (do not reset), keep AI
  sub-state so a later takeover resumes sensibly; announce (new string, needs approval; suggestion
  `${f.name}: il giocatore è tornato`).
* A second connection presenting a token already attached to a live connection replaces the old one (old socket gets
  `no`/closed) — this handles "reload before the old socket timed out".
* Detect dead sockets quickly: WS ping every 5 s, disconnect after ~15 s without pong (instead of 90 s).
* A lobby member that leaves the lobby voluntarily (back button = `Net.leave()`) invalidates its token; a reload in the
  lobby keeps the slot for the grace period.

**R4 — Lobby protocol (preserve rules).**
* Max 5 players; requested color if free else first free in `COLORS` order; `pick` only to a free color; level
  selectable only by the lobby leader; start only by the leader; no minimum players; random base assignment at start;
  free kingdoms are AI at the chosen level. All lobby strings of A.5 kept verbatim.
* Rejections: `Partita già in corso.`, `Partita al completo.`.
* Make start atomic on the server: the player set is fixed at start; late `join`s get a proper rejection without
  receiving `init`.

**R5 — Messages and encoding.**
* Binary protocol (e.g. `bincode`/`postcard` over WS binary frames) with a version handshake
  (`hello { protocol_version, build_hash }`; mismatch → readable error, e.g. ask to reload).
* Message sets: client→server `Hello{token?}`, `Join{color}`, `Pick{color}`, `SetLevel{level}`, `Start`,
  `Command{seq, cmd}`, `Leave`; server→client `Welcome{id, token, urls}`, `Lobby{players, level, leader}`, `No{why}`,
  `Init{seed, world_hash/or world, factions, you}`, `Snapshot{tick, ...}`, `CommandResult{seq, why?}`, `Events`,
  `MatchOver`.
* World: send the **seed** + generation params (deterministic Rust generator) and a hash to verify; fall back to the
  compressed world only if hashes differ.
* Commands: typed enum mirroring A.10 (same validations, same 200-id caps, same rejection strings); reply to the
  sender only with `CommandResult` so the client plays `deny` + toast exactly like solo. Rate-limit per client.
* Events: typed enum (fx/prop/snd/play/float/toast/alert/say) with **structured parameters** (string keys + args) so
  the client formats the Italian text; send private events (`play`, `float`, `toast`, `alert`, `say` with `to >= 0`) only
  to the recipient; drop or coalesce cosmetic events under congestion but never private notices.

**R6 — Snapshots and interest management.**
* 20 Hz snapshots (`SNAP_DT = 0.05`), tick-stamped; slow data (buildings, nodes, points, stumps) still at 250 ms or
  on change — better: delta-encode everything against the last acked snapshot.
* Per-recipient **fog-of-war filtering**: send enemy units/buildings only when visible to that faction (keep
  last-known building ghosts client-side), own faction full detail, enemy faction resources/research not sent (UI does
  not need them — verify with UI spec). Eliminated players/spectators and game-over (`Fog.on = false`) receive
  everything.
* Replicate the same per-entity fields as A.7 (positions, hp, anim id as enum, animT, flip, hurt, inside, job, act,
  faith; buildings incl. queue/rally; faction economy incl. caps, trades, rates, `mods` or enough to derive them,
  plus `has`/`sites` counts the UI needs).
* Client interpolation with a time-stamped buffer (~100 ms behind), keeping the "progress bar advances between
  snapshots" behavior and the pickup fade rule.
* Backpressure per connection: if a client lags, skip snapshots for it (latest-wins), never block the sim.

**R7 — Robustness.**
* No host-tab dependency (Worker clock, `onScreen()` override, `stepUnseen` all removed).
* Handle WS close frames properly; max message size for client→server small (e.g. 64 KiB).
* Multiple matches per server optional (single room is acceptable as long as the lobby flow is unchanged).
* Server keeps running after a match ends; when all humans have left a finished/abandoned match, it is discarded and
  a new lobby opens.

### B.3 Open questions

1. Reconnect policy: grace period length, and whether the AI drives the kingdom during the disconnection (README rule
   "il suo regno passa all'IA") or the kingdom just idles.
2. With a server-authoritative sim, should the match continue when the lobby leader leaves (recommended) — then the
   strings `Se esci finisce per tutti.` / `Sei l'host: se esci la partita finisce per tutti.` / `L'host ha chiuso la
   partita.` become obsolete. Confirm removal or replacement texts.
3. Should non-players be allowed to join a running match as pure spectators? (Current rule: forbidden.)
4. New Italian strings needed (reconnect banner, version mismatch, reconnecting overlay) — need wording approval.
5. Should the server also be able to run a solo match (for low-power devices), or is solo always in-browser WASM?

---

## Addenda (critic)

Completeness check: every function and constant of `net.js` and `server.py` is referenced in this file
(script-checked), and every printed/sent Italian string is present verbatim.

**N1. Final reconnect / session policy (resolves B.3 Q1, Q2, Q3, Q5 and 04b Q2, Q4).** The user's request is explicit:
a player who reloads the page must get their kingdom back; the AI must not keep it forever.
1. Identity = session token (R3). Stored in `localStorage` under `tinyswords.session` as `{token, matchId}`; read and
   write wrapped in try/catch.
2. Disconnect detection: WS ping every 5 s, connection considered lost after 15 s without traffic, or immediately on a
   close frame / socket error.
3. `RECONNECT_GRACE = 20 s`. During the grace window the kingdom stays human-controlled (`f.human = true`, `f.conn = None`):
   no AI, no idling of soldiers; units keep executing their last orders, pawns keep their jobs, human-only automation
   (alarm response, `raidOn`, pawn shelter) keeps working. A page reload (2–5 s) therefore causes no visible change for
   other players.
4. After the grace window: AI takeover exactly as JS (`f.human = false`, `idle` soldiers, banner
   `` `${f.name}: il giocatore è uscito, ora lo guida l'IA` ``) plus the AI fast-forward of 04b addenda A1. The slot
   stays reserved for that token **for the whole match**.
5. Reclaim at any time with the same token while the faction is alive (04b §12.3 procedure, no unit idling), banner
   `` `${f.name}: il giocatore è tornato` `` (4 s, coloured with the faction). If the faction was eliminated in the
   meantime, the player re-enters as the eliminated spectator.
6. A second connection with a token already attached replaces the old socket (old one closed silently).
7. Lobby leader leaving: the match **continues** (server-authoritative); during the lobby, leadership passes to the next
   player in lobby order. Last human leaving a running match: the server keeps simulating for `ABANDON_TIMEOUT = 300 s`
   (so a reload/reconnect of the last player still works), then discards the match and reopens the lobby.
8. New players cannot join a running match (rule unchanged: `Partita già in corso.`); no pure spectators (unchanged).
9. Solo play is always the in-browser WASM sim (no server needed); the native server serves the same bundle.

**N2. B.1 item 13 (client-derived data) — resolved: no UI disagreement.** See 04a addenda E7: the JS UI never reads
`has/sites/pawnN/troopN`; it uses replicated caps, `age`, `tech/busy`, `floor(res)` and rebuilt `pawns/soldiers`
arrays. One real client-only inaccuracy: between snapshots the client advances the first queue item at 1× speed
(net.js:322) while the host uses `trainSpeed(n)` (entities.js:1184), so with several factories the bar jumps forward at
each slow snapshot. Rust: replicate `train_speed` (or the building count) and advance the bar with it.

**N3. Strings that change or are new in the Rust port (wording proposals; all other strings stay verbatim).**
| Where | JS text | Rust |
|---|---|---|
| net.js:12 `NO_SERVER` | `Multigiocatore: avvia "python3 server.py" e apri l'indirizzo che stampa.` | `Multigiocatore: avvia il server di Tiny Swords e apri l'indirizzo che stampa.` |
| ui.js:525 pause note (leader) | `La partita continua. Se esci finisce per tutti.` | use the non-host text for everyone: `La partita continua mentre il menu è aperto.` |
| ui.js:550 | `Sei l'host: se esci la partita finisce per tutti.` | removed |
| net.js:79 `hostleft` | `L'host ha chiuso la partita.` | unused (kept only if the server shuts down: `Il server ha chiuso la partita.`) |
| net.js:40 | `Connessione con l'host persa.` / `Connessione col server persa.` | `Connessione col server persa.` for everyone, followed by automatic reconnection attempts |
| new: reconnect overlay | — | `Riconnessione in corso…` |
| new: reclaim banner | — | `` `${f.name}: il giocatore è tornato` `` |
| new: version mismatch | — | `Versione diversa dal server: ricarica la pagina.` |
| new: session expired | — | `La partita a cui eri collegato è finita.` |

**N4. Bind address.** Unchanged and mandatory (user requirement): default `--bind 0.0.0.0`, port 8000, so the game
is always reachable on all interfaces; the printed LAN URLs (`Gli altri giocatori aprono: {url}`) are computed as in
A.2.1 (`lan_urls`, server.py:222-237). The client must connect the WebSocket to the **same host it was loaded from**
(`location.host`, as net.js:36 does today: `ws(s)://` + `location.host` + `/ws`), never to a hard-coded `localhost`.
