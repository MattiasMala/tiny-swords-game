# 03b — Economy, gathering, sheep, shelter, construction, buildings, palisades, towers, capture points, projectiles

Porting spec for the Rust rewrite (shared sim crate). Source of truth: `js/entities.js` (1407 lines) plus the
helpers it depends on in `js/factions.js`, `js/world.js`, `js/assets.js`, `js/main.js`, `js/control.js`,
`js/ui.js`, `js/panels.js`. Unit combat/movement (soldiers, monks, scouts, formation, `hurt` internals) is
covered by another spec; it is referenced here only where economy code calls it.

Conventions:
- `TILE = 64` px (`assets.js:4`). World coordinates are pixels; cell index `c = y*W + x`.
  `World.cx(c) = (c % W)*TILE + 32`, `World.cy(c) = floor(c / W)*TILE + 32` (`world.js:40-41`).
- `dt` = simulation step in seconds. All timers below are in seconds, distances in px unless "cells/tiles".
- `rand(a,b)` = uniform float in [a,b). `Math.random()` is used freely inside sim code: the Rust sim MUST use
  its own seeded deterministic RNG for all of these (see §17).
- "human" = faction controlled by a player (local or remote); otherwise AI.
- Order of the per-step update (`main.js:290-307`): `World.refresh = 2; World.fresh = 2;` → `updateFaction` for
  each faction → `updateUnit` for each live unit → `separateUnits()` → drop dead units → `updateBuilding` for each
  live building → drop dead buildings → `updateResources` → `updatePoints` → `updateArrows` → `updatePickups` →
  `updateFx` (presentation) → `updateView` (presentation). Keep this order: it affects tie-breaks.

---

## 1. Constants (entities.js:4-35 and others)

| Name | Value | Meaning | Ref |
|---|---|---|---|
| `JOB_TOOL` | `{wood:'Axe', gold:'Pickaxe', stone:'Pickaxe', meat:'Knife'}` | tool shown/animated per resource | entities.js:5 |
| `JOB_TIME` | `{wood:3.0, gold:3.4, stone:3.4, meat:20}` | seconds of work to produce one load (before gather mods) | :8 |
| `NODE_USERS` | `{tree:2, gold:3, stone:3, sheep:1}` | max pawns working the same node | :9 |
| `BIG_USERS` | 8 | max pawns on a big (2x2) gold/stone deposit | :10 |
| `NODE_RES` | `{tree:'wood', gold:'gold', stone:'stone', sheep:'meat'}` | node kind → resource | :11 |
| `LOAD_NAME` | `{wood:'Wood', gold:'Gold', meat:'Meat', stone:'Stone'}` | suffix of carry animation | :12 |
| `CARRY` | 10 | base carry capacity | :13 |
| `SHEEP_MEAT` | 15 | meat per sheep (always, ignores capacity) | :13 |
| `TREE_WOOD` | 40 | wood per tree (and amount after regrow) | :13 |
| `GOLD_MAX`, `STONE_MAX` | 240, 300 | declared but UNUSED (world gen uses literals 240/300) | :13 |
| `FARM_BREED` | 9 | s between sheep births at a farm | :14 |
| `FARM_SHEEP` | 5 | max sheep around a farm (homed to its pasture) | :14 |
| `TREASURE` | 50 | resources in a map chest | :15 |
| `BUILDING_ARMOR` | 0.4 | buildings take 40% of damage | :16 |
| `REPAIR_RATE` | 0.035 | fraction of maxHp per second repaired per pawn | :17 |
| `ALERT_EVERY` | 12 | s between two "under attack" alerts to the same kingdom | :18 |
| `SHELTER_CAP` | 5 | pawns a house can hide (castle unlimited) | :22 |
| `CALM_TIME` | 4 | s without enemy troops in sight before pawns go back to work | :23 |
| `POINT_INCOME` | 0.5 | resources/s from a captured point (= 30/min) | :29 |
| `CAPTURE_TIME` | 8 | s of sole presence to capture | :30 |
| `CAPTURE_RADIUS` | 300 | px around point where troops count | :31 |
| `POINT_SIGHT` | 7 | tiles seen by an owned point | :32 |
| `TRAIN_BONUS` | 0.2 | +20% training speed per extra building of same kind, max +100% | :33 |
| `PAWN_MAX`, `TROOP_MAX`, `QUEUE_MAX` | 30, 50, 5 | caps | factions.js:4 |
| `WORK_RADIUS` | 2600 | px from castle in which pawns are auto-sent to work | factions.js:6 |
| `DEFENSE_RADIUS`, `BUILD_RADIUS`, `OUTPOST_RADIUS` | 12, 40, 9 (cells) | placement zones | factions.js:9 |
| `START_SIGHT` | 15 | cells around castle explored at start | control.js:10 |
| `CROSS_COST` | 30 | extra field cost (tenths of cell) per own-building cell crossed | world.js:18 |
| `FIELD_CACHE` | 128 | distance fields kept in cache | world.js:15 |
| `FIELD_REFRESH` | 2 | stale-but-usable fields recomputed per step | world.js:16 |
| `FIELD_NEW` | 2 | group units per step that switch to their own path | world.js:17 |
| `DIRS8` | `[[1,0],[-1,0],[0,1],[0,-1],[1,1],[-1,1],[1,-1],[-1,-1]]` (3rd elem = cost 1/1.414) | neighbour order | world.js:14 |
| `RES_KINDS` | `['wood','gold','meat','stone']` | resource index order (job index) | assets.js:415 |
| `RES_LABEL` | `{wood:'Legna', gold:'Oro', meat:'Cibo', stone:'Pietra'}` | UI names | assets.js:417 |
| Initial `G.sheepT` | 5 | first wild-sheep respawn check | main.js:43 |
| Initial `G.pickT`, `G.pointT` | 0, 0 | | main.js:43 |

Activity labels (`ACT_LABEL`, entities.js:34-35), index = `u.act` (keep verbatim, Italian):
`0 'Inattivo', 1 'In marcia', 2 'In combattimento', 3 'Taglia legna', 4 'Estrae oro', 5 'Macella pecore',
6 'Costruisce', 7 'Ripara', 8 'Porta il carico', 9 'Cura i feriti', 10 'Di guardia', 11 'Estrae pietra',
12 'Al riparo', 13 'In fuga', 14 'In esplorazione'`.
`RES_ACT = {wood:3, gold:4, meat:5, stone:11}` (panels.js:28).

Pawn unit def (assets.js:116-118): `hp 50, speed 92, sight 5, cost {meat:50}, time 13, from 'castle', age 1`.

---

## 2. Resource nodes (entities.js:104-191)

### 2.1 Data structures
All nodes are tracked in `G.index` by `id` (ids are allocated in the same order on host and client — `NEXT_ID`
reset to 1 in `G.reset`, main.js:40 — orders reference nodes by id; Rust must keep deterministic id allocation
for scenery: trees, then golds (small then big), then stones (small then big), then points; main.js:51-54).

Tree (`makeTree(c, v)`, :106-111):
```
{ id, kind:'tree', c, cells:[c], v /*variant 0..3*/, x: cx(c), y: cy(c)+22, sy: same,
  wood: 40, regrow: 0, users: 0, phase: random*8 /*render only*/ }
```
Gold (`makeGold(c, amount, big)`, :113-119) / Stone (`makeStone`, :120-126):
```
x = cx(c) + (big ? 32 : 0)
y = cy(c) + (big ? 32 : (gold ? 0 : 6))        // small stone is 6 px lower
sy (draw sort) = y + (big ? 56 : gold ? 18 : 20)
cells = big ? quad(c) /* [c, c+1, c+W, c+W+1] */ : [c]
{ id, kind:'gold'|'stone', c, cells, x, y, amount, max: amount, big, users:0, regen:0, shine (gold only, render) }
```
World amounts (world.js:616, 687): small gold 240, small stone 300, big gold 1500, big stone 1800.

Sheep (`makeSheep(home)`, :128-136):
```
{ id, kind:'sheep', home /*object {x,y}; compared BY IDENTITY*/, x: home.x+rand(-90,90), y: home.y+rand(-60,60),
  state:'idle', t: rand(0.5,3), animT:0, flip: random<0.5, tx:0, ty:0, users:0, dead:false }
if !World.passable(cellAt(x,y)) → (x,y) = World.nearestPassable(home.x, home.y)
push to G.sheep
```
`home` is either a wild meadow `{x,y}` (`World.meadows`, world.js:622) or a farm's `b.pasture` object.

### 2.2 Helpers (:137-150)
- `nodeValid(n)`: tree → `wood > 0`; sheep → `!dead`; gold/stone → `amount > 0`.
- `nodeLeft(n)`: tree → `wood`; sheep → `15`; else `amount`.
- `nodeRoom(n)`: `users < (big ? 8 : NODE_USERS[kind])`.
- `nodesOf(res)`: wood→G.trees, gold→G.golds, stone→G.stones, meat→G.sheep.
- `chopTree(t, amount)`: `wood -= amount; if wood <= 0 { wood = 0; regrow = 110; fx 'dust2' at tree }`.
- `killSheep(s)`: `dead = true; G.index.delete(id); fx 'dust1'`. (Removed from `G.sheep` at the end of
  `updateResources`.)

