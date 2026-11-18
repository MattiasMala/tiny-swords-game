# 04b — AI of the computer-controlled kingdoms (porting spec)

Source of truth: `js/factions.js` lines 11–37 (levels and plan), 39–62 (faction state), 430–902 (AI),
plus the AI hooks in `js/entities.js`, `js/net.js` (takeover) and `js/main.js` (setup / tick order).
All numbers below are copied verbatim from the JS. Distances are **world pixels** (`TILE = 64` px per cell,
`js/assets.js:4`) unless written "cells". Time is **seconds of simulation time** (`G.time`).

Everything in this document is pure simulation (no rendering), except the items explicitly tagged
**[presentation]** (announcements / sounds), which in the Rust sim must become *events* emitted to clients.

---

## 0. Summary of the architecture

- Every faction (human or not) gets an `ai` sub-object at creation (`makeFaction`, factions.js:59–60).
  It is only *used* while `f.human === false`.
- `updateFaction(f, dt)` (factions.js:891–902) runs for every faction at the start of every sim step
  (`updateWorld`, main.js:290–294: factions first, then units, then buildings…).
  - non-human: passive income `trickle` (per level) and, every `level.think` seconds, `aiThink(f, level.think)`.
- `aiThink` (factions.js:813–889) is a single function that, in this fixed order:
  1. assigns idle pawns to resources (`aiAssignPawns`);
  2. advances the fixed **build/research plan** (`AI_PLAN`), builds houses / camps / lost buildings / extra farms;
  3. gathers intel on enemies (`aiIntel`), computes the desired army mix (`aiWant`);
  4. defends (`aiDefend`: rally, recall wave, emergency tower);
  5. builds extra factories when rich;
  6. trains pawns or soldiers;
  7. trades at the market;
  8. runs the attack-wave state machine (`aiLaunch` / `aiMarch`);
  9. sends capture squads (`aiCapture`);
  10. sends stray soldiers home.
- The AI acts exclusively through the same primitives a player uses: `execCommand(f, 'place'|'train'|'research'|'trade', …)`
  and `orderUnits(f, ids, 'amove'|'move', x, y)`, plus direct `setOrder` on its own pawns.
  It never reads hidden information restrictions: **the AI has no fog of war** (`explored()` returns true for
  non-human factions, factions.js:292–296).
- Randomness: `rand(a,b) = a + Math.random()*(b-a)` (assets.js:600). Used in `makeFaction` (thinkT, waveT),
  `aiLaunch` (next waveT), `chooseTarget` (weighted pick), `findSpot` (ring shuffle). The Rust sim must replace
  every `Math.random()` with the deterministic sim RNG (seeded per match), consumed in the same order.

---

## 1. Difficulty levels — `AI_LEVEL` (factions.js:11–27)

Keys / order: `AI_LEVELS = ['easy', 'normal', 'hard']` (factions.js:27; order is used by the UI row).
UI labels (verbatim, must be kept): **`Facile`**, **`Normale`**, **`Difficile`**.
Default level when none is given: `'normal'` (makeFaction default, main.js:82 `setup.level || 'normal'`, net.js:19).
In multiplayer the host picks the level (`Net.setLevel`, net.js:110–114); one level applies to **all** AI kingdoms.

| field | meaning | easy (Facile) | normal (Normale) | hard (Difficile) |
|---|---|---|---|---|
| `trickle` | passive income, **each** of the 4 resources, per second | 0 | 0 | 0.6 |
| `think` | seconds between two `aiThink` calls (also the `dt` passed to it) | 2.4 | 1.5 | 1 |
| `pawns` | desired pawn count per age `[age1, age2, age3]` | [7, 10, 12] | [9, 13, 16] | [12, 18, 22] |
| `army` | army cap (soldiers incl. queued; also used for camp building) | 9 | 18 | 36 |
| `first` | `[min,max]` seconds before the first wave | [420, 540] | [360, 450] | [240, 300] |
| `gap` | `[min,max]` seconds between later waves | [160, 220] | [160, 200] | [90, 120] |
| `farms` | max farms (plan farm steps beyond this are skipped) | 2 | 3 | 4 |
| `factories` | max production buildings per type for "extra factories" | 1 | 1 | 2 |
| `trade` | buys food at market when gold is plentiful; also sells more eagerly | false | false | true |
| `counter` | how strongly the army mix chases counters (0 = base army) | 0.3 | 1 | 1.8 |
| `garrison` | share of the army that stays home | 0.1 | 0.15 | 0.3 |
| `odds` | wave power required relative to target defense | 0.8 | 0.8 | 1.1 |
| `retreat` | wave retreats when remaining value < retreat × start value | 0.2 | 0.3 | 0.45 |
| `reinforce` | free home soldiers join a running assault | false | true | true |
| `rally` | idle home soldiers move against invaders | false | false | true |
| `recall` | recall the wave if the base is in danger | false | false | true |
| `towers` | max towers (plan + emergency) | 0 | 0 | 3 |
| `monks` | max monks | 1 | 1 | 3 |
| `siege` | max siege engines | 1 | 1 | 2 |
| `capture` | sends squads to capture neutral points | false | true | true |

Original Italian comment explaining the fields (factions.js:11–18) — for reference only, not UI.

Derived: `waveNeed` cap = `round(army*(1-garrison)) - 1` → easy **7**, normal **14**, hard **24**.

UI only (rendering): `LEVEL_UNIT = { easy: 'pawn', normal: 'warrior', hard: 'knight' }` (ui.js:557) — the figure drawn
for each difficulty in the selector. Menu strings: `'Difficoltà dei regni avversari'` (ui.js:609),
`'Difficoltà dell\'IA'` / `'Difficoltà dell\'IA (la sceglie l\'host)'` (ui.js:634),
lobby: `` `Giocatori: ${n} su ${COLORS.length} · i regni liberi li guida l'IA` `` (ui.js:627), slot label `'IA'` (ui.js:631).

---

## 2. Other constants used by the AI

| const | value | where | meaning |
|---|---|---|---|
| `PAWN_MAX` | 30 | factions.js:4 | absolute pawn cap |
| `TROOP_MAX` | 50 | factions.js:4 | absolute troop cap |
| `QUEUE_MAX` | 5 | factions.js:4 | max queue length per building (the AI itself never queues beyond 2) |
| `RAID_SPACING` | 120 s | factions.js:5 | a kingdom just raided is an unlikely target for 120 s |
| `BUILD_RADIUS` | 40 cells | factions.js:9 | build zone around castle (depot search uses `BUILD_RADIUS-4`) |
| `DEFENSE_RADIUS` | 12 cells | factions.js:9 | towers/walls zone |
| `OUTPOST_RADIUS` | 9 cells | factions.js:9 | build zone around owned outposts |
| `START_RES` | wood 200, gold 100, meat 200, stone 100 | factions.js:10 | |
| `AI_BASE` | `{ warrior: 3, lancer: 2, archer: 2, knight: 2 }` | factions.js:433 | base army weights |
| `AI_HOME` | 1400 px | factions.js:434 | "at home" / "threatens base" radius around castle |
| `RES_KINDS` | `['wood','gold','meat','stone']` | assets.js:415 | iteration order matters (tie-breaks) |
| `SELL_PRICE`, `SELL_DROP`, `BUY_PRICE`, `BUY_RISE` | 50, 6, 130, 10 | factions.js:257 | market |
| `sellPrice(f,r)` | `max(20, 50 - trades[r]*6)` | factions.js:258 | gold per 100 sold |
| `buyPrice(f,r)` | `min(320, 130 + trades[r]*10)` | factions.js:259 | gold per 100 bought |
| `CAPTURE_RADIUS` | 300 px | entities.js:31 | |
| `HOME_RADIUS` | 1500 px | entities.js:20 | (human alarm only) |
| `RAID_RADIUS` | 900 px | entities.js:21 | (human auto-assault chain only) |

