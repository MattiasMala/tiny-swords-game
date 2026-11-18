# 04a — Factions, Economy, Commands, Victory (porting spec)

Source of truth: `js/factions.js` (913 lines, read completely). Related logic that the faction/economy
layer depends on was also read and is specified here where it is economic:
`js/entities.js` (queues, training speed, building lifecycle, gathering, pickups, points),
`js/assets.js` (UNIT_DEF / BUILD_DEF / TECH_DEF costs), `js/panels.js` (Workers, Market, Units, Research, Build
screens), `js/ui.js` (queue panel, market price list, game over, spectator), `js/control.js` (how local input
creates commands), `js/main.js` (G.start, checkEnd), `js/net.js` (command transport, player leaving).

The AI part of `factions.js` (lines 11-37 constants, 430-889 functions) is specified by another document; section 14
here only lists the parts of the AI that touch the economy/command layer so the boundary is clear.

Conventions used below:
- `TILE = 64` px per cell (`assets.js:4`). "cell" = grid tile, "px" = world pixels. All world positions in the JS are px.
- `rand(a,b) = a + Math.random()*(b-a)` (`assets.js:600`), `clamp(v,a,b)` (`assets.js:603`).
- **[SIM]** = pure deterministic simulation, must live in the Rust sim crate. **[UI]** = client-only presentation.
  **[EVT]** = the sim emits a presentation event (sound/announcement/toast/float/fx) — in Rust these should be
  emitted as an event list from the sim and rendered by the client.
- All `Math.random()` calls listed here MUST become the sim's seeded RNG in Rust (determinism for server + solo).

---

## 1. Global constants (factions.js:4-10, entities.js:8-33, assets.js)

| Name | Value | Meaning | Ref |
|---|---|---|---|
| `PAWN_MAX` | 30 | hard cap on pawn population cap | factions.js:4 |
| `TROOP_MAX` | 50 | hard cap on troop population cap | factions.js:4 |
| `QUEUE_MAX` | 5 | max items (units + techs together) in one building's queue | factions.js:4 |
| `RAID_SPACING` | 120 s | (AI) recently raided kingdom is unlikely target | factions.js:5 |
| `WORK_RADIUS` | 2600 px | pawns sent to work by management screens only to nodes within this distance of the castle | factions.js:6 |
| `DEFENSE_RADIUS` | 12 cells | towers/palisades must be within this of castle center | factions.js:9 |
| `BUILD_RADIUS` | 40 cells | any other building within this of castle center | factions.js:9 |
| `OUTPOST_RADIUS` | 9 cells | anything (defense included) within this of an owned outpost point | factions.js:9 |
| `START_RES` | `{ wood: 200, gold: 100, meat: 200, stone: 100 }` | starting resources | factions.js:10 |
| `SELL_PRICE` | 50 | gold received for selling 100 (base) | factions.js:257 |
| `SELL_DROP` | 6 | gold lost on sell price per recent trade | factions.js:257 |
| `BUY_PRICE` | 130 | gold paid to buy 100 (base) | factions.js:257 |
| `BUY_RISE` | 10 | gold added to buy price per recent trade | factions.js:257 |
| `START_SIGHT` | 15 cells | radius explored around the castle at start | control.js:10 |
| `JOB_TIME` | `{ wood: 3.0, gold: 3.4, stone: 3.4, meat: 20 }` s | work seconds per load | entities.js:8 |
| `NODE_USERS` | `{ tree: 2, gold: 3, stone: 3, sheep: 1 }` | pawns per node | entities.js:9 |
| `BIG_USERS` | 8 | pawns per big (2x2) gold/stone node | entities.js:10 |
| `NODE_RES` | `{ tree: 'wood', gold: 'gold', stone: 'stone', sheep: 'meat' }` | | entities.js:11 |
| `CARRY` | 10 | base load per trip | entities.js:13 |
| `SHEEP_MEAT` | 15 | meat from one sheep (one trip) | entities.js:13 |
| `TREE_WOOD` | 40 | wood per tree | entities.js:13 |
| `GOLD_MAX`, `STONE_MAX` | 240, 300 | (map gen node amounts; see world spec) | entities.js:13 |
| `FARM_BREED` | 9 s | farm spawns a sheep every 9 s (× breed mod) | entities.js:14 |
| `FARM_SHEEP` | 5 | max sheep homed on one farm pasture | entities.js:14 |
| `TREASURE` | 50 | resources in a map chest | entities.js:15 |
| `POINT_INCOME` | 0.5 res/s | income of a held resource point (= 30/min) | entities.js:29 |
| `CAPTURE_TIME` | 8 s | | entities.js:30 |
| `CAPTURE_RADIUS` | 300 px | | entities.js:31 |
| `POINT_SIGHT` | 7 cells | | entities.js:32 |
| `TRAIN_BONUS` | 0.2 | +20% training speed per extra building of same kind | entities.js:33 |
| `REPAIR_RATE` | 0.035 | fraction of maxHp per second repaired per pawn | entities.js:17 |
| `RES_KINDS` | `['wood', 'gold', 'meat', 'stone']` (ORDER MATTERS: job index) | | assets.js:415 |
| `RES_LABEL` | `{ wood: 'Legna', gold: 'Oro', meat: 'Cibo', stone: 'Pietra' }` | | assets.js:417 |
| `COLORS` | `['Blue', 'Red', 'Yellow', 'Purple', 'Black']` | | assets.js:5 |
| `COLOR_NAME` | `{ Blue: 'Blu', Red: 'Rosso', Yellow: 'Giallo', Purple: 'Viola', Black: 'Nero' }` | | assets.js:6 |
| `AGE_NAME` | `['', 'Età del Villaggio', 'Età Feudale', 'Età dei Castelli']` | index = age 1..3 | assets.js:107 |
| `AGE_SHORT` | `['', 'I', 'II', 'III']` | | assets.js:108 |

---

## 2. Cost / stat tables relevant to the economy

### 2.1 Units (assets.js:115-169) — only economic fields (combat fields are in the units spec)

| type | label | cost | time (s) | from | age | hp | speed |
|---|---|---|---|---|---|---|---|
| pawn | Pedone | meat 50 | 13 | castle | 1 | 50 | 92 |
| scout | Esploratore | meat 40 | 12 | castle | 1 | 60 | 165 |
| warrior | Guerriero | meat 58, gold 40 | 15 | barracks | 1 | 130 | 98 (foot) |
| lancer | Lanciere | meat 60, gold 40 | 17 | barracks | 2 | 110 | 118 (foot) |
| archer | Arciere | wood 38, gold 32 | 17 | archery | 2 | 70 | 102 (foot) |
| knight | Cavaliere | meat 80, gold 60 | 20 | stable | 2 | 190 | 150 |
| monk | Monaco | gold 80 | 22 | monastery | 3 | 65 | 94 |
| ram | Ariete | wood 160, gold 60 | 26 | siege | 3 | 340 | 62 |
| catapult | Catapulta | wood 140, gold 120 | 28 | siege | 3 | 110 | 58 |

`UNIT_TYPES` order = `pawn, scout, warrior, lancer, archer, knight, monk, ram, catapult` (Object.keys order; used for
network codes and the Units screen row order). Every unit except `pawn` counts as a **troop** (scouts, monks and
siege included).

### 2.2 Buildings (assets.js:224-293)

| kind | label | w×h | hp | cost | work (s) | sight | pawns | troops | age | drop | trains | techs | flags |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| castle | Castello | 5×2 | 2600 | — (none) | 0 | 10 | 10 | 6 | — (none) | wood,gold,meat,stone | pawn, scout | age2, age3, carts, masonry | |
| house | Casa | 2×1 | 320 | wood 30 | 9 | 4 | 5 | | 1 | | | | `sprites` (3 variants) |
| camp | Accampamento | 3×2 | 420 | wood 60 | 12 | 5 | | 8 | 1 | | | | |
| depot | Deposito | 2×1 | 380 | wood 60 | 12 | 5 | | | 1 | wood,gold,stone | | axes, picks | |
| farm | Fattoria | 3×2 | 420 | wood 150 | 14 | 5 | | | 1 | meat | | breeding | |
| barracks | Caserma | 3×2 | 700 | wood 90 | 18 | 5 | | 2 | 1 | | warrior, lancer | boots | |
| wall | Palizzata | 1×1 | 300 | wood 12 | 3 | 2 | | | 1 | | | | `defense`, `neutral` sprite |
| archery | Arcieria | 3×2 | 600 | wood 80, gold 30 | 18 | 5 | | 2 | 2 | | archer | | |
| stable | Scuderia | 3×2 | 650 | wood 110, gold 40 | 18 | 5 | | 2 | 2 | | knight | horseshoes | |
| smith | Fucina | 3×2 | 650 | wood 100, stone 40 | 18 | 5 | | | 2 | | | blades, shields, fletching, armor | |
| market | Mercato | 3×2 | 600 | wood 120 | 18 | 5 | | | 2 | | | maps | `trade` |
| tower | Torre | 2×1 | 620 | stone 80, wood 40 | 16 | 9 | | | 2 | | | | `defense` |
| monastery | Monastero | 3×2 | 600 | wood 80, gold 100, stone 40 | 22 | 5 | | 2 | 3 | | monk | herbs, scriptures | |
| siege | Officina d'assedio | 3×2 | 650 | wood 160, gold 60 | 22 | 5 | | 2 | 3 | | ram, catapult | engineering | |