### 2.3 `updateResources(dt)` (:160-191)
```
for tree: if wood <= 0 { regrow -= dt; if regrow <= 0 { wood = 40; fx 'dust1' (if on screen) } }
twinkle(dt)                                   // gold shine: PRESENTATION ONLY (each player its own)
for gold & stone (separately, same rule):
   if amount < max { regen += dt; if regen > 25 { regen = 0; amount = min(max, amount + 10) } }
   // depleted (amount 0) deposits ALSO regenerate: nodes are never permanently gone.
   // note: regen is only accumulated while amount < max, never reset when full.
for sheep s:
   s.animT += dt; s.t -= dt
   if s.state == 'move':
       d = dist(s, (tx,ty))
       if d < 4 || s.t <= 0: state='idle'; t=rand(1,3); animT=0
       else: World.tryMove(s, dx/d*38*dt, dy/d*38*dt); flip = dx < 0      // sheep speed 38 px/s
   else if s.t <= 0:
       if s.state == 'idle' && random < 0.45: state='grass'; t=1.2; animT=0
       else: state='move'; t=4; animT=0; tx = home.x+rand(-120,120); ty = home.y+rand(-80,80)
       // from 'grass' it ALWAYS goes to 'move'
G.sheep = filter(!dead)
G.sheepT -= dt; if <= 0:                       // wild meadow repopulation
   G.sheepT = 10
   m = random meadow (pick(World.meadows))
   if count(sheep with home === m) < 3: makeSheep(m); fx 'dust1'
```
Sheep `tryMove` has no faction → side 0: sheep are blocked by every building and every palisade
(world.js:68-72). Initial population: 3 sheep per meadow (main.js:76).

Sheep state machine: `idle (1-3s) → {grass 1.2s (45%) | move ≤4s (55%)}`, `grass → move`, `move → idle` on arrival
(<4 px) or 4 s timeout. States are replicated to clients for animation (net.js:178).

### 2.4 Treasure chests / pickups (:94-102, :1266-1293)
`dropPickup(kind, x, y, amount, life, chest=false)`:
```
a = rand(0, 2π); r = chest ? 0 : rand(24,54)
{ id, kind, amount, chest, x, y, sx:x, sy0:y, tx: x+cos(a)*r, ty: y+sin(a)*r*0.6 + (chest?0:10),
  t: chest ? 1 : 0, life, hop:0 }
```
- Map chests: one per `World.treasures` entry, at `(cx(c), cy(c)+6)`, amount 50, `life = Infinity`, kind cycles
  through `RES_KINDS` in generation order (main.js:77, world.js:701).
- Loot (see §9.5): amount 15, life 60.

`updatePickups(dt)`:
```
scan = (G.pickT -= dt) <= 0; if scan: G.pickT = 0.2
for p:
  p.t += dt; p.life -= dt
  if p.t < 0.4:                        // "hop" to landing spot, cannot be picked meanwhile
     k = p.t/0.4; p.x = lerp(sx,tx,k); p.y = lerp(sy0,ty,k); p.hop = sin(k*π)*30; continue
  p.hop = 0
  if !scan: continue
  for u in G.units (array order):
     if u.dead || u.inside || |u.x-p.x| > 32 || |u.y-p.y| > 28: continue
     // ANY unit type of ANY faction picks it up (first in array order wins)
     p.life = 0; f = u.faction; f.res[p.kind] += p.amount      // NOT added to f.gathered
     if p.chest: emit ['prop','chest_open', round(p.x), round(p.y)]   // permanent open-chest decor for everyone
     if f.human: float '+amount' at (u.x, u.y-80) icon kind; play 'coin';
                 if chest: toast `Forziere trovato: +${amount} ${RES_LABEL[kind].toLowerCase()}`
     break
G.pickups = filter(life > 0)
```
Rendering: loot blinks when `life < 5` (`floor(life*6) % 2`), drawn 1.5× with a small bob (:1406-1409).

---

## 3. Pawn orders relevant to economy (entities.js:272-288)

Order objects (`u.order`), economy subset:
- `{k:'idle', x, y}`
- `{k:'move', x, y, ...}` (pawns just `travel` there then `idle`, :977)
- `{k:'gather', res, first, px, py}` — `first` = node chosen by the player/rally (or null), `(px,py)` = search
  origin. Created by `gatherOrder(n) = {k:'gather', res: NODE_RES[n.kind], first: n, px: n.x, py: n.y}` (:288).
- `{k:'build'|'repair', b, resume}` — `resume` = gather order to go back to afterwards (or null).
- `{k:'drop', b}` — just deliver the carried load to building b, then idle.
- `{k:'shelter', b, back, calm}` — hiding; `back` = order to resume; `calm` = seconds without threats.

`claim(u, n)` (:273-278): a pawn "books" a node: `if u.node === n return; if u.node: u.node.users--; u.node = n;
if n: n.users++`.

`setOrder(u, o)` (:282-286) — every order change goes through it:
```
if u.inside: leaveShelter(u)
claim(u, null)                       // releases booked node
u.order = o; target=null; skip=null; workT=0; bestD=Infinity; stallT=0; breachT=-99
```
Consequences: any new order (including fleeing to shelter) loses current work progress (`workT`) and the
booking; the CARRIED LOAD (`u.carry`) is kept.

`idle(u)` = `setOrder(u, {k:'idle', x:u.x, y:u.y})`.

Pawn-related fields on a unit (`spawnUnit`, :250-268): `carry:null|{kind, amount}`, `tool:null|'Axe'|...|'Hammer'`,
`workT:0`, `pauseT:0`, `node:null`, `skip:null|Set<nodeId>`, `inside:null|building`, `brave:false`,
`fearT: random*0.4`, `lookT:0`, `calmT:0`, `act:0`, `job:-1`, plus `trip`, `haul` written by `estimateRates`.

How player orders reach pawns (`orderUnits`, :353-406, kind = right-click result from control.js:178-204):
- First, if the faction is human, `embolden(u)` for every ordered unit (§6.5).
- `'gather'` on a node with `NODE_RES[t.kind]` and `nodeValid(t)` → `setOrder(u, gatherOrder(t))` (pawns only).
- `'build'` on an own building → `{k: t.done ? 'repair' : 'build', b: t, resume}` where
  `o = (u.order.k === 'shelter' ? u.order.back : u.order)`, `resume = o.k === 'gather' ? o : (o.resume || null)`.
- `'drop'` on own finished building → `{k:'drop', b:t}`.
- Right-click on own building (control.js:195-198): `work = !t.done || t.hp < t.maxHp`; pawns get `'build'` if
  work, else `'drop'` if `t.def.drop`, else `'move'`; non-pawns get `'move'`.
- Right-click on a node: pawns `'gather'`, others `'move'` (control.js:192-194).

---

## 4. Gathering (entities.js:746-854)

### 4.1 Drop-off selection (:748-758)
```
dropSiteAt(f, x, y, kind):
  nearest (by rectDist(x,y,b)) building b with b.faction == f && b.done && !b.dead && b.def.drop includes kind
dropSite(u, kind) = dropSiteAt(u.faction, u.x, u.y, kind)   // from the PAWN's current position
rectDist(px,py,b) = hypot(max(b.x0-px, 0, px-b.x1), max(b.y0-py, 0, py-b.y1))   (:39-41)
```
Drop sets (assets.js): castle `['wood','gold','meat','stone']`, depot `['wood','gold','stone']`, farm `['meat']`.
Distance is straight-line to the rectangle, not path length.

### 4.2 Node search `findNode(u, o)` (:765-788)
```
f = u.faction; c = f.castle; wide = f.human && c
room(n) = nodeValid(n) && nodeRoom(n) && !(u.skip has n.id)
free(n) = room(n) && explored(f, n.x, n.y)           // explored(): §4.6 ; AI → always true
if o.first && room(o.first): return o.first           // player's pick: explored NOT required
best = null; bd = ∞
if !(wide && o.res == 'meat'):
   for n in nodesOf(o.res):
      if !free(n): continue
      d0 = dist(n, (o.px,o.py)); if d0 > 900: continue
      d = d0 + dist(n, u) * 0.3
      keep min d
   if best || !wide: return best          // AI pawns never widen the search
// human only (and always for meat): whole territory around castle
for n in nodesOf(o.res):
   if !free(n) || dist(n, castle) > 2600: continue
   d = dist(n, u); keep min
if best: o.first = null; o.px = best.x; o.py = best.y    // origin MOVES to the new node
return best
```
Meat for human pawns: always "nearest free explored sheep to the pawn within 2600 of the castle" (sheep move and
respawn at farms, so the origin is irrelevant).

### 4.3 `gatherStep(u, o, dt)` (:806-854) — called from `updatePawn` when `o.k == 'gather'` and `pauseT <= 0`
```
if u.carry:                                          // deliver first
   site = dropSite(u, carry.kind)
   if site && navTo(u, fieldOf(u, site), dt) == 'arrived': deposit(u)
   return                                            // no site → pawn waits in place with the load, forever
n = u.node
if !n || !nodeValid(n):
   n = findNode(u, o); claim(u, n)
   if !n:
      if f.human: u.skip = null; u.pauseT = 1        // keep the job, retry in 1 s (clears unreachable list)
      else: idle(u)                                  // AI: free the pawn, the AI reassigns it
      return
if n.kind == 'sheep':
   d = dist(n, u)
   if d > 120 && n !== o.first && (u.lookT -= dt) <= 0:      // opportunistic re-target while walking
      u.lookT = 0.5
      m = findNode(u, o); dm = m ? dist(m, u) : ∞
      if dm < d * 0.6: claim(u, m); n = m; d = dm
   at = d < 36
   if !at && !approach(u, n.x, n.y, dt): at = null    // approach() false = unreachable
else:
   r = navTo(u, fieldOf(u, n), dt)                    // field to ring of n.cells (8-neighbour cells around)
   at = r == 'arrived' ? true : r == 'blocked' ? null : false
if at === null: (u.skip ||= Set).add(n.id); claim(u, null); return     // unreachable → try another next step
if !at: return
workOn(u, n, dt)                                      // facing, sfx 'chop' every 0.6 s of workT, anim 'work'+tool
u.workT += dt * f.mods.gather[o.res]
if u.workT < JOB_TIME[o.res]: return
cap = 10 + f.mods.carry
if tree:  amount = min(cap, n.wood); chopTree(n, amount)
elif sheep: amount = 15; killSheep(n)                 // whole sheep at once, ignores cap
else:     amount = min(cap, n.amount); n.amount -= amount
if !nodeValid(n): claim(u, null)
u.carry = {kind: o.res, amount}; u.workT = 0; u.pauseT = 0.5
```
Notes / edge cases:
- `workT` is NOT reset when the node is lost mid-work (another pawn depleted it) and the pawn finds a new node:
  progress carries over. It is reset only by `setOrder` or after producing a load.