Unit costs & `aiCost` (sum of cost values, factions.js:542) — from `UNIT_DEF` (assets.js:115–169):

| type | cost | aiCost | from | age | dmg | aiFighter |
|---|---|---|---|---|---|---|
| pawn | meat 50 | 50 | castle | 1 | – | no |
| scout | meat 40 | 40 | castle | 1 | – | no (never trained by AI) |
| warrior | meat 58, gold 40 | 98 | barracks | 1 | 13 | yes |
| lancer | meat 60, gold 40 | 100 | barracks | 2 | 20 | yes |
| archer | wood 38, gold 32 | 70 | archery | 2 | 12 | yes |
| knight | meat 80, gold 60 | 140 | stable | 2 | 18 | yes |
| monk | gold 80 | 80 | monastery | 3 | – (heal 40) | no |
| ram | wood 160, gold 60 | 220 | siege | 3 | 55, `onlyBuildings` | **no** |
| catapult | wood 140, gold 120 | 260 | siege | 3 | 42, `siege`, splash 72 | **yes** (has dmg, not onlyBuildings) |

`aiFighter(type) = !!def.dmg && !def.onlyBuildings` (factions.js:544). Note the asymmetries the port must keep:
- **catapult counts as a fighter** in `aiNextUnit`'s `fighters`, in `aiSpare.total`, and as an enemy in `aiEdge`;
  but it is excluded from `aiWant` (which filters `!siege`).
- **ram is not a fighter**, but in `aiSpare`/`aiMarch` "fighters" are filtered by `def.dmg` (truthy for ram),
  so rams *are* sent in squads and *do* count toward wave value.

`aiWorth(u) = aiCost(u.type) * u.hp / u.maxHp` (factions.js:543).

---

## 3. AI state attached to a faction

Created in `makeFaction(color, base, human, level='normal')` (factions.js:40–62) for **every** faction:

```
ai = {
  level:     AI_LEVEL[level]   // reference to the level record (Rust: enum AiLevel + const table)
  step:      0                 // index into AI_PLAN of the next plan item
  thinkT:    rand(1, 3)        // countdown to next aiThink
  waveT:     rand(L.first[0], L.first[1])  // countdown to the next wave (only decremented inside aiThink)
  waveNeed:  4                 // minimum squad size to launch a wave; +2 after each launch, capped
  target:    null              // faction last chosen as wave target (weights intel)
  shuffleT:  20                // countdown for pawn rebalancing
  mix:       null              // desired army share per type (output of aiWant), or null
  wave:      null              // running wave, see §9
  dueT:      null              // G.time when the wave countdown expired (waiting for enough power)
  attackedT: -999              // last G.time the base was in danger
  lackT:     0                 // last G.time the preferred unit was affordable
  captureT:  60                // G.time before which no capture squad is sent (absolute time!)
}
```

Faction fields read/written by the AI (other faction fields documented in the economy spec):
`human, alive, castle, base {x,y cells}, res, age, tech{}, busy{}, has{}, sites{}, pawns[], soldiers[],
pawnN, troopN, pawnCap, troopCap, mods, trades, raidedT (default -999), threat, threatT, idx, conn`.

- `has`, `sites`, `pawns`, `soldiers`, `pawnN`, `troopN`, `pawnCap`, `troopCap` come from `census(f)`
  (factions.js:202–218), recomputed every 1 s (`tickT`, factions.js:895–897), on `completeBuilding`, on demolish,
  and partly incrementally: `place` does `f.sites[a]++` (factions.js:349); `deploy` does `pawnN++`/`troopN++`
  (entities.js:1172). **The AI therefore reads values that may be up to 1 s stale** — keep the same cadence.
- `f.raidedT` is written on the **target** faction by `aiLaunch`.
- Wave unit ids are stored in `ai.wave.ids` (a Set of unit ids); ids are resolved through `G.index`.

`ai.wave` record (factions.js:739):
```
wave = { ids: Set<unitId>, target: Faction, start: number /*value at launch, grows with reinforcements*/,
         phase: 'march' | 'assault', legT: -99 /*G.time of last leg order*/, t0: G.time }
```
(`t0` is written but never read.)

---

## 4. The plan — `AI_PLAN` (factions.js:28–37)

Ordered list; each entry `['build', kind]` or `['tech', id]`:

```
 0 build farm        1 build barracks    2 build depot       3 tech age2         4 build farm
 5 build archery     6 build smith       7 build farm        8 build stable      9 tech blades
10 build tower      11 tech axes        12 build market     13 build farm       14 tech age3
15 build monastery  16 tech armor       17 build siege      18 tech picks       19 tech fletching
20 build tower      21 tech carts       22 tech horseshoes  23 tech herbs       24 tech breeding
25 tech boots       26 tech masonry     27 tech engineering 28 tech scriptures
```
(29 entries; after index 28 `AI_PLAN[ai.step]` is `undefined` → no step, no stepCost.)

Rules:
- **Skip rule** (factions.js:818–819): if the current step's kind is `farm` or `tower` and
  `has[kind] + sites[kind] >= L.farms` (resp. `L.towers`), then `ai.step++`. Only **one** skip per think.
  Easy/normal have `towers: 0`, so both tower steps are always skipped.
- Build steps advance only when `aiBuild` succeeds (factions.js:832). There is **no** "already owned" check:
  a build step always places a *new* building (relevant for takeover, §12.4).
- Tech steps (§6.2) advance when the tech is researched/being researched; prerequisites (`req`) are researched first
  automatically; a missing research building is built first.
- Houses, camps, rebuilding and extra farms/factories are decided outside the plan.

Tech requirements used (assets.js:326–395): `age3.req = age2`, `picks.req = axes`, `armor.req = shields`,
`scriptures.req = herbs`. So the AI also researches **shields** (as prerequisite of armor). Never researched by AI:
`maps`. Research buildings: age2/age3/carts/masonry → castle; axes/picks → depot; breeding → farm; boots → barracks;
horseshoes → stable; blades/shields/fletching/armor → smith; maps → market; herbs/scriptures → monastery;
engineering → siege.

---

## 5. Building placement — `aiBuild(f, kind, toward=null)` (factions.js:501–528)

Returns true if a construction site was placed.