Notes:
- `BUILD_MENU = ['house', 'camp', 'depot', 'farm', 'barracks', 'wall', 'archery', 'stable', 'smith', 'market', 'tower', 'monastery', 'siege']` (assets.js:298) — the only kinds a `place` command accepts (castle can never be placed). Also the order of the Build screen grid (4 per row).
- The castle has no `cost` and no `age`; `ageLock` on it is never true (`f.age < undefined` is false). Rust: treat as age 1, cost empty.
- Building descriptions (`desc`, user visible, keep verbatim):
  - castle: `Cuore del regno: se cade, hai perso. Addestra Pedoni ed Esploratori e fa avanzare d'era.`
  - house: `+5 Pedoni al massimo. Con i nemici in vista i Pedoni ci si rifugiano.`
  - camp: `+8 truppe al massimo.`
  - depot: `Punto di consegna per legna, oro e pietra. Costruiscilo vicino a boschi e cave.`
  - farm: `Alleva pecore nei dintorni, abbastanza per due o tre pedoni, e fa da punto di consegna per il cibo.`
  - barracks: `Addestra Guerrieri e, dall'Età Feudale, Lancieri. +2 truppe al massimo.`
  - wall: `Sbarra il passaggio ai nemici; le tue unità ci passano. Solo vicino al castello o a un tuo avamposto. Maiusc + trascina per posarne una fila.`
  - archery: `Addestra Arcieri. +2 truppe al massimo.`
  - stable: `Addestra Cavalieri. +2 truppe al massimo.`
  - smith: `Migliora armi e armature dell'esercito.`
  - market: `Scambia legna, cibo e pietra con oro.`
  - tower: `Scaglia frecce sui nemici vicini e vede lontano. Solo vicino al castello o a un tuo avamposto.`
  - monastery: `Addestra Monaci guaritori. +2 truppe al massimo.`
  - siege: `Costruisce Arieti e Catapulte. +2 truppe al massimo.`
- Unit `desc` strings are in the units spec (assets.js:119-167).

### 2.3 Technologies (assets.js:326-395)

`TECH_IDS` order (bit index for network, tree layout): `age2, age3, carts, masonry, axes, picks, breeding, boots,
horseshoes, blades, shields, fletching, armor, maps, herbs, scriptures, engineering`.

| id | label | at | age | req | cost | time (s) | desc (verbatim) |
|---|---|---|---|---|---|---|---|
| age2 | Età Feudale | castle | 1 | | meat 250, gold 100 | 40 | Sblocca Arcieria, Scuderia, Fucina, Mercato, Torre, Lancieri e nuove ricerche. |
| age3 | Età dei Castelli | castle | 2 | age2 | meat 400, gold 300, stone 100 | 55 | Sblocca Monastero, Officina d'assedio e le ricerche avanzate. |
| carts | Carriole | castle | 2 | | wood 120, meat 80 | 28 | I Pedoni portano 5 risorse in più e camminano più svelti. |
| masonry | Muratura | castle | 3 | | stone 150, wood 100 | 36 | +30% di resistenza a tutti gli edifici. |
| axes | Asce affilate | depot | 1 | | meat 80, wood 60 | 24 | I Pedoni tagliano la legna il 30% più in fretta. |
| picks | Picconi d'acciaio | depot | 2 | axes | meat 80, wood 60 | 24 | I Pedoni estraggono oro e pietra il 30% più in fretta. |
| breeding | Allevamento | farm | 1 | | wood 80, gold 40 | 24 | Le Fattorie allevano il 50% più in fretta. |
| boots | Marcia forzata | barracks | 2 | | meat 100, gold 40 | 26 | Guerrieri, Lancieri e Arcieri si muovono il 12% più veloci. |
| horseshoes | Ferratura | stable | 2 | | meat 100, gold 80 | 28 | I Cavalieri sono il 15% più veloci. |
| blades | Lame temprate | smith | 2 | | meat 100, gold 80 | 30 | +4 attacco a Guerrieri, Lancieri e Cavalieri. |
| shields | Scudi rinforzati | smith | 2 | | wood 120, gold 60 | 30 | Guerrieri, Lancieri e Cavalieri subiscono metà del danno dalle frecce. |
| fletching | Impennaggio | smith | 2 | | wood 100, gold 60 | 30 | +3 attacco e più gittata ad Arcieri, Torri e Castello. |
| armor | Corazze | smith | 3 | shields | meat 120, gold 120 | 36 | Tutti i soldati subiscono 3 danni in meno per colpo. |
| maps | Cartografia | market | 2 | | wood 80, gold 60 | 24 | Unità ed edifici vedono 2 caselle più lontano. |
| herbs | Erbe curative | monastery | 3 | | gold 120 | 30 | I Monaci curano il 50% in più e ricaricano la riserva il 50% più in fretta. |
| scriptures | Sacre scritture | monastery | 3 | herbs | gold 100, wood 60 | 30 | I Monaci curano da più lontano e resistono il doppio. |
| engineering | Ingegneria | siege | 3 | | wood 150, gold 120 | 36 | +25% di danno ad Arieti e Catapulte. |

`icon` fields (rendering only) are listed in assets.js:328-392.

---

## 3. Faction data structure (factions.js:40-62)

`makeFaction(color, base, human, level = 'normal')` returns:

```
color        : 'Blue'|'Red'|'Yellow'|'Purple'|'Black'
colorIdx     : COLORS.indexOf(color)            // ribbon/sprite index [UI]
name         : 'Regno ' + COLOR_NAME[color]     // e.g. "Regno Blu", "Regno Rosso", "Regno Giallo", "Regno Viola", "Regno Nero"
base         : {x, y} cell of the base (from World.bases[i])
human        : bool — controlled by a player (local or network) instead of AI
alive        : true
idx          : 0 (set by G.start to position in G.factions; = network faction index)
conn         : 0 (network connection id of the controlling player; 0 = none)
res          : copy of START_RES  (floats! income is fractional)
age          : 1
tech         : {}   // id -> true for completed techs
busy         : {}   // id -> true for techs currently queued/in progress (in any building)
castle       : null // the castle building (set by setupFaction)
mods         : (see 6.1)
trades       : { wood: 0, meat: 0, stone: 0 }  // recent market trades per resource (float, decays)
pawnN, troopN: 0    // live counts (census + incremented on deploy)
pawnCap, troopCap: 0
has          : {}   // kind -> count of COMPLETED, alive buildings
sites        : {}   // kind -> count of UNFINISHED, alive buildings (construction sites)
pawns, soldiers : [] // arrays of live units (census)
houses       : 0    // counter used ONLY to pick house sprite variant (variant = houses++ % 3) [render]
rate         : { wood: 0, gold: 0, meat: 0, stone: 0 } // estimated income per minute (human factions only)
kills, razed : 0    // enemy units killed / enemy buildings destroyed (stats)
gathered     : 0    // total resources delivered + point income (stat; float)
threat: null, threatT: -99   // last enemy unit that hurt us + time (used by unit AI)
alertT       : -99  // last "under attack" alert time (ALERT_EVERY = 12 s throttle)
tickT        : Math.random()  // 1-second census timer phase (RNG!)
raidedT      : -999 // time the last AI wave was launched against this kingdom
alarm        : null // { x, y, t }: where/when the kingdom was last hit at home (alarmFor in entities)
seen         : null // Uint8Array(W*H) explored cells, human factions only (lazy)
seeT         : 0    // explore() timer
ai           : { level: AI_LEVEL[level], step: 0, thinkT: rand(1, 3), waveT: rand(...L.first), waveNeed: 4,
                 target: null, shuffleT: 20, mix: null, wave: null, dueT: null, attackedT: -999, lackT: 0, captureT: 60 }
```
Every faction (human too) gets an `ai` block; it is used only when `human === false` (also after a player
leaves, see 12).

### 3.1 setupFaction (factions.js:64-69)
```
f.castle = addBuilding(f, 'castle', f.base.x - 2, f.base.y - 1, done=true)   // 5x2 footprint, top-left cell
repeat 4: idle(deploy(f.castle, 'pawn'))
idle(deploy(f.castle, 'warrior'))
census(f)
```
`deploy` places the unit at `World.nearestPassable(b.x + rand(-0.3,0.3)*w*TILE, b.y1 + 24)` and gives it a
move-to-rally order, which `idle()` immediately replaces with idle-in-place. Start = 4 pawns + 1 warrior.

### 3.2 Match start (main.js:68-95)
- Kingdom order: `COLORS` shuffled randomly; `factions[i]` gets `World.bases[i]` ("le basi sono equivalenti: i regni vi sono distribuiti a sorte"). So all 5 kingdoms always exist; those without a human are AI.
- `f.idx = i`; for a human player: `f.conn = p.conn || 0`, and the local one becomes `G.player`.
- AI level for every AI faction = `setup.level || 'normal'` (single global difficulty).
- Then `setupFaction` for all, then announce `'Distruggi i castelli nemici!'` (dur 3.5, faction null) unless demo.
- Demo (no humans) = menu background battle, all AI.

---

## 4. Population caps & census (factions.js:201-218; entities.js:1176-1197)

`census(f)` [SIM] — recomputes from scratch:
```
pawns = live units of f with type=='pawn'; soldiers = other live units of f
for each live building of f:
  if !done: sites[kind]++ ; continue           // sites never give caps
  has[kind]++ ; pawnCap += def.pawns||0 ; troopCap += def.troops||0
pawnN = pawns.length ; troopN = soldiers.length
pawnCap = min(PAWN_MAX=30, pawnCap) ; troopCap = min(TROOP_MAX=50, troopCap)
```
Formulas:
- **pawnCap = min(30, 10 [castle] + 5 × completed houses)** → max reached with 4 houses.
- **troopCap = min(50, 6 [castle] + 8 × completed camps + 2 × each completed barracks/archery/stable/monastery/siege)**.