- While a pawn works, the node stays booked (`users`) even during the delivery trip (the pawn keeps `u.node`
  unless the node became invalid) — so 2 pawns "own" a tree slot each round trip.
- Arrival for trees/deposits/buildings = standing on a cell whose field value is 0, i.e. a passable cell
  8-adjacent to the target's cells on the same level (`World.ring`, world.js:245-256; `fieldFor` world.js:258).
- Sheep arrival = within 36 px of the sheep's position. Sheep keep wandering while being approached (but not while
  being worked? NO — sheep keep moving even while a pawn "works" on them; if the sheep walks > 36 px away the pawn
  re-approaches and continues accumulating workT when back in range).
- With `axes` tech: `gather.wood = 1.3`; `picks`: gold and stone 1.3; `carts`: `carry = 5` (cap 15) and pawn speed
  ×1.15 (factions.js:83-85). Meat has no gather tech (`mods.gather.meat` stays 1); `breeding` affects farms.
- Effective yields (doc comment :6-7): about 38/min per pawn on each resource.

### 4.4 `deposit(u)` (:797-804)
```
f.res[carry.kind] += carry.amount; f.gathered += carry.amount
if f.human: float '+amount' at (u.x, u.y-70) with resource icon; sfx 'coin' at u
u.carry = null; u.pauseT = 0.4
```

### 4.5 `workOn(u, target, dt)` (:790-795)
`flip = target.x < u.x`; play sfx 'chop' whenever `floor((workT+dt)/0.6) != floor(workT/0.6)` (on screen);
`state = 'work'`; anim `'work' + u.tool`. Render/audio only except `state = 'work'`, which matters for
`separateUnits` (working/attacking units are pushed with weight 0.2 instead of 1, :1061).

### 4.6 Explored territory (factions.js:269-296)
- `f.seen: Uint8Array(W*H)` only for human factions; updated by `explore(f)` every 0.25 s (`f.seeT`,
  factions.js:894).
- First call marks a disc of radius `START_SIGHT = 15` cells around the castle.
- Each call marks, for every own live unit, a disc radius `def.sight + mods.sight` at the unit; for every own live
  building, radius `(done ? def.sight : 2) + mods.sight` at `(b.x, b.y - 30)`; for every point owned, radius
  `POINT_SIGHT + mods.sight` at `(p.x, p.y - TILE)`. A disc marks cell (i,j) iff `(i-cx)^2 + (j-cy)^2 <= r*r + r`.
  Optimization: an entity is re-marked only if `(cell index*32 + r)` changed since last time (`e.seenKey`).
- `explored(f, x, y)`: `true` if `!f.human || !f.seen` (AI has no fog); else `seen[cellAt(x,y)] == 1`
  (false off-map).
- Pawns working on their own (auto search, `afterBuild`, sliders, `job`) only pick explored nodes; a node the player
  clicked explicitly (`o.first`) is accepted even if unexplored.

### 4.7 Manual job assignment (factions.js:298-334, 357-367) — the "Lavoratori" sliders and "job" command
`nodeNear(f, res, p)`: nearest (straight line to p) node of res with `nodeValid && nodeRoom && dist(n, castle) <=
2600 && explored` → `{node, d}` or null. (Crashes in JS if castle is null — guard in Rust.)

`setWorkers(f, res, n)` ('jobs' command, slider target count):
```
pawns = own live pawns; on = pawns with jobOf(u) == index(res)      // jobOf counts sheltered pawns too
free(u) = u.order.k == 'idle' || u.order.k == 'drop'
sort `on` so pawns WITHOUT carry come first (stable)
remove the first max(0, on.length - n) of them:
   if order is 'shelter': u.order.back = {k:'idle', x:u.x, y:u.y}      // stays free after leaving shelter
   elif carry && dropSite exists: setOrder(u, {k:'drop', b: site})
   else idle(u)
while have < n:
   best = argmin over free pawns u of nodeNear(f,res,u).d
   if none: return pawns.some(free) ? `${RES_LABEL[res]}: nessun posto libero nel territorio esplorato`
                                    : 'Nessun pedone libero'
   setOrder(best.u, gatherOrder(best.node))
return null
```
'job' command (unit screen: send selected pawns to a resource): for each listed own pawn, `near = nodeNear(...)`;
if found → `embolden(u); setOrder(u, gatherOrder(near.node))`. If some were asked and none sent → returns
`${RES_LABEL[b]}: nessun posto libero nel territorio esplorato`.

Workers screen header string (panels.js:211):
`Pedoni ${n}/${pawnCap}  ·  Liberi ${free}  ·  Al cantiere ${building}` + (hidden ? `  ·  Al riparo ${hidden}` : '')
+ (moving ? `  ·  In viaggio ${moving}` : '')` where free = act 0 or 8, building = act 6 or 7, hidden = act 12 or 13,
moving = act 1. Per-row count = pawns with `u.job == i`; label `+${rate}/min`. Toast `'Nessun pedone libero'` when
pressing + with no free pawn.

### 4.8 `unitAct(u)` and `jobOf(u)` (:980-997)
```
unitAct: if state=='attack' || target: monk ? 9 : 2
  gather → wood 3 / gold 4 / stone 11 / meat 5   (also while carrying)
  build 6, repair 7, drop 8, shelter → inside ? 12 : 13, explore 14, idle → pawn ? 0 : 10, else 1
jobOf: o = (order.k=='shelter' ? order.back : order); o.k=='gather' ? RES_KINDS.indexOf(o.res) : -1
```
`u.job` is only computed for pawns (-1 for others). Both are replicated (net.js:176).

---

## 5. Pawn update `updatePawn(u, dt)` (:955-978)

```
o = u.order
if (u.fearT -= dt) <= 0:                         // threat check every 0.4 s
   u.fearT = 0.4
   b = u.inside
   cx,cy = b ? (b.x, b.y) : (u.x, u.y)
   seen = threatNear(f, cx, cy, sightPx(u) + ((b || u.brave) ? 2*TILE : 0))
   if o.k == 'shelter': o.calm = seen ? 0 : o.calm + 0.4
   elif u.brave: u.calmT = seen ? 0 : u.calmT + 0.4; if u.calmT >= 4: u.brave = false
   elif seen:
      to = shelterFor(u)
      if to: setOrder(u, {k:'shelter', b: to, back: o, calm: 0}); o = u.order
if o.k == 'shelter': shelterStep(u, o, dt); return
u.tool = gather ? JOB_TOOL[o.res] : (build|repair ? 'Hammer' : null)
if u.pauseT > 0: u.pauseT -= dt; return          // short pause after harvesting (0.5) / delivering (0.4) / no node (1)
switch o.k:
  gather → gatherStep
  build | repair → buildStep
  drop → r = b.dead ? 'blocked' : navTo(u, fieldOf(u,b), dt)
         if r=='arrived' && u.carry && b.def.drop includes carry.kind: deposit(u)
         if r != 'moving': idle(u)                // arrived (delivered or not) or blocked → idle
  idle → nothing (pawns do not return to a guard post)
  other (move...) → if travel(u, o.x, o.y, dt): idle(u)
```
Any order type flees (gather, build, repair, drop, move, idle) — only `brave` pawns or pawns already sheltering
do not. The carried load is kept while sheltering.

`updateUnit` (:999-1027) for a sheltered unit (`u.inside`) only runs `updatePawn` + `act`/`job`, nothing else
(no cooldowns, no animation, no push-out, no separation).

Non-sheltered pawns go through the generic `updateUnit` prelude: if the unit stands on a cell that is not
`passable` for its side AND (not `crossable` OR (order idle and no target)), it walks to `nearestPassable` instead
of thinking this step (:1007-1009) — this is how units are pushed out of a building just placed over them, and
why idle units never park inside their own buildings (see §10.3).

Animation (:1015-1020, presentation but replicated as `u.anim`): when state idle/move: base `'run'` if moving else
`'idle'`; for pawns append `LOAD_NAME[carry.kind]` if carrying, else the tool name (or nothing).
E.g. `runWood`, `idleAxe`, `workHammer`, `run`.

---

## 6. Sheltering in houses (entities.js:893-953)

### 6.1 `threatNear(f, x, y, r)` (:896-902)
True if any unit `e` with `!e.dead && !e.inside && e.faction != f && e.def.dmg && !e.def.onlyBuildings` has
`hypot(e.x-x, e.y-y) < r` (box pre-check `|dx|<r && |dy|<r`). So enemy pawns, monks, scouts (no `dmg`) and rams
(`onlyBuildings`) never scare pawns. Catapults (dmg, not onlyBuildings) do.

`sightPx(u) = (u.def.sight + f.mods.sight) * TILE` → pawn: 5*64 = 320 px (448 with `maps`, +2).
Effective fear radius: 320 normally; 320+128 = 448 when inside a building (measured from building anchor
`(b.x, b.y)`) or when brave.

### 6.2 `shelterFor(u)` (:904-915)
```
shelterFull(b) = b.kind != 'castle' && b.inside.length >= 5
nearest (rectDist from pawn) own building with !dead && done && kind=='house' && !shelterFull
else castle if it exists and !dead, else null
```
Only houses and the castle shelter pawns. No other building does.

### 6.3 Enter / leave (:916-930)
```
enterShelter(u, b): fx dust1; u.inside = b; b.inside.push(u)
                    u.x = (b.x0+b.x1)/2; u.y = (b.y0+b.y1)/2      // centre of rect: lets clients know which building
                    u.moving = false; u.state = 'idle'