```
def = BUILD_DEF[kind]; c = f.castle
if f.age < def.age or !canAfford(f, def.cost) or f.pawns.length == 0: return false
x, y, rmin = f.base.x, f.base.y + 1, 4            // cells; base = castle cell (castle spans base.x-2..base.x+2, base.y-1..base.y)
if kind == 'depot':
    best = null; bd = +inf
    for n in [...G.trees, ...G.golds, ...G.stones]:          // this order
        if !nodeValid(n): continue
        d = hypot(n.x - c.x, n.y - c.y)
        if d < 420 or d > bd or d > (BUILD_RADIUS - 4) * TILE /*36*64 = 2304*/: continue
        if exists building b of f with b.def.drop (castle, depot, FARM) and rectDist(n.x, n.y, b) < 380: continue
        bd = d; best = n
    if best: x = floor(best.x / TILE); y = floor(best.y / TILE); rmin = 2
elif kind == 'tower':
    foe = toward ?? nearestEnemyFaction(f)?.castle      // toward = a unit (the closest invader) or null
    if foe:
        a = atan2(foe.y - c.y, foe.x - c.x)
        x += round(cos(a) * 7); y += round(sin(a) * 6); rmin = 1
spot = findSpot(kind, x, y, rmin)                       // rmax default 18
if !spot: return false
near = f.pawns sorted by hypot(p.x - spot[0]*TILE, p.y - spot[1]*TILE) ascending   // includes sheltered pawns
builders = near[0 .. (def.work > 15 ? 2 : 1)]
return execCommand(f, 'place', kind, spot[0], spot[1], builders.ids) == null
```
Notes:
- Note `d > bd` (not `>=`) — ties keep the earlier node. Node order is the global arrays' order.
- `farm` has `drop: ['meat']`, so a farm within 380 px of a wood/gold/stone node also blocks a depot there (faithful quirk).
- Builders: 2 pawns for `work > 15` (barracks 18, archery 18, stable 18, smith 18, market 18, tower 16, monastery 22,
  siege 22), else 1 (house 9, camp 12, depot 12, farm 14).
- `findSpot(kind, x0, y0, rmin=3, rmax=18)` (factions.js:177–199): for r = rmin..rmax, build the square ring of
  offsets `[i,-r],[i,r],[-r,i],[r,i]` for i=-r..r (corners appear twice), **shuffle randomly**
  (`sort(() => Math.random()-0.5)` — in Rust use a proper RNG shuffle; exact permutation need not match JS), and return
  the first `(cx,cy) = (round(x0+dx - w/2), round(y0+dy - h/2))` where `canPlace` holds and the 1-cell border around
  the footprint is inside the map and passable.
- The command `place` re-validates (`placeBlock`: age, cost, `zoneBlock` = within 40 cells of castle centre
  (12 for defense) or 9 of an owned outpost, `canPlace`), pays, increments `sites`, orders the builders `'build'`.
  If it returns a reason string the AI simply gets `false` (towers placed > 12 cells away fail this way).
- `nearestEnemyFaction(f)` (factions.js:530–538): alive faction ≠ f with the closest castle (strict `<`).
- Builders who finish (`afterBuild`, entities.js:858–877) go to another unfinished own site within 700 px, or for a
  just-finished depot/farm start gathering the nearest node within 520 px (sheep for farm), or resume their previous
  gather order, or idle. Idle AI pawns are picked up by `aiAssignPawns`.

---

## 6. Economy

### 6.1 Worker allocation — `aiJobMix(f)` (factions.js:439–459) and `aiAssignPawns(f, dt)` (473–499)

`aiJobMix` returns a normalized share per resource:

```
need = { wood: 0.5, gold: 0.15, meat: 0.35, stone: (f.age > 1 ? 0.12 : 0) }
add(cost, w): s = Σ cost[k]; for k in cost: need[k] += w * cost[k] / s
step = AI_PLAN[ai.step]
if step: add((step is build ? BUILD_DEF : TECH_DEF)[step[1]].cost, 1)
if f.age == 1 and !(step and step[1] == 'age2'): add(TECH_DEF.age2.cost /*meat 250, gold 100*/, 0.6)
if f.pawns.length < L.pawns[f.age-1]: add(UNIT_DEF.pawn.cost /*meat 50*/, 0.5)
if ai.mix: for t in ai.mix: add(UNIT_DEF[t].cost, ai.mix[t] * (f.age > 1 ? 1.5 : 0.8))
for k in need (wood, gold, meat, stone):
    if f.res[k] > 800: need[k] *= 0.25
    elif f.res[k] > 400: need[k] *= 0.5
all = Σ need; need[k] /= all
```
(Uses the **previous** think's `ai.mix`, since `aiWant` runs later in `aiThink`.)

`aiAssignPawns(f, dt)` (dt = level.think):
```
jobs = {wood:0,gold:0,meat:0,stone:0}; idlers = []; n = f.pawns.length
for p in f.pawns:
    if p.order.k == 'gather': jobs[p.order.res]++
    elif p.order.k == 'idle': idlers.push(p)
    // pawns building / repairing / dropping / sheltering are neither counted nor reassigned
mix = aiJobMix(f); lack(k) = mix[k]*n - jobs[k]
neediest() = argmax over RES_KINDS of lack (strict >, so ties → earliest in RES_KINDS)
ai.shuffleT -= dt
if ai.shuffleT <= 0 and idlers is empty:
    ai.shuffleT = 6
    over = argmin over RES_KINDS of lack (strict <, ties → earliest)
    if lack(over) < -1 and lack(neediest()) > 1:
        p = first pawn in f.pawns with order gather, res == over, and no carry
        if p: jobs[over]--; idlers.push(p)
for p in idlers:
    order = RES_KINDS stably sorted by lack descending      // lack recomputed with updated jobs
    for res in order:
        node = aiNode(f, res, p)
        if node: setOrder(p, gatherOrder(node)); jobs[res]++; break
```
Note: when `idlers` is non-empty at the moment shuffleT expires, `shuffleT` stays ≤ 0 and the shuffle fires on the
first later think with no idlers.

`aiNode(f, res, p)` (factions.js:462–471): over `G.trees | G.sheep | G.golds | G.stones` (by res), skipping
`!nodeValid(n) || !nodeRoom(n)`, minimize `d = hypot(n - castle) + 0.3 * hypot(n - pawn)` with initial bound **2200**
(strict `<`). No fog check for AI. `nodeRoom` uses `n.users`, which is only incremented when the pawn actually claims
the node in `gatherStep` — so several idlers in one think may target the same node; `findNode` sorts it out later.

`gatherOrder(n) = { k:'gather', res: NODE_RES[n.kind], first: n, px: n.x, py: n.y }` (entities.js:288).

Entity-side hooks specific to AI pawns (entities.js):
- `gatherStep`, entities.js:817–821: if no node can be found, an **AI** pawn goes `idle` (to be reassigned by
  `aiAssignPawns`); a human pawn keeps its job and retries after 1 s.
- `findNode`, entities.js:765–788: `wide = f.human && c` — only human pawns widen the search to `WORK_RADIUS`
  around the castle; AI pawns only search within 900 px of `(o.px, o.py)` (and, for sheep, the closest overall within that).
- Fog: `explored()` is always true for AI (factions.js:293).
- Pawns flee to shelter (entities.js:955–966) identically for AI and humans; only humans get `embolden`
  from orders (entities.js:360).

### 6.2 Plan, houses, camps, rebuild, farms — part of `aiThink` (factions.js:816–843)

```
open = Σ values of f.sites          // number of open construction sites (all kinds, incl. walls)
<skip rule, §4>
step = AI_PLAN[ai.step]; stepCost = step ? (build ? BUILD_DEF : TECH_DEF)[step[1]].cost : null
comingPawns  = queued(f, t => t == 'pawn')
comingTroops = queued(f, t => t != 'pawn')       // includes scouts, monks, siege in queues
if !f.sites.house and f.pawnCap < PAWN_MAX and f.pawnN + comingPawns >= f.pawnCap - 2
   and L.pawns[f.age-1] > f.pawnCap - 2:
    aiBuild(f, 'house')
elif !f.sites.camp and f.troopCap < TROOP_MAX and f.troopN + comingTroops >= f.troopCap - 2
   and L.army > f.troopCap - 2:
    aiBuild(f, 'camp')
elif open < 2:
    lost = first k in ['farm', 'barracks', 'depot'] with !f.has[k] and !f.sites[k]
           and AI_PLAN.findIndex(s => s[1] == k) < ai.step     // first plan occurrence already passed
    farms = (f.has.farm || 0) + (f.sites.farm || 0)
    if lost: aiBuild(f, lost)
    elif farms > 0 and farms < L.farms and !f.sites.farm
         and farms * 2.5 + 2 < aiJobMix(f).meat * f.pawns.length:
        aiBuild(f, 'farm')                                      // one farm ≈ 2.5 pawns of food
    elif step and step[0] == 'build':
        if aiBuild(f, step[1]): ai.step++
```
- House/camp/rebuild/extra farm/plan build are mutually exclusive per think (else-chain); the house and camp checks
  ignore `open`.
- Note the house check is on `pawnCap` (houses give +5, castle 10), the camp check on `troopCap`
  (camp +8, castle 6, barracks/archery/stable/monastery/siege +2 each).
- `queued(f, test)` (factions.js:678–682): count queue items with `q.unit` across all own buildings.

Tech step (factions.js:834–843), executed in the same think regardless of the else-chain above:
```
if step and step[0] == 'tech':
    id = step[1]
    while TECH_DEF[id].req and !f.tech[TECH_DEF[id].req]: id = TECH_DEF[id].req
    t = TECH_DEF[id]; b = freeBuilding(f, t.at)
    if f.tech[step[1]] or f.busy[step[1]]: ai.step++
    elif f.busy[id]: pass                                   // prerequisite already in progress
    elif !b: if !f.sites[t.at] and open < 2: aiBuild(f, t.at)   // `open` is the value computed at the top
    elif b.queue.length < 2 and execCommand(f, 'research', b.id, id) == null and id == step[1]: ai.step++
```
- `freeBuilding(f, kind)` (factions.js:671–677): finished, alive own building of that kind with the shortest queue
  (strict `<`, first wins).
- `research` can fail (age lock, cost, queue full, already busy) → no change, retried next think.
- After researching a prerequisite (`id != step[1]`) the step is not advanced; next thinks see `busy[id]` → wait,
  then research `step[1]`.

### 6.3 Extra factories (factions.js:848–852)
After intel and defense:
```
if open < 2 and ai.mix and for every k in RES_KINDS: (k == 'stone' or f.res[k] > 500):
    top  = type with the highest ai.mix share (ties → first in key order: warrior, lancer, archer, knight)
    kind = UNIT_DEF[top].from
    if (f.has[kind] || 0) + (f.sites[kind] || 0) < L.factories: aiBuild(f, kind)
```
(With factories = 1 on easy/normal this only rebuilds a missing one; hard builds a 2nd copy. Extra buildings of the
same type also train faster: `trainSpeed(n) = 1 + 0.2 * clamp(n-1, 0, 5)`, entities.js:1177.)

### 6.4 Training (factions.js:854–869)
```
if f.pawnN + comingPawns < min(L.pawns[f.age-1], f.pawnCap)
   and !(intel.danger > intel.homeValue and f.soldiers.length < 4):
    if c.queue.length < 2: execCommand(f, 'train', c.id, 'pawn')       // c = castle
elif f.troopN + comingTroops < min(L.army, f.troopCap):
    t = aiNextUnit(f, intel)
    if t and canAfford(f, UNIT_DEF[t].cost): ai.lackT = G.time
    elif G.time - (ai.lackT || 0) > 12: t = aiNextUnit(f, intel, cheap=true)
    d = t and UNIT_DEF[t]; b = d and freeBuilding(f, d.from)
    hold = (f.age == 1 and !f.busy.age2) ? TECH_DEF.age2.cost : stepCost
    saving = d and hold and !intel.danger and f.soldiers.length >= 3
             and exists k in RES_KINDS: (d.cost[k] || 0) > 0 and f.res[k] - d.cost[k] < (hold[k] || 0)
    if b and !saving and b.queue.length < 2: execCommand(f, 'train', b.id, t)
```
- At most **one** unit is queued per think. While more pawns are wanted (and not in danger with < 4 soldiers),
  **no soldiers** are trained, even if the castle queue is full.
- Pawn training does not respect `stepCost` savings.
- `f.soldiers` = all own non-pawn units (incl. scouts, monks, siege).
- A unit finished but without room waits in the queue (`updateQueue`, entities.js:1189–1196); the "Servono più case"
  toasts are human-only.

### 6.5 Next unit — `aiNextUnit(f, intel, cheap=false)` (factions.js:620–652)
```
have = count of own alive soldiers by type + queued units by type in all own buildings (incl. 'pawn' in castle)
ok(t) = f.has[UNIT_DEF[t].from] and f.age >= UNIT_DEF[t].age
fighters = Σ have[t] for t with aiFighter(t)          // warriors, lancers, archers, knights, catapults
list = []
for t in UNIT_TYPES (order: pawn, scout, warrior, lancer, archer, knight, monk, ram, catapult):
    if UNIT_DEF[t].heal and ok(t) and (have[t]||0) < min(L.monks, floor(fighters / 5)): list.push(t)   // monk
siege = [t in UNIT_TYPES with siege and ok(t)]                      // ram, catapult
sieges = Σ have[t] for t in siege
if siege non-empty and fighters >= 6 and sieges < min(L.siege, floor(fighters / 6)):
    hunted(t) = (Σ over foes, over k in e.mix with UNIT_DEF[k].bonus?[t] > 1 of e.mix[k]) / intel.total   (0 if total == 0)
    opts = [t in siege with !(have[t] > 0) and (UNIT_DEF[t].onlyBuildings or hunted(t) < 0.3)]
    stable-sort opts: onlyBuildings first (ram before catapult)
    list.push(...opts)
want = ai.mix or {}
lag(t) = want[t] * (fighters + 1) - (have[t] || 0)
list.push(...keys(want) stably sorted by lag descending)     // keys order: warrior, lancer, archer, knight
return cheap ? first t in list with canAfford(f, cost) : list[0]   (or null)
```
Only knights have a bonus > 1 vs catapult (`knight.bonus.catapult = 2`), so `hunted(catapult)` = knights' weighted share.
Rams are always eligible (onlyBuildings). Each siege type at most one at a time (`!(have[t] > 0)`).

### 6.6 Market (factions.js:870–877)
```
market = f.has.market and freeBuilding(f, 'market')
if market:
    extra = first k in ['wood', 'meat', 'stone'] with f.res[k] > (f.res.gold < 80 ? 450 : 700) and sellPrice(f, k) >= 40
    if extra and f.res.gold < (L.trade ? 250 : 80): execCommand(f, 'trade', market.id, 'sell', extra)
    elif L.trade and f.res.meat < 150 and f.res.gold > 450 and buyPrice(f, 'meat') <= 170:
        execCommand(f, 'trade', market.id, 'buy', 'meat')
```
One trade per think. Trades sell/buy 100 units and increment `f.trades[res]`, which decays by 0.04 per second
(factions.js:899). Note `sellPrice >= 40` ⇔ `trades[k] <= 1.67`; `buyPrice <= 170` ⇔ `trades.meat <= 4`.

### 6.7 Passive income (factions.js:893)
Every sim step, for non-human alive factions: `f.res[k] += L.trickle * dt` for all 4 resources (only hard: 0.6/s).
Fractional resources are kept as floats (network snapshots floor them).

---

## 7. Intelligence — `aiIntel(f)` (factions.js:561–593)

```
intel = { foes: Map<Faction, {o, near, mix:{type→weighted value}, defense}>, total: 0,
          danger: 0, dangerAt: null, home: [], homeValue: 0 }
for o in G.factions where o != f and o.alive and o.castle:
    near = 1 / (1 + dist²(o.castle, f.castle) / 4e6) + (o == ai.target ? 1 : 0)
    foes[o] = { o, near, mix: {}, defense: 0 }
dd = +inf
for u in G.units where !u.dead and u.type != 'pawn':        // scouts, monks, siege included
    d = hypot(u - f.castle)
    if u.faction == f:
        if d < AI_HOME: intel.home.push(u); if u.def.dmg: intel.homeValue += aiWorth(u)
        continue
    e = foes[u.faction]; if !e: continue
    v = aiWorth(u); w = e.near + (d < AI_HOME + 800 /*2200*/ ? 2 : 0)
    e.mix[u.type] += v * w
    if hypot(u - u.faction.castle) < AI_HOME: e.defense += v
    intel.total += v * w
    if d < AI_HOME and u.def.dmg:
        intel.danger += v
        if d < dd: dd = d; intel.dangerAt = u
for e in foes: e.defense += 150 * ((e.o.has.tower || 0) + 1)   // castle + towers shoot arrows
```
- `intel.home` contains own non-pawn units within 1400 px (incl. those inside a wave passing by!).
- `danger` is the total worth of armed (`def.dmg`, includes rams) enemy units within 1400 px of the castle.
- Units sheltered (`inside`) are pawns only, so they are excluded anyway.

---

## 8. Desired army composition — `aiWant`, `aiPower`, `aiEdge` (factions.js:546–618)

```
aiPower(f, t, o, e):            // strength per cost² of f's unit t vs o's unit e
    d = UNIT_DEF[t]; de = UNIT_DEF[e]
    hit   = max(1, ((d.dmg || 0) + (f.mods.atk[t] || 0)) * (d.bonus?[e] || 1)
                   - (de.dmg and !de.siege ? o.mods.armor : 0))
    reach = 1 + max(0, d.range - 60) / 1500
    tough = unitMaxHp(f, t) / (1 - (d.block || 0) * 0.65)
    return hit / d.cd * reach * tough / aiCost(t)²

aiEdge(f, t, o, e):
    if !aiFighter(e): return ln(UNIT_DEF[t].bonus?[e] || 1)    // monks, rams, scouts: only the bonus counts
    return ln(aiPower(f, t, o, e) / aiPower(o, e, f, t))

aiWant(f, intel):
    types = [t in UNIT_TYPES with aiFighter(t) and !siege and f.has[UNIT_DEF[t].from] and f.age >= UNIT_DEF[t].age]
            // subset of [warrior, lancer, archer, knight]
    if types empty: return null
    for t in types:
        s = 0
        if intel.total > 300:
            for e in intel.foes: for k in e.mix: s += e.mix[k] / intel.total * aiEdge(f, t, e.o, k)
        else:
            all = Σ AI_BASE (= 9)
            for k in AI_BASE: s += AI_BASE[k] / all * aiEdge(f, t, f, k)   // imagined enemy = own base army, own mods
        score[t] = (AI_BASE[t] || 1) * exp(L.counter * s)
    sum = Σ score
    want[t] = 0.85 * score[t] / sum + 0.15 / types.length
    return want
```
- `unitMaxHp(f, type) = round(hp * (type == 'monk' ? f.mods.monkHp : 1))` (entities.js:236).
- `aiEdge` against a catapult uses the full power formula (catapult is a fighter); against a ram returns ln(bonus) = 0
  for all current units (nobody has a bonus vs ram).
- The baseline case (`aiEdge(f, t, f, k)` with `k` possibly a type the faction cannot yet build) is fine — pure math.
- `ai.mix` is overwritten each think (`ai.mix = aiWant(f, intel)`, factions.js:846), possibly to `null`
  (no barracks yet).

---

## 9. Attack waves

### 9.1 Spare soldiers — `aiSpare(f, intel)` (factions.js:711–723)
```
inWave(s) = ai.wave and ai.wave.ids.has(s.id)
ready    = [s in intel.home with s.order.k == 'idle' and !s.target and !inWave(s)]
fighters = [s in ready with s.def.dmg]       // warriors…knights, ram, catapult
monks    = [s in ready with s.def.heal]
total = count of f.soldiers alive with aiFighter(type)      // excludes rams, monks, scouts
busy  = count of s in intel.home with s.def.dmg and s ∉ fighters and !inWave(s)   // armed home units already busy
keep  = max(2, round(total * (L.garrison + (G.time - (ai.attackedT || -999) < 90 ? 0.15 : 0))))
stable-sort fighters by: (range > 200 ? 1 : 0) ascending (melee first, archers/catapults last), then aiWorth descending
squad = fighters[0 .. max(0, fighters.length - max(0, keep - busy))]
return { squad, monks, total }
```
(i.e. the garrison keeps the ranged/weakest; units already fighting at home count toward the garrison.)

### 9.2 Target choice — `chooseTarget(f, intel=null, power=0)` (factions.js:654–669)
```
opts = [o in G.factions with o != f and o.alive]     // humans and AIs alike
w(o) = 1 / (1 + dist²(o.castle, f.castle) / 4e6)
       * (intel and intel.foes[o] ? 0.3 + power / (power + e.defense) : 1)
       * (G.time - o.raidedT < RAID_SPACING /*120*/ ? 0.1 : 1)
r = random() * Σw; walk opts subtracting w; return first where r <= 0; fallback opts[0] or null
```

### 9.3 Launch — `aiLaunch(f, intel)` (factions.js:725–747)
Called from `aiThink` when `!ai.wave && ai.waveT <= 0 && !intel.danger`; just before it, `if ai.dueT == null: ai.dueT = G.time`.
```
{squad, monks, total} = aiSpare(f, intel)
if squad.length < ai.waveNeed: return false
power = Σ aiWorth(s) for s in squad
target = chooseTarget(f, intel, power); if !target: return false
def  = intel.foes[target].defense
full = total + queued(f, t => t != 'pawn') >= L.army
late = G.time - ai.dueT
if power < L.odds * def * max(0.5, (full ? 0.6 : 1) - late / 300): return false   // retry next think; target re-rolled
squad += monks[0 .. ceil(monks.length * 0.7)]
ai.wave = { ids: Set(squad ids), target, start: power, phase: 'march', legT: -99, t0: G.time }
ai.dueT = null; ai.target = target; target.raidedT = G.time
ai.waveT = rand(L.gap[0], L.gap[1])
ai.waveNeed = min(round(L.army * (1 - L.garrison)) - 1, ai.waveNeed + 2)
if target.human: [presentation] G.announce(f.name + ' ti attacca!', f, 3.5, target); emit(['play', target.idx, 'horn'])
return true
```
- `waveT` keeps decreasing below 0 while waiting; the threshold factor decays from 1 (or 0.6 if the army is full)
  by `late/300` down to a floor of 0.5 — i.e. after 150 s (or 30 s when full) the AI accepts power ≥ 0.5·odds·defense.
- Note: `waveT` is decremented **before** the check in the same think (§11), by `dt = L.think`.
- Wave units are **not** given an order at launch: the first `aiMarch` call (next think) issues the first leg
  (`legT = -99` ⇒ immediate).
- `f.name = 'Regno ' + COLOR_NAME[color]` (factions.js:43; Blu, Rosso, Giallo, Viola, Nero).

### 9.4 March / assault — `aiMarch(f, intel)` (factions.js:749–795)
Called every think while `ai.wave` exists (regardless of danger).
```
w = ai.wave; c = f.castle
units = []; for id in w.ids: u = G.index[id]; if u and !u.dead and u.faction == f: units.push(u) else w.ids.delete(id)
fighters = [u in units with u.def.dmg]
home() = orderUnits(f, units ids, 'amove', c.x, c.y1 + 130); ai.wave = null
if fighters empty: if units non-empty: home() else ai.wave = null; return
value = Σ aiWorth(fighters); (cx, cy) = centroid of fighters
if !w.target.alive:
    next = value >= 0.5 * w.start ? nearestEnemyFaction(f) : null
    if !next or hypot(next.castle - (cx,cy)) > 2600: home(); return
    w.target = ai.target = next; w.phase = 'march'; w.legT = -99
tc = w.target.castle; dT = hypot(tc - (cx,cy))
if value < L.retreat * w.start and tc.hp > 0.3 * tc.maxHp:          // decimated → retreat
    orderUnits(f, units ids, 'move' /*not amove: no fighting*/, c.x, c.y1 + 130)
    ai.wave = null; ai.waveT = max(ai.waveT, 40); return
if w.phase == 'march':
    if dT < 900: w.phase = 'assault'; orderUnits(f, units ids, 'amove', tc.x, tc.y1 + 30); return
    waiting = count of fighters with order.k == 'idle' and !target
    if waiting >= 0.75 * fighters.length or G.time - w.legT > 35:
        step = min(700, dT - 500)
        p = World.nearestPassable(cx + (tc.x - cx)/dT * step, cy + (tc.y - cy)/dT * step)
        orderUnits(f, units ids, 'amove', p.x, p.y); w.legT = G.time
    return
// assault
idle = [u in units with order.k == 'idle' and !target and hypot(u - tc) > 200]
if idle: orderUnits(f, idle ids, 'amove', tc.x, tc.y1 + 30)
if L.reinforce and !intel.danger:
    {squad} = aiSpare(f, intel)
    if squad.length >= 3:
        for s in squad: w.ids.add(s.id); w.start += aiWorth(s)
        orderUnits(f, squad ids, 'amove', tc.x, tc.y1 + 30)
```
- `c.y1` / `tc.y1` = bottom edge (px) of the castle rectangle; `+130` = in front of own castle, `+30` = at the
  enemy castle's front.
- Retreat and home **orphan** the units (wave = null); they become normal idle units at home (strays logic brings
  them back if they stop elsewhere).
- `orderUnits` (entities.js:353–406) lays units out in formation and, for `amove` with armed units, sets a common
  `pace` = slowest armed unit speed; monks get `escort: true` and follow soldiers. When an AI unit reaches an
  `amove` destination it calls `raidOn(u)`, which for **AI** factions always results in `idle(u)` (the auto-chain
  to nearby enemies is human-only, entities.js:626–645). Then `acquireTarget` (idle guard, leash 430 px from the post)
  makes it fight anything within `aggro`.

### 9.5 Defense — `aiDefend(f, intel, stepCost)` (factions.js:684–706)
```
if !intel.danger: return
ai.attackedT = G.time
at = intel.dangerAt
if L.rally:
    free = [s in intel.home with s.def.dmg and !s.def.onlyBuildings and !s.target and s.order.k == 'idle' and !inWave(s)]
    if free: orderUnits(f, free ids, 'amove', at.x, at.y)
w = ai.wave
if L.recall and w and intel.danger > 0.8 * intel.homeValue + 250:
    tc = w.target.castle
    if !w.target.alive or !tc or tc.dead or tc.hp > 0.3 * tc.maxHp:      // unless enemy castle nearly down
        orderUnits(f, w.ids, 'amove', at.x, at.y); ai.wave = null; ai.waveT = max(ai.waveT, 30)
if !f.sites.tower and (f.has.tower || 0) < L.towers
   and f.res.stone - BUILD_DEF.tower.cost.stone /*80*/ >= ((stepCost && stepCost.stone) || 0):
    aiBuild(f, 'tower', at)          // fails silently in age 1 or when unaffordable / out of DEFENSE_RADIUS
```
Entity-side defensive hook (entities.js:541–544): an **AI** soldier on idle guard with no enemy in aggro range
targets `f.threat` (last unit that hurt anything of the faction, set in `hurt`, entities.js:485–486) if
`G.time - f.threatT < 5`, the threat is within 950 px of the castle and the soldier is within 950 px of the castle
("the garrison flushes out whoever hits the kingdom from out of range"). Human factions instead use `f.alarm`
(entities.js:489, 608–622).

### 9.6 Capture squads — `aiCapture(f, intel)` (factions.js:797–811)
```
if !L.capture or ai.wave or intel.danger or G.time < ai.captureT: return
ai.captureT = G.time + 25
{squad} = aiSpare(f, intel)
if squad.length < 3: return
best = null; bd = 2200 + 600 * f.age               // 2800 / 3400 / 4000 px
for p in G.points: d = hypot(p - castle); if p.owner != f and d < bd: bd = d; best = p
if best: orderUnits(f, squad[0..3] ids, 'amove', best.x, best.y1 + 40)
```
- Only normal and hard (`capture: true`). First possible capture at `G.time >= 60` (initial `captureT = 60`, absolute).
- Targets points owned by nobody **or by another kingdom** (it will fight the holders).
- Squad = the 3 first in `aiSpare` order: melee first, strongest first.
- Capture mechanics (entities.js:212–232): a point flips after `CAPTURE_TIME = 8` s with only one faction's
  non-pawn units within `CAPTURE_RADIUS`.

### 9.7 Strays (factions.js:884–888)
```
stray = [s in f.soldiers with !s.dead and s.order.k == 'idle' and !s.target and !inWave(s)
         and hypot(s - castle) > 900
         and no point p with p.owner != f and hypot(p - s) < CAPTURE_RADIUS]
if stray: orderUnits(f, stray ids, 'amove', c.x, c.y1 + 130)
```
Applies to all non-pawn units incl. monks, scouts, siege. Units sitting on a not-yet-owned point stay (they are
capturing); once captured they become strays and return home.

---

## 10. Other entity-side AI hooks (`js/entities.js`)

| line | behavior | AI vs human |
|---|---|---|
| 360 | `orderUnits`: `embolden` pawns (ignore fear) | human only |
| 485–486 | `hurt`: `tf.threat = src; tf.threatT = G.time` | all (read by AI garrison, 543) |
| 489, 510 | alarm / "sotto attacco" alert events | human only |
| 541–544 | idle AI guard targets `f.threat` within 950 px of castle | AI only |
| 612 | `alarmFor`: idle soldiers answer alarm | human only |
| 629–645 | `raidOn`: auto-continue assault on nearest enemy within 900 px | human only (AI → `idle`) |
| 766 | `findNode` wide search | human only |
| 817–821 | pawn with no node: human retries, AI goes idle | split |
| 801, 1134, 1192, 1284, 209–210, 729 | floats, sounds, toasts, announces | human only **[presentation]** |
| factions.js 107, 122, 419, 911 | sounds / announces on age, research, trade, win | human only **[presentation]** |
| factions.js 293 | `explored()` always true for AI (no fog) | AI |
| factions.js 894 | `explore(f)` every 0.25 s updates `f.seen` | human only |
| factions.js 898 | `estimateRates` every 1 s | human only |

The AI never sets building rally points; units come out at the default rally `{x: b.x, y: b.y1 + 72}`
(entities.js:1121) and walk there (`deploy`, entities.js:1162–1171).

---

## 11. Full AI tick — pseudo-code

```
// once per sim step, for every faction, BEFORE units/buildings update (main.js:294)
fn update_faction(f, dt):
    if !f.alive: return
    if !f.human: for k in RES: f.res[k] += f.ai.level.trickle * dt
    else: f.seeT -= dt; if f.seeT <= 0: f.seeT = 0.25; explore(f)
    f.tickT -= dt
    if f.tickT <= 0:
        f.tickT = 1; census(f)
        if f.human: f.rate = estimate_rates(f)
        for k in f.trades: f.trades[k] = max(0, f.trades[k] - 0.04)
    if !f.human:
        f.ai.thinkT -= dt
        if f.ai.thinkT <= 0:
            f.ai.thinkT = L.think            // reset, not accumulate
            ai_think(f, L.think)             // dt passed = level period, not elapsed time

fn ai_think(f, dt):
    ai = f.ai; L = ai.level; c = f.castle
    ai_assign_pawns(f, dt)                                          // §6.1
    open = sum(f.sites.values())
    apply_skip_rule()                                               // §4 (max one skip)
    step, stepCost = current plan step and its cost (or None)
    comingPawns, comingTroops = queued counts
    house / camp / (open<2: lost rebuild | extra farm | plan build)  // §6.2 else-chain
    if step is tech: tech handling                                  // §6.2
    intel = ai_intel(f)                                             // §7
    ai.mix = ai_want(f, intel)                                      // §8
    ai_defend(f, intel, stepCost)                                   // §9.5
    extra factories                                                 // §6.3
    training (pawn | soldier)                                       // §6.4
    market                                                          // §6.6
    ai.waveT -= dt
    if ai.wave: ai_march(f, intel)                                  // §9.4
    elif ai.waveT <= 0 and !intel.danger:
        if ai.dueT is None: ai.dueT = G.time
        ai_launch(f, intel)                                         // §9.3
    ai_capture(f, intel)                                            // §9.6 (no-op if a wave exists)
    send strays home                                                // §9.7
```
Important ordering subtleties to keep:
- `intel` is computed **once** per think, after the economy part and before defense; it is not refreshed after
  `aiDefend` / `aiLaunch` (so `aiCapture` after a successful `aiLaunch` sees `ai.wave` set and returns).
- `open` is computed before any building is placed in this think and is not refreshed.
- `aiDefend` may null the wave before `aiMarch` would run; then `aiLaunch` runs only if `waveT <= 0` (it was just
  raised to ≥ 30) — in practice no relaunch the same think.
- `aiJobMix` uses the previous `ai.mix`.

Determinism (Rust): all of this is pure and can run identically on server and in the browser (solo play) provided
(1) the RNG is the sim's seeded RNG, (2) iteration orders of `G.factions`, `G.units`, `G.buildings`, `G.points`,
node arrays and `RES_KINDS` are preserved, (3) stable sorts are used where JS used `Array.prototype.sort`
(stable in ES2019+), (4) `Map`/`Set` iterate in insertion order (use `IndexMap`/`IndexSet` or Vec).
Performance hint: `aiIntel` and `aiNextUnit` scan all units/buildings per AI per think; with 5 factions at 1 s that
is negligible, but a spatial index can be reused if available.

---

## 12. Human ↔ AI control handover (takeover and **reclaim**)

### 12.1 Current JS behavior (takeover only)
- Match start (main.js:79–87): factions get random base order; a faction is `human` if a lobby player picked its
  colour; `f.conn` = network peer id (0 for local/AI). The menu demo battle has no humans: all 5 kingdoms AI at
  `'normal'`.
- Player leaves (`Net.playerLeft`, net.js:137–148), on the host only, when the relay reports the peer gone:
  ```
  remove from lobby
  f = faction with f.conn == id; if !f or !f.alive: return
  f.human = false; f.conn = 0
  for u in f.soldiers: idle(u)            // soldiers stop and guard where they stand
  [presentation] G.announce(`${f.name}: il giocatore è uscito, ora lo guida l'IA`, f, 4)
  ```
  The `ai` object is the one created at match start and **never touched while human** (thinkT, waveT, step… all
  frozen at their initial values). So after takeover: first think within `rand(1,3)` s; `ai.step = 0`;
  first wave after the original `rand(first)` seconds counted from the takeover; `captureT = 60` is absolute so
  capture squads may start immediately (normal/hard); pawns keep their current gather orders.
- **Reconnection is impossible** in JS: `join` during a match is refused with `'Partita già in corso.'`
  (net.js:121), and the peer id changes on reconnect. If the **host** leaves, the relay closes the match for
  everyone (`hostleft`, server.py:102–115) — irrelevant for the Rust server-authoritative design.
- Other AIs treat human and AI kingdoms identically for targeting (the only difference: the `' ti attacca!'`
  announcement and horn for human targets).

### 12.2 Complete list of state that depends on `f.human`
1. `f.human` flag itself (gates everything in §10 and `update_faction`).
2. `f.conn` (peer → faction mapping for incoming commands, net.js:129).
3. `f.ai.*` (only mutated while AI-controlled): `step, thinkT, waveT, waveNeed, target, shuffleT, mix, wave, dueT,
   attackedT, lackT, captureT`.
4. Unit orders the AI issued: wave units (`amove`/`move` orders + membership in `ai.wave.ids`), capture squads,
   strays, pawn gather orders, builders on AI-placed sites.
5. `f.seen` / `f.seeT` (fog memory; frozen while AI-controlled because `explore` only runs for humans).
6. `f.alarm` (human only; stays stale while AI), `f.rate` (stale while AI).
7. Pawn `brave` / `calmT` (only set via human orders; harmless).
8. Cross-faction: `o.ai.target == f`, `f.raidedT` — independent of control, no cleanup needed.
9. Resources received from `trickle` while AI (hard) — keep them (no rollback).

### 12.3 Required Rust behavior — reclaim after reconnect (new feature, requested by the user)
Identify players by a persistent session token (not by connection id). Suggested procedure:

**On disconnect (takeover)** — keep the JS semantics:
```
f.human = false; f.conn = None (keep f.owner_token = token)
for u in f.soldiers: idle(u)
ai_reset_for_takeover(f)        // see open question Q1
emit Announce(`${f.name}: il giocatore è uscito, ora lo guida l'IA`, f, 4)
```

**On reconnect with the same token while `f.alive`** (reclaim):
```
f.human = true; f.conn = new connection
// detach AI cleanly:
f.ai.wave = None                 // forget the wave; units keep their current orders (human may re-command)
f.ai.dueT = None
f.ai.mix stays (harmless); f.ai.step, waveNeed etc. are kept for a possible later takeover
f.alarm = None
f.seeT = 0                       // explore() runs on the next step
f.tickT = 0                      // census + estimateRates on the next step (economy screen correct immediately)
send full state snapshot (+ f.seen) to the client, then resume normal command processing
emit Announce(<new string, see Q3>, f, 4)
```
Do **not** idle the units on reclaim (orders in progress are valid for a human too; the human auto-assault
chain in `raidOn` will take over naturally when the current `amove` ends). Pawns keep their jobs; the Lavoratori
screen counts them by `jobOf`.

Fog continuity: since `explore` does not run while AI-controlled, `f.seen` misses what the AI's units saw. Because
`explore` caches per-entity `seenKey`, on reclaim only *current* positions get marked. Recommended: in Rust, keep
calling `explore(f)` for any faction that has an `owner_token` (human-owned, even while AI-controlled), so the
returning player gets an up-to-date explored map. This does not change AI decisions (AI ignores fog).

If the faction was eliminated meanwhile, the reconnecting player joins as spectator (`G.spectate` equivalent;
UI string `'Spettatore · frecce o minimappa per guardare'`, ui.js:489).

### 12.4 Takeover quirk: duplicated buildings
Because `ai.step` is 0 at the first takeover and build steps don't check ownership, an AI that takes over a
developed kingdom re-runs the plan from the start and places **additional** barracks/depot/archery/smith/stable/
market/monastery/siege (farms and towers are capped by the skip rule; techs are skipped since `f.tech` is set).

---

## 13. Open questions / ambiguities

- **Q1 – AI state on takeover.** Faithful port = leave `ai` untouched (frozen initial values; on a second takeover,
  values left from the previous AI period). Recommended for Rust (deviation, needs approval): on every takeover
  `ai.wave = None; ai.dueT = None; ai.thinkT = rng(1,3); ai.waveT = max(ai.waveT, rng(L.gap))`, and fast-forward
  `ai.step` past build steps whose kind is already owned in sufficient number (count occurrences of that kind in
  `AI_PLAN[0..=i]` vs `has+sites`) and tech steps already researched — this removes the duplicate-building quirk (§12.4).
- **Q2 – Grace period.** JS hands over to the AI instantly. A page reload takes a few seconds; consider a short
  grace period (e.g. 10–20 s) before AI takeover, or keep instant takeover (faithful) and rely on reclaim. Product
  decision.
- **Q3 – New UI string for reclaim.** No Italian string exists; proposal: `` `${f.name}: il giocatore è tornato` ``
  (must be confirmed; all other strings are verbatim from JS).
- **Q4 – Host leaving.** In JS the whole match ends; with a dedicated Rust server this disappears. If the *last*
  human leaves, should the server keep simulating (all-AI) so they can come back? Recommend: pause or keep running
  for a timeout; product decision.
- **Q5 – `findSpot` shuffle / `chooseTarget` randomness**: exact JS permutation is not reproducible nor needed;
  only distribution matters. Server and solo client must use the same seeded RNG for determinism within Rust.
- **Q6 – Fractional resources**: trickle and point income keep `res` as floats; affordability compares floats.
  Rust should use `f32`/`f64` (or fixed-point ×1000) consistently on server and client.
- Faithful-but-odd behaviors to keep unless told otherwise: farms counted as drop sites for depot placement (§5);
  rams count as wave "fighters" but not in `total`; catapults count as fighters; `waveT` decremented by `L.think`
  (not real dt); one-unit-per-think training; pawn training blocks soldier training.

---

## Addenda (critic)

**A1. Final rule for AI state on takeover (resolves Q1; deviation approved by the user's reconnect requirement).**
Verified: build steps never check ownership (only farm/tower via the level cap, factions.js:818-819; plan step advances
only when `aiBuild` succeeds, factions.js:832), tech steps self-skip when done/busy (factions.js:839). A human kingdom
also carries an `ai` object built at match start with the match difficulty (`makeFaction(c, base, human, setup.level)`,
main.js:82; factions.js:59-60), so the takeover AI plays at the match level. Rust procedure, run on **every** takeover:
```
ai.wave = None; ai.dueT = None; ai.target = None
ai.thinkT = rng.range(1, 3)
ai.waveT  = max(ai.waveT, rng.range(L.gap))   // countdown (factions.js:880); no instant attack after a reload
ai.captureT = max(ai.captureT, G.time + 60)  // absolute time (factions.js:801-802); JS would allow captures at once
// fast-forward the plan past what the kingdom already has
step = 0
while step < AI_PLAN.len():
    (k, id) = AI_PLAN[step]
    if k == 'build':
        need = count of i in 0..=step with AI_PLAN[i] == ('build', id)
        if id in {farm: L.farms, tower: L.towers} and has[id]+sites[id] >= cap: step += 1; continue   // same skip rule
        if has[id] + sites[id] >= need: step += 1; continue
        break
    else:   // 'tech'
        if tech[id] or busy[id]: step += 1; continue
        break
ai.step = step
```
`census(f)` must run right before (has/sites fresh). Everything else in `ai` stays. Soldiers are idled as in JS
(net.js:145). On reclaim, `ai` is left as is (next takeover runs this procedure again).

**A2. Q2/Q4 (grace period, everyone leaves)** — final policy in 08-net addenda N1.

**A3. Q3 new string** — adopt `` `${f.name}: il giocatore è tornato` `` (banner, same call shape as the takeover
banner: `G.announce(text, f, 4)`). Listed with all new strings in 08-net addenda N3.

**A4. Q5 randomness.** `findSpot` (factions.js:192) and `main.js:80` use `sort(() => Math.random() - 0.5)`; the AI
target pick in factions.js:666 is a weighted draw `r = rng()*total`. Rust: Fisher–Yates / weighted draw with the sim RNG,
never a random comparator (Rust `sort_by` may panic on inconsistent comparators since 1.81).

**A5. Q6 fractional resources** — `f64` in the sim (see 04a addenda E1); AI affordability compares the same `f64`.

**A6. Fog for returning players — adopted.** Keep running `explore(f)` every 0.25 s for any faction that has an owner
token (human-owned) even while AI-controlled; AI decisions ignore it (`explored()` returns true when `!f.human`,
factions.js:292-295, so the AI still sees everything). Implementation note: `explore` must use the faction's
owner flag, not `human`, for the "should I record exploration" test, while `explored()` keeps testing `human`
(AI-controlled = no fog restrictions for its pawns).
