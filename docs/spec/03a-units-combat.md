# 03a — Units, Orders, Movement, Formations, Combat (porting spec)

Source of truth: `js/entities.js` (unit sections), plus the data tables it depends on in `js/assets.js`,
`js/factions.js`, `js/world.js`, `js/control.js`, `js/main.js`, `js/net.js`. Every rule below cites `file:line`.
Gathering, construction, buildings' queues/rally deployment internals, resources, capture points and the
pawn shelter logic in depth are covered by the sibling spec (gathering/buildings); they are mentioned here only where
they touch unit orders or combat.

Conventions:
- World units are **pixels**; `TILE = 64` px (`assets.js:4`). Map is `World.W=288 × World.H=192` cells (`world.js:24`).
- `x, y` of a unit = its **feet** (ground contact point). Draw order key `sy = y`.
- Angles/dirs: screen space, +y = down (south).
- `rand(a,b) = a + random()*(b-a)`, `clamp(v,a,b)`, `pick(arr)` (`assets.js:600-603`). All JS uses `Math.random()`
  (non-deterministic). **Rust sim must use one seeded sim RNG** for every call marked `[RNG]` below, and a
  separate presentation RNG for purely cosmetic ones marked `[RNG-fx]`.
- `dt` in JS is **variable**, `min(0.05, frameDelta)` (`main.js:463`); the network host also steps from a 50 ms worker
  (`net.js:155`). All timers below are in seconds and must simply be decremented by the fixed tick `dt` in Rust.
- "[SIM]" = must be in the pure deterministic crate. "[PRES]" = presentation only (client/renderer). "[EVT]" = sim
  emits a presentation event (`emit`, `entities.js:58-61`) that the renderer turns into FX/sound/UI.

---

## 1. Constants (entities.js:4-35, assets.js, factions.js, world.js)

| Name | Value | Meaning | Ref |
|---|---|---|---|
| `BUILDING_ARMOR` | 0.4 | buildings take only 40% of incoming damage | entities.js:16 |
| `ALERT_EVERY` | 12 s | min gap between two "under attack" alerts to the same human kingdom | :18 |
| `ALARM_TIME` | 10 s | after a hit at home, idle troops of a human kingdom run to the alarm for this long | :19 |
| `HOME_RADIUS` | 1500 px | hit on a soldier within this distance of own castle counts as an attack on the kingdom | :20 |
| `RAID_RADIUS` | 900 px | a human assault keeps going by itself onto the next enemy within this radius | :21 |
| `SHELTER_CAP` | 5 | pawns a house can hide (castle unlimited) | :22 |
| `CALM_TIME` | 4 s | seconds without enemy troops in sight before pawns go back to work | :23 |
| `HEALER_FOCUS` | 0.6 | an enemy Monk in aggro range counts as if at 0.6 × its distance | :24 |
| `KITE_RANGE` | 110 px | ranged unit with an enemy *melee* closer than this backs off while reloading | :25 |
| `MONK_SPACE` | 150 px | Monk with an enemy soldier closer than this steps away | :26 |
| `ESCORT_GAP` | 200 px | distance a following Monk keeps from the soldier it escorts | :27 |
| `LINE_GAP` | 44 px | spacing between units in formation (lateral and rows) | :28 |
| `SEP_CELL` | 64 px | bucket size of separation grid | :1033 |
| separation radius `R` | 32 px | units closer than this push apart | :1035 |
| separation strength | 0.12 | `push = (R - d) * 0.12` | :1060 |
| busy separation weight | 0.2 | pushed amount multiplier for units in state `work` or `attack` | :1061 |
| leash (guard) | 430 px | idle (guarding) units only pick targets within 430 of their post | :530 |
| AI threat chase radius | 950 px | see §8.2 | :544 |
| AI threat memory | 5 s | see §8.2 | :543 |
| building auto-target radius | `max(260, unitRange)`; rams 520 | :546 |
| breach search radius | 900 px, top 4 candidates | :560-563 |
| breach lock time | 8 s (`breachT = time + 8`) | :665, :674 |
| arrow speed | 640 px/s | :442 |
| arrow overshoot | life = `(d + 70)/640` | :445 |
| arrow hit box vs unit | `|u.x - a.x| < 18 && |(u.y - 28) - a.y| < 26` | :1250 |
| arrow hit box vs building | `x0 < a.x < x1 && y0 - 70 < a.y < y1` | :1260 |
| arrow damage vs Ram | ×0.2 | :495 |
| block damage multiplier | ×0.35 | :500 |
| block duration | 0.5 s | :420 |
| hurt flash | `hurtT = 0.12` s | :504 |
| rock flight time | `0.5 + d/520` s | :453 |
| rock arc height | 150 px (visual) | :1241 |
| rock building splash | `rectDist < splash*0.5` | :1229 |
| melee hit-confirm reach | `unitRange*1.5 (+44 vs buildings)` | :475 |
| pawn target weight | ×2.2 distance | :537 |
| `WORK_RADIUS` | 2600 px | factions.js:6 |
| `FIELD_NEW` | 2 | units per tick allowed to switch from shared group path to own slot path | world.js:17 |
| `FIELD_REFRESH` | 2 | stale distance fields recomputed per tick | world.js:16 |
| clearLine shortcut distance | < 280 px | entities.js:322 |
| travel stall timeout | 1.2 s if `d<120`, else 4 s; "progress" = path length decreasing by >3 px | :341-342 |
| tower range / castle range | 420 / 380 px (+ `mods.range`) | :1209 |
| tower / castle arrow dmg | 15 / 14 (+ `mods.atk.archer`) | :1217 |
| tower / castle reload | 1.1 / 1.2 s; idle rescan 0.3 s | :1216-1218 |

`ACT_LABEL` (index = `u.act`, shown in UI, keep verbatim) — entities.js:34-35:
```
0 'Inattivo', 1 'In marcia', 2 'In combattimento', 3 'Taglia legna', 4 'Estrae oro', 5 'Macella pecore',
6 'Costruisce', 7 'Ripara', 8 'Porta il carico', 9 'Cura i feriti', 10 'Di guardia', 11 'Estrae pietra',
12 'Al riparo', 13 'In fuga', 14 'In esplorazione'
```

---

## 2. Unit definitions (assets.js:115-169) [SIM data]

Fields: `hp`, `speed` (px/s), `dmg`, `range` (px), `cd` (reload s), `hitFrame` (frame index of the attack anim at which
the hit lands), `block` (chance), `aggro` (auto-target radius px), `sight` (cells), `cost`, `time` (train s), `from`
(building), `age`, `bonus` {targetType: mult}, `vsBuilding` (mult), flags `foot`, `siege`, `onlyBuildings`,
`splash` (px), `scout`, `heal` (hp per cast), `faith` (pool), `faithRegen` (/s). `barY` = HP bar height above feet [PRES].

| type | label (IT) | hp | speed | dmg | range | cd | hitFrame | block | aggro | sight | bonus | vsBuilding | flags |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| pawn | Pedone | 50 | 92 | – | – | – | – | – | – | 5 | – | – | – |
| scout | Esploratore | 60 | 165 | – | – | – | – | – | – | 10 | – | – | scout |
| warrior | Guerriero | 130 | 98 | 13 | 52 | 0.95 | 2 | 0.25 | 300 | 6 | lancer 1.5 | 1.6 | foot |
| lancer | Lanciere | 110 | 118 | 20 | 88 | 1.25 | 1 | 0.2 | 320 | 6 | knight 1.5, archer 1.1, monk 1.5 | – | foot |
| archer | Arciere | 70 | 102 | 12 | 330 | 1.5 | 5 | – | 390 | 7 | warrior 1.7 | 0.5 | foot |
| knight | Cavaliere | 190 | 150 | 18 | 58 | 1.1 | 3 | – | 340 | 7 | archer 1.6, catapult 2, pawn 1.3 | – | – |
| monk | Monaco | 65 | 94 | – (heal 40) | 170 | 1.8 | 6 | – | – | 6 | – | – | heal, faith 100, faithRegen 0.4 |
| ram | Ariete | 340 | 62 | 55 | 60 | 2.2 | 2 | – | 330 | 5 | – | 4 | siege, onlyBuildings |
| catapult | Catapulta | 110 | 58 | 42 | 430 | 4.2 | 2 | – | 450 | 8 | – | 2.2 | siege, splash 72 |