leaveShelter(u):    remove u from b.inside; u.inside = null
                    p = nearestPassable(b.x + rand(-0.3,0.3) * b.def.w * TILE, b.y1 + 24); u.x,u.y = p
                    fx dust1
```
A sheltered pawn is invisible and untargetable: every scan skips `u.inside` (targeting, arrows, rocks, capture,
pickups, threat, separation, rendering, minimap ui.js:293).
Leaving happens on: any `setOrder` (incl. resumeWork), `killUnit` (:515), `destroyBuilding` (all occupants
ejected, :1142 — their order is still 'shelter', so next step `shelterStep` picks another shelter), and
`relocateBuilding` moves occupants with the building (:1104).

### 6.4 `shelterStep(u, o, dt)` (:946-953) and `resumeWork(u)` (:932-937)
```
shelterStep:
  if o.calm >= 4: resumeWork(u); return            // calm counts also while still running to the shelter
  if u.inside: return
  if o.b.dead || !o.b.done || shelterFull(o.b): o.b = shelterFor(u)
  r = o.b ? navTo(u, fieldOf(u, o.b), dt) : 'blocked'
  if r == 'arrived': enterShelter(u, o.b)
  elif r == 'blocked': u.brave = true; u.calmT = 0; resumeWork(u)   // no reachable shelter: keep working, brave
resumeWork:
  back = u.order.back
  if back.k == 'gather': setOrder(u, {...back, first: (back.first && nodeValid(back.first)) ? back.first : null})
  elif back.k == 'idle': setOrder(u, back); idle(u)                  // idle where it came out
  else: setOrder(u, back)                                             // build/repair/drop/move resumed as-is
```
Edge: the house capacity check happens only when choosing/arriving; two pawns arriving the same step can both
enter if `inside.length` was 4 (first check per pawn in array order: the 2nd sees 5 → `shelterFull` → reselects;
the check is at the start of `shelterStep` before navTo, so the sequence is consistent within a step).

### 6.5 Direct-order override: `embolden(u)` (:941-945)
```
if u.type != 'pawn': return
b = u.inside
if !threatNear(f, b ? b.x : u.x, b ? b.y : u.y, sightPx(u) + 2*TILE): return   // order in peacetime changes nothing
u.brave = true; u.calmT = 0
```
Called for every unit of a human faction ordered through `orderUnits` (any kind, before `setOrder`) and in the
'job' command. NOT called by the 'jobs' sliders (`setWorkers`), deploy, AI orders. A brave pawn ignores threats
until it has seen no threat (radius sight+2 tiles) for 4 consecutive seconds (accumulated in 0.4 s ticks),
after which it becomes fearful again.

UI: workers screen counts acts 12/13 as "Al riparo". Building info does not show occupant count in the JS
(no explicit garrison counter UI exists; `b.inside` is not replicated — clients infer from `u.inside` flag bit 4
and the unit's position at the building centre, net.js:176/264).

---

## 7. Construction and repair (entities.js:856-891, 1128-1135; factions.js:343-353)

### 7.1 Placing a site ('place' command, factions.js:343-353)
```
if kind not in BUILD_MENU or cx,cy not integers: ignore
why = placeBlock(f, kind, cx, cy)       // ageLock → 'Risorse insufficienti' → zoneBlock → canPlace ('Qui non si può costruire')
if why: return why (toast)
site = addBuilding(f, kind, cx, cy, done=false); pay(f, cost); f.sites[kind]++
fx dust2 at each of the w cells along the bottom edge
if builders list: orderUnits(f, builders[..200], 'build', 0, 0, site.id)
```
`BUILD_MENU = ['house','camp','depot','farm','barracks','wall','archery','stable','smith','market','tower','monastery','siege']`.
Client-side extra check: the footprint centre must be known (fog `Fog.knows`) else 'Qui non si può costruire'
(control.js:421-423) — this is a UI check only, not enforced by the server.

`canPlace(kind, cx, cy, own=null)` (factions.js:147-159): every footprint cell inside map, `passable(c)` (side 0 —
so even own palisades block) unless it's one of `own` cells with `block[c] == 1`, not `stair`, not `noBuild`, and all
cells on the same `level`.

`zoneBlock(f, kind, cx, cy)` (factions.js:161-166): centre `(cx + w/2, cy + h/2)` in cells; OK if within
`r = def.defense ? 12 : 40` cells of the castle centre `(c.cx + c.def.w/2, c.cy + c.def.h/2)`, or within 9 cells
of an owned point with `def.zone` (outposts; point centre `(gx, gy)`). Message:
`${d.defense ? 'Torri e palizzate' : 'Troppo lontano'}: entro ${r} caselle dal castello o ${OUTPOST_RADIUS} da un tuo avamposto`.

Auto-builders (`Ctl.builders`, control.js:268-277; client computes, sends ids):
selected own pawns if any; else if kind is wall and some own pawn with act 6 (building) is within 600 px of the
site → `[]` (it will chain to it via afterBuild); else free pawns (act 0 or 8) sorted by distance, then gatherers
(act in RES_ACT) sorted by distance; take `d.work > 15 ? 2 : 1`.

Palisade row placement (control.js:436-444): with Shift held and mouse dragged, every cell from the last placed
cell to the current one (`n = max(|dx|,|dy|)`, `cell_i = round(x0 + (cx-x0)*i/n)`, same for y) is placed if valid,
builders = `this.builders('wall', ...)` only if the player has pawns selected, else `[]`.

### 7.2 `buildStep(u, o, dt)` (:879-891)
```
b = o.b
if b.dead || (o.k=='build' ? b.done : b.hp >= b.maxHp): afterBuild(u, b); return
r = navTo(u, fieldOf(u, b), dt)
if r == 'blocked': idle(u); return              // NB: resume is lost
if r != 'arrived': return
workOn(u, b, dt); u.workT += dt                  // workT only drives the hammer sfx here
if o.k == 'repair': b.hp = min(maxHp, hp + maxHp * 0.035 * dt); return      // free of charge
b.progress += dt
b.hp = min(maxHp, hp + maxHp * 0.9 * dt / def.work)
if b.progress >= def.work: completeBuilding(b)
```
- Builders stack linearly: n pawns → n× speed (each adds dt). No gather mods apply.
- A site starts at `hp = 0.1*maxHp`, `progress = 0`; one builder brings it to exactly maxHp at completion; damage
  taken during construction is not healed by completion (needs repair afterwards).
- Repair rate: 3.5% of maxHp per second per pawn → ~28.6 s for one pawn from 0 to full.
- The castle has `work: 0` and is always created done.

### 7.3 `completeBuilding(b)` (:1128-1135)
```
b.done = true
fx dust2 per footprint column at (x0 + (i+0.5)*TILE, b.y + 4), scale 1.5 if w > 1
if farm: makeSheep(b.pasture) ×2
census(f)
if f.human && kind != 'wall': play 'build'; G.announce('Costruzione finita: ' + def.label, f, 2.2, f)
```
`G.announce(text, faction, dur=3, to=null)` (main.js:33): banner coloured by `faction`, shown only to `to`
(null = everyone).

### 7.4 `afterBuild(u, b)` (:858-877)
```
wasBuilding = u.order.k == 'build'; resume = u.order.resume || null
next = nearest (distTo = rectDist from pawn) own building with !done && !dead, within 700 px
if next: setOrder(u, {k:'build', b: next, resume}); return
if wasBuilding && b && !b.dead && b.done && (b.kind == 'depot' || b.kind == 'farm'):
   candidates = farm ? G.sheep : trees ∪ golds ∪ stones
   node = nearest by rectDist(n.x, n.y, b) with nodeValid && explored, within 520 px     // nodeRoom NOT checked
   if node: setOrder(u, gatherOrder(node)); return