When census runs: every 1 s per faction (`tickT`, 7), on `completeBuilding`, after `demolish`, in `setupFaction`.
Additionally `deploy()` increments `pawnN`/`troopN` immediately (entities.js:1172) so two units finishing in the
same second can't overflow. `place` increments `f.sites[kind]` immediately (factions.js:349).
Unit deaths are NOT reflected until the next census (≤1 s).

`roomFor(f, type)` = `type==='pawn' ? pawnN < pawnCap : troopN < troopCap` (entities.js:1179). Population is checked
only when a trained unit is about to come out, NOT when enqueuing (train command only checks age, queue, cost).

---

## 5. Production queues & training speed (entities.js:1111-1197, factions.js:131-142, 377-393)

Each building has `queue: [{ unit: type, t, total } | { tech: id, t, total }]` (single mixed queue, max 5), `rally`,
`housedT = -99` (last "no room" toast time).

### 5.1 trainSpeed (entities.js:1177) — **+20% per extra building, up to ×2**
```
trainSpeed(n) = 1 + TRAIN_BONUS * clamp(n - 1, 0, 5)     // n=1→1.0, 2→1.2, 3→1.4, 4→1.6, 5→1.8, ≥6→2.0
```
`n = f.has[b.kind] || 1` = number of COMPLETED buildings of the same kind owned by the faction (census value).
Applies to every building that trains units, including the castle (only one castle → ×1). Applies to units only;
**research time is never accelerated**. Each building keeps its own queue (parallel production).

### 5.2 updateQueue(b, dt) — called from updateBuilding only when `b.done` (entities.js:1199-1202)
```
q = queue[0]; if none return
if q.t < q.total:
    q.t = min(q.total, q.t + dt * (q.unit ? trainSpeed(f.has[b.kind] || 1) : 1))
    return                                  // NOTE: completion is handled on the NEXT tick
if q.tech:
    queue.shift(); delete f.busy[q.tech]; applyTech(f, q.tech)
elif roomFor(f, q.unit):
    queue.shift(); deploy(b, q.unit)
elif f.human and G.time - b.housedT > 15:  // ready but no room: item stays at head, blocks the queue
    b.housedT = G.time
    full = (pawn ? f.pawnCap >= PAWN_MAX : f.troopCap >= TROOP_MAX)
    [EVT] toast to f: pawn ? (full ? 'Pedoni al massimo' : 'Servono più case')
                           : (full ? 'Truppe al massimo' : 'Servono più accampamenti')
```
A unit waiting for room keeps `t == total`; the UI shows `'In attesa di case'` (pawn) or `'In attesa di accampamenti'`
(others) in red (ui.js:409).

### 5.3 deploy(b, type) (entities.js:1162-1174)
- Spawn at `nearestPassable(b.x + rand(-0.3,0.3) * def.w * TILE, b.y1 + 24)`; fx dust if on screen.
- Rally `r = b.rally` (default `{ x: b.x, y: b.y1 + 72, t: null }`, entities.js:1121):
  - pawn & `r.t` is a resource node & `nodeValid(r.t)` → `gatherOrder(r.t)`.
  - pawn & `r.t` is a building, alive and **not done** → `{ k: 'build', b: r.t }`.
  - otherwise → move to `nearestPassable(r.x + rand(-34,34), r.y + rand(-22,22))`.
- `pawnN++` or `troopN++`.

### 5.4 Blocking reasons (strings verbatim)
```
ageLock(f, def)  = f.age < def.age ? 'Richiede: ' + AGE_NAME[def.age] : null
techLock(f, def) = ageLock(f, def) || (def.req && !f.tech[def.req] ? 'Richiede: ' + TECH_DEF[def.req].label : null)
trainBlock(f, b, type):
   d = UNIT_DEF[type]
   if !d || d.from !== b.kind || !b.done  → 'Non disponibile'
   ageLock(f, d) || (b.queue.length >= QUEUE_MAX ? 'Coda piena' : canAfford(f, d.cost) ? null : 'Risorse insufficienti')
techBlock(f, b, id):
   d = TECH_DEF[id]
   if !d || d.at !== b.kind || !b.done → 'Non disponibile'
   if f.tech[id] → 'Già ricercata'
   if f.busy[id] → 'Ricerca in corso'
   techLock(f, d) || (queue full ? 'Coda piena' : affordable ? null : 'Risorse insufficienti')
```
Order of checks matters for which message the player sees.

### 5.5 Resources helpers (factions.js:71-76)
`canAfford(f,cost)`: every `f.res[k] >= cost[k]`. `pay`: subtract. `refund`: add. Resources are floats (income from
points/trickle is fractional); comparisons are on floats; displays use `Math.floor`.

---

## 6. Research application (factions.js:78-123)

### 6.1 Modifiers (`f.mods`) defaults and effect of each tech (`techMods`, factions.js:81-99)

| mod | default | set by | value after | used by |
|---|---|---|---|---|
| `atk.warrior/lancer/knight` | 0 | blades | **+=** 4 each | unitDmg (+flat to dmg) |
| `atk.archer` | 0 | fletching | **+=** 3 | archer dmg; also tower/castle arrows (`15/14 + atk.archer`) |
| `range` | 0 | fletching | = 45 | archer range; tower (420) / castle (380) range |
| `armor` | 0 | armor | = 3 | `unitArmor`: units with `dmg` and not `siege` subtract 3 per hit (min dmg 1) |
| `arrowRes` | 1 | shields | = 0.5 | arrow damage × arrowRes on units with `block` (warrior, lancer) or knight |
| `siege` | 1 | engineering | = 1.25 | dmg × for siege units (ram, catapult) |
| `gather.wood` | 1 | axes | = 1.3 | work speed multiplier on node |
| `gather.gold`, `gather.stone` | 1 | picks | = 1.3 | |
| `gather.meat` | 1 | — | | |
| `carry` | 0 | carts | = 5 | load = CARRY + carry |
| `pawnSpeed` | 1 | carts | = 1.15 | pawn speed |
| `foot` | 1 | boots | = 1.12 | speed of units with `foot` (warrior, lancer, archer) |
| `horse` | 1 | horseshoes | = 1.15 | knight speed |
| `heal` | 1 | herbs | = 1.5 | monk heal amount and faith regen |
| `healRange` | 0 | scriptures | = 70 | monk range + |
| `monkHp` | 1 | scriptures | = 2 | monk max hp × |
| `breed` | 1 | breeding | = 1.5 | farm breeding timer speed |
| `buildHp` | 1 | masonry | = 1.3 | building max hp × (round) |
| `sight` | 0 | maps | = 2 | +cells to unit/building/point sight (also explore()) |

`techMods` is also run on network clients (net.js:245) so displayed stats follow research — in Rust the mods are
part of the replicated faction state (or recomputed from the tech bitset).

### 6.2 applyTech(f, id) (factions.js:101-123)
```
f.tech[id] = true
if id is 'age2' or 'age3':
    f.age = (id=='age2') ? 2 : 3
    [EVT] announce `${f.name}: ${AGE_NAME[f.age]}` (color f, 4 s, to everyone)
    if f.human: [EVT] play 'age' for f.idx
    return                                    // no mods, no unit refresh
techMods(f.mods, id)
for every unit u of f (including dead-not-yet-removed — JS loops G.units filtered by faction only):
    hp = unitMaxHp(f, u.type)                 // round(UNIT_DEF.hp * (monk ? monkHp : 1))
    u.speed = unitSpeed(f, u.type)            // speed * (pawn ? pawnSpeed : foot ? foot : knight ? horse : 1)
    if hp != u.maxHp: u.hp *= hp / u.maxHp; u.maxHp = hp     // proportional HP rescale
for every building b in G.buildings (the faction test is inside the if, same result as filtering by f):
    hp = round(b.def.hp * mods.buildHp)
    if b.faction === f && hp != b.maxHp: b.hp *= hp / b.maxHp; b.maxHp = hp   // includes construction sites
if f.human: [EVT] play 'research' for f.idx; announce 'Ricerca completata: ' + TECH_DEF[id].label (color f, 2.6 s, to f only)
```
New units/buildings created later read the mods at creation (`spawnUnit`, `addBuilding: maxHp = round(def.hp*buildHp)`).
Note: on research completion the tech leaves `busy` before `applyTech` (entities.js:1186-1188).

---

## 7. Per-tick faction update (factions.js:891-902) [SIM]

Called for every faction at the start of each sim step (main.js:294), before units and buildings:
```
updateFaction(f, dt):
  if !f.alive: return
  if !f.human:  for k in res: f.res[k] += f.ai.level.trickle * dt      // easy 0, normal 0, hard 0.6 /s each resource
  elif (f.seeT -= dt) <= 0: f.seeT = 0.25; explore(f)                   // human: explored-map update every 0.25 s
  if (f.tickT -= dt) <= 0:
      f.tickT = 1
      census(f)
      if f.human: f.rate = estimateRates(f)
      for k in trades: trades[k] = max(0, trades[k] - 0.04)              // market recovery
  if !f.human and (f.ai.thinkT -= dt) <= 0:
      f.ai.thinkT = level.think; aiThink(f, level.think)                 // AI spec
```
Sim step order (main.js:290-307): time += dt → factions → units → separateUnits → remove dead units → buildings
(queues, farms, tower/castle arrows) → remove dead buildings → resources (sheep etc.) → points → arrows → pickups → fx.

---

## 8. Market (factions.js:254-259, 408-420; panels.js:353-378; ui.js:381-389)