Costs / train time / building / age (needed by other specs, copied for completeness):
pawn {meat 50} 13 s castle age1; scout {meat 40} 12 s castle age1; warrior {meat 58, gold 40} 15 s barracks age1;
lancer {meat 60, gold 40} 17 s barracks age2; archer {wood 38, gold 32} 17 s archery age2;
knight {meat 80, gold 60} 20 s stable age2; monk {gold 80} 22 s monastery age3; ram {wood 160, gold 60} 26 s siege age3;
catapult {wood 140, gold 120} 28 s siege age3.

Italian descriptions (verbatim, UI):
- pawn: `Raccoglie risorse, costruisce e ripara.`
- scout: `Veloce e con la vista lunga: esplora la mappa, anche da solo (tasto E), e occupa i punti liberi. Non combatte.`
- warrior: `Fante con scudo, disponibile da subito. Forte contro Lancieri ed edifici, teme gli Arcieri.`
- lancer: `Colpisce da lontano con la lancia. Forte contro i Cavalieri, teme i Guerrieri.`
- archer: `Tira da lontano. Forte contro i Guerrieri, fragile in mischia: teme i Cavalieri.`
- knight: `Cavalleria veloce e resistente. Travolge Arcieri e Catapulte, teme i Lancieri.`
- monk: `Cura le unità alleate ferite. Ha una riserva di 100 punti di cura che si ricarica piano. I nemici vicini lo preferiscono ad altri bersagli.`
- ram: `Sfonda edifici e mura. Non può colpire le unità; le frecce gli fanno poco.`
- catapult: `Lancia massi a grande distanza: colpiscono tutti i nemici in una zona.`

Derived classification predicates used everywhere (keep exact):
- **combatant / "has dmg"**: `def.dmg` truthy → warrior, lancer, archer, knight, ram, catapult.
- **soldier** (threat to units): `def.dmg && !def.onlyBuildings` → all combatants except ram (entities.js:693, 898).
- **ranged**: `def.range > 200` → archer, catapult (and as a *target* test: `t.def.range > 200`; monk range 170 is NOT ranged).
- **armored** (`unitArmor`): `def.dmg && !def.siege` → warrior, lancer, archer, knight (:243).
- **blocker**: `def.block` → warrior (0.25), lancer (0.2).
- Monk detection is `def.heal`; scout `def.scout`.

### 2.1 Attack animation timing (sim-relevant!) — assets.js:177-200, frame counts from PNG widths
`tickAction` (entities.js:426-438) lands the hit when `floor(animT*fps) >= hitFrame` and ends the action when
`floor(animT*fps) >= n` (n = frames in sheet). Because the JS reads `n` from image width, the Rust sim must
hard-code this table:

| type | anim (sheet) | frames n | fps | action length n/fps | hitFrame | hit at hitFrame/fps |
|---|---|---|---|---|---|---|
| warrior | `attack1`/`attack2` (Warrior_Attack1/2, 768/192) | 4 | 12 | 0.333 s | 2 | 0.1667 s |
| lancer | `attack{Dir}` (Lancer_{Dir}_Attack, 960/320) | 3 | 9 | 0.333 s | 1 | 0.111 s |
| archer | `attack` (Archer_Shoot, 1536/192) | 8 | 14 | 0.571 s | 5 | 0.357 s |
| knight | `attack` (Extra Knight_Attack) | 6 | 12 | 0.5 s | 3 | 0.25 s |
| monk | `attack` (Heal, 2112/192) | 11 | 12 | 0.917 s | 6 | 0.5 s |
| ram | `attack` (Extra Ram_Attack) | 4 | 7 | 0.571 s | 2 | 0.2857 s |
| catapult | `attack` (Extra Catapult_Attack) | 4 | 7 | 0.571 s | 2 | 0.2857 s |

Other anims (presentation, fps): warrior idle 8 / run 10 / block(Guard) 10; lancer idle 10 / run 10 / block{Dir} 10;
archer idle 8 / run 10; monk idle 8 / run 10; pawn idle 8 / run 10 / work{Tool} 10; scout idle 8 / run 10;
knight idle 8 / run 12; ram & catapult idle = first frame of Run (fps 0), run 8.
Pawn anim names: `idle|run` + (carry load `Wood|Gold|Meat|Stone`, else tool `Axe|Pickaxe|Knife|Hammer`, else ''),
`work{Tool}`.

Note `cd` keeps counting down during the attack animation (entities.js:1001), and every cd (≥0.95 s) is longer than
its attack anim, so the real attack period is `cd` (quantised to ticks).

---

## 3. Tech modifiers that touch units (factions.js:46-50, 81-99, 101-117)

`f.mods` default: `atk {warrior 0, lancer 0, archer 0, knight 0}, range 0, armor 0, arrowRes 1, siege 1,
gather{…}, carry 0, pawnSpeed 1, foot 1, horse 1, heal 1, healRange 0, monkHp 1, breed 1, buildHp 1, sight 0`.

| tech id | effect |
|---|---|
| carts | `carry = 5; pawnSpeed = 1.15` |
| boots | `foot = 1.12` |
| horseshoes | `horse = 1.15` |
| blades | `atk.warrior += 4; atk.lancer += 4; atk.knight += 4` |
| shields | `arrowRes = 0.5` |
| fletching | `atk.archer += 3; range = 45` |
| armor | `armor = 3` |
| maps | `sight = 2` |
| herbs | `heal = 1.5` |
| scriptures | `healRange = 70; monkHp = 2` |
| engineering | `siege = 1.25` |
| masonry | `buildHp = 1.3` |

Stat functions (entities.js:236-248):
```
unitMaxHp(f,type) = round(UNIT_DEF[type].hp * (type=='monk' ? f.mods.monkHp : 1))
unitSpeed(f,type) = def.speed * (type=='pawn' ? mods.pawnSpeed : def.foot ? mods.foot : type=='knight' ? mods.horse : 1)
unitDmg(u)       = ((def.dmg or 0) + (mods.atk[type] or 0)) * (def.siege ? mods.siege : 1)
unitRange(u)     = def.range + (type=='archer' ? mods.range : type=='monk' ? mods.healRange : 0)
unitArmor(u)     = def.dmg && !def.siege ? mods.armor : 0
damageTo(src, base, t) = base * (t.isBuilding ? (src.def.vsBuilding || 1) : (src.def.bonus?.[t.type] || 1))
```
Note `mods.range` (fletching +45) also extends tower/castle range (entities.js:1209) and `mods.atk.archer` adds to
tower/castle arrows (:1217). `damageTo` with a building as `src` uses BUILD_DEF which has no bonus/vsBuilding → ×1.

When a tech completes, every existing unit of the faction gets `speed = unitSpeed(...)`, and if max HP changed:
`hp *= newMax/oldMax; maxHp = newMax` (factions.js:112-117). `u.speed` is otherwise fixed at spawn.

---

## 4. Unit object (entities.js:250-268)

`spawnUnit(f, type, x, y)` creates and pushes into `G.units`, registers in `G.index` by id (`track`, :54).