if resume: setOrder(u, {...resume, first: null}) else idle(u)
```
So: a pawn chains through nearby sites (palisade rows), then starts working next to a freshly built depot/farm,
otherwise resumes the gathering it left.

---

## 8. Building objects (entities.js:1068-1219)

### 8.1 Definitions (assets.js:224-298)
`w,h` cells; `hp`; `cost`; `work` s; `sight` cells; `pawns`/`troops` capacity; `drop`; `trains`; `techs`; `age`;
`defense`. `ax, ay` = sprite anchor (render; `ay` is also used by tower arrow origin and fx heights).

| kind | label | w×h | hp | cost | work | sight | caps | drop | trains | techs | age |
|---|---|---|---|---|---|---|---|---|---|---|---|
| castle | Castello | 5×2 | 2600 | — | 0 | 10 | pawns 10, troops 6 | all 4 | pawn, scout | age2, age3, carts, masonry | — |
| house | Casa | 2×1 | 320 | wood 30 | 9 | 4 | pawns 5 | | | | 1 |
| camp | Accampamento | 3×2 | 420 | wood 60 | 12 | 5 | troops 8 | | | | 1 |
| depot | Deposito | 2×1 | 380 | wood 60 | 12 | 5 | | wood, gold, stone | | axes, picks | 1 |
| farm | Fattoria | 3×2 | 420 | wood 150 | 14 | 5 | | meat | | breeding | 1 |
| barracks | Caserma | 3×2 | 700 | wood 90 | 18 | 5 | troops 2 | | warrior, lancer | boots | 1 |
| wall | Palizzata | 1×1 | 300 | wood 12 | 3 | 2 | | | | | 1 (defense) |
| archery | Arcieria | 3×2 | 600 | wood 80, gold 30 | 18 | 5 | troops 2 | | archer | | 2 |
| stable | Scuderia | 3×2 | 650 | wood 110, gold 40 | 18 | 5 | troops 2 | | knight | horseshoes | 2 |
| smith | Fucina | 3×2 | 650 | wood 100, stone 40 | 18 | 5 | | | | blades, shields, fletching, armor | 2 |
| market | Mercato | 3×2 | 600 | wood 120 | 18 | 5 | | | | maps (trade:true) | 2 |
| tower | Torre | 2×1 | 620 | stone 80, wood 40 | 16 | 9 | | | | | 2 (defense) |
| monastery | Monastero | 3×2 | 600 | wood 80, gold 100, stone 40 | 22 | 5 | troops 2 | | monk | herbs, scriptures | 3 |
| siege | Officina d'assedio | 3×2 | 650 | wood 160, gold 60 | 22 | 5 | troops 2 | | ram, catapult | engineering | 3 |

Descriptions (verbatim, assets.js): castle `'Cuore del regno: se cade, hai perso. Addestra Pedoni ed Esploratori e fa avanzare d\'era.'`;
house `'+5 Pedoni al massimo. Con i nemici in vista i Pedoni ci si rifugiano.'`; camp `'+8 truppe al massimo.'`;
depot `'Punto di consegna per legna, oro e pietra. Costruiscilo vicino a boschi e cave.'`;
farm `'Alleva pecore nei dintorni, abbastanza per due o tre pedoni, e fa da punto di consegna per il cibo.'`;
barracks `'Addestra Guerrieri e, dall\'Età Feudale, Lancieri. +2 truppe al massimo.'`;
wall `'Sbarra il passaggio ai nemici; le tue unità ci passano. Solo vicino al castello o a un tuo avamposto. Maiusc + trascina per posarne una fila.'`;
archery `'Addestra Arcieri. +2 truppe al massimo.'`; stable `'Addestra Cavalieri. +2 truppe al massimo.'`;
smith `'Migliora armi e armature dell\'esercito.'`; market `'Scambia legna, cibo e pietra con oro.'`;
tower `'Scaglia frecce sui nemici vicini e vede lontano. Solo vicino al castello o a un tuo avamposto.'`;
monastery `'Addestra Monaci guaritori. +2 truppe al massimo.'`; siege `'Costruisce Arieti e Catapulte. +2 truppe al massimo.'`.
House has 3 sprite variants; wall sprite is neutral (not faction-coloured).

### 8.2 Geometry (`buildingPlace`, :1077-1084)
```
cells = all (cx+x, cy+y) for x<w, y<h
x = (cx + w/2)*TILE            // bottom-centre anchor
y = (cy + h)*TILE - 4          // = sy (draw sort)
x0 = cx*TILE, y0 = cy*TILE, x1 = (cx+w)*TILE, y1 = (cy+h)*TILE
```

### 8.3 Data structure (`buildingShape` + `addBuilding`, :1086-1126)
```
{ isBuilding:true, kind, def, cx, cy, cells, x, y, sy, x0, y0, x1, y1,
  variant: 0, queue: [], rally, fireSeed: random*100 /*render*/, dead:false,
  id, faction, maxHp: round(def.hp * f.mods.buildHp),
  hp: done ? maxHp : maxHp*0.1, done, progress: done ? def.work : 0,
  cd: rand(0,1) /*tower/castle shot cooldown*/, breedT: 0 /*farm*/, housedT: -99 /*last "no room" toast*/,
  inside: [] /*sheltered pawns*/, pasture }
if def.sprites (house): variant = f.houses++ % 3
rally   = { x: b.x, y: b.y1 + 72, t: null }
pasture = { x: b.x, y: b.y1 + 70 }          // every building has one; only farms use it
blockBuilding(b, true); G.buildings.push(b)
```
`masonry` (buildHp 1.3) rescales existing buildings: `hp *= newMax/oldMax; maxHp = newMax` (factions.js:118-121).

### 8.4 Blocking (`blockBuilding`, :1093-1096; world.js:85-101)
```
World.setBlock(cells, on ? +1 : -1)              // block[c] is a COUNTER
World.setOwner(cells, on ? f.idx+1 : 0, kind == 'wall')
   per cell: if wallOf[c]: walls[wallOf[c]]--; ownOf[c] = side; wallOf[c] = wall ? side : 0; if wall && side: walls[side]++
both bump World.version; unblocking also sets World.opened = version (invalidates cached fields, see world spec)
```

### 8.5 `census(f)` (factions.js:202-218) — every 1 s (`f.tickT`) and after completion/demolish
Rebuilds `f.pawns`, `f.soldiers`, `f.pawnN`, `f.troopN`, `f.has[kind]` (finished, alive), `f.sites[kind]`
(unfinished), `pawnCap = min(30, Σ def.pawns)`, `troopCap = min(50, Σ def.troops)` over FINISHED buildings.

### 8.6 Training / research queue (`updateQueue`, :1176-1197; commands factions.js:377-393)
Queue item: `{unit, t, total}` or `{tech, t, total}`; `total = UNIT_DEF[type].time` / `TECH_DEF[id].time`.
```
trainSpeed(n) = 1 + 0.2 * clamp(n-1, 0, 5)              // n = f.has[b.kind] (count of finished same-kind buildings)
roomFor(f, type) = type=='pawn' ? pawnN < pawnCap : troopN < troopCap
updateQueue(b, dt):                                       // only for done buildings
  q = queue[0]; if !q return
  if q.t < q.total: q.t = min(total, t + dt * (q.unit ? trainSpeed(f.has[b.kind] || 1) : 1)); return   // techs not accelerated
  if q.tech: shift; delete f.busy[q.tech]; applyTech(f, q.tech)
  elif roomFor(f, q.unit): shift; deploy(b, q.unit)
  elif f.human && G.time - b.housedT > 15:               // ready but no room: wait, toast at most every 15 s
     b.housedT = G.time
     full = pawn ? pawnCap >= 30 : troopCap >= 50
     toast pawn ? (full ? 'Pedoni al massimo' : 'Servono più case') : (full ? 'Truppe al massimo' : 'Servono più accampamenti')
```
- 'train': `trainBlock` → 'Non disponibile' (wrong building / not done) | ageLock `'Richiede: ' + AGE_NAME[age]` |
  'Coda piena' (≥5) | 'Risorse insufficienti'; pays immediately; push.
- 'research': `techBlock` adds 'Già ricercata', 'Ricerca in corso'; sets `f.busy[id] = true`.
- 'cancel' (b = index, c = expected unit/tech id): only if `queue[b]` matches c (net race protection); splice; FULL
  refund; clear busy.
- UI (ui.js:399-414): bar of `q.t/q.total`; when waiting (`q.unit && q.t >= q.total`): `'In attesa di case'` (pawn)
  / `'In attesa di accampamenti'`; tooltip `'Clic per annullare: le risorse tornano indietro.'`. Idle info lines:
  `'Clic destro sulla mappa: punto di raccolta'`; for trainers other than castle: `same > 1 ?
  `${label} ×${same}: addestramento +${round((trainSpeed(same)-1)*100)}%` : 'Un\'altra uguale: +20% di velocità'`.
  Under construction: `In costruzione: ${floor(100*progress/work)}%`.

`deploy(b, type)` (:1162-1174):
```
p = nearestPassable(b.x + rand(-0.3,0.3)*w*TILE, b.y1 + 24); u = spawnUnit(f, type, p.x, p.y); fx dust2
r = b.rally; pawn = type == 'pawn'
if pawn && r.t && NODE_RES[r.t.kind] && nodeValid(r.t): setOrder(u, gatherOrder(r.t))
elif pawn && r.t && r.t.isBuilding && !r.t.dead && !r.t.done: setOrder(u, {k:'build', b: r.t})   // no resume
else: q = nearestPassable(r.x + rand(-34,34), r.y + rand(-22,22)); setOrder(u, {k:'move', x:q.x, y:q.y})
pawn ? f.pawnN++ : f.troopN++              // keeps caps correct until next census
```
Note: a rally node that is no longer valid, or a rally site that is now finished, falls back to moving to the rally
point. Non-pawns always go to the rally point. Start of game (factions.js:64-69): castle placed done at
`(base.x-2, base.y-1)`, 4 pawns + 1 warrior deployed then immediately `idle`.

### 8.7 Rally point ('rally' command, factions.js:394-397; control.js:206-212, 299-300, 385-394)
`bld.rally = {x, y, t}` where `t = G.index.get(d)` kept only if `!t.dead && (NODE_RES[t.kind] || (t.isBuilding &&
t.faction == f))`. Client only sends a target id for a node or an own UNFINISHED building; it also sets
`b.rally` locally for immediate feedback. Right-click with a single own finished training building selected sets
the rally. Action button: key G, title `'Punto di raccolta'`, desc `'Poi clic sul terreno o su una risorsa (anche col tasto destro, direttamente).'`;
mode hint `'Clic sul punto di raccolta  ·  Esc: annulla'`. Shown only if `b.done && b.def.trains`.

### 8.8 Moving buildings instantly ('move' command, factions.js:398-402; `relocateBuilding` :1098-1109)
```
if b,c not integers or same cell: ignore
moveBlock: castle → 'Il castello non si sposta'; zoneBlock(...); canPlace(kind, cx, cy, own=b.cells) else 'Qui non può stare'
relocateBuilding(b, cx, cy):
   dx,dy = (cx-b.cx)*TILE, (cy-b.cy)*TILE
   fx dust2 along old bottom edge
   blockBuilding(b, false); assign buildingPlace(kind,cx,cy) fields; blockBuilding(b, true)
   occupants: u.x,u.y = centre of new rect
   rally = {x+dx, y+dy, t: same}
   pasture = NEW object {x+dx, y+dy}        // !! sheep homed on the old pasture object keep grazing at the old spot
                                             //    and no longer count toward this farm's 5 (JS quirk; see open questions)
   fx dust2 along new edge; sfx 'build'