```
sellPrice(f, res) = max(20, 50 - f.trades[res] * 6)      // gold received for 100 units
buyPrice(f, res)  = min(320, 130 + f.trades[res] * 10)   // gold paid for 100 units
```
- Tradable resources: only `wood`, `meat`, `stone` (keys of `f.trades`). Gold is the currency.
- Each trade (buy OR sell of a resource) does `f.trades[res]++` (after paying with the current price) — selling and
  buying the same resource both worsen BOTH prices of that resource (shared counter).
- Recovery: every 1-s faction tick, `trades[res] = max(0, trades[res] - 0.04)` → one trade fully recovers in 25 s
  ("i prezzi tornano normali in qualche minuto" after many trades).
- Because `trades` is fractional, prices are fractional (e.g. 50 − 0.96×6 = 44.24). The JS adds/subtracts the exact
  fractional gold, and the UI prints the raw number (`${sellPrice}` / `${buyPrice}`), which can show decimals.
  **Open question**: keep fractional prices (faithful) or round for display; recommend keeping the sim exact and
  rounding only in the UI (`Math.round`), and note the floor-on-display for resources.
- Sell: requires `f.res[res] >= 100` else `'Risorse insufficienti'`; `res -= 100; gold += sellPrice`.
- Buy: requires `gold >= buyPrice` else `'Oro insufficiente'`; `gold -= price; res += 100`.
- Requires the command's building to be the faction's own `market` and `done` (else silently ignored).
- On success, if human: [EVT] play 'coin' for f.idx.
- Network: `trades` sent rounded to 2 decimals (net.js:183) so clients can show prices.

AI market use (factions.js:870-877, for reference): with a market, `extra` = first of wood/meat/stone with
`res > (gold < 80 ? 450 : 700)` and `sellPrice >= 40`; sell it if `gold < (level.trade ? 250 : 80)`; else if
`level.trade && meat < 150 && gold > 450 && buyPrice('meat') <= 170` buy meat.

---

## 9. Build-location rules (factions.js:144-199)

### 9.1 canPlace(kind, cx, cy, own = null) — terrain test (factions.js:147-159)
For every cell (x,y) of the footprint `[cx, cx+w) × [cy, cy+h)`:
- must be `World.inside(x,y)`;
- `mine = own !== null && own.includes(c) && World.block[c] === 1` (cell occupied only by the building being moved);
- reject if `(!mine && !World.passable(c))` — passable(c, side=0) = land && !cliff && block==0 (any building,
  including own walls, blocks; trees/nodes block as world defines);
- reject if `World.stair[c]` (stair cell) or `World.noBuild[c]` (reserved, e.g. stair access);
- all cells must have the same `World.level[c]` (no building across height levels).

In the new voxel terrain, "level" = height layer of the column; keep the rule "flat footprint on one layer, no ramps/stairs,
no reserved cells".

### 9.2 zoneBlock(f, kind, cx, cy) — territory rule (factions.js:161-166)
```
d = BUILD_DEF[kind]
x = cx + d.w/2 ; y = cy + d.h/2                         // footprint center, in cells (fractional)
r = d.defense ? DEFENSE_RADIUS(12) : BUILD_RADIUS(40)   // defense = tower, wall
c = f.castle
if c && hypot(x - (c.cx + c.def.w/2), y - (c.cy + c.def.h/2)) <= r: OK
for p in G.points: if p.owner === f && p.def.zone (outpost only) && hypot(x - p.gx, y - p.gy) <= OUTPOST_RADIUS(9): OK
       // p.gx = point cell x + 1, p.gy = point cell y + 1 (center of the 2x2 point). Outposts allow EVERYTHING, towers/walls included.
else reason:
  defense:  `Torri e palizzate: entro 12 caselle dal castello o 9 da un tuo avamposto`
  other:    `Troppo lontano: entro 40 caselle dal castello o 9 da un tuo avamposto`
  (template: `${d.defense ? 'Torri e palizzate' : 'Troppo lontano'}: entro ${r} caselle dal castello o ${OUTPOST_RADIUS} da un tuo avamposto`)
```
Distances are Euclidean in cell units, inclusive (`<=`).

### 9.3 placeBlock / moveBlock
```
placeBlock(f, kind, cx, cy) =
   ageLock(f, d)
   || (!canAfford(f, d.cost) ? 'Risorse insufficienti'
       : zoneBlock(f, kind, cx, cy) || (canPlace(kind, cx, cy) ? null : 'Qui non si può costruire'))

moveBlock(f, b, cx, cy) =
   b.kind === 'castle' ? 'Il castello non si sposta'
   : zoneBlock(f, b.kind, cx, cy) || (canPlace(b.kind, cx, cy, b.cells) ? null : 'Qui non può stare')
```
Client-only extra check during placement preview (control.js:421-423): the footprint center must be known in the
fog (`Fog.knows`) or `'Qui non si può costruire'`; the server does NOT check this. In Rust do the fog check
client-side for preview and optionally server-side using `f.seen` (equivalent data) — open question, see end.

### 9.4 findSpot(kind, x0, y0, rmin = 3, rmax = 18) — used by AI only (factions.js:178-199)
Rings r = rmin..rmax around (x0,y0); ring cells `(i,-r),(i,r),(-r,i),(r,i)` for i in -r..r (duplicates at corners),
**shuffled randomly**; candidate `cx = round(x0+dx - w/2)`, `cy = round(y0+dy - h/2)`; accept if `canPlace` AND the
1-cell border around the footprint is all inside and passable ("corridoio di una cella tutto intorno"). Returns
first hit or null. Note: does NOT check zoneBlock; the subsequent `place` command does (and may refuse).

---

## 10. Building lifecycle (economic parts) (entities.js:1076-1159, 879-891)

- `addBuilding(f, kind, cx, cy, done)`: `maxHp = round(def.hp * f.mods.buildHp)`; `hp = done ? maxHp : maxHp*0.1`;
  `progress = done ? def.work : 0`; `cd = rand(0,1)`; `breedT = 0`; `housedT = -99`; `inside = []`;
  if `def.sprites` (house) `variant = f.houses++ % 3`; `rally = { x: b.x, y: b.y1 + 72, t: null }`;
  `pasture = { x: b.x, y: b.y1 + 70 }`; block cells (owner = faction idx+1; walls are passable for own side).
- Geometry: `x = (cx + w/2)*TILE`, `y = (cy + h)*TILE - 4` (foot point), rect `x0=cx*TILE, y0=cy*TILE, x1=(cx+w)*TILE, y1=(cy+h)*TILE`.
- Construction (`buildStep`): per pawn working, per dt: `progress += dt`, `hp = min(maxHp, hp + maxHp*0.9*dt/work)`;
  complete when `progress >= work` (multiple builders stack linearly). Repair: `hp += maxHp*0.035*dt` per pawn.
- `completeBuilding`: `done = true`; dust fx; farm → spawn 2 sheep at its pasture; `census(f)`;
  if human and kind != wall: [EVT] play 'build' for f; announce `'Costruzione finita: ' + label` (2.2 s, to f only).
- `destroyBuilding(b, by)`: dead; free cells; sheltered pawns ejected; `delete f.busy[tech]` for queued techs in it
  (**queued items are NOT refunded on destruction**); explosion fx; if `by.faction` exists and is another faction:
  `bf.razed++` and, unless wall, loot: 3 pickups of 15 each, life 60 s, near `nearestPassable(b.x, b.y1 + 20)` ±30 px:
  farm → `meat, meat, wood`; tower → `stone, stone, wood`; others (castle included) → `wood, wood, gold`.
  If castle → `eliminateFaction(f, bf)`.
- Farm breeding (updateBuilding, only done farms): `breedT += dt * mods.breed`; when `>= 9`: `breedT = 0`, spawn a sheep
  at the pasture if fewer than 5 live sheep have `home === b.pasture`.
- Relocation (`relocateBuilding`): keeps hp, queue, progress; moves rally and pasture by the same delta; sheltered
  pawns move with it; dust fx.

---

## 11. Gathering economy (entities.js:765-854, 797-804) — summary needed for rates/screens

- Pawn on node: `workT += dt * mods.gather[res]`; when `workT >= JOB_TIME[res]`: amount = tree `min(cap, wood)`,
  sheep `15` (sheep killed), gold/stone `min(cap, amount)`; `cap = CARRY + mods.carry`; `carry = {kind, amount}`;
  `pauseT = 0.5`. Then walks to nearest drop site (`dropSiteAt`: own, done, alive, `def.drop` includes kind, nearest
  by rect distance) and `deposit`: `res[kind] += amount; gathered += amount`; if human [EVT] float `'+' + amount`
  with resource icon and 'coin' sound at the pawn; `pauseT = 0.4`.
- Drop sites: castle (all 4), depot (wood/gold/stone), farm (meat).
- Pickups (entities.js:1266-1293): every 0.2 s, the first unit in `G.units` order within |dx|≤32, |dy|≤28 of a landed
  pickup (after 0.4 s hop) takes it: `res[kind] += amount` (any unit of any faction, pawns included). Map chests:
  amount 50, never expire, [EVT] prop 'chest_open', and if human toast
  `` `Forziere trovato: +${amount} ${RES_LABEL[kind].toLowerCase()}` ``. Loot pickups expire after `life` s.
- Resource points (entities.js:214-231): owner of a point with `def.res` gets `+0.5 * dt` of that resource each step
  (and `gathered`). Outposts (`zone`) give no income but enable building (9.2).