| field | init | meaning |
|---|---|---|
| `id` | `NEXT_ID++` | global unique id (shared counter with all entities) |
| `isUnit` | true | type tag |
| `faction` | f | owner |
| `type`, `def` | | UNIT_DEF entry |
| `x, y, sy` | spawn | feet position; `sy` draw key [PRES] |
| `hp, maxHp` | `unitMaxHp` | |
| `speed` | `unitSpeed` | current px/s (temporarily overridden in `march`) |
| `state` | 'idle' | action state: `idle` / `move` / `attack` / `block` / `work` |
| `anim` | 'idle' | animation name; `animT` = time in anim, init `random()` [RNG-fx] |
| `flip` | false | facing left |
| `act` | 0 | ACT_LABEL index, recomputed every tick (§13) |
| `job` | -1 | pawn job resource index (RES_KINDS = wood,gold,meat,stone), -1 none |
| `cd` | 0 | attack/heal cooldown remaining |
| `thinkT` | `random()*0.4` [RNG] | countdown to next target re-evaluation |
| `target` | null | current combat/heal target (unit or building) |
| `order` | `{k:'idle', x, y}` | current order object (§5) |
| `faith` | `def.faith||0` | monk heal pool |
| `foe`, `lead` | null | monk: nearest enemy soldier; nearest own soldier (escort) |
| `carry, tool, workT, pauseT, node, skip` | | pawn gathering (other spec) |
| `hurtT` | 0 | >0 → hurt flash [PRES but set in sim] |
| `lastHurt` | -99 | game time of last damage |
| `moving` | false | moved this tick |
| `dustT` | 0 | dust fx timer [PRES] |
| `combo` | 0 | warrior attack anim alternation (0/1) |
| `hitDone` | false | hit of current attack already applied |
| `aimX, aimY` | x,y | point aimed at by current attack (fallback aim for projectiles) |
| `blockT` | 0 | block action remaining |
| `bestD` | ∞ | best remaining path length seen (stall detection) |
| `stallT` | 0 | time without progress |
| `breachT` | -99 | until this game time the unit is locked on a breach target |
| `dead` | false | |
| `cross` | false | path being followed crosses own buildings (set by field queries) |
| `inside` | null | building the pawn sheltered in (unit invisible/untargetable) |
| `brave` | false | pawn: has a direct player order, won't flee until calm |
| `fearT` | `random()*0.4` [RNG] | pawn threat-scan timer |
| `lookT`, `calmT` | 0 | pawn timers |
| `pathLeft` | (set by approach) | remaining path length px |

---

## 5. Orders (entities.js:279-288)

Order object kinds (`u.order.k`) and fields:

| k | fields | used by |
|---|---|---|
| `idle` | `x, y` = guard post; `escort` (monk follows soldiers) | all ("hold" = idle) |
| `move` | `x, y` = own slot; `ax, ay` = group common goal (only if group n>1); `pace` (0 = none); `escort`; runtime `own` (switched to own slot path) | all |
| `amove` | same as move + `auto` (auto-raid step) ; runtime `fought`, `own` | soldiers only |
| `attack` | `t` = target unit/building | soldiers |
| `heal` | `t` = own unit | monks |
| `explore` | `goal` {x,y} or null; runtime `bad` Set<cell> | scouts |
| `gather` | `res, first, px, py` | pawns (other spec) |
| `build` / `repair` | `b`, `resume` | pawns (other spec) |
| `drop` | `b` | pawns |
| `shelter` | `b, back` (order to resume), `calm` | pawns (§11) |

There is **no** separate patrol, hold-position, or follow order: "hold" = `idle` with a guard post; "follow" =
monk `escort`; "patrol/explore" = scout `explore`; "rally" = building rally point used on unit deploy (§12).

### 5.1 setOrder / idle (entities.js:282-288)
```
setOrder(u, o):
  if u.inside: leaveShelter(u)
  claim(u, null)                         // release gathering node
  u.order = o; u.target = null; u.skip = null; u.workT = 0
  u.bestD = ∞; u.stallT = 0; u.breachT = -99
idle(u) = setOrder(u, {k:'idle', x:u.x, y:u.y})   // post = current position
```

### 5.2 orderUnits(f, ids, kind, x, y, targetId) — entities.js:353-406
Called by `execCommand('order', ids, kind, x, y, targetId)` (factions.js:339-341; ids array ≤ 200 else ignored),
by `recall` (factions.js:368-373), by AI and by `place` (build).
```
units = [G.index[id] for id in ids if isUnit && !dead && faction==f]
if none: return
if f.human: for u in units: embolden(u)            // pawn courage, §11
t = targetId ? G.index[targetId] : null
'stop'    → idle(u) for all; return
'explore' → for u with def.scout: setOrder(u, {k:'explore', goal:null}); return   (others untouched)
'move' | 'amove' → formation (§6); return
if !t or t.dead: return
for u in units:
  'attack' and (t.isUnit or t.isBuilding) and t.faction != f:
       if u.def.dmg and (t.isBuilding or !u.def.onlyBuildings): setOrder(u, {k:'attack', t})
       elif u.def.heal: setOrder(u, {k:'idle', x:u.x, y:u.y, escort:true})    // monks follow attackers
       (pawns, scouts, rams-vs-unit: order unchanged)
  'gather' (pawn, node valid) → gather order          (other spec)
  'build' (pawn, own building) → build or repair (+resume)  (other spec)
  'drop' (pawn, own finished building) → drop
  'heal' and u.type=='monk' and t.isUnit and t.faction==f and t!=u → setOrder(u, {k:'heal', t})
```

### 5.3 Player input → commands (control.js:178-204, 281-354) [client side, keep behavior]
Right-click with own units selected (`own`), `t` = entity under cursor:
- `t` enemy unit/building: `band = army(def.dmg) + monks` get `'attack' t`; everyone else `'move'` to click.
  Marker 'attack' if any army else 'move'.
- Attack-move mode (key X, `Attacco in marcia`) then left/right click: all selected get `'amove'`; marker 'attack'.
- `t` resource node: pawns `'gather'`, others `'move'`.
- `t` own building: pawns `'build'` if `!t.done || t.hp < t.maxHp`, else `'drop'` if `t.def.drop`, else `'move'`;
  others `'move'`.
- `t` own damaged unit and monks selected: monks `'heal'`, others `'move'`.
- else everyone `'move'`. Coordinates are rounded to ints.
Action buttons (keep strings verbatim):
- `X` — title `Attacco in marcia`, desc `Poi clic sul terreno: le unità vanno lì in formazione, al passo del più lento, attaccando chi incontrano; poi proseguono da sole sui nemici vicini.` (only if some selected unit has dmg)
- `E` — `Esplora`, desc `L'Esploratore va da solo, una dopo l'altra, nelle zone mai viste più vicine, finché ce ne sono.` (only scouts receive it)
- `F` — `Ferma`, desc `Annulla l'ordine in corso.` → `'stop'`
- `G` (building) — `Punto di raccolta`, desc `Poi clic sul terreno o su una risorsa (anche col tasto destro, direttamente).`
- Recall (control.js:348-354): command `recall` → all given non-pawn units `'move'` to `(castle.x, castle.y1 + 130)`
  (so they do **not** stop to fight). Toast if none: `Non hai truppe`.
Cursor (control.js:449-466): `attack` over enemies when any selected has dmg; tool cursors for pawns.

---

## 6. Formation / group move (entities.js:346-386)