```
Everything else is kept: hp, maxHp, done, progress, queue, cd, breedT, inside, id. Works for sites too.
Allowed for any own building except the castle; instant and free. UI: key V, title `'Sposta'`, desc
`'Scegli la nuova casella col mouse o con le frecce (Invio per posare), dentro il raggio in cui si può costruire.'`;
pick mode toasts `'Clicca un tuo edificio'`, `'Scegli un edificio con le frecce'`.

### 8.9 Demolish ('demolish' command, factions.js:403-407)
```
castle → 'Il castello non si demolisce'
if !done: refund(full cost)          // finished buildings: no refund
destroyBuilding(b, null); census(f)
```
UI: key Delete ('Canc'), title `b.done ? 'Demolisci' : 'Annulla cantiere'`, desc
`b.done ? 'Abbatte l\'edificio, senza rimborso.' : 'Rimuove il cantiere e restituisce le risorse.'`.

### 8.10 `destroyBuilding(b, by)` (:1137-1159)
```
if dead return; dead = true; G.index.delete(id); blockBuilding(b, false)
for u in copy(b.inside): leaveShelter(u)
for q in queue: if q.tech: delete f.busy[q.tech]          // queued units/techs are LOST (no refund)
fx boom2 ×(w+1) at (b.x + rand(-0.4,0.4)*w*TILE, b.y - rand(10, ay*0.5)), scale w>1 ? rand(1,1.5) : 0.7; sfx boom
bf = by && by.faction
if bf && bf != f:
   bf.razed++
   if kind != 'wall':                                     // loot
      p = nearestPassable(b.x, b.y1 + 20)
      loot = farm ? [meat, meat, wood] : tower ? [stone, stone, wood] : [wood, wood, gold]
      each: dropPickup(kind, p.x + rand(-30,30), p.y, 15, 60)
if castle: eliminateFaction(f, bf)
```
(Removed from `G.buildings` after the building update loop.) Demolish passes `by = null` → no loot, no razed.

Damage to buildings (`hurt`, :482-512): `amount *= 0.4` (BUILDING_ARMOR); human owner gets an `'alert'` event at most
every 12 s (`f.alertT`); `hp <= 0 → destroyBuilding(t, by)`. Siege `vsBuilding` multipliers come from the unit
def (other spec).

### 8.11 Farm breeding (`updateBuilding`, :1203-1206)
```
if farm (done): breedT += dt * f.mods.breed            // breeding tech: 1.5
   if breedT >= 9: breedT = 0
      if count(G.sheep with home === b.pasture) < 5: makeSheep(b.pasture)
```
Plus 2 sheep at completion. Timer runs (and resets) even when full. Farm sheep are a shared resource: any
faction's pawns can butcher them.

### 8.12 Building rendering notes (presentation only; :1335-1355)
- Site: ghost at alpha 0.3, plus the sprite revealed bottom-up by `progress/work`.
- Damage fires when `hp/maxHp < 0.7`: `<0.25` fire3 ×4, `<0.45` fire2 ×3, else fire1 ×2; deterministic positions
  from `fireSeed` hash: `h1 = |sin(seed + i*12.9898)*43758.5453| % 1`, `h2 = |sin(seed + i*78.233)*12543.123| % 1`;
  at `(b.x + (h1-0.5)*w*TILE*0.7, b.y - 24 - h2*ay*0.45)`, scale 1.4.

---

## 9. Towers and castle shooting (`updateBuilding`, :1207-1219)

```
if !done: return (also no queue, no breeding)
... queue, farm ...
if kind not in {tower, castle}: return
if (b.cd -= dt) > 0: return
range = (tower ? 420 : 380) + f.mods.range                  // fletching: range +45
oy = b.y - def.ay * 0.62                                    // tower: b.y-143.84 ; castle: b.y-155
best = nearest unit o (hypot from (b.x, b.y)) with !dead && !inside && faction != f && d < range
       // ANY unit type: pawns, monks, scouts, rams, catapults
if !best: b.cd = 0.3; return
fireArrow(b, b.x, oy, best.x, best.y - 28, (tower ? 15 : 14) + f.mods.atk.archer)   // fletching +3
b.cd = tower ? 1.1 : 1.2
```
Initial `cd = rand(0,1)`. Shooting does not use line of sight or fog. `oy` depends on sprite anchors — in the Rust
sim keep it as a per-kind constant "muzzle height" (tower 143.84 px, castle 155 px) since arrows are physically
simulated in 2D px space and hit detection depends on their y.

---

## 10. Palisades, passability and the anti-trap rule

### 10.1 Rules (world.js:43-72)
```
passable(c, side=0)  = c>=0 && land[c]==1 && cliff[c]==0 && (block[c]==0 || (side!=0 && wallOf[c]==side))
crossable(c, side=0) = c>=0 && land[c]==1 && cliff[c]==0 && (block[c]==0 || (side!=0 && ownOf[c]==side))
side(u) = u.faction.idx + 1   (sideOf, entities.js:299)
```
- Own palisade cells: own units can stand AND path through them freely (cost 10/14 like open ground).
- Enemy palisades (and all enemy buildings): impassable. Enemy troops must destroy them (`breach`, other spec:
  picks among the 4 enemy buildings within 900 px sorted by `rectDist(u,b) + rectDist(goal,b)` the first one whose
  field reaches the unit; then attacks it for up to 8 s, `breachT`). Auto-targeting (`acquireTarget`, `raidOn`)
  ignores walls; walls are attacked only when they block the way or by explicit order.
- Pawns/sheep/neutral (side 0) treat every wall as solid.
- Own non-wall buildings: crossable but not passable (can walk through only if needed, never stop there).
- Placing a palisade requires the cell to be `passable(c)` with side 0 — can't build over a unit-free own wall etc.
  (units standing on a cell do not block placement; they are pushed out, §5).

### 10.2 Distance fields and the anti-trap rule (world.js:207-243; entities.js:300-315)
```
getField(key, seedFn, side, from):
   walls = World.walls[side] ? side : 0           // sides with no palisades share the neutral field
   d = cached(key + ':' + walls, seedFn, walls, cross=false, from)
   World.crossed = from >= 0 && side != 0 && d[from] == ∞
   if crossed: return cached(key + ':x' + side, seedFn, side, cross=true, from)
   return d
```
- Normal field: own walls open, own other buildings closed.
- If the unit's current cell cannot reach the goal (it's enclosed by its own buildings, or the goal is), a
  "cross" field is used where own buildings are open but each own non-wall building cell costs +30 (CROSS_COST)
  on entering it (`computeField`, world.js:199). `u.cross = World.crossed` is stored on the unit and makes
  `tryMove/stepOK` allow entering crossable cells (world.js:61-71).
- `stepOK(a→b)`: if same cell OK; requires `(cross || !passable(a)) ? crossable(b) : passable(b)`; if `a` itself is
  not crossable (e.g. a building was just placed over the unit) any move is allowed (escape); then `linkOK` (level /
  stairs). `tryMove` tests x and y separately with a 9 px look-ahead radius.
- `stepFrom` from a non-standable cell picks the 8-neighbour crossable cell with the lowest field value.
- `updateUnit` pushes a unit out of a non-passable cell when it is not crossable, or when the unit is idle with no
  target (:1007) — units may cross their own buildings but never rest inside them.
- Fields seed: `fieldFor(e) = ring(e.cells, side)` (passable 8-neighbours of the cells on the same level as
  `cells[0]`); `fieldToCell(c)` seeds `[c]` if passable else its ring.
- Cache/refresh policy (performance; FIELD_CACHE 128, FIELD_REFRESH 2 per step, FIELD_NEW 2): see the world/
  pathfinding spec. Building changes bump `version`; removals also `opened`.

### 10.3 Palisade auto-orientation (rendering only, `drawWall` :1319-1333)
```
wall(k) = k in range && World.wallOf[k] != 0               // palisade of ANY faction
c = b.cells[0]
side  = wall(c-1) || wall(c+1)          // NB: no row-wrap check on x edges
up    = wall(c-W); down = wall(c+W)
front = side || !(up || down)           // horizontal fence (or isolated)
if !b.done: alpha = 0.35 + 0.65*clamp(progress/work, 0, 1)
if !front || up:                        // vertical stake column (all of it if purely vertical; the back part otherwise)
   n = front ? 4 : 5 ; x = round(b.x - 7)
   for i<n: draw image column sx=31 width 14 full height at (x, round(b.y0 + (i+1)*64/5 - 72))