### 11.1 Explored territory (human factions only) (factions.js:269-296)
`explore(f)` (every 0.25 s):
```
seen = f.seen || new Uint8Array(W*H)   (first = true when created)
mark(x, y, r): cx = floor(x/TILE), cy = floor(y/TILE)
   for cells (i,j) in the clamped square: if (i-cx)^2 + (j-cy)^2 <= r*r + r: seen = 1
if first && castle: mark(castle.x, castle.y, START_SIGHT = 15)
look(e, x, y, r): key = (floor(y/TILE)*W + floor(x/TILE)) * 32 + r ; if e.seenKey != key: e.seenKey = key; mark(x, y, r)
units of f (alive):        look(u, u.x, u.y, u.def.sight + mods.sight)
buildings of f (alive):    look(b, b.x, b.y - 30, (b.done ? def.sight : 2) + mods.sight)
points owned by f:         look(p, p.x, p.y - TILE, POINT_SIGHT(7) + mods.sight)
```
`explored(f, x, y)`: AI (or no `seen` yet) → always true; else `f.seen[cellAt(x,y)] === 1` (out of map → false).
This is the same set of cells the client fog shows as known; it is SIM state (pawns of human players only
auto-pick nodes in explored territory). The `seenKey` cache lives on the entity; only the owning faction's
explore() reads it, so one field per entity is enough.

### 11.2 nodeNear(f, res, p) (factions.js:299-308)
Nearest (Euclidean px to pawn `p`) node of `res` among `nodesOf(res)` (trees/golds/stones/sheep) such that
`nodeValid(n)` (tree wood>0, sheep alive, else amount>0), `nodeRoom(n)` (`users < (big ? 8 : NODE_USERS[kind])`),
`hypot(n - castle) <= WORK_RADIUS (2600 px)`, and `explored(f, n)`. Returns `{ node, d }` or null.
Requires `f.castle` non-null (faction alive).

### 11.3 estimateRates(f) — income per minute, humans only, every 1 s (factions.js:220-252)
Pure sim function (uses distance fields), result displayed in top bar and Workers screen, sent in snapshots.
```
rate = {wood:0, gold:0, meat:0, stone:0}; side = f.idx + 1
for u in f.pawns:
   o = u.order; n = (u.node && nodeValid(u.node)) ? u.node : null
   if o.k != 'gather': continue
   if n:
      site = dropSiteAt(f, n.x, n.y, o.res); here = cellAt(n.x, n.y); d = ∞
      if site:
         field = World.fieldFor(site, side, cellAt(u.x, u.y))   // distance field in TENTHS of a cell
         d = field[here]
         if d not finite and n.cells: d = min over k in n.cells, (dx,dy) in DIRS8 of field[k + dx + dy*W]
      u.trip = d finite ? max(0, d/10 - 0.7) + (n.kind=='sheep' ? 1.3 : 0) : -1      // cells per leg
      u.haul = n.kind=='sheep' ? 15 : min(CARRY + mods.carry, nodeLeft(n))
   elif !u.carry: continue                      // searching, doesn't count
   // (with carry but no node: reuse the last u.trip/u.haul)
   if !(u.trip >= 0): continue
   rate[o.res] += u.haul / (JOB_TIME[o.res] / mods.gather[o.res] + 2 * u.trip * TILE / u.speed + 0.9) * 60
flock = count of live sheep (any home, any owner) within WORK_RADIUS of castle and explored(f)
rate.meat = min(rate.meat, ((has.farm||0) * 60 / FARM_BREED * mods.breed + flock) * SHEEP_MEAT)
for points owned with def.res: rate[res] += 0.5 * 60    (= +30)
round each rate
```
`u.trip`, `u.haul` are cached per unit between calls. The 0.9 = sum of the 0.5 + 0.4 pauses. (Quirk preserved:
`flock` is a count, not a per-minute rate.)

---

## 12. Player commands — the complete list (factions.js:336-428)

Transport: `command(cmd, ...args)` (factions.js:425-428): if this client is remote (`Net.remote`) it sends
`{ t: 'c', c: cmd, a: args }` to the host and returns null (no immediate feedback); otherwise it calls
`execCommand(G.player, cmd, ...args)` and returns the refusal string (the UI shows it as a toast + 'deny' sound
via `say()`). Host side (net.js:128-134): accepts only while playing, `typeof c === 'string'`, `Array.isArray(a)`;
faction = the one with `conn === senderId` and alive; executes `execCommand(f, c, ...a.slice(0, 5))` inside
try/catch; a non-null refusal is sent back as [EVT] `['toast', f.idx, why]`.

`execCommand(f, cmd, a, b, c, d, e)` — returns refusal reason string or null. All validation is defensive: invalid
payloads are silently ignored (return null). **If `!f.alive` → null (no-op).**

Rust: model as `enum Command` with these exact variants and validation; preserve the refusal strings.

| cmd | payload | behavior |
|---|---|---|
| `order` | a = unit ids (array, ≤200), b = kind, c,d = x,y px, e = target id (0 = none) | `orderUnits(f, a, b, c, d, e)` (units spec). Kinds: `move`, `amove`, `stop`, `explore`, `attack`, `gather`, `build`, `drop`, `heal`. Ignored if `a` not array or length > 200. Always returns null. |
| `place` | a = building kind, b,c = top-left cell (integers), d = builder pawn ids (array) | see 12.1 |
| `jobs` | a = resource, b = target number of pawns (integer ≥ 0) | `setWorkers(f, a, b)` (12.2). Invalid → null. |
| `job` | a = pawn ids (array ≤200), b = resource | 12.3 |
| `recall` | a = soldier ids (array ≤200) | 12.4 |
| `train` | a = building id, b = unit type | 12.5 |
| `research` | a = building id, b = tech id | 12.5 |
| `cancel` | a = building id, b = queue index, c = expected unit type or tech id | 12.5 |
| `rally` | a = building id, b,c = x,y px (finite numbers), d = target id (node or own building) or 0 | 12.6 |
| `move` | a = building id, b,c = new top-left cell (integers) | 12.6 |
| `demolish` | a = building id | 12.6 |
| `trade` | a = building id (market), b = `'sell'`/`'buy'`, c = resource | section 8 |

For `train/research/cancel/rally/move/demolish/trade`: `bld = ownBuilding(f, a)` = entity with that id that is a
building, of faction f, not dead; else return null. Unknown `cmd` with a valid building → null.

### 12.1 place (factions.js:343-353)
```
if a not in BUILD_MENU or b,c not integers: return null
why = placeBlock(f, a, b, c); if why: return why
site = addBuilding(f, a, b, c, done=false)
pay(f, def.cost)
f.sites[a] = (f.sites[a]||0) + 1
for i in 0..w-1: fx 'dust2' at ((b+i+0.5)*TILE, (c+h)*TILE)
if d is array: orderUnits(f, d.slice(0, 200), 'build', 0, 0, site.id)
return null
```
No check that the faction has pawns (the Build screen checks `'Non hai pedoni'` client-side, panels.js:344).
Builder choice is done by the CLIENT (control.js:268-277 `builders`):
- selected pawns if any;
- else: for walls, if any pawn has act 6 (building) within 600 px of the spot, send `[]` (it will chain to the next site
  by itself);
- else nearest free pawns (act 0 idle or 8 dropping), then nearest gathering pawns (acts 3,4,5,11), take
  `def.work > 15 ? 2 : 1` (2 pawns for barracks/archery/stable/smith/market/tower/monastery/siege, 1 otherwise).
- A gathering pawn sent to build keeps `resume` = its gather order and returns to it after (entities.js:396-399, 858-877).
Wall line drag (Shift + drag, control.js:436-444): every cell on the line from the last placed cell to the cursor
gets its own `place('wall', cx, cy, builders)`, with builders only if pawns are selected.

### 12.2 jobs → setWorkers(f, res, n) — Workers screen sliders (factions.js:310-334)
```
pawns = all own live pawns
job = RES_KINDS.indexOf(res)
on = pawns with jobOf(u) == job     // jobOf: order (or, if sheltering, order.back) is gather of that res
free(u) = u.order.k == 'idle' || u.order.k == 'drop'
sort `on` with non-carrying first (stable sort: carry ? 1 : 0)
// remove excess: the first (on.length - n) of the sorted list, i.e. empty-handed pawns are removed first
for u in on.splice(0, max(0, on.length - n)):
    site = u.carry && dropSite(u, u.carry.kind)
    if u.order.k == 'shelter': u.order.back = { k: 'idle', x: u.x, y: u.y }   // stays free when it comes out
    elif site: setOrder(u, { k: 'drop', b: site })                            // deliver load first, then idle
    else: idle(u)
// add missing: each iteration picks the GLOBAL best (pawn, node) pair
for have = on.length; have < n; have++:
    best = null
    for u in pawns: if !free(u) continue
        near = nodeNear(f, res, u); if near && (!best || near.d < best.d): best = { u, node, d }
    if !best:
        return pawns.some(free) ? `${RES_LABEL[res]}: nessun posto libero nel territorio esplorato` : 'Nessun pedone libero'
    setOrder(best.u, gatherOrder(best.node))     // pawn now has k='gather' → no longer free
return null
```
Edge cases to preserve: partial success before a refusal is kept; node `users` is only incremented when the pawn
reaches the node (claim in gatherStep), so several pawns can be sent to the same 1-user sheep in one command (they
will redistribute on arrival). A pawn that is `drop`-ing counts as free and switches immediately (its load is
delivered first anyway because gatherStep delivers carried loads before working). `embolden` is NOT called here
(unlike `job`).
`gatherOrder(n) = { k: 'gather', res: NODE_RES[n.kind], first: n, px: n.x, py: n.y }`.