```
lineOf(u): d = u.def
  !d.dmg → 3   (monks, scouts, pawns: rear)
  d.splash → 2 (catapult)
  d.range > 200 → 1 (archer)
  else → 0     (warrior, lancer, knight, ram: front)
```
Algorithm for kind ∈ {move, amove}:
```
if !isFinite(x) or !isFinite(y): return
p = World.nearestPassable(x, y)
n = len(units); amove = kind=='amove'; army = any(u.def.dmg)
mx,my = centroid of units
pace = min(u.speed for u with def.dmg)   (∞ if none)
far = |p - (mx,my)|
(dx,dy) = far > 1 ? (p - m)/far : (0, 1)          // facing direction
per = max(3, ceil(sqrt(n) * 1.5))                   // units per row
lines = 4 buckets by lineOf
back = 0
for line in lines[0..3] (skip empty):
   sort line by (u.x * -dy + u.y * dx) ascending    // lateral coordinate: who is left stays left
   for i, u in line:
      row   = floor(i / per)
      inRow = min(per, len(line) - row*per)
      side  = (i % per - (inRow - 1)/2) * LINE_GAP
      deep  = back + row * LINE_GAP
      q = nearestPassable(p.x - dy*side - dx*deep, p.y + dx*side - dy*deep)
      setOrder(u, { k: (amove && u.def.dmg) ? 'amove' : 'move',
                    x: q.x, y: q.y,
                    ax: n > 1 ? p.x : undefined, ay: p.y,
                    pace: (amove && army) ? pace : 0,
                    escort: amove && army && !!u.def.heal })
   back += ceil(len(line)/per) * LINE_GAP + 36
```
So line 0 stands on the click point, each further line is behind (opposite to travel direction), rows inside a line
are 44 px apart, consecutive lines separated by an extra 36 px. Non-combatants in an amove get `k:'move'` (they never
auto-fight). Monks in an amove-with-army get `escort:true` → after arrival they become escorting idle monks (§9).
`pace` applies to every unit in an amove group that contains a combatant (the slowest *combatant* speed, siege
included); in plain `move` there is no pace (everyone at own speed).

### 6.1 march(u, o, dt, near) — entities.js:588-600 (soldiers & monks only)
```
own = u.speed
if o.pace: u.speed = min(own, o.pace)
if o.ax !== undefined and !o.own:
    // walk toward the shared group goal until allowed to compute own slot path
    if (World.fresh-- > 0) or travel(u, o.ax, o.ay, dt): o.own = true; u.bestD = ∞; u.stallT = 0
    done = false
else:
    done = travel(u, o.x, o.y, dt, near)
u.speed = own
return done
```
`World.fresh` is reset to `FIELD_NEW = 2` at the start of every sim tick (main.js:293); it's a global budget:
at most 2 units per tick switch to their own slot path (to spread distance-field computations); others keep walking
to the common goal (one shared path). Post-decrement: the counter goes negative and stays "exhausted".
**Pawns and scouts do NOT use march**: they call `travel(u, o.x, o.y, dt)` directly and ignore `ax/ay/pace`
(entities.js:722, :977).

---

## 7. Movement & navigation [SIM]

### 7.1 moveUnit(u, tx, ty, dt) — entities.js:290-297
```
d = |t - u|; if d < 0.5: return
step = min(d, u.speed*dt)
World.tryMove(u, dx/d*step, dy/d*step)
if |dx| > 4: u.flip = dx < 0
u.moving = true      // even if tryMove was blocked
```
### 7.2 World.tryMove(e, dx, dy) — world.js:68-72 (axis-separated collision)
```
r = 9; side = e.faction ? e.faction.idx+1 : 0; cross = e.cross === true
if dx and stepOK(e.x, e.y, e.x + dx + sign(dx)*r, e.y, side, cross): e.x += dx
if dy and stepOK(e.x, e.y, e.x, e.y + dy + sign(dy)*r, side, cross): e.y += dy
```
`stepOK(x0,y0,x1,y1,side,cross)` (world.js:61-67): same cell → ok; target cell must be `crossable` if
`cross || !passable(from)` else `passable`; if the from-cell isn't crossable (stuck in blocked cell) → ok (can always
leave); else `linkOK(a,b)` (same height level, or both are stair cells vertically adjacent).
`passable(c, side)`: land, not cliff, and (no building, or own palisade). `crossable(c, side)`: land, not cliff, and
(no building, or any own building). (world.js:46-51)

### 7.3 Pathing helpers
- `sideOf(u) = u.faction.idx + 1` (:299).
- `fieldOf(u, e)` = `World.fieldFor(e, side, cellAt(u))` — distance field (tenths of a cell; 10 straight, 14 diagonal,
  +30 `CROSS_COST` per own-building cell when crossing) to the ring of passable cells around entity `e`'s cells;
  sets `u.cross = World.crossed` (true when the normal field can't reach from the unit's cell, so the cross field
  through own buildings is used). (entities.js:302-306, world.js:217-258)
- `navTo(u, field, dt)` (:308-315): `field[cell]==0` → `'arrived'`; `n = stepFrom(field, cell, side, u.cross)`;
  `n<0` → `'blocked'`; else moveUnit toward center of cell n → `'moving'`.
- `approach(u, tx, ty, dt)` (:318-333) — returns "can still make progress":
```
d = |t - u|; u.pathLeft = d; u.cross = false
if d < 280 and World.clearLine(u, t, side): moveUnit(u, t); return true        // straight line
b = cellAt(t); if b < 0: return false
a = cellAt(u); f = fieldToCell(b, side, a); u.cross = World.crossed
if f[a] == 0: return false
n = stepFrom(f, a, side, u.cross); if n < 0: return false
u.pathLeft = max(d, f[a] * TILE / 10)
moveUnit(u, cx(n), cy(n)); return true
```
- `clearLine` (world.js:74-83): samples every ≤24 px, each sample checked with two axis `stepOK(..., cross=true)`.
- `travel(u, x, y, dt, near=10)` (:337-344) — returns true when arrived OR can't get closer:
```
d = |(x,y) - u|; if d <= near: return true
moving = approach(u, x, y, dt)
if u.pathLeft < u.bestD - 3: u.bestD = u.pathLeft; u.stallT = 0
elif (u.stallT += dt) > (d < 120 ? 1.2 : 4): return true
return !moving
```
(Callers must reset `bestD=∞, stallT=0` when changing goal; setOrder does.)
- `World.nearestPassable(px,py)` (world.js:104-121): point itself if its cell is passable (side 0); else center of the
  nearest passable cell searching square rings r=1..10; else the point.
- `holdPost(u, dt)` (:603-606): `if dist(post) > 48 and travel(u, post, dt, 24): post = (u.x, u.y)` — after a chase the
  unit walks back; if it can't arrive, its post becomes where it stands.

### 7.4 Eject from building cells (entities.js:1005-1009)
Before any behavior, if the unit's cell is not `passable(side)` and (not `crossable(side)` OR (order is idle and no
target)) → `moveUnit(u, nearestPassable(u.x,u.y))` and skip behavior this tick. (Units may walk *through* own
buildings while following a cross path, but never stand in them; units under a freshly placed building leave.)

### 7.5 Separation (entities.js:1029-1066) — runs once per tick after all unit updates (main.js:296)
```
grid: key(x,y) = floor(x/64)*4096 + floor(y/64); insert index i of every unit with !inside (in G.units order)
for i in 0..n: a = units[i]; skip if a.inside
   near = all j > i from the 9 buckets around a (sorted ascending)
   for j in near: b = units[j]
      dx = b.x-a.x; dy = b.y-a.y; skip if |dx|>32 or |dy|>32; d = hypot; skip if d >= 32
      push = (32 - d) * 0.12
      (ux,uy) = d > 0.01 ? (dx,dy)/d : (cos(i), sin(i))
      wa = a.state in {work, attack} ? 0.2 : 1; wb likewise
      tryMove(a, -ux*push*wa, -uy*push*wa)
      tryMove(b,  ux*push*wb,  uy*push*wb)
```
All factions push each other. Note positions move during the loop but the grid is built once (fine as long as
nobody moves >32 px per tick). Quirk: units killed this tick are still in `G.units` during separation (removed right
after, main.js:297); Rust may skip dead units — negligible difference.

---

## 8. Soldier AI (warrior, lancer, archer, knight, ram, catapult) — updateSoldier, entities.js:647-679