if front: draw full fence sprite at (b.x - ax, b.y - ay)
```
`STAKE = {sx:31, sw:14, n:5}`. For HD-2D, replicate the topology (horizontal segment / vertical segment / corner =
both) using neighbour walls of any faction; exact sprite math is not needed.

---

## 11. Capture points (entities.js:193-232; assets.js:307-318; ui.js:426-436)

### 11.1 Definitions
```
POINT_DEF = {
  outpost: {label:'Avamposto', sprite:'Outpost', zone:true, desc:'Chi lo tiene può costruire nei dintorni, torri e palizzate comprese.'},
  mine:    {label:'Miniera',   sprite:'Mine',    res:'gold',  desc:'Rende oro a chi la tiene, senza Pedoni.'},
  quarry:  {label:'Cava',      sprite:'Quarry',  res:'stone', desc:'Rende pietra a chi la tiene, senza Pedoni.'},
  sawmill: {label:'Segheria',  sprite:'Sawmill', res:'wood',  desc:'Rende legna a chi la tiene, senza Pedoni.'},
  ranch:   {label:'Ovile',     sprite:'Ranch',   res:'meat',  desc:'Rende cibo a chi lo tiene, senza Pedoni.'},
}
```
### 11.2 Data (`makePoint(c, kind)`, :197-204) — occupies 2×2 cells from top-left c
```
cx = c % W; cy = floor(c / W)
{ id, isPoint:true, kind, def, c, cells: quad(c),
  x: (cx+1)*TILE, y: (cy+2)*TILE - 4, sy: y, x0: cx*TILE, y0: cy*TILE, x1: (cx+2)*TILE, y1: (cy+2)*TILE,
  gx: cx+1, gy: cy+1 /*centre in cells*/, owner: null, by: null, t: 0 }
```
(Whether point cells block movement is decided by world generation — see world spec.)

### 11.3 `updatePoints(dt)` (:214-232)
```
tick = (G.pointT -= dt) <= 0; if tick: G.pointT = 0.5
for p in G.points:
   o = p.owner
   if o && p.def.res: o.res[res] += 0.5*dt; o.gathered += 0.5*dt          // every step, continuous
   if !tick: continue
   one = null; many = false
   for u in G.units:
      skip if u.dead || u.inside || u.type == 'pawn'
      skip if |u.x - p.x| > 300 || hypot(u.x - p.x, u.y - (p.y - TILE)) > 300    // circle centred 64 px above anchor
      if one && one != u.faction: many = true; break
      one = u.faction
   if many: continue                                   // contested: everything frozen (t keeps its value)
   if !one || one == o:                                 // nobody, or only the owner
      p.t = max(0, p.t - 0.5); if p.t == 0: p.by = null; continue
   if p.by != one: p.by = one; p.t = 0                  // a new challenger restarts from 0
   p.t += 0.5
   if p.t >= 8: capturePoint(p, one)
```
Counts every non-pawn unit (soldiers, monks, scouts, rams, catapults). Capture time 8 s = 16 ticks of 0.5 s.
Progress decays at 1 s/s when the challenger leaves.

### 11.4 `capturePoint(p, f)` (:205-211)
```
old = p.owner; p.owner = f; p.by = null; p.t = 0; fx dust2 scale 1.6
if f.human: play 'build' for f; G.announce('Hai conquistato: ' + label, f, 2.6, f)
if old && old.human: alert event for old at (p.x, p.y); G.announce('Hai perso: ' + label, f, 3, old)
```
On `eliminateFaction` (factions.js:909) the faction's points become free and its `by` progress is cleared.
Owned points give vision (POINT_SIGHT 7 + mods.sight) and outposts allow building within 9 cells (§7.1).

### 11.5 UI strings (ui.js:430-434)
Owner line: `o ? (o === player ? 'È tuo' : `Lo tiene il ${o.name}`) : 'Libero'`; progress (if `by && t > 0`):
`${by === player ? 'Lo stai conquistando' : 'Lo sta conquistando il ' + by.name}: ${round(100*t/8)}%`;
body: `desc + (res ? ` +${0.5*60} al minuto.` : ` Entro ${9} caselle.`) + ` Si conquista tenendoci accanto delle truppe per ${8} secondi, senza nemici attorno.``
→ i.e. " +30 al minuto." / " Entro 9 caselle.".

---

## 12. Expected income per minute (HUD) — `estimateRates(f)` (factions.js:220-252)

Computed every 1 s for HUMAN factions only (`updateFaction`, factions.js:895-898), stored in `f.rate` and
replicated (net.js:184). Displayed in the top bar under each resource as `+${r}/min` (green `#2f7a3a` if > 0, grey
`#9a8f80` otherwise; ui.js:180-186) with tooltip title `RES_LABEL[k]` and text
`Rendita attesa: +${r} al minuto. Conta chi ci lavora adesso, quanto porta a ogni viaggio e quanto dura il giro fra la risorsa e il punto di consegna, più i punti conquistati.`
Also in the Workers screen.
```
rate = {wood:0, gold:0, meat:0, stone:0}; side = f.idx+1
for u in f.pawns:
   o = u.order; n = (u.node && nodeValid(u.node)) ? u.node : null
   if o.k != 'gather': continue                  // sheltered pawns (order 'shelter') do NOT count
   if n:
      site = dropSiteAt(f, n.x, n.y, o.res)       // nearest drop site to the NODE
      here = cellAt(n.x, n.y); d = ∞
      if site:
         field = World.fieldFor(site, side, cellAt(u.x,u.y))
         d = field[here]
         if !(d < ∞) && n.cells: d = min over k in n.cells, (dx,dy) in DIRS8 of field[k + dx + dy*W]
      u.trip = d < ∞ ? max(0, d/10 - 0.7) + (sheep ? 1.3 : 0) : -1        // cells per leg
      u.haul = sheep ? 15 : min(10 + mods.carry, nodeLeft(n))
   elif !u.carry: continue                        // searching: doesn't count
   // carrying with no node: reuse last trip/haul stored on the unit
   if !(u.trip >= 0): continue
   rate[o.res] += u.haul / (JOB_TIME[res] / mods.gather[res] + 2*u.trip*TILE / u.speed + 0.9) * 60
flock = count of live sheep within 2600 px of castle and explored            // (0 if no castle)
rate.meat = min(rate.meat, (has.farm * 60/9 * mods.breed + flock) * 15)
for p in G.points: if p.owner == f && p.def.res: rate[res] += 30
round each rate
```
`0.9` = pauses (0.5 after harvest + 0.4 after delivery). The `-0.7` / `+1.3` are empirical corrections (comment
factions.js:225-226). Note `flock` mixes a count with a per-minute rate — keep as is for parity.
Neighbour index `k + dx + dy*W` is not bounds/row-wrap checked in JS (reads `undefined` → ignored); in Rust check
bounds.

---

## 13. Projectiles (entities.js:440-457, 1221-1264)

### 13.1 Arrow (`fireArrow(owner, x, y, tx, ty, dmg)`)
```
d = hypot(tx-x, ty-y) || 1; sp = 640
{ id, x, y, vx: (tx-x)/d*sp, vy: (ty-y)/d*sp, ang: atan2(ty-y, tx-x), faction: owner.faction, dmg, owner,
  life: (d + 70)/sp }                       // flies 70 px past the aim point
sfx 'shoot'
```
Owners: archers (other spec; spawn at `(u.x ± 16, u.y - 34)`, aim at target `(t.x, t.y-28)` or building centre
`((x0+x1)/2, (y0+y1)/2 - 20)`), towers and castle (§9).

### 13.2 Rock (`fireRock(owner, tx, ty, dmg)`) — catapults
```
sx = owner.x + (owner.flip ? -20 : 20); sy = owner.y - 50; d = hypot(tx-sx, ty-sy)
{ id, rock:true, x:sx, y:sy, sx, sy, tx, ty, t:0, dur: 0.5 + d/520, ang:0, faction, dmg, owner }
```

### 13.3 `updateArrows(dt)`
```
for a in G.arrows:
  if a.rock:
     k = min(1, (a.t += dt)/a.dur)
     a.x = lerp(sx,tx,k); a.y = lerp(sy,ty,k) - sin(k*π)*150; a.ang += 9*dt     // arc is visual: no mid-flight hits
     if k >= 1: a.dead = true; rockImpact(a)
     continue
  a.x += vx*dt; a.y += vy*dt
  if (a.life -= dt) <= 0: dead; continue
  for u in G.units (order):                       // first enemy unit overlapped is hit (not necessarily the target)
     skip dead / inside / same faction
     if |u.x - a.x| < 18 && |u.y - 28 - a.y| < 26: hurt(u, damageTo(a.owner, a.dmg, u), a); sfx hit; dead; break
  if dead: continue
  for b in G.buildings:                            // enemy buildings in the path also stop arrows
     skip dead / same faction
     if a.x > x0 && a.x < x1 && a.y > y0 - 70 && a.y < y1: hurt(b, damageTo(owner, dmg, b), a); dead; break
G.arrows = filter(!dead)
rockImpact(a):
  R = a.owner.def.splash
  units: !dead && !inside && faction != a.faction && hypot(u - (tx,ty)) < R → hurt(u, damageTo(owner, dmg, u), a)
  buildings: !dead && faction != a.faction && rectDist(tx, ty, b) < R*0.5 → hurt(b, damageTo(owner, dmg, b), a)
  fx boom1 at (tx, ty-10) dy 30; fx dust2 at (tx, ty+6) scale 1.6; sfx boom
```
`damageTo(src, base, t) = base * (t.isBuilding ? (src.def.vsBuilding || 1) : (src.def.bonus?.[t.type] || 1))`
(:245-248). For tower/castle owners `def` = BUILD_DEF (no bonus) → ×1. `hurt` with an arrow (`by.vx !== undefined`):
ram ×0.2; units with `def.block` or knights × `mods.arrowRes` (shields 0.5); buildings ×0.4. When the shooter is a
building, `src = by.owner` is not a unit → no threat/alarm bookkeeping. No friendly fire. Arrows/rocks persist
even if their owner dies (they keep `owner` reference for `def`).