### 12.3 job — Units screen "send selected pawns to resource" (factions.js:357-367)
```
if a not array or len > 200 or b not in RES_KINDS: return null
sent = asked = 0
for u in ownPawns(f) (all own live pawns, in G.units order):
    if u.id not in a: continue
    asked++
    near = nodeNear(f, b, u)
    if near: embolden(u); setOrder(u, gatherOrder(near.node)); sent++
return asked && !sent ? `${RES_LABEL[b]}: nessun posto libero nel territorio esplorato` : null
```
`embolden(u)` (entities.js:941): if pawn and an enemy combat troop is within its sight + 2 cells, set `brave = true`,
`calmT = 0` (won't flee to shelter until 4 s of calm).

### 12.4 recall (factions.js:368-374)
```
c = f.castle; if a not array or len > 200 or !c: return null
ids = own live units with type != 'pawn' (scouts, monks, siege included) whose id ∈ a
orderUnits(f, ids, 'move', c.x, c.y1 + 130)     // plain move: they don't stop to fight
```
Client: Backspace shortcut / "Richiama alla base" / Units-screen "Richiama" (control.js:348-354). Client shows toast
`'Non hai truppe'` if none.

### 12.5 train / research / cancel (factions.js:377-393)
```
train:    why = trainBlock(f, bld, b); if why return why
          pay(f, UNIT_DEF[b].cost); bld.queue.push({ unit: b, t: 0, total: UNIT_DEF[b].time })
research: why = techBlock(f, bld, b); if why return why
          pay(f, TECH_DEF[b].cost); f.busy[b] = true; bld.queue.push({ tech: b, t: 0, total: TECH_DEF[b].time })
cancel:   q = bld.queue[b]
          if !q or (q.unit || q.tech) !== c: return null    // network safety: queue may have advanced meanwhile
          bld.queue.splice(b, 1)
          refund(f, q.tech ? TECH_DEF[q.tech].cost : UNIT_DEF[q.unit].cost)   // FULL refund, even if in progress
          if q.tech: delete f.busy[q.tech]
```
Cost is paid at enqueue time. Cancelling the head item resets progress (next item starts from its own t=0).

### 12.6 rally / move / demolish (factions.js:394-407)
```
rally:    if b or c not finite: return null
          t = d ? G.index.get(d) : null
          bld.rally = { x: b, y: c, t: (t && !t.dead && (NODE_RES[t.kind] || (t.isBuilding && t.faction === f))) ? t : null }
          // No check that bld trains anything (UI only offers it on done buildings with `trains`).
move:     if b,c not integers or (b == bld.cx && c == bld.cy): return null
          why = moveBlock(f, bld, b, c); if why return why
          relocateBuilding(bld, b, c)        // works for finished buildings AND construction sites; free of charge
demolish: if bld.kind == 'castle': return 'Il castello non si demolisce'
          if !bld.done: refund(f, bld.def.cost)      // site: full refund; finished building: nothing
          destroyBuilding(bld, null)                  // by=null → no loot, no razed++; queued techs freed from busy, queue lost (no refund)
          census(f)
```
Client sets `b.rally` locally too (control.js:209) for immediate display; client only sends a target id if it is a
node or an own UNFINISHED building (control.js:207). Rally UI: right-click on map with a done training building
selected, or "Punto di raccolta" (G).

### 12.7 Client-side order dispatch on right click (control.js:178-204) — for reference
- Target is enemy unit/building: army+monks → `order attack target`; others → `move`.
- Attack-move mode (X then click): all selected → `amove`.
- Target is a resource node: pawns → `gather`; others → `move`.
- Target is a building (own or neutral): pawns → `build` if `!done || hp < maxHp`, else `drop` if it has `def.drop`, else `move`; others → `move`.
- Target is a hurt unit and monks selected: monks → `heal`; others → `move`.
- Else everyone → `move`.
- Coordinates are rounded px; target id 0 when none.
Other senders: Units screen 'Ferma' → `order stop`; 'Invia' → `order amove x y 0`; E (scouts) → `order explore 0 0 0`;
F → `order stop`.

---

## 13. Victory, defeat, elimination, spectator

### 13.1 Elimination (factions.js:904-913; trigger entities.js:1158)
A kingdom is eliminated **only when its castle is destroyed** (`destroyBuilding` of kind castle → `eliminateFaction(f, by.faction)`).
Losing all units or other buildings does not eliminate.
```
eliminateFaction(f, by):
  if !f.alive: return
  f.alive = false
  every live unit of f: killUnit(u, null)          // no kill credit
  every live building of f: destroyBuilding(b, null)   // no loot, no razed
  for p in G.points: if p.owner === f: p.owner = null
                     if p.by === f: p.by = null; p.t = 0       // its capture progress is reset
  [EVT] announce `${f.name} è caduto!` (color f, 4 s, everyone)
  if by && by.human: [EVT] play 'win' for by.idx
  G.checkEnd()
```
Note the castle itself already counts for `razed`/loot via the original destroyBuilding call (attacker gets
`wood, wood, gold` ×15).

### 13.2 checkEnd (main.js:100-113) — mostly client/session level
```
alive = factions with alive
if demo: if alive <= 1 and demoT <= 0: demoT = 8 (restart the demo battle after 8 s); return
if spectate && alive.length <= 1: spectate = false; state = 'over'
if result already set: return
if !player.alive: result = 'lose'
elif alive.length == 1: result = 'win'          // only the local player left
if result: endT = 2.5; endTime = time; placing = null; clear selection; fog off (whole map visible); play sound result ('win'/'lose')
```
After `endT` (2.5 s) the state becomes `'over'` (main.js:330) unless spectating. Called on every elimination and, on
network clients, after every snapshot (net.js:287).
- Solo: when state becomes 'over' the sim stops (update runs only in state 'play'/'menu' or when hosting).
- Network host: the sim keeps running regardless of the host's own result (`Net.hosting`).
- In Rust: the authoritative sim only needs `alive` flags + an event; per-player win/lose is derived client-side:
  lose = own faction dead; win = own alive and it is the only alive faction. Server-side game end = ≤1 alive faction.

### 13.3 Game-over screen (ui.js:530-551) — strings verbatim
- Ribbon: `'VITTORIA!'` (win) / `'SCONFITTA'`.
- Line: win → `'I cinque regni sono tuoi.'`; else if exactly one alive → `` `Vince il ${alive[0].name}.` `` else `` `Il ${f.name} è caduto.` ``.
- Stats: `` `Tempo ${m}:${ss}    Nemici abbattuti ${f.kills}    Edifici distrutti ${f.razed}    ${AGE_NAME[f.age]}` `` (m = floor(endTime/60), ss 2-digit; endTime = G.time at result).
- Solo: buttons `'Rigioca'` (same setup) and `'Menu'`.
- Network: if more than one kingdom still alive, button `'Guarda'` → `G.spectate = true; G.state = 'play'`; button `'Esci'` → back to menu
  (leaves the match). If running and you are host: `'Sei l\'host: se esci la partita finisce per tutti.'`.

### 13.4 Eliminated spectator (ui.js:482-490, control.js:379, main.js:446-451)
- Only in network games. HUD replaced by `drawSpectator`: pause/menu button, sound toggle, minimap, announcements, ribbon
  `'Spettatore · frecce o minimappa per guardare'`.
- No input reaches the sim (Ctl.update returns early when `G.spectate || !G.player.alive`), management panels are not drawn.
- Camera moves freely with arrows/minimap. Fog stays off (set by checkEnd).
- When the battle ends (≤1 alive) the spectator goes to the game-over screen (`state = 'over'`).
- Host commands from an eliminated faction are ignored (`!f.alive` in net.js:130 and execCommand).

### 13.5 Player leaving / reconnect (net.js:137-148) — KNOWN BUG TO FIX IN THE PORT
Current JS: when a connection drops during a match, `Object.assign(f, { human: false, conn: 0 })`, all its soldiers
are idled, announce `` `${f.name}: il giocatore è uscito, ora lo guida l'IA` `` (4 s). There is no way back: a page
reload creates a new connection id, `join` is refused with `'Partita già in corso.'`, so the AI keeps the kingdom forever.
The user explicitly requires the Rust port to allow reconnection. Faction-level implications for the Rust design:
- Separate "controller" from faction identity: keep a stable player/session token (e.g. stored in the browser) bound
  to the faction slot, instead of a volatile `conn` id.
- On disconnect: mark the slot disconnected (optionally let the AI play it meanwhile, as now — `human = false` switches
  `updateFaction` to AI trickle/think and stops explore/rate); keep `f.seen`.
- On reconnect with the same token: set `human = true`, restore `conn`, send full state; AI state `f.ai` may be reset
  or kept. Note that while AI-controlled the faction gets `trickle` income (hard: 0.6/s) — decide if that is acceptable.

---

## 14. Boundary with the AI (factions.js:430-889) — owned by the AI spec

The AI calls the same command executor for economic actions, so these rules apply to it too:
`execCommand(f, 'place', kind, cx, cy, pawnIds)` (aiBuild, with builders = 1 or 2 nearest pawns by `def.work > 15`),
`execCommand(f, 'research', b.id, id)`, `execCommand(f, 'train', b.id, type)`, `execCommand(f, 'trade', market.id, 'sell'|'buy', res)`.
Unit movement is done with `orderUnits` directly (not via the command list), and pawn assignment with
`setOrder(p, gatherOrder(node))` (aiAssignPawns). AI helpers that are generic economy utilities and may be reused:
`freeBuilding(f, kind)` (done, alive building of kind with the shortest queue; factions.js:671-677 — also used by the
Units/Research/Market screens), `queued(f, test)` (count of queued units matching test across all own buildings,
factions.js:678-682), `nearestEnemyFaction(f)` (factions.js:530-538).
AI factions have no fog (`explored` returns true) and get `trickle` income per level (easy 0, normal 0, hard 0.6/s).
`AI_LEVEL` labels (UI): `'Facile'`, `'Normale'`, `'Difficile'`. AI announcement on wave launch: `` `${f.name} ti attacca!` `` (3.5 s, to target) + 'horn'.

---

## 15. Management screens logic (panels.js) — economic parts [UI, commands as above]

All screens are modal overlays; **the match keeps running underneath**. Shortcuts row (panels.js:9-18), titles/desc verbatim:
- U `'Unità'` — `'Tutte le tue unità in un elenco: le addestri, le spunti e cambi il loro lavoro.'`
- L `'Lavoratori'` — `'Quanti pedoni su ogni risorsa, con un cursore per ciascuna.'`
- I `'Ricerche'` — `'L\'albero delle ricerche: un livello per ogni era.'`
- K `'Mercato'` — `'Vendi e compra legna, cibo e pietra in cambio d\'oro.'`
- C `'Costruisci'` — `'Scegli un edificio e poi dove metterlo: ci va il pedone libero più vicino, o quelli selezionati.'`
- V `'Sposta edificio'` — `'Sposta l\'edificio selezionato, o il prossimo che scegli (clic, oppure Frecce e Invio); poi la nuova casella, allo stesso modo.'`
- Tab `'Tutte le truppe'` — `'Seleziona tutti i soldati, ovunque siano.'`
- Backspace (`Bksp`) `'Richiama alla base'` (red) — `'Tutte le truppe tornano davanti al castello senza fermarsi a combattere.'`

### 15.1 Workers screen ("Lavoratori", panels.js:202-248)
Data (client view, works on network clients because `u.act` and `u.job` are replicated):
```
pawns = own live pawns; N = max(1, pawns.length)
free     = count act ∈ {0 (Inattivo), 8 (Porta il carico)}
building = count act ∈ {6 (Costruisce), 7 (Ripara)}
hidden   = count act ∈ {12 (Al riparo), 13 (In fuga)}
moving   = count act == 1 (In marcia)
header: `Pedoni ${pawns.length}/${f.pawnCap}  ·  Liberi ${free}  ·  Al cantiere ${building}` + (hidden ? `  ·  Al riparo ${hidden}` : '') + (moving ? `  ·  In viaggio ${moving}` : '')
        (red when hidden > 0)
per resource i (RES_KINDS order) row:
   n   = pawns with u.job == i     (job counts sheltering/fleeing pawns on the job they'll return to)
   max = n + free                  (slider limit marker drawn at max/N)
   label RES_LABEL[k]; if rate: `+${rate}/min` (green) under it
   '−' button: if n > 0 → send(k, n - 1)
   '+' button: if free > 0 → send(k, n + 1) else toast 'Nessun pedone libero'
   bar fill = n / N; dragging the knob: want = clamp(round((mouse.x - tx) / tw * N), 0, max); if want != n → send(k, want)
   number n shown at right
send(k, v): skip if same (k, v) was sent less than 0.5 s ago (G.clock) — network latency debounce; else command('jobs', k, v), toast refusal
footer: `Liberi: ${free}` and text:
 'Alzando un cursore parte il pedone libero più vicino a quella risorsa; abbassandolo, i pedoni tolti restano liberi. I nuovi pedoni non vengono assegnati da soli.'
```
"Nearest idle pawn" rule = the global best pair in setWorkers (12.2). New pawns are not auto-assigned (they go to the
rally point / rally node).

### 15.2 Market screen ("Mercato", panels.js:353-378)
- Uses `market = freeBuilding(f, 'market')` (any done market).
- Header: scales icon, gold icon, `` `Oro: ${Math.floor(f.res.gold)}` ``.
- No market: `'Non hai ancora un Mercato.'` and `` `Si costruisce dall'${AGE_NAME[2]}: schermata Costruisci (tasto C).` `` (= "Si costruisce dall'Età Feudale: ...").
- Rows wood, meat, stone: label, `` `ne hai ${Math.floor(res)}` ``, buttons `` `Vendi 100:  +${sell}` `` (dimmed if res < 100) and
  `` `Compra 100:  −${buy}` `` (dimmed if gold < buy; note the U+2212 minus sign and two spaces after the colon).
  Clicks: `command('trade', market.id, 'sell'|'buy', k)`.
- Footer: `'I prezzi sono in oro. Ogni scambio li peggiora per un po\'.'`
- Market selection panel with empty queue (ui.js:381-389): per resource `` `${sell}/${buy}` ``, and (if wide)
  `'oro per vendere / comprare 100'`.

### 15.3 Units screen ("Unità", panels.js:82-163) — economic parts
- Header: `` `Pedoni ${pawns}/${f.pawnCap}  ·  Truppe ${others}/${f.troopCap}   —   + addestra  ·  − / + spunta una unità di quel tipo  ·  clic su un'unità o sul nome per spuntare` ``
- One row per UNIT_TYPES type; '+' trains in `freeBuilding(f, d.from)` (shortest queue); lock = no building →
  `'Richiede: ' + BUILD_DEF[d.from].label`, else `trainBlock`. Badge = number of that type queued in all own buildings.
  Tooltip title `'Addestra ' + label`, desc `d.desc + (queued ? ` In coda: ${queued}.` : '')`.
- Marked pawns → buttons per resource send `command('job', ids, k)`; label `` `Pedoni spuntati: ${n}` ``.
- Marked troops: `` `Truppe spuntate: ${n}` ``, 'Ferma' (`order stop`, all marked), 'Richiama' (recall), 'Invia' (send screen → `order amove`).
- Other strings: `'Tutte'`, `'Nessuna'`, `'Seleziona sulla mappa'`, tooltip `` `Spuntate ${picked} su ${total}: − ne toglie una, + ne aggiunge una. Poi, in basso, cosa farne.` ``,
  unit tile tooltip `` `${ACT_LABEL[act]}  ·  vita ${ceil(hp)}/${maxHp}` ``; send screen title `'Dove le mando?'`, text
  `` `${n} unità: clic sul punto d'arrivo, oppure Frecce e Invio. Ci vanno da sole, attaccando chi incontrano.  Esc: torna a Unità` ``,
  toast `` `${n} unità in marcia` ``.
- `ACT_LABEL` (entities.js:34-35): `['Inattivo', 'In marcia', 'In combattimento', 'Taglia legna', 'Estrae oro', 'Macella pecore', 'Costruisce', 'Ripara', 'Porta il carico', 'Cura i feriti', 'Di guardia', 'Estrae pietra', 'Al riparo', 'In fuga', 'In esplorazione']`.

### 15.4 Research screen ("RICERCHE", panels.js:252-320)
- `techState`: `b = freeBuilding(f, d.at)`; `lock = done||busy ? null : !b ? 'Richiede: ' + BUILD_DEF[d.at].label : techLock(f,d) || (queue full ? 'Coda piena' : null)`;
  `poor = !done && !busy && !lock && !canAfford`. Click (not done/busy) → `say(lock || command('research', b.id, id))`.
- Layout rows `TREE_ROWS = ['castle', 'depot', 'farm', 'barracks', 'stable', 'smith', 'market', 'monastery', 'siege']`, columns = ages 1..3
  with header `` `${'I'|'II'|'III'}  ·  ${AGE_NAME[a]}` ``; lines from `req` to tech; missing building row label `'da costruire'`.
- Tooltip notes: `'Già ricercata. '`, `'Ricerca in corso. '` prefixed to desc. Progress bar = `q.t / q.total` of the queue item.
- Footer: `'Clic su una ricerca per avviarla nell\'edificio che la offre  ·  rosso: mancano le risorse  ·  spento: manca un requisito'`.

### 15.5 Build screen ("COSTRUISCI", panels.js:324-349)
- Grid of BUILD_MENU (4 columns). Lock = `ageLock` (shows `AGE_NAME[d.age]` in red) else cost; `poor` → tooltip lock `'Risorse insufficienti'`.
- Click: `why = lock || (poor ? 'Risorse insufficienti' : no own pawn ? 'Non hai pedoni' : null)`; if ok, close panel and start placement.
- Footer: `'Scegli l\'edificio, poi dove metterlo (clic, o Frecce e Invio): ci va il pedone libero più vicino, o quelli che hai selezionato.'`

### 15.6 Building selection panel (ui.js:370-415)
- Not done: `` `In costruzione: ${floor(100 * progress / work)}%` ``.
- Empty queue, trains & not castle: second line = same > 1 ? `` `${label} ×${same}: addestramento +${round((trainSpeed(same)-1)*100)}%` `` : `'Un\'altra uguale: +20% di velocità'`
  (`same` = done, alive own buildings of the kind); first line hint `'Clic destro sulla mappa: punto di raccolta'` for trainers, else `def.desc`.
- Queue: one round button per item; tooltip desc `'Clic per annullare: le risorse tornano indietro.'`; click → `command('cancel', b.id, i, item.unit || item.tech)`.
- Context actions (control.js:296-306): `'Punto di raccolta'` (desc `'Poi clic sul terreno o su una risorsa (anche col tasto destro, direttamente).'`),
  `'Sposta'` (desc `'Scegli la nuova casella col mouse o con le frecce (Invio per posare), dentro il raggio in cui si può costruire.'`),
  `'Demolisci'` / `'Annulla cantiere'` (desc `'Abbatte l\'edificio, senza rimborso.'` / `'Rimuove il cantiere e restituisce le risorse.'`).
  Move mode errors: `'Il castello non si sposta'`, `'Clicca un tuo edificio'`, `'Scegli un edificio con le frecce'`.

### 15.7 Top bar (ui.js:174-200)
- Per resource: `floor(res)` and `` `+${rate}/min` ``; tooltip title RES_LABEL, desc
  `` `Rendita attesa: +${r} al minuto. Conta chi ci lavora adesso, quanto porta a ogni viaggio e quanto dura il giro fra la risorsa e il punto di consegna, più i punti conquistati.` ``
- Pawns `` `${np}/${pawnCap}` `` (red if np >= cap), tooltip `'Pedoni'`, `` `${np} su ${pawnCap} posti. Ogni Casa ne aggiunge 5, fino a 30.` ``
- Troops `` `${nt}/${troopCap}` ``, tooltip `'Truppe'`, `` `${nt} su ${troopCap} posti. Ogni Accampamento ne aggiunge 8, ogni edificio che addestra truppe 2, fino a 50.` ``
- Age name and match clock `mm:ss`. Idle pawns button tooltip `'Pedoni inattivi'` / `'Seleziona il prossimo pedone senza un compito.'`.
- No-selection panel (ui.js:450-454): `` `Pedoni ${n}   Truppe ${n}   Punti conquistati ${mine}/${total}   Nemici abbattuti ${kills}   Edifici distrutti ${razed}` `` and `` `Regni rimasti: ${names joined ', '}` `` (COLOR_NAME).

### 15.8 Test mode (solo only, F4; panels.js:384-398) — economic cheat
`TEST_GIFT = 1000`: buttons `` `+${TEST_GIFT}` `` per resource (adds directly to `f.res`, bypassing commands) and
`` `+${TEST_GIFT} di tutte e quattro` ``; plus freeze time and fog toggle. Strings: `'Modalità test'`,
`'Solo nelle partite da soli. Si apre e si chiude con F4.'`, `'Tempo fermo: fallo ripartire'` / `'Ferma il tempo'`,
`'Togli la nebbia: tutta la mappa in vista'` / `'Nebbia tolta: rimettila'`,
`'A tempo fermo puoi guardare in giro e dare ordini: partono quando lo fai ripartire.'`.
In Rust solo mode this should be a local-only debug command (not accepted by the network server).

---

## 16. Network replication of faction state (net.js:181-185, 237-248)

Per faction per snapshot: `alive, human, floor(wood), floor(gold), floor(meat), floor(stone), pawnCap, troopCap, age,
bits(tech), bits(busy), kills, razed, r2(trades.wood), r2(trades.meat), r2(trades.stone), rate.wood, rate.gold, rate.meat, rate.stone`
(bit j = TECH_IDS[j]). Clients rebuild `pawns`/`soldiers` from the unit rows each snapshot and apply `techMods` for new
tech bits. Building rows carry the queue as `[code, t, total]`, `code = UNIT_TYPES index` or `100 + TECH_IDS index`;
clients advance `queue[0].t` locally between snapshots for a smooth bar. The Rust protocol can be different but must
carry the same information (the UI uses all of it).

---

## 17. Presentation events emitted by this layer [EVT]

| Event | When | Recipient |
|---|---|---|
| say `${name}: ${AGE_NAME}` 4 s | age up | all |
| play 'age' | age up, human | f |
| play 'research' + say `Ricerca completata: ${label}` 2.6 s | tech done, human | f |
| play 'coin' | trade, deposit, pickup (human) | f |
| float `+N` resource icon | deposit/pickup (human) | f |
| toast `Forziere trovato: +50 legna` etc. | chest (human) | f |
| toast pop messages (5.2) | ready unit without room, every ≥15 s (human) | f |
| play 'build' + say `Costruzione finita: ${label}` 2.2 s | building done (human, not wall) | f |
| say `${name} è caduto!` 4 s | elimination | all |
| play 'win' | eliminator is human | eliminator |
| toast refusal reason | network command refused | f |
| say `Distruggi i castelli nemici!` 3.5 s | match start | all |
| fx 'dust2' / 'boom2' | place / complete / move / destroy | all (visual) |

`G.announce(text, faction, dur=3, to=null)` = `['say', to ? to.idx : -1, text, faction ? faction.idx : -1, dur]`; max 4
queued banners on screen.

---

## 18. Determinism notes for the Rust sim

Replace with seeded sim RNG: `makeFaction` (`tickT`, `ai.thinkT`, `ai.waveT`), `findSpot` ring shuffle
(`sort(() => Math.random() - 0.5)` — a biased shuffle; any proper seeded shuffle is fine), `deploy` spawn jitter and
rally jitter, `addBuilding` `cd`/`fireSeed`, `chooseTarget`, `destroyBuilding` loot positions, block chance in `hurt`,
color shuffle in `G.start`. Iteration orders that affect results: `G.units` order (pickups, setWorkers ties, `job`),
`G.buildings` order (dropSiteAt ties, freeBuilding ties keep the first), `RES_KINDS` order, `BUILD_MENU`, `TECH_IDS`.
Float accumulations (`res`, `trades`, `q.t`) — use f32/f64 consistently on server and client (solo uses the same crate).

---

## Open questions / ambiguities

1. Market prices are fractional (shown with decimals in the JS UI). Keep exact in sim; round in UI? (recommended).
2. Server does not check fog for `place`; the client preview requires the cell to be known. Add a server check
   using `f.seen` (cheap, prevents placing blind via a modified client)?
3. Completion of a queue item happens one tick after `t` reaches `total` — harmless; keep or simplify.
4. `estimateRates` meat cap mixes a per-minute rate with an instantaneous flock count — faithful port keeps it.
5. Reconnect design (13.5) is new behavior requested by the user; decide whether AI plays a disconnected kingdom
   (with trickle) or the kingdom simply idles until the player returns, and for how long the slot is reserved.
6. `jobs`/`job` pick nodes without immediately claiming them; several pawns can target the same small node in one
   command — keep (they re-pick on arrival).

---

## Addenda (critic)

Resolutions of the open questions, verified in the code.

**E1. Fractional market prices and resources.** `f.trades[k]` is incremented by 1 per trade (factions.js:418) and
decays by `0.04` each 1-second faction tick (factions.js:899), so it is fractional; `sellPrice = max(20, 50 - trades*6)`
and `buyPrice = min(320, 130 + trades*10)` (factions.js:258-259) are therefore fractional too, and `f.res.gold`
receives/loses fractional amounts (factions.js:412, 416). The JS UI prints them raw (`` `Vendi 100:  +${sell}` ``,
panels.js:370; `` `${sellPrice}/${buyPrice}` `` ui.js:386) — e.g. `+44.24` or float noise like `44.239999999999995`.
Decision: sim keeps `f64` for `res` and `trades` (exact JS arithmetic: IEEE double, same operation order);
UI displays prices with `round()` (cosmetic fix of the float noise) and resources with `floor()` as now
(ui.js:183, panels.js:367). Snapshots send `floor(res)` and `trades` rounded to 2 decimals (net.js:182-183), so
client-side price previews can differ by < 0.1 from the server; the server's value is the one applied.
Cross-platform bit-exactness between native server and WASM is **not** required (server-authoritative; solo runs
only in the browser), so `f64` with `hypot/sin/cos/atan2` is fine.

**E2. Fog check on placement — FIX: validate on the server too.** Client preview requires
`Fog.knows(centre)` for *new* placements (control.js:421-423: centre = `((cx + w/2)*TILE, (cy + h/2)*TILE)`), not for
moves. `execCommand('place')` (factions.js:339-347) does not check it. The sim already owns an equivalent per-faction
explored grid `f.seen` (filled by `explore`, factions.js:271-290, same disc formula `(i-cx)²+(j-cy)² <= r²+r` as
`Fog.reveal`, control.js:42-48, and the same `START_SIGHT = 15` disc around the castle). Rust: in `place`, for human
factions, reject with the existing string `'Qui non si può costruire'` when `!explored(f, centre)`. AI factions are
exempt (`explored` returns true when `!f.human`, factions.js:292-295). Make the client's fog "known" layer come from
the sim's `f.seen` (one source of truth) so preview and server agree.

**E3. Queue item comes out one tick after its timer fills — keep.** `updateQueue` (entities.js:1181-1197) first
clamps `q.t` to `q.total` and returns; the next call shifts/deploys. At a 20 Hz tick this is a 50 ms delay; it also
gives the "no room" toast its natural retry cadence (one check per tick, toast throttled by `housedT` 15 s). Keep for
parity; the UI shows a full bar during that tick.

**E4. Food estimate cap** — keep the exact formula (see 03b addenda G5).

**E5. Reconnect and AI takeover** — see 08-net addenda N1 (final policy) and 04b addenda A1 (AI state fast-forward).

**E6. Workers commands without reservation** — keep (see 03b addenda G7).

**E7. Client-side derived data.** Clients never receive `has`, `sites`, `pawnN`, `troopN` (net.js:237-248 sets only
caps, res, age, tech/busy, kills, razed, trades, rates; `pawns/soldiers` are rebuilt from the unit rows,
net.js:266). Verified that the JS UI does **not** read `f.has`/`f.sites`/`pawnN`/`troopN` (only `entities.js`/AI do):
the HUD uses `f.pawns.length`/`f.soldiers.length` vs `pawnCap/troopCap` (ui.js:188-193), locks use `age`, `tech`,
`busy`, `res`, `queue.length` (factions.js:122-142), and "has a building" checks iterate `G.buildings`
(`freeBuilding`, factions.js:671). So there is no client/server disagreement except the `floor(res)` vs fractional `res`
rounding, which only makes the client more conservative (costs are integers). The Rust snapshot may still include
`has/sites` for convenience.

**E8. `findSpot` ring shuffle.** `ring.sort(() => Math.random() - 0.5)` (factions.js:192) is a biased,
comparator-inconsistent shuffle. In Rust **never** pass a random comparator to `sort_by` (since Rust 1.81 it may
panic on an inconsistent order); use Fisher–Yates with the sim RNG. Only the distribution "try ring cells in random
order" must be preserved.