### 8.1 State machine
```
o = u.order
breaching = time < u.breachT and u.target and !u.target.dead
alarm = alarmFor(u)                       // §10
if o.k == 'attack':
    if o.t.dead: raidOn(u); return        // target gone → continue raid (human) / idle (AI)
    if !breaching: u.target = o.t
elif o.k == 'move':
    u.target = null                       // plain move ignores all enemies
elif !breaching and (u.thinkT -= dt) <= 0:   // idle or amove
    u.thinkT = rand(0.3, 0.6)             [RNG]
    t = acquireTarget(u, roam = alarm != null)
    if t != u.target: u.target = t; u.bestD = ∞; u.stallT = 0
t = u.target
if t and !t.dead:
    if fight(u, t, dt): o.fought = true; return
    // target unreachable
    gate = (o.k == 'idle') ? null : breach(u, t.x, t.y)
    u.target = gate
    if gate: u.breachT = time + 8
    elif o.k == 'attack': idle(u)
    return
u.target = null
if alarm: answerAlarm(u, alarm, dt)
elif o.k == 'idle': holdPost(u, dt)
elif march(u, o, dt):                     // move / amove reached destination (or stalled)
    gate = (o.k == 'amove' and |o - u| > 200) ? breach(u, o.x, o.y) : null
    if gate: u.target = gate; u.breachT = time + 8; u.bestD = ∞; u.stallT = 0
    elif o.k == 'amove' and (o.fought or !o.auto): raidOn(u)
    else: idle(u)
```
Additionally, `hurt()` sets `t.target = attacker` for an untargeted soldier in `idle`/`amove` (§10.2) — instant
retaliation independent of thinkT.

### 8.2 acquireTarget(u, roam=false) — entities.js:529-553
```
f = u.faction; o = u.order; guard = (o.k=='idle' and !roam); leash = 430
best = null; bd = u.def.aggro
if !u.def.onlyBuildings:
   for e in G.units:
      skip if e.dead or e.inside or e.faction == f
      far = |e - u|; skip if far >= aggro
      d = far * (e.type=='pawn' ? 2.2 : e.def.heal ? 0.6 : 1)
      if d < bd and (!guard or |e - post| < 430): bd = d; best = e
   if best: return best
   // AI only: garrison goes after whoever hits the kingdom from out of range
   th = f.threat; c = f.castle
   if !f.human and guard and th and !th.dead and time - f.threatT < 5
      and |th - c| < 950 and |u - c| < 950: return th
bd = u.def.onlyBuildings ? 520 : max(260, unitRange(u))
for b in G.buildings:
   skip if b.dead or b.faction == f or b.kind == 'wall'
   d = rectDist(u, b)
   if d < bd and (!guard or rectDist(post, b) < 430): bd = d; best = b
return best
```
Key subtleties:
- Weighted distance is compared against `aggro` itself → enemy pawns are only auto-targeted within `aggro/2.2`
  (e.g. warrior 136 px), monks are "pulled in" (preferred over a closer non-monk unless that one is < 0.6× as far).
- Units always beat buildings. Walls are never auto-targeted (only via breach).
- Rams (onlyBuildings) skip units entirely and search buildings within 520.
- Every other faction is an enemy (no alliances).
- `rectDist(px,py,b) = hypot(max(b.x0-px, 0, px-b.x1), max(b.y0-py, 0, py-b.y1))` (:39-41);
  `distTo(u,t)` = rectDist for buildings else Euclidean (:42).

### 8.3 fight(u, t, dt) — entities.js:568-585 (returns false if unreachable)
```
d = distTo(u, t)
inRange = d <= unitRange(u) + (t.isBuilding ? 0 : 6)
if !inRange and t.isBuilding and u.def.range < 200:
    inRange = fieldOf(u, t)[cellAt(u)] == 0           // melee standing in the ring around the building
if inRange:
    // archer kiting
    if t.isUnit and d < 110 and u.cd > 0.3 and u.def.range > 200 and !u.def.siege and !(t.def.range > 200):
        moveUnit(u, 2u - t)                            // step directly away
        return true
    tx = t.isBuilding ? clamp(u.x, t.x0, t.x1) : t.x
    ty = t.isBuilding ? (t.y0 + t.y1)/2 : t.y
    if |tx - u.x| > 4: u.flip = tx < u.x
    if u.cd <= 0: startAttack(u, tx, ty)
    return true
if t.isBuilding: return navTo(u, fieldOf(u, t), dt) != 'blocked'
return approach(u, t.x, t.y, dt)
```
Kiting applies only to archers (only non-siege unit with range>200), against any unit whose range is not >200
(warriors, lancers, knights, rams, monks, pawns, scouts — `undefined > 200` is false). It happens only while
reloading with more than 0.3 s left, so the archer still shoots on schedule.

### 8.4 startAttack / startBlock / tickAction (entities.js:410-438)
```
startAttack(u, tx, ty):
   u.state = 'attack'; u.hitDone = false; u.aimX = tx; u.aimY = ty; u.cd = u.def.cd
   if |tx - u.x| > 2: u.flip = tx < u.x
   anim = 'attack'
   warrior: u.combo ^= 1; anim = 'attack' + (combo+1)        // alternates attack2, attack1, ...
   lancer:  anim = 'attack' + dir5(tx-u.x, ty-u.y)
   u.anim = anim; u.animT = 0                                 // always restarts
dir5(dx,dy): a = atan2(-dy, |dx|); a>1.18 'Up' | a>0.39 'UpRight' | a>-0.39 'Right' | a>-1.18 'DownRight' | 'Down'
startBlock(u, from):
   u.state = 'block'; u.blockT = 0.5
   if from: u.flip = from.x < u.x
   setAnim(u, lancer ? 'block' + (from ? dir5(from - u) : 'Right') : 'block')
tickAction(u, dt) -> busy:
   state 'attack': fr = floor(u.animT * fps(anim))           // table §2.1
       if !hitDone and fr >= hitFrame: hitDone = true; doHit(u)
       if fr >= n: state = 'idle'; return false
       return true
   state 'block': if (blockT -= dt) <= 0: state='idle'; return false; else return true
   else false
```
While `busy`, the unit does nothing else (no movement, no retargeting). **Blocking freezes the unit for 0.5 s.**
When the action ends the same tick continues into the behavior update.
`setAnim(u,a)` changes anim and resets animT only if different (:52).

### 8.5 doHit(u) — entities.js:459-479
```
t = u.target; alive = t and !t.dead
archer:
   (ax,ay) = alive ? (t.isBuilding ? ((x0+x1)/2, (y0+y1)/2 - 20) : (t.x, t.y - 28)) : (u.aimX, u.aimY)
   fireArrow(u, u.x + (flip ? -16 : 16), u.y - 34, ax, ay, unitDmg(u))
catapult:
   fireRock(u, alive && t.isUnit ? t.x : u.aimX, alive && t.isUnit ? t.y : u.aimY, unitDmg(u))
monk: heal (§9.2)
other (melee incl. ram):
   if alive and distTo(u,t) <= unitRange(u)*1.5 + (t.isBuilding ? 44 : 0):
       hurt(t, damageTo(u, unitDmg(u), t), u)
       sound 'boom' (ram) or 'hit'               [EVT]
   (else the swing whiffs: target moved away)
```
Projectile damage stores the *base* `unitDmg`; bonus multipliers are applied at impact against whoever is hit.

### 8.6 breach(u, gx, gy) — entities.js:557-565
"The road to the goal is blocked: choose the enemy building (usually a palisade) to knock down."
```
here = cellAt(u)
cands = buildings where !dead and faction != u.faction and rectDist(u, b) < 900
sort by rectDist(u,b) + rectDist((gx,gy), b) ascending
for b in first 4: if fieldOf(u, b)[here] < ∞: return b
return null
```
Walls ARE eligible here. While `time < breachT` and the gate is alive, an `attack`/`idle`/`amove` soldier keeps the
gate as target (no retargeting).