Replication: arrows sent as `[id, x, y, ang, factionIdx, rock]` every snapshot (net.js:179).

---

## 14. Effects and presentation events (entities.js:56-92, 1295-1300)

The sim produces presentation EVENTS; the host queues them for clients (`Net.events`), and they're shown
locally. Event tuples (recipient index -1 = everyone):
- `['fx', name, x, y, scale, dy, followId, color]` — sprite animation; `dy` = draw-order offset; follow = unit id.
  Names used in this scope: `dust1`, `dust2`, `boom1`, `boom2`, `heal`. Removed when `t*fps >= frames`.
- `['prop', name, x, y]` — permanent decor (e.g. `chest_open`).
- `['snd', ...]` positional sound (`Sfx.at`); `['play', factionIdx, name]` UI sound for one player
  (`build`, `coin`, `research`, `age`, `win`).
- `['float', factionIdx, x, y, str, kind]` — floating "+N" with resource icon; rises 28 px/s, lasts 1.2 s.
- `['toast', factionIdx, text]`; `['alert', factionIdx, x, y]` (minimap ping); `['say', ...]` banners (max 4 queued).
`onScreen()` culling (:44-47) only gates cosmetic fx/sfx and is always true when hosting; the Rust sim should emit
events unconditionally (or not at all) and let clients cull — never let camera state affect simulation.

---

## 15. Separation of units (entities.js:1029-1066) — relevant to workers
Spatial hash of 64 px cells (key `floor(x/64)*4096 + floor(y/64)`), skip `inside`. For each pair (i<j, visited in
ascending j order) closer than R = 32: `push = (32 - d) * 0.12` along the pair axis (if `d <= 0.01` use
`(cos i, sin i)`); weight 0.2 for units in state `work` or `attack`, 1 otherwise; `World.tryMove(a, -u*push*wa)`,
`World.tryMove(b, +u*push*wb)`. Working pawns are thus hard to shove off their node.

---

## 16. Interactions with faction economy (factions.js)
- Start resources `{wood:200, gold:100, meat:200, stone:100}` (factions.js:10).
- `f.gathered` (stats) increases with deposits and point income only (not pickups, not market).
- AI pawn job assignment (`factions.js:439-` `aiJobMix` etc.) is in the AI spec; AI pawns that find no node go
  `idle` and are reassigned by the AI.

---

## 17. Determinism / sim-vs-render notes for the Rust port
- Random calls inside the sim that must use the seeded sim RNG: `makeSheep` (position, t, flip), sheep wander
  targets and 45% grass choice, meadow pick for respawn, `dropPickup` angle/radius, loot x jitter, `deploy`
  positions, `leaveShelter` exit x, `addBuilding` `cd`, `fireSeed` (render — may use any RNG), `spawnUnit`
  (`animT`, `thinkT`, `fearT`), fx positions (render).
- `twinkle`, `phase`, `fireSeed`, `drawWall`, `drawBuilding`, fx/floats/banners, sfx: render only.
- `state = 'work'`, `u.anim`, `u.flip`: sim-visible (separation weights / attack frame timing for soldiers) and
  replicated.
- Tower arrow muzzle height (`def.ay * 0.62`) and unit hit box (`y - 28`, ±18/±26) are sprite-derived constants
  baked into sim; keep numbers.
- Iteration order matters: units in `G.units` order (spawn order), buildings in `G.buildings` order, pickups go to
  the first unit in array order, points/capture scans use unit order.
- Use `f64` (JS numbers) or carefully chosen `f32`; the field distances are integers in tenths of a cell.

---

## 18. Open questions / ambiguities
1. `relocateBuilding` replaces `b.pasture` with a new object: sheep born before the move keep their old `home`
   (they graze at the old place and are no longer counted toward the farm's cap of 5, so the farm can exceed 5
   nearby sheep). Likely unintended — recommend: update `home` of the farm's sheep (or store home as building id +
   offset). Decide whether to preserve the quirk.
2. Sheep keep wandering while being butchered; work continues only while within 36 px. Intended?
3. `workT` carries over when the booked node is lost mid-work (a new node can complete faster). Minor exploit.
4. A carrying pawn with no valid drop site (e.g. only a farm exists and it carries wood after castle loss) waits
   forever with no feedback.
5. `buildStep` 'blocked' → `idle` drops `resume`; shelter `resumeWork` for `build` keeps resume. Preserve.
6. `afterBuild` near depot/farm ignores `nodeRoom`, so pawns may pile onto a full node and then re-search.
7. `estimateRates` neighbour lookup lacks bounds/row-wrap checks; `flock` (count) is added to a per-minute rate.
8. `drawWall` neighbour test `c-1`/`c+1` wraps across rows at map edges (render only).
9. Whether depleted tree stumps and capture-point cells block movement is defined in world generation
   (`world.js`), not here — see world spec.
10. `nodeNear` dereferences `f.castle` without null check (castle lost = faction eliminated, so practically safe).

---

## Addenda (critic)

Resolutions of §18 open questions, re-verified in the code. Default rule: port faithfully unless the quirk is a
visible bug; deviations are marked **FIX**.

**G1. Moved farms (FIX).** `relocateBuilding` (entities.js:1098-1110) *replaces* `b.pasture` with a new object
(entities.js:1106). Sheep keep `s.home` = old object (`makeSheep(home)`, entities.js:128-135), keep wandering around
the old spot (`updateResources`, entities.js:176-178), and the breeding cap
`G.sheep.filter(s => s.home === b.pasture).length < FARM_SHEEP` (entities.js:1205) counts 0 → the farm breeds up to
5 more. Rust: sheep store `home: Home::Farm(BuildingId) | Home::Meadow(idx)`; the pasture point is read from the farm
each time; relocation therefore moves the flock's wander centre and the cap stays 5. When a farm is destroyed its
sheep become `Home::Point(x,y)` at the last pasture (JS behavior: they keep grazing the old spot forever, and are not
counted by any farm or meadow respawn).

**G2. Work timer carries over — keep.** `claim` (entities.js:273-278) does not reset `u.workT`; only `setOrder`
does (entities.js:285). After a harvest `workT = 0` (entities.js:852). Carry-over only happens when a node is lost
*mid-work* (taken/depleted by another pawn, or the closer-sheep switch at entities.js:829-833): the next node needs
less time. Harmless; keep for parity.

**G3. "Stuck carriers" — not reachable in practice; keep.** With a load, `gatherStep` (entities.js:808-812) calls
`dropSite` every tick and waits if `site == null`. The castle accepts all four kinds
(`drop: ['wood','gold','meat','stone']`, assets.js:227), and losing the castle eliminates the faction
(`destroyBuilding` → `eliminateFaction`, entities.js:1158). So `site` is never null for a living faction. If the path
is blocked, `navTo` returns 'blocked' which `gatherStep` ignores → the pawn retries every tick until the path opens.
Keep.

**G4. Sheep keep moving while butchered — keep.** `updateResources` wanders every live sheep regardless of `users`
(entities.js:166-179); the pawn re-approaches (`d < 36`, `approach`, entities.js:834-835) and `workOn` only runs
while within 36 px. Speed 38 px/s, so the pawn follows; it only slightly lengthens the job. Keep.

**G5. Income estimate quirks — keep the formula, add bounds.** `estimateRates` (factions.js:227-252):
`rate.meat = min(rate.meat, (farms*60/FARM_BREED*breed + flock) * SHEEP_MEAT)` where `flock` is a *count* of sheep
within `WORK_RADIUS` of the castle and explored. Keep this exact formula (it only feeds the HUD number). The
neighbour lookup `field[k + dx + dy*W]` (factions.js:238) has no bounds check; in Rust treat out-of-range as
`INFINITY` (JS: `undefined < d` is false). Numbers are rounded with `Math.round` at the end (factions.js:251).

**G6. Lost resume when the build path is blocked — FIX (small).** `buildStep` (entities.js:879-891, blocked branch at :883):
`if r === 'blocked' { idle(u); return; }` drops `o.resume`, whereas the normal completion path (`afterBuild`,
entities.js:858-877) resumes it. Rust: on 'blocked', `if let Some(resume) = o.resume { set_order(u, resume with first=None) } else { idle(u) }`
(same as the last line of `afterBuild`). Visible effect only: a pawn sent to build an unreachable site goes back to
its previous gathering job instead of standing idle.

**G7. Workers sliders send several pawns to the same node — keep.** `setWorkers` (factions.js:313-333) picks
`nodeNear` per pawn without `claim`, so the loop can pick the same node repeatedly (users are only incremented when the
pawn's `gatherStep` claims it). The losers' `findNode` falls back to the nearest free node of the same resource around
`o.px,o.py` (entities.js:765-787), so the crowd spreads out within one or two ticks. Keep (no gameplay damage).

**G8. Gold twinkle consumes RNG inside the sim.** `twinkle` (entities.js:153-158) is called from `updateResources`
and draws `rand(3,7)`; it is purely visual (the client also runs it, net.js:322). Move it to the client in Rust (see
03a addenda U6).