### 8.7 raidOn(u) — entities.js:626-645 (auto-continuing assault)
```
best = null; bd = 900
if u.faction.human:
   if !u.def.onlyBuildings: nearest enemy unit (not dead, not inside) with dist < bd
   nearest enemy non-wall building by rectDist < bd (competes with the unit by the same bd)
if best: setOrder(u, {k:'amove', x: best.x, y: best.isBuilding ? best.y1 + 24 : best.y, auto:true})
else: idle(u)
```
AI factions always go idle (their AI re-orders them). Chain logic (§8.1): an `amove` given by a player (`auto`
false) always raids on arrival; an auto step raids again only if it actually fought (`o.fought`), otherwise idles
— prevents endless wandering.

---

## 9. Monks — updateMonk, entities.js:684-715

### 9.1 Behavior
```
o = u.order; f = u.faction
if o.k == 'move':
    if march(u, o, dt): setOrder(u, {k:'idle', x:u.x, y:u.y, escort:o.escort})
    return
if o.k == 'heal' and (o.t.dead or o.t.hp >= o.t.maxHp): idle(u); return
if (u.thinkT -= dt) <= 0:
    u.thinkT = rand(0.4, 0.7)            [RNG]
    heal=null; hr=0.9; foe=null; fd=150*1.6 (=240); lead=null; ld=1500
    for e in G.units (not dead, not inside, e != u):
        d = |e - u|; soldier = e.def.dmg and !e.def.onlyBuildings
        if e.faction != f: if soldier and d < fd: fd=d; foe=e
                           continue
        if !e.def.siege and d < 430 and e.hp/e.maxHp < hr: hr = ratio; heal = e     // lowest HP ratio < 90%
        if soldier and d < ld: ld = d; lead = e
    u.foe = foe; u.lead = lead; u.target = heal
if o.k == 'heal': u.target = o.t                       // explicit heal order overrides (siege allowed here)
t   = u.target if (alive and hp < maxHp) else null
foe = u.foe if alive else null
reach = t and distTo(u, t) <= unitRange(u)             // 170 (+70 scriptures)
if reach and u.cd <= 0 and u.faith >= 1: startAttack(u, t.x, t.y); return     // cast heal
if foe and |foe - u| < 150:                            // step away; guard post moves with the monk
    moveUnit(u, 2u - foe); if o.k == 'idle': o.x,o.y = u.x,u.y
    return
if t: if !reach: approach(u, t.x, t.y, dt)
      return                                            // with empty faith it waits in place
u.target = null
if o.k != 'idle': idle(u); return
lead = (o.escort and u.lead alive) ? u.lead : null
if !lead: holdPost(u, dt)
elif |lead - u| > 200: approach(u, lead.x, lead.y, dt); o.x,o.y = u.x,u.y
```
Monks never fight, never respond to alarms, aren't affected by `raidOn`. Auto-heal ignores siege units; an explicit
`heal` order works on any own unit except itself. A monk in an `attack` command becomes an escorting idle monk.

### 9.2 Heal cast (doHit for monk, entities.js:467-474)
```
if target alive:
   amount = min(def.heal * mods.heal, t.maxHp - t.hp, u.faith)     // 40 (×1.5 herbs)
   t.hp += amount; u.faith -= amount
   fx 'heal' on target (follow target, dy 4, color = monk faction color)   [EVT]
   sound 'heal'                                                     [EVT]
```
No range check at landing (only at cast start). Pool regen every tick (entities.js:1002):
`faith = min(def.faith (100, not modified), faith + 0.4 * mods.heal * dt)`. Cast requires `faith >= 1`.
Cooldown 1.8 s, action 0.917 s, heal lands at 0.5 s.

---

## 10. Damage, alarms, death

### 10.1 hurt(t, amount, by) — entities.js:482-512
`by` = attacking unit, building, or projectile (projectiles carry `owner` and `faction`).
```
if t.dead: return
src = by?.owner ? by.owner : by;  tf = t.faction
if src and src.isUnit and !src.dead:
    tf.threat = src; tf.threatT = time
    c = tf.castle
    if tf.human and (t.isBuilding or t.type=='pawn' or (c and |t - c| < 1500)):
        tf.alarm = {x: src.x, y: src.y, t: time}
if t.isBuilding:
    amount *= 0.4
else:
    if by and by.vx !== undefined:             // ARROW (unit or tower/castle arrow); rocks have no vx
        if t.type == 'ram': amount *= 0.2
        elif t.def.block or t.type == 'knight': amount *= tf.mods.arrowRes   // shields tech
    amount = max(1, amount - unitArmor(t))
    if t.def.block and t.state != 'attack' and random() < t.def.block:     [RNG]
        amount *= 0.35; startBlock(t, src); sound 'block'
    t.hurtT = 0.12; t.lastHurt = time
    k = t.order.k
    if !t.target and t.def.dmg and !t.def.onlyBuildings and k in {idle, amove} and src?.isUnit and !src.dead:
        t.target = src                          // instant retaliation
t.hp -= amount
if tf.human and time - tf.alertT > 12: tf.alertT = time; emit ['alert', tf.idx, round(t.x), round(t.y)]   [EVT]
if t.hp <= 0: t.isUnit ? killUnit(t, by) : destroyBuilding(t, by)
```
Full damage formula for a unit target:
```
raw   = unitDmg(attacker) * (bonus[target.type] || 1)                 // projectiles: computed at impact
raw  *= (arrow? (ram 0.2 | blocker/knight arrowRes | 1) : 1)
dmg   = max(1, raw - armor(target))                                   // armor only on warrior/lancer/archer/knight
dmg  *= (block proc ? 0.35 : 1)                                       // after the min-1 clamp
```
For a building target: `unitDmg * (vsBuilding || 1) * 0.4` (no armor, no min). Damage values are floats; HP is float
(snapshot rounds with ceil).
Tower/castle arrows: the source is a building → no threat/alarm bookkeeping, no retaliation target set.

### 10.2 Alarm response (human kingdoms) — entities.js:610-622
```
alarmFor(u): a = f.alarm
   return (f.human and a and time - a.t < 10 and u.order.k == 'idle' and u.def.dmg and !u.def.onlyBuildings) ? a : null
answerAlarm(u, a, dt):
   o = u.order; c = castle
   if c and |post - c| > 1500:            // was idling far from home → after the alarm it guards the castle
       p = nearestPassable(c.x + rand(-140,140), c.y1 + rand(70,150))     [RNG]
       post = p
   if |a - u| > 120: approach(u, a.x, a.y, dt)
```
While the alarm is active, `acquireTarget(u, roam=true)` drops the 430 px leash. Archers and catapults respond too;
rams, monks, scouts, pawns do not. After 10 s the unit goes back to its (possibly relocated) post via `holdPost`.
Note: the post relocation re-rolls random each tick while far (only first call matters since the post then is
near the castle).

### 10.3 Guarding / AI-side defense (for completeness; AI spec owns details)
- AI garrison chase: §8.2 threat clause (idle AI soldiers within 950 of own castle chase an attacker within 950
  of the castle that hit the kingdom in the last 5 s).
- `aiDefend` (factions.js:686-706): with level.rally, idle home fighters with no target are sent `amove` to the
  danger point; with level.recall the wave is recalled when danger > 0.8 × homeValue + 250.
- AI stray recall (factions.js:886-888): idle soldiers > 900 px from castle (not in wave, not near an un-owned
  point within 300) get `amove` to `(c.x, c.y1 + 130)`.
- When a network player leaves, the faction becomes AI and all its non-pawn units `idle()` (net.js:144-147),
  announce `` `${f.name}: il giocatore è uscito, ora lo guida l'IA` ``. (The rewrite must additionally allow the
  player to reconnect and take the faction back — user requirement.)

### 10.4 killUnit(u, by) — entities.js:514-523
```
if u.inside: leaveShelter(u)
u.dead = true; claim(u, null); G.index.delete(u.id)
fx 'boom1' at (u.x, u.y - 22), dy 60       [EVT]   (Explosion_01, 192 px frames, 14 fps)
sound 'death'                               [EVT]
bf = by?.faction; if bf and bf != u.faction: bf.kills++
```
**No corpse entity**: units vanish in an explosion puff. Dead units are removed from `G.units` at the end of the
units phase (main.js:297). References held by others (targets, orders) must be checked with `dead` (Rust: generational
ids / existence check). `eliminateFaction` kills all units with `by = null` (no kill credit) (factions.js:907).

---

## 11. Pawn fear / shelter (summary; full detail in gathering spec) — entities.js:893-978

- `threatNear(f, x, y, r)`: any enemy unit (not dead, not inside) with `def.dmg && !onlyBuildings` within r.
- `sightPx(u) = (def.sight + mods.sight) * TILE`.
- Every 0.4 s (`fearT`): `seen = threatNear(f, inside ? b.x,b.y : u.x,u.y, sightPx + (inside || brave ? 2*TILE : 0))`.
  - in `shelter`: `o.calm = seen ? 0 : o.calm + 0.4`; when `calm >= 4` → resumeWork.
  - brave: `calmT = seen ? 0 : calmT + 0.4`; `calmT >= 4` → `brave = false`.
  - else if seen: `to = shelterFor(u)` (nearest own finished house with < 5 inside, else castle); `setOrder(shelter{b, back: old order, calm: 0})`.
- `shelterStep`: if target building invalid/full → re-pick; navTo; arrived → `enterShelter` (u placed at building
  center, invisible, untargetable, excluded from separation and arrows); blocked → `brave = true`, resumeWork.
- `embolden(u)` (on direct player orders): pawn only, if a threat is within `sightPx + 2*TILE` → `brave = true; calmT = 0`.
- `leaveShelter`: exits at `nearestPassable(b.x + rand(-0.3,0.3)*w*TILE, b.y1 + 24)` [RNG].
- Pawns are never auto-targeted beyond `aggro/2.2` and never fight back.

---

## 12. Scouts (explore) and deploy/rally

### 12.1 updateScout — entities.js:719-730
```
if o.k == 'idle': holdPost(u, dt); return
if o.k != 'explore': if travel(u, o.x, o.y, dt): idle(u)
                     return
g = o.goal
next = !g or explored(f, g.x, g.y)
if !next and travel(u, g.x, g.y, dt, 40): next = true; (o.bad ||= Set()).add(cellAt(g))   // reached or unreachable
if !next: return
o.goal = frontier(u, o.bad); u.bestD = ∞; u.stallT = 0
if !o.goal: idle(u); if f.human: emit toast 'Esplorazione finita: a piedi non resta nulla da scoprire'
```
`frontier(u, bad)` (:733-744): needs `f.seen` (only human factions have fog; AI → null → immediately idle, no toast).
`field = fieldToCell(here, side, here)` (walking distance from the scout). Scan cells with `y = 2, 5, 8, … < H-2` and
`x = 2, 5, 8, … < W-2` (`c` stepping by 3 from `y*W+2`); pick unseen, not in `bad`, minimal finite field value.
Return the cell center or null. `explored(f,x,y)` = true for AI, else `f.seen[cell] == 1` (factions.js:292-296).
Scouts never fight or respond to alarms; they help capture points (they're not pawns). They ignore attack commands.

### 12.2 Deploy at rally (entities.js:1162-1174)
New unit spawns at `nearestPassable(b.x + rand(-0.3,0.3)*w*TILE, b.y1 + 24)` [RNG]; fx 'dust2' at (x, y+2).
Pawn + rally target is a valid resource node → gather; pawn + rally target is an own unfinished building → build;
otherwise `{k:'move', x,y}` to `nearestPassable(rally.x + rand(-34,34), rally.y + rand(-22,22))` [RNG] (single-unit
move: no ax, no pace). Default rally `= (b.x, b.y1 + 72)`. Rally command (factions.js:394-397):
`rally = {x, y, t: target if alive and (resource node or own building) else null}`.

---

## 13. Per-tick unit update — updateUnit, entities.js:999-1027

```
if u.inside: updatePawn(u, dt); u.act = unitAct(u); u.job = jobOf(u); return
u.cd -= dt; u.hurtT -= dt; u.animT += dt; u.moving = false; u.cross = false
if def.heal: faith regen (§9.2)
if u.state == 'work': u.state = 'idle'
if !tickAction(u, dt):
    eject-from-building rule (§7.4) else dispatch:
    pawn → updatePawn; def.heal → updateMonk; def.scout → updateScout; else updateSoldier
if state in {idle, move}:
    state = moving ? 'move' : 'idle'
    a = moving ? 'run' : 'idle'; if pawn: a += carry ? LOAD_NAME[carry.kind] : (tool || '')
    setAnim(u, a)
u.act = unitAct(u); u.job = pawn ? jobOf(u) : -1
if moving and (dustT -= dt) <= 0: dustT = rand(0.7, 1.5) [RNG-fx]; fx 'dust1' at (x, y+4) dy -6 [EVT, only if onScreen]
```
`unitAct(u)` (:980-991): `state=='attack' || target` → monk 9 else 2; gather → wood 3 / gold 4 / stone 11 / meat 5;
build 6; repair 7; drop 8; shelter → inside 12 else 13; explore 14; idle → pawn 0 else 10; else (move/amove/attack
order without target) 1.

Global sim order per tick (main.js:290-305): `time += dt; World.refresh = 2; World.fresh = 2;` factions (AI) →
all live units `updateUnit` (in `G.units` array order) → `separateUnits` → drop dead units → buildings (queues,
farm breeding, tower/castle shooting) → drop dead buildings → resources → capture points → projectiles → pickups →
fx timers. **Iteration order matters** (shared budgets `World.fresh`, first-come targets): keep units in a stable
spawn-ordered list.

---

## 14. Projectiles — entities.js:440-457, 1223-1264

### 14.1 Arrow (fireArrow(owner, x, y, tx, ty, dmg))
```
d = |t - (x,y)| || 1; sp = 640
arrow = {id, x, y, vx: (tx-x)/d*sp, vy: (ty-y)/d*sp, ang: atan2(ty-y, tx-x), faction: owner.faction, dmg, owner,
         life: (d + 70)/sp}
sound 'shoot' at (x,y)
```
Update per tick:
```
x += vx*dt; y += vy*dt
if (life -= dt) <= 0: dead; continue
for u in G.units (not dead, not inside, faction != arrow.faction):       // first in list order
    if |u.x - x| < 18 and |(u.y - 28) - y| < 26:
        hurt(u, damageTo(owner, dmg, u), arrow); sound 'hit'; dead; break
if dead: continue
for b in G.buildings (not dead, faction != arrow.faction):
    if x0 < x < x1 and y0 - 70 < y < y1: hurt(b, damageTo(owner, dmg, b), arrow); dead; break
```
Arrows are straight, non-homing, and **hit the first enemy they pass through** (not necessarily the intended
target); they fly 70 px past the aim point, then expire. Units are checked before buildings. Arrows from dead owners
still deal damage (owner.def is still readable).

### 14.2 Rock (fireRock(owner, tx, ty, dmg))
```
sx = owner.x + (flip ? -20 : 20); sy = owner.y - 50; d = |(tx,ty) - (sx,sy)|
rock = {id, rock:true, x:sx, y:sy, sx, sy, tx, ty, t:0, dur: 0.5 + d/520, ang:0, faction, dmg, owner}
sound 'shoot'
update: k = min(1, (t += dt)/dur); x = lerp(sx,tx,k); y = lerp(sy,ty,k) - sin(k*π)*150 [visual]; ang += 9*dt
        if k >= 1: dead; rockImpact
rockImpact: R = owner.def.splash (72)
   for u in units (not dead, not inside, enemy) with |u - (tx,ty)| < R: hurt(u, damageTo(owner, dmg, u), rock)
   for b in buildings (not dead, enemy) with rectDist((tx,ty), b) < R*0.5: hurt(b, damageTo(owner, dmg, b), rock)
   fx 'boom1' (tx, ty-10, dy 30); fx 'dust2' (tx, ty+6, scale 1.6); sound 'boom'
```
Rocks land on a fixed ground point (target position at release) → moving units can dodge. No friendly fire.
No arrow reductions (rocks have no `vx`). Catapult vs building: `42 * siegeMod * 2.2 * 0.4`.
Projectile position/angle are networked for rendering; in HD-2D the arc height is a pure visual.

### 14.3 Tower & castle shooting (entities.js:1207-1219)
Only finished towers/castles. `cd` initial `rand(0,1)` [RNG]. When `cd <= 0`: range = (tower 420 | castle 380) +
`mods.range`; target = nearest enemy unit (not inside) by distance from `(b.x, b.y)` under range; none → `cd = 0.3`.
Else `fireArrow(b, b.x, b.y - def.ay*0.62, t.x, t.y - 28, (tower 15 | castle 14) + mods.atk.archer)`; `cd` = 1.1 / 1.2.
(`def.ay` is the sprite anchor height — origin height of the arrow; the sim needs it as data.)

---

## 15. Effects, events and rendering notes [PRES]

Event tuples (entities.js:58-89), hosted games queue them for clients:
- `['fx', name, x, y, scale, dy, followId, color]` — FX sheet at (x,y), draw order `sy = y + dy`, optional entity to
  follow. Removed when `t*fps >= n`. Names used here: `boom1` (death, rock impact), `boom2` (building destroyed),
  `dust1` (walking, shelter in/out), `dust2` (spawn, rock impact, building), `heal` (Heal_Effect of monk color,
  192 px, 12 fps).
- `['snd', name, x, y]` positional sound: `shoot`, `hit`, `boom`, `heal`, `block`, `death`, `chop`.
- `['alert', factionIdx, x, y]` → minimap/UI "under attack" ping for that player.
- `['toast', idx, text]`, `['float', idx, x, y, str, kind]`, `['say', …]`, `['play', idx, sound]`.
`onScreen` checks only gate cosmetic fx in local play (always true when hosting) — they must not affect the sim.

Unit rendering (entities.js:1304-1317, 1448-1453): selection ellipse (rx 24/ry 10, or 38/13 for siege & knight;
white own / `#ff5a4a` others, line 2.5); while `hurtT > 0` draw at alpha 0.5; attack anim frame clamped to last;
HP bar (48 px wide, at `y - def.barY`) when damaged or selected, in faction color. Arrow sprite `Units/{Color} Units/Archer/Arrow.png`
rotated by `ang`; rock drawn as `Stone_Resource` ×1.3 spinning. Arrows hidden in fog.
In HD-2D: units are billboard sprites standing on voxel terrain; sim stays 2D (x,y) — height is visual only.

Network snapshot per unit (net.js:174-177), useful for the client: `[id, factionIdx, typeIdx, x, y, ceil(hp), maxHp,
anim, animT, flags(flip|hurt<<1|inside<<2|(job+1)<<3), act, floor(faith)]`; arrows `[id, x, y, ang, factionIdx, rock]`.

---

## 16. Open questions / ambiguities for the Rust port

1. **Variable vs fixed dt**: JS uses `dt ≤ 0.05` variable; hit frames, stall timers and the `World.fresh` budget are
   per-tick. Recommend fixed 20 Hz (dt = 0.05, matching the hidden-tab host) or 30 Hz; outcomes will differ slightly
   from JS but rules are identical.
2. **Animation-driven timing**: action lengths come from sprite frame counts (§2.1). If HD-2D sprites change frame
   counts, keep the *sim* table as above (timing is gameplay), decouple from art.
3. `hurt()` reaction and acquisition consider any non-own faction an enemy (FFA). No teams exist.
4. Archers' arrows may hit an unintended unit in the path — this is intended behavior (keep).
5. `separateUnits` includes units killed in the same tick (quirk) — safe to skip dead.
6. `answerAlarm` relocates the post with fresh randomness each tick while the old post is far — harmless; replicate
   or compute once.
7. Monk faith cap does not scale with techs; `mods.heal` scales both heal amount and regen.
8. Explicit heal orders can target siege units though auto-heal skips them (keep).
9. Pawns/scouts ignore group `ax/pace` — group moves with pawns are not synchronized (keep).

---

## Addenda (critic)

Completeness check: every function of `entities.js` is referenced by name in 03a/03b/07 (script-checked). The
quirks listed in §16 were re-read in the code; decisions:

**U1. Alarm guard-post re-roll — happens at most once per alarm, keep as is.** `answerAlarm` (entities.js:615-620)
re-rolls `o.x,o.y = nearestPassable(c.x + rand(-140,140), c.y1 + rand(70,150))` only while the current post is
farther than `HOME_RADIUS = 1500` px from the castle (entities.js:20). The new post is ≤ ~210 px from the castle,
so from the next tick on the condition is false. Port faithfully (one RNG draw pair per responding unit).

**U2. Units killed this tick still take part in separation — keep.** `separateUnits` (entities.js:1034-1066) skips
only `inside`, not `dead`; dead units are removed from `G.units` later in `updateWorld`. Effect: one extra push from
a just-killed unit. Port as is (harmless; keeps tie behavior). Pair order must stay `i` ascending then `j` ascending
(`near.sort`) because `tryMove` mutates positions in place.

**U3. Explicit heal order on siege/pawns — keep.** `orderUnits` 'heal' accepts any own unit `t !== u` (entities.js:402-403);
the auto-heal scan skips `def.siege` (entities.js:696). So the player can heal rams/catapults manually; the AI never
does. Keep (it is a usable feature, not a crash).

**U4. Pawns and scouts ignore group pace — keep.** `pace` is only set for `amove && army` orders
(entities.js:382); pawns get `move` orders and scouts are not `army`. Keep.

**U5. Camera-dependent code inside the sim — must be removed from the sim.** `onScreen()` (entities.js:44-47)
returns `true` on a network host and tests the *local camera* otherwise. Call sites inside simulation code:
entities.js:162 (tree regrow dust), 188 (wild sheep respawn dust), 792 (`chop` sound), 917 (enter shelter dust),
929 (leave shelter dust), 1025 (running dust; also consumes `rand(0.7,1.5)` regardless), 1165 (deploy dust).
Rust sim: always emit these as presentation events (they are cheap) and let each client cull by its own camera;
never consult a camera in the sim. Because the host already always emits them in network games, this matches the
current multiplayer behavior exactly. (Client-only uses in control.js:119 and :168 are covered in 05 addenda.)

**U6. RNG draws that are presentation-only.** `animT: Math.random()` (entities.js:255), `phase` of trees
(entities.js:109), `shine` / `twinkle` of gold (entities.js:117,156), `fireSeed` of buildings (entities.js:1089),
`dustT` (entities.js:1024) are visual. In Rust put them on the client (derive from entity id with a hash, e.g.
`hash(id) → [0,1)`) so they do not consume the sim RNG. Sim RNG draws that remain: `thinkT` (255-256),
`fearT` (264), alarm post (618), soldier/monk think (655, 689), shelter exit spot (927), building `cd`
(1117), loot scatter (1146 boom fx is visual; 1155 loot x jitter is sim), deploy spot and rally jitter (1163, 1169),
`dropPickup` angle/radius (97), sheep spawn/wander (130-131, 172-178, 185), block roll (499).
