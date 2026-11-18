# 01 — Definitions, constants & assets (porting spec)

Source files: `js/assets.js` (611 lines, read completely), `js/extra.js` (493 lines, read completely),
`js/manifest.js` (592 lines, structure). Where a definition is only meaningful together with the code that consumes
it, the consuming formula is quoted with its `file:line` (mainly `js/entities.js`, `js/factions.js`) — those files
are owned by other spec chapters, the formulas here are the ones that read the tables below.

Conventions used in the whole game:

- **World unit = pixel**, `TILE = 64` px per grid cell (`assets.js:4`). Speeds are **px/s**, ranges / aggro / splash in
  **px**, `sight` in **cells (tiles)**, times in **seconds**, `cd` = attack cooldown in seconds.
- An entity's `(x, y)` is its **foot point** (ground contact); sprites are drawn so that pixel `(ax, ay)` of the
  frame lands on `(x, y)`. The HD-2D billboard pivot = `(ax / fw, 1 - ay / fh)` in normalised UV (bottom-left origin).
- Everything in this chapter marked **[SIM]** must live in the pure Rust simulation crate; **[RENDER]** is client-only;
  **[SIM!]** marks a place where the JS simulation depends on sprite-sheet data (frame counts / fps) — the Rust sim
  must carry those numbers as data even without loading images.
- All user-visible strings are Italian and must be kept verbatim (copied below between backticks/quotes).

---

## 1. Global constants & enums (`assets.js:4-9`, `105-108`, `412-420`)

| Name | Value | Notes |
|---|---|---|
| `TILE` | `64` | px per cell [SIM] |
| `COLORS` | `['Blue','Red','Yellow','Purple','Black']` | **Order is load-bearing**: index = `colorIdx` = row in ribbon/sword sheets and avatar block (`assets.js:5`). Faction `colorIdx = COLORS.indexOf(color)` (`factions.js:43`). |
| `COLOR_NAME` | `Blue:'Blu', Red:'Rosso', Yellow:'Giallo', Purple:'Viola', Black:'Nero'` | Kingdom name = `'Regno ' + COLOR_NAME[color]` (e.g. `Regno Blu`) (`factions.js:43`) |
| `COLOR_CSS` | `Blue:'#46a5bb', Red:'#e6645c', Yellow:'#e3c84e', Purple:'#b981c6', Black:'#7887a0'` | UI tint per kingdom; also used to tint the small bar fill (§9.1) |
| `FONT` | `'Fredoka, "Trebuchet MS", Verdana, "DejaVu Sans", sans-serif'` | file `fonts/Fredoka.ttf`, variable weight 300–700 |
| `fontOf(size)` | `` `600 ${Math.round(size)}px ${FONT}` `` | every label is weight **600** |
| `AGE_NAME` | `['', 'Età del Villaggio', 'Età Feudale', 'Età dei Castelli']` | index = age 1..3 |
| `AGE_SHORT` | `['', 'I', 'II', 'III']` | roman numeral drawn on the crown icon |
| `RES_KINDS` | `['wood','gold','meat','stone']` | **order is load-bearing** (job index `RES_KINDS.indexOf(res)` `entities.js:996`, UI rows) |
| `RES_LABEL` | `wood:'Legna', gold:'Oro', meat:'Cibo', stone:'Pietra'` | |
| `RES_ICON` | wood `UI Elements/UI Elements/Icons/Icon_02.png`, gold `…/Icon_03.png`, meat `…/Icon_04.png`, stone `Extra/Icons/stone.png` | |
| `LANCE_DIRS` | `['Up','UpRight','Right','DownRight','Down']` | lancer 5-direction attack/defence sheets |
| `PAWN_TOOLS` | `['Axe','Pickaxe','Knife','Hammer']` | |
| `PAWN_LOADS` | `['Wood','Gold','Meat','Stone']` | carried-resource sprites |

Path prefixes (`assets.js:46-48`, `412-420`): `UIP = 'UI Elements/UI Elements/'`, `STOREP = 'UI Elements/UI Banners from the store page/'`,
`TERR = 'Terrain/'`, `RES = 'Terrain/Resources/'`, `XICON(name) = 'Extra/Icons/'+name+'.png'`,
`PROP(name) = 'Extra/Props/'+name+'.png'`, `ICON(n) = UIP+'Icons/Icon_'+pad2(n)+'.png'`,
`TOOL(n) = RES+'Tools/Tool_0'+n+'.png'` (1 = hammer, 2 = axe, 3 = knife, 4 = pickaxe).

**Index orders that the network protocol depends on** (`net.js:171,175,191-192,244,251,292,296`): the key
order of `UNIT_DEF` (`UNIT_TYPES`), `BUILD_DEF` (`BUILD_KINDS`) and `TECH_DEF` (`TECH_IDS`) is used as numeric ids;
queue entries encode units as `UNIT_TYPES.indexOf(type)` and techs as `100 + TECH_IDS.indexOf(id)`; researched techs
are a bitmask `1 << TECH_IDS.indexOf(id)`. In Rust use `#[repr(u8)]` enums in exactly the orders below.

```
UNIT_TYPES  = pawn, scout, warrior, lancer, archer, knight, monk, ram, catapult            (0..8)
BUILD_KINDS = castle, house, camp, depot, farm, barracks, wall, archery, stable, smith,
              market, tower, monastery, siege                                               (0..13)
TECH_IDS    = age2, age3, carts, masonry, axes, picks, breeding, boots, horseshoes, blades,
              shields, fletching, armor, maps, herbs, scriptures, engineering               (0..16)
BUILD_MENU  = house, camp, depot, farm, barracks, wall, archery, stable, smith, market,
              tower, monastery, siege   (build-panel order; also the whitelist for the 'place' command, factions.js:344 — castle can never be placed)
```

---

## 2. Units — `UNIT_DEF` (`assets.js:112-169`)

Field meanings (`assets.js:112-114`, `150`):
`label` UI name · `hp` max HP · `speed` px/s · `sight` cells · `cost` resources · `time` training seconds ·
`from` building kind that trains it · `age` minimum era · `dmg` damage per hit · `range` px (attack or heal reach) ·
`cd` cooldown s (set at attack start) · `hitFrame` animation frame on which the hit is applied [SIM!] ·
`block` probability of blocking a hit · `aggro` auto-target radius px · `bonus` damage multiplier vs unit type ·
`vsBuilding` damage multiplier vs buildings · `foot` foot soldier (affected by tech `boots`) · `siege` siege engine ·
`onlyBuildings` cannot target units · `splash` area radius px · `scout` explorer flag · `heal` HP per heal ·
`faith` heal reserve · `faithRegen` reserve per second · render-only: `dir` sprite folder, `fw` frame width,
`ax`,`ay` anchor, `avatar` index in the avatar pack, `barY` HP-bar height above foot (px; also used for click hit box in `control.js:131`).

### 2.1 Gameplay numbers

| type | label | hp | speed | sight | cost | time | from | age | dmg | range | cd | hitFrame | block | aggro | bonus | vsBuilding | flags |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| pawn | `Pedone` | 50 | 92 | 5 | meat 50 | 13 | castle | 1 | – | – | – | – | – | – | – | – | – |
| scout | `Esploratore` | 60 | 165 | 10 | meat 40 | 12 | castle | 1 | – | – | – | – | – | – | – | – | `scout` |
| warrior | `Guerriero` | 130 | 98 | 6 | meat 58, gold 40 | 15 | barracks | 1 | 13 | 52 | 0.95 | 2 | 0.25 | 300 | lancer ×1.5 | 1.6 | `foot` |
| lancer | `Lanciere` | 110 | 118 | 6 | meat 60, gold 40 | 17 | barracks | 2 | 20 | 88 | 1.25 | 1 | 0.2 | 320 | knight ×1.5, archer ×1.1, monk ×1.5 | – (1) | `foot` |
| archer | `Arciere` | 70 | 102 | 7 | wood 38, gold 32 | 17 | archery | 2 | 12 | 330 | 1.5 | 5 | – | 390 | warrior ×1.7 | 0.5 | `foot` |
| knight | `Cavaliere` | 190 | 150 | 7 | meat 80, gold 60 | 20 | stable | 2 | 18 | 58 | 1.1 | 3 | – | 340 | archer ×1.6, catapult ×2, pawn ×1.3 | – (1) | – |
| monk | `Monaco` | 65 | 94 | 6 | gold 80 | 22 | monastery | 3 | – (`heal: 40`) | 170 | 1.8 | 6 | – | – | – | – | `faith: 100`, `faithRegen: 0.4` |
| ram | `Ariete` | 340 | 62 | 5 | wood 160, gold 60 | 26 | siege | 3 | 55 | 60 | 2.2 | 2 | – | 330 | – | 4 | `siege`, `onlyBuildings` |
| catapult | `Catapulta` | 110 | 58 | 8 | wood 140, gold 120 | 28 | siege | 3 | 42 | 430 | 4.2 | 2 | – | 450 | – | 2.2 | `siege`, `splash: 72` |

Render fields:

| type | dir | fw | ax | ay | avatar | barY | sheet source |
|---|---|---|---|---|---|---|---|
| pawn | `Pawn` | 192 | 96 | 134 | 4 | 78 | pack (+2 Extra sheets for Stone) |
| scout | – | (Extra) | | | – | 78 | Extra |
| warrior | `Warrior` | 192 | 96 | 134 | 0 | 92 | pack |
| lancer | `Lancer` | 320 | 160 | 196 | 1 | 92 | pack |
| archer | `Archer` | 192 | 96 | 134 | 2 | 92 | pack |
| knight | – | (Extra) | | | – | 106 | Extra |
| monk | `Monk` | 192 | 96 | 134 | 3 | 78 | pack |
| ram | – | (Extra) | | | – | 100 | Extra |
| catapult | – | (Extra) | | | – | 74 | Extra |

### 2.2 Descriptions (UI, verbatim)

- pawn: `Raccoglie risorse, costruisce e ripara.`
- scout: `Veloce e con la vista lunga: esplora la mappa, anche da solo (tasto E), e occupa i punti liberi. Non combatte.`
- warrior: `Fante con scudo, disponibile da subito. Forte contro Lancieri ed edifici, teme gli Arcieri.`
- lancer: `Colpisce da lontano con la lancia. Forte contro i Cavalieri, teme i Guerrieri.`
- archer: `Tira da lontano. Forte contro i Guerrieri, fragile in mischia: teme i Cavalieri.`
- knight: `Cavalleria veloce e resistente. Travolge Arcieri e Catapulte, teme i Lancieri.`
- monk: `Cura le unità alleate ferite. Ha una riserva di 100 punti di cura che si ricarica piano. I nemici vicini lo preferiscono ad altri bersagli.`
- ram: `Sfonda edifici e mura. Non può colpire le unità; le frecce gli fanno poco.`
- catapult: `Lancia massi a grande distanza: colpiscono tutti i nemici in una zona.`

### 2.3 Derived stats & damage pipeline [SIM] (consumers of UNIT_DEF; `entities.js:236-248`, `482-512`)

```text
unitMaxHp(f,type)  = round(def.hp * (type=='monk' ? f.mods.monkHp : 1))
unitSpeed(f,type)  = def.speed * (type=='pawn' ? mods.pawnSpeed
                                 : def.foot ? mods.foot          // warrior, lancer, archer
                                 : type=='knight' ? mods.horse : 1)
unitDmg(u)         = (def.dmg||0 + mods.atk[type]||0) * (def.siege ? mods.siege : 1)
unitRange(u)       = def.range + (archer ? mods.range : monk ? mods.healRange : 0)
unitArmor(u)       = (def.dmg && !def.siege) ? mods.armor : 0     // only warrior, lancer, archer, knight
damageTo(src,base,t) = base * (t.isBuilding ? (src.def.vsBuilding || 1) : (src.def.bonus?.[t.type] || 1))
```

`hurt(t, amount, by)` (`entities.js:482`):
1. If `t` is a building: `amount *= BUILDING_ARMOR (0.4)`.
2. Else (unit): if `by` is an arrow (`by.vx !== undefined`, i.e. archer / tower / castle arrow, **not** catapult rock):
   `ram → ×0.2`; else if target has `def.block` (warrior, lancer) **or** is a knight → `× mods.arrowRes` (tech `shields`: 0.5).
   Then `amount = max(1, amount - unitArmor(t))`.
   Then if `t.def.block && t.state != 'attack' && rand() < def.block` → `amount *= 0.35`, start block (state `block`, `blockT = 0.5` s, anim `block` / `block<Dir5>` for lancer), play `block` sound.
3. `t.hp -= amount`; death at `hp <= 0`.

Melee/ram hit validity at the hit frame (`entities.js:475`): `distTo(u,t) <= unitRange(u)*1.5 + (t.isBuilding ? 44 : 0)`
(`distTo` to a building = distance to its pixel rectangle).
Monk heal at hit frame (`entities.js:470`): `amount = min(def.heal * mods.heal, t.maxHp - t.hp, u.faith)`; `t.hp += amount; u.faith -= amount`.
Faith regen every tick (`entities.js:1002`): `faith = min(def.faith, faith + def.faithRegen * mods.heal * dt)`.
Initial `faith = def.faith` (100) (`entities.js:257`).
Catapult splash (`entities.js:1223`): at impact point, every non-inside enemy **unit** with `hypot(u - impact) < splash (72)`
and every enemy **building** with `rectDist(impact, b) < splash*0.5 (36)` takes `damageTo(owner, dmg, target)`.
Rock flight: start `(owner.x ± 20 (flip), owner.y - 50)`, duration `0.5 + d/520` s, arc height `sin(kπ)*150` (visual).
Arrow: speed 640 px/s, `life = (d + 70)/640` (`entities.js:441-446`).

### 2.4 Training rules [SIM] (`factions.js:4`, `131-135`, `entities.js:1176-1197`, `factions.js:202-218`)

- `QUEUE_MAX = 5` entries per building (units **and** techs share the queue). `trainBlock` reasons (verbatim):
  `'Non disponibile'` (wrong building / not finished), `'Richiede: ' + AGE_NAME[age]`, `'Coda piena'`, `'Risorse insufficienti'`.
- Unit progress per tick: `q.t += dt * trainSpeed(n)` with `trainSpeed(n) = 1 + TRAIN_BONUS(0.2) * clamp(n-1, 0, 5)`,
  `n = f.has[b.kind]` = number of **completed** buildings of that kind (max ×2.0). Techs progress at ×1.
- Completed unit spawns only if `roomFor(f,type)`: pawns need `pawnN < pawnCap`, **every other type (scout included!) counts as a troop** (`troopN < troopCap`).
  Otherwise it waits; for human factions a toast every ≥15 s: `'Pedoni al massimo'` / `'Servono più case'` / `'Truppe al massimo'` / `'Servono più accampamenti'`.
- Caps: `pawnCap = min(PAWN_MAX=30, Σ def.pawns)`, `troopCap = min(TROOP_MAX=50, Σ def.troops)` over completed buildings (see §3).

### 2.5 Animations — `UNIT_ANIM` (`assets.js:176-206`) and sheet data [SIM!]

`UNIT_ANIM[type][anim] = [fileBaseName, fps]`. Sheet lookup `unitSheet(color,type,anim)`: if `EXTRA.units[file]` exists →
`Extra/Units/{Color}/{file}.png` with that entry's `fw, ax, ay`; else `Units/{Color} Units/{def.dir}/{file}.png` with
the unit's `fw, ax, ay`. Frame height = image height; frame count `n = round(imageWidth / fw)`.

Frame counts below are measured from the actual PNGs (Blue; all colours identical).

| type | anim key | file | fps | fw×fh | n | anchor ax,ay |
|---|---|---|---|---|---|---|
| warrior | idle | Warrior_Idle | 8 | 192×192 | 8 | 96,134 |
| | run | Warrior_Run | 10 | 192 | 6 | |
| | attack1 | Warrior_Attack1 | 12 | 192 | 4 | |
| | attack2 | Warrior_Attack2 | 12 | 192 | 4 | |
| | block | Warrior_Guard | 10 | 192 | 6 | |
| lancer | idle | Lancer_Idle | 10 | 320×320 | 12 | 160,196 |
| | run | Lancer_Run | 10 | 320 | 6 | |
| | attack{Up,UpRight,Right,DownRight,Down} | Lancer_{Dir}_Attack | 9 | 320 | 3 | |
| | block{Dir} | Lancer_{Dir}_Defence | 10 | 320 | 6 | |
| archer | idle | Archer_Idle | 8 | 192×192 | 6 | 96,134 |
| | run | Archer_Run | 10 | 192 | 4 | |
| | attack | Archer_Shoot | 14 | 192 | 8 | |
| monk | idle | Idle | 8 | 192×192 | 6 | 96,134 |
| | run | Run | 10 | 192 | 4 | |
| | attack | Heal | 12 | 192 | 11 | |
| pawn | idle | Pawn_Idle | 8 | 192×192 | 8 | 96,134 |
| | run | Pawn_Run | 10 | 192 | 6 | |
| | idle{Axe,Pickaxe,Knife,Hammer,Wood,Gold,Meat} | `Pawn_Idle {T}` | 8 | 192 | 8 | |
| | idleStone | `Pawn_Idle Stone` (Extra) | 8 | 192 | 8 | 96,134 |
| | run{Axe,Pickaxe,Knife,Hammer,Wood,Gold,Meat} | `Pawn_Run {T}` | 10 | 192 | 6 | |
| | runStone | `Pawn_Run Stone` (Extra) | 10 | 192 | 6 | 96,134 |
| | workAxe / workPickaxe | `Pawn_Interact Axe/Pickaxe` | 10 | 192 | 6 | |
| | workKnife | `Pawn_Interact Knife` | 10 | 192 | 4 | |
| | workHammer | `Pawn_Interact Hammer` | 10 | 192 | 3 | |
| scout | idle | Scout_Idle (Extra) | 8 | 96×96 | 6 | 48,86 |
| | run | Scout_Run (Extra) | 10 | 96 | 6 | 48,86 |
| knight | idle | Knight_Idle (Extra) | 8 | 128×128 | 6 | 64,112 |
| | run | Knight_Run (Extra) | 12 | 128 | 6 | 64,113 |
| | attack | Knight_Attack (Extra) | 12 | 160×128 | 6 | 80,113 |
| ram | idle | Ram_Run (Extra) | **0** | 128×128 | 4 | 64,117 |
| | run | Ram_Run | 8 | 128 | 4 | 64,117 |
| | attack | Ram_Attack (Extra) | 7 | 160×128 | 4 | 80,118 |
| catapult | idle | Catapult_Run (Extra) | **0** | 96×96 | 4 | 48,85 |
| | run | Catapult_Run | 8 | 96 | 4 | 48,85 |
| | attack | Catapult_Attack (Extra) | 7 | 96 | 4 | 48,86 |

fps 0 ⇒ frame stays 0 (siege engines at rest show the first marching frame).

**Attack timing is driven by these numbers** (`entities.js:410-438`): `startAttack` sets `state='attack'`, `hitDone=false`,
`cd = def.cd`, `animT = 0`, picks anim (`warrior`: alternates `attack1`/`attack2` via `combo ^= 1`; `lancer`:
`'attack' + dir5(tx-x, ty-y)`; others `attack`). Each tick `animT += dt` (before `tickAction`); `fr = floor(animT*fps)`;
when `fr >= hitFrame` (once) → `doHit`; when `fr >= n` → state back to `idle`. The unit cannot do anything else while
in `attack` or `block` state. Resulting timings (Rust sim must reproduce; store `attackFrames`, `attackFps` per type/anim):

| type | frames @fps | anim length | hit at |
|---|---|---|---|
| warrior | 4 @12 | 0.333 s | frame 2 → 0.167 s |
| lancer | 3 @9 | 0.333 s | frame 1 → 0.111 s |
| archer | 8 @14 | 0.571 s | frame 5 → 0.357 s |
| knight | 6 @12 | 0.500 s | frame 3 → 0.250 s |
| monk | 11 @12 | 0.917 s | frame 6 → 0.500 s |
| ram | 4 @7 | 0.571 s | frame 2 → 0.286 s |
| catapult | 4 @7 | 0.571 s | frame 2 → 0.286 s |

`dir5(dx,dy)` (`entities.js:48`): `a = atan2(-dy, |dx|)`; `a>1.18→'Up'`, `>0.39→'UpRight'`, `>-0.39→'Right'`, `>-1.18→'DownRight'`, else `'Down'`; horizontal facing is the separate `flip` flag (`flip = tx < x` when `|tx-x| > 2`).
Locomotion anim (`entities.js:1015-1020`): `run` if moved this tick else `idle`; for pawns append `LOAD_NAME[carry.kind]`
(`Wood/Gold/Meat/Stone`) if carrying, else the current tool (`JOB_TOOL`: wood→`Axe`, gold→`Pickaxe`, stone→`Pickaxe`, meat→`Knife`; building → `Hammer`), else nothing. Working = `'work' + tool` (`entities.js:794`). `setAnim` resets `animT` only when the anim changes. Spawned units start with `animT = random()`.

Drawing a unit (`entities.js:1304-1317`) [RENDER]: frame `floor(animT*fps)` (wrapped mod n; clamped to `n-1` while attacking),
mirrored when `flip`; alpha 0.5 while `hurtT > 0` (hurtT set to 0.12 s on every hit); selection ellipse centred `(x, y+1)`,
radii 38×13 for siege & knight else 24×10, stroke 2.5 px, `#ffffff` for own units, `#ff5a4a` for others.

### 2.6 Portraits (`assets.js:208-215`, `575-581`) [RENDER]

- Avatar file `UIP + 'Human Avatars/Avatars_NN.png'`, `NN = pad2(colorIdx*5 + def.avatar + 1)` → Blue 01–05, Red 06–10,
  Yellow 11–15, Purple 16–20, Black 21–25; within a block: +0 warrior, +1 lancer, +2 archer, +3 monk, +4 pawn.
  Frame 256×256; only the centre crop `(40,40,176,176)` is drawn.
- Units without `avatar` (scout, knight, ram, catapult) → crop of frame 0 of their `idle` sheet:
  `side = min(fw,fh)*0.86`, source rect `((fw-side)/2, ay - side*0.9, side, side)`.

---

## 3. Buildings — `BUILD_DEF` (`assets.js:217-305`)

Field meanings (`assets.js:219-223`): `w,h` footprint in cells · `ax,ay` sprite anchor · `work` construction seconds for
**one** pawn · `crop` visible sprite rect `[sx,sy,sw,sh]` (thumbnails) · `drop` resources that can be delivered here ·
`extra` sprite from `assets/Extra` (anchor/crop then come from `EXTRA.buildings`, copied in at `assets.js:294-296`) ·
`pawns`/`troops` population slots added · `defense` may only be placed near castle/own outpost · `trains` unit types ·
`techs` research ids offered · `trade` market · `neutral` sprite is not team-coloured · `sprites` variant list (houses).

| kind | label | w×h | hp | cost | work | sight | age | pawns | troops | drop | trains | techs | flags |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| castle | `Castello` | 5×2 | 2600 | – (start only) | 0 | 10 | – | 10 | 6 | wood, gold, meat, stone | pawn, scout | age2, age3, carts, masonry | |
| house | `Casa` | 2×1 | 320 | wood 30 | 9 | 4 | 1 | 5 | – | – | – | – | 3 variants |
| camp | `Accampamento` | 3×2 | 420 | wood 60 | 12 | 5 | 1 | – | 8 | – | – | – | |
| depot | `Deposito` | 2×1 | 380 | wood 60 | 12 | 5 | 1 | – | – | wood, gold, stone | – | axes, picks | |
| farm | `Fattoria` | 3×2 | 420 | wood 150 | 14 | 5 | 1 | – | – | meat | – | breeding | |
| barracks | `Caserma` | 3×2 | 700 | wood 90 | 18 | 5 | 1 | – | 2 | – | warrior, lancer | boots | |
| wall | `Palizzata` | 1×1 | 300 | wood 12 | 3 | 2 | 1 | – | – | – | – | – | `neutral`, `defense` |
| archery | `Arcieria` | 3×2 | 600 | wood 80, gold 30 | 18 | 5 | 2 | – | 2 | – | archer | – | |
| stable | `Scuderia` | 3×2 | 650 | wood 110, gold 40 | 18 | 5 | 2 | – | 2 | – | knight | horseshoes | |
| smith | `Fucina` | 3×2 | 650 | wood 100, stone 40 | 18 | 5 | 2 | – | – | – | – | blades, shields, fletching, armor | |
| market | `Mercato` | 3×2 | 600 | wood 120 | 18 | 5 | 2 | – | – | – | – | maps | `trade` |
| tower | `Torre` | 2×1 | 620 | stone 80, wood 40 | 16 | 9 | 2 | – | – | – | – | – | `defense` |
| monastery | `Monastero` | 3×2 | 600 | wood 80, gold 100, stone 40 | 22 | 5 | 3 | – | 2 | – | monk | herbs, scriptures | |
| siege | `Officina d'assedio` | 3×2 | 650 | wood 160, gold 60 | 22 | 5 | 3 | – | 2 | – | ram, catapult | engineering | |

Population check: castle gives 10 pawns + 6 troops; each house +5 pawns (cap 30 ⇒ castle + 4 houses);
camp +8 troops; barracks/archery/stable/monastery/siege +2 troops each (cap 50).

Sprites & anchors (render):

| kind | sprite file(s) | image size | ax | ay | crop |
|---|---|---|---|---|---|
| castle | `Buildings/{C} Buildings/Castle.png` | 320×256 | 160 | 250 | [4,41,312,208] |
| house | `…/House1.png`, `House2.png`, `House3.png` | 128×192 | 64 | 178 | [8,16,112,157] |
| barracks | `…/Barracks.png` | 192×256 | 96 | 246 | [4,58,184,187] |
| archery | `…/Archery.png` | 192×256 | 96 | 242 | [3,61,183,179] |
| tower | `…/Tower.png` | 128×256 | 64 | 232 | [4,46,120,184] |
| monastery | `…/Monastery.png` | 192×320 | 96 | 312 | [16,45,160,265] |
| camp | `Extra/Buildings/{C}/Camp.png` | 200×189 | 100 | 179 | [6,6,188,177] |
| depot | `Extra/Buildings/{C}/Depot.png` | 136×118 | 68 | 108 | [6,6,124,106] |
| farm | `Extra/Buildings/{C}/Farm.png` | 202×167 | 101 | 157 | [6,6,190,155] |
| stable | `Extra/Buildings/{C}/Stable.png` | 200×174 | 100 | 164 | [6,6,188,162] |
| smith | `Extra/Buildings/{C}/Smith.png` | 196×184 | 98 | 174 | [6,6,184,172] |
| market | `Extra/Buildings/{C}/Market.png` | 202×207 | 101 | 197 | [6,6,190,195] |
| siege | `Extra/Buildings/{C}/Siege.png` | 202×192 | 101 | 182 | [6,6,190,180] |
| wall | `Extra/Buildings/Wall.png` (neutral, no colour) | 76×78 | 38 | 68 | [6,6,64,66] |

`{C}` = `Blue|Red|Yellow|Purple|Black`. `buildingImg(color, kind, variant=0)` at `assets.js:301-305`.
House variant: `b.variant = f.houses++ % 3` (per-faction round-robin, `entities.js:1120`).

Descriptions (verbatim):
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

### 3.1 Building geometry & lifecycle [SIM] (consumers; `entities.js:1070-1135`, `879-891`)

- Grid placement by top-left cell `(cx, cy)`; cells = all `w×h` cells. Pixel rect `x0=cx*64, y0=cy*64, x1=(cx+w)*64, y1=(cy+h)*64`.
  Foot point `x = (cx + w/2)*64`, `y = sy = (cy+h)*64 - 4`. Sprite drawn at `(x - ax, y - ay)`.
- `maxHp = round(def.hp * mods.buildHp)`; new site starts at `hp = maxHp*0.1`, `progress = 0`; a pre-built building (castle at start) has `hp = maxHp`, `progress = work`.
- Each working pawn per tick: `progress += dt`; `hp = min(maxHp, hp + maxHp*0.9*dt/work)`; done when `progress >= work` (pawns stack linearly). Repair: `hp += maxHp * REPAIR_RATE(0.035) * dt` per pawn.
- `rally = {x: b.x, y: b.y1 + 72, t: null}`, `pasture = {x: b.x, y: b.y1 + 70}` (farm sheep home).
- Farm completion spawns 2 sheep; then every `FARM_BREED = 9` s (`breedT += dt*mods.breed`) one sheep if `< FARM_SHEEP = 5` around its pasture.
- Placement zone: `DEFENSE_RADIUS = 12` cells (for `defense` kinds) / `BUILD_RADIUS = 40` cells (others) from castle centre, or `OUTPOST_RADIUS = 9` from an owned outpost (any kind). Distances from building centre `(cx + w/2, cy + h/2)` (`factions.js:9`, `161-166`). Messages: `'Torri e palizzate: entro 12 caselle dal castello o 9 da un tuo avamposto'` / `'Troppo lontano: entro 40 caselle dal castello o 9 da un tuo avamposto'` (template `` `${d.defense ? 'Torri e palizzate' : 'Troppo lontano'}: entro ${r} caselle dal castello o ${OUTPOST_RADIUS} da un tuo avamposto` ``), `'Qui non si può costruire'`, `'Qui non può stare'`, `'Il castello non si sposta'`.
- Tower & castle shooting (`entities.js:1207-1218`): range `(tower 420 | castle 380) + mods.range`; picks the nearest enemy unit (not sheltered) by distance from `(b.x, b.y)`; arrow from `(b.x, b.y - ay*0.62)` to `(t.x, t.y-28)`, damage `(tower 15 | castle 14) + mods.atk.archer`; cooldown `tower 1.1 | castle 1.2` s; no target → retry after 0.3 s. Initial `cd = rand(0,1)`. Only when `done`.
- Destruction loot (`entities.js:1149-1157`), if killed by another faction and kind ≠ wall: 3 pickups of 15 each, life 60 s: farm `meat, meat, wood`; tower `stone, stone, wood`; others `wood, wood, gold`. Castle destruction eliminates the faction.
- Arrows hit buildings when inside `x0 < x < x1`, `y0 - 70 < y < y1` (`entities.js:1260`).

### 3.2 Building rendering [RENDER] (`entities.js:1319-1355`)

- Under construction: whole sprite at alpha 0.3, then the bottom `round(imgH * progress/work)` rows at full alpha (rises from the ground).
- Damage fire when done and `hp/maxHp < 0.7`: `<0.25` → `fire3` ×4; `<0.45` → `fire2` ×3; else `fire1` ×2. Flame i position:
  `h1 = |sin(fireSeed + i*12.9898)*43758.5453| % 1`, `h2 = |sin(fireSeed + i*78.233)*12543.123| % 1`;
  at `(b.x + (h1-0.5)*w*64*0.7, b.y - 24 - h2*ay*0.45)`, frame `floor(time*fps + i*3)`, scale 1.4. `fireSeed = random()*100` per building.
- Wall (palisade) auto-tiling (`STAKE = {sx:31, sw:14, n:5}`): neighbours via `World.wallOf`; `side` = wall left or right,
  `up`/`down` = wall above/below, `front = side || !(up || down)`. If `!front || up`: draw a column of stakes — the 14-px
  column `sx=31` of the image repeated `n = front ? 4 : 5` times at `y = y0 + (i+1)*64/5 - 72`, centred on `b.x`.
  If `front`: draw the full fence sprite at the anchor. Site alpha `0.35 + 0.65*clamp(progress/work)`.

---

## 4. Capture points — `POINT_DEF` (`assets.js:307-320`)

Neutral 2×2 structures; captured by being the only faction with troops (pawns excluded) nearby (logic in `entities.js:193-232`).

| kind | label | sprite (Extra/Buildings/) | res | zone | size | ax,ay | crop | desc (verbatim) |
|---|---|---|---|---|---|---|---|---|
| outpost | `Avamposto` | Outpost.png | – | yes | 168×230 | 84,220 | [6,6,156,218] | `Chi lo tiene può costruire nei dintorni, torri e palizzate comprese.` |
| mine | `Miniera` | Mine.png | gold | – | 168×150 | 84,140 | [6,6,156,138] | `Rende oro a chi la tiene, senza Pedoni.` |
| quarry | `Cava` | Quarry.png | stone | – | 168×161 | 84,151 | [6,6,156,149] | `Rende pietra a chi la tiene, senza Pedoni.` |
| sawmill | `Segheria` | Sawmill.png | wood | – | 168×162 | 84,152 | [6,6,156,150] | `Rende legna a chi la tiene, senza Pedoni.` |
| ranch | `Ovile` | Ranch.png | meat | – | 168×145 | 84,135 | [6,6,156,133] | `Rende cibo a chi lo tiene, senza Pedoni.` |

Related constants (`entities.js:29-32`): `POINT_INCOME = 0.5` res/s, `CAPTURE_TIME = 8` s, `CAPTURE_RADIUS = 300` px,
`POINT_SIGHT = 7` cells. Geometry (`entities.js:197-204`): `c` = top-left of the 2×2; `x = (cx+1)*64`, `y = (cy+2)*64 - 4`,
`gx = cx+1, gy = cy+1` (centre in cells, used for the outpost build radius). Capture check every 0.5 s; unit counts if
`|u.x - p.x| <= 300` and `hypot(u.x - p.x, u.y - p.y + 64) <= 300`. Messages: `'Hai conquistato: '+label`, `'Hai perso: '+label`.
`pointImg(kind)` = `Extra/Buildings/{sprite}.png` (neutral). `bigNodeImg(kind)` = `Extra/Resources/Gold_Big.png` (gold) or `Stone_Big.png` (anything else).

---

## 5. Research — `TECH_DEF` (`assets.js:322-396`) and effects (`factions.js:78-142`)

Fields: `at` building kind · `age` minimum era (also its row in the tech tree panel) · `req` prerequisite tech · `cost` · `time` seconds · `icon` drawSpec (§9.6).

| id | label | at | age | req | cost | time | icon | effect (`techMods`, `factions.js:81-99`) |
|---|---|---|---|---|---|---|---|---|
| age2 | `Età Feudale` | castle | 1 | – | meat 250, gold 100 | 40 | ['age',2] | `f.age = 2` |
| age3 | `Età dei Castelli` | castle | 2 | age2 | meat 400, gold 300, stone 100 | 55 | ['age',3] | `f.age = 3` |
| carts | `Carriole` | castle | 2 | – | wood 120, meat 80 | 28 | ['x','barrow'] | `carry = 5` (CARRY 10 → 15), `pawnSpeed = 1.15` |
| masonry | `Muratura` | castle | 3 | – | stone 150, wood 100 | 36 | ['x','tower'] | `buildHp = 1.3` |
| axes | `Asce affilate` | depot | 1 | – | meat 80, wood 60 | 24 | ['x','axe'] | `gather.wood = 1.3` |
| picks | `Picconi d'acciaio` | depot | 2 | axes | meat 80, wood 60 | 24 | ['x','pickaxe'] | `gather.gold = 1.3; gather.stone = 1.3` |
| breeding | `Allevamento` | farm | 1 | – | wood 80, gold 40 | 24 | ['x','wheat'] | `breed = 1.5` |
| boots | `Marcia forzata` | barracks | 2 | – | meat 100, gold 40 | 26 | ['x','boot'] | `foot = 1.12` (warrior, lancer, archer) |
| horseshoes | `Ferratura` | stable | 2 | – | meat 100, gold 80 | 28 | ['x','horseshoe'] | `horse = 1.15` (knight) |
| blades | `Lame temprate` | smith | 2 | – | meat 100, gold 80 | 30 | ['x','sword'] | `atk.warrior += 4; atk.lancer += 4; atk.knight += 4` |
| shields | `Scudi rinforzati` | smith | 2 | – | wood 120, gold 60 | 30 | ['x','shield'] | `arrowRes = 0.5` |
| fletching | `Impennaggio` | smith | 2 | – | wood 100, gold 60 | 30 | ['x','arrows'] | `atk.archer += 3; range = 45` (archer range, tower & castle range and damage) |
| armor | `Corazze` | smith | 3 | shields | meat 120, gold 120 | 36 | ['x','breastplate'] | `armor = 3` |
| maps | `Cartografia` | market | 2 | – | wood 80, gold 60 | 24 | ['x','spyglass'] | `sight = 2` (+2 cells for units, buildings, owned points) |
| herbs | `Erbe curative` | monastery | 3 | – | gold 120 | 30 | ['x','herbs'] | `heal = 1.5` (heal amount and faith regen) |
| scriptures | `Sacre scritture` | monastery | 3 | herbs | gold 100, wood 60 | 30 | ['x','book'] | `healRange = 70; monkHp = 2` |
| engineering | `Ingegneria` | siege | 3 | – | wood 150, gold 120 | 36 | ['x','anvil'] | `siege = 1.25` (ram & catapult damage) |

Descriptions (verbatim):
- age2: `Sblocca Arcieria, Scuderia, Fucina, Mercato, Torre, Lancieri e nuove ricerche.`
- age3: `Sblocca Monastero, Officina d'assedio e le ricerche avanzate.`
- carts: `I Pedoni portano 5 risorse in più e camminano più svelti.`
- masonry: `+30% di resistenza a tutti gli edifici.`
- axes: `I Pedoni tagliano la legna il 30% più in fretta.`
- picks: `I Pedoni estraggono oro e pietra il 30% più in fretta.`
- breeding: `Le Fattorie allevano il 50% più in fretta.`
- boots: `Guerrieri, Lancieri e Arcieri si muovono il 12% più veloci.`
- horseshoes: `I Cavalieri sono il 15% più veloci.`
- blades: `+4 attacco a Guerrieri, Lancieri e Cavalieri.`
- shields: `Guerrieri, Lancieri e Cavalieri subiscono metà del danno dalle frecce.`
- fletching: `+3 attacco e più gittata ad Arcieri, Torri e Castello.`
- armor: `Tutti i soldati subiscono 3 danni in meno per colpo.`
- maps: `Unità ed edifici vedono 2 caselle più lontano.`
- herbs: `I Monaci curano il 50% in più e ricaricano la riserva il 50% più in fretta.`
- scriptures: `I Monaci curano da più lontano e resistono il doppio.`
- engineering: `+25% di danno ad Arieti e Catapulte.`

Faction modifier record and defaults (`factions.js:46-50`):
```
mods = { atk:{warrior:0,lancer:0,archer:0,knight:0}, range:0, armor:0, arrowRes:1, siege:1,
         gather:{wood:1,gold:1,meat:1,stone:1}, carry:0, pawnSpeed:1, foot:1, horse:1,
         heal:1, healRange:0, monkHp:1, breed:1, buildHp:1, sight:0 }
```
Effects are **assignments**, not stacking (each tech can only be researched once). Gather multiplier applies to
`workT += dt * gather[res]` against `JOB_TIME` (wood 3.0, gold 3.4, stone 3.4, meat 20 s per load).

Research rules [SIM]:
- `techLock`: `'Richiede: ' + AGE_NAME[def.age]` if `f.age < def.age`; else `'Richiede: ' + TECH_DEF[req].label` if req not researched.
- `techBlock` (`factions.js:136-142`), in order: `'Non disponibile'` (wrong building or not done) · `'Già ricercata'` · `'Ricerca in corso'` (`f.busy[id]`, blocks the same tech in another building) · age/req lock · `'Coda piena'` · `'Risorse insufficienti'`.
- On completion `applyTech` (`factions.js:101-123`): `f.tech[id] = true`. Age techs: set `f.age`, announce `` `${f.name}: ${AGE_NAME[f.age]}` `` (4 s), play `age` for humans; no mods. Others: apply mods, then for every existing unit of that faction recompute `speed = unitSpeed` and if `unitMaxHp` changed scale `hp *= newMax/oldMax`; for every own building `newMax = round(def.hp*buildHp)`, scale `hp` likewise. Human: play `research`, announce `'Ricerca completata: ' + label` (2.6 s).
- Destroyed building: its queued techs are removed from `f.busy` (`entities.js:1144`); costs are not refunded there.

Tech tree (prerequisite edges): `age2 → age3`, `axes → picks`, `shields → armor`, `herbs → scriptures`.

### 5.1 What each era unlocks (derived from `age` fields)

| Era | Units | Buildings | Techs available |
|---|---|---|---|
| I `Età del Villaggio` (start) | pawn, scout, warrior | house, camp, depot, farm, barracks, wall (+ castle given at start) | age2, axes, breeding |
| II `Età Feudale` | lancer, archer, knight | archery, stable, smith, market, tower | age3, carts, picks*, boots, horseshoes, blades, shields, fletching, maps |
| III `Età dei Castelli` | monk, ram, catapult | monastery, siege | masonry, armor*, herbs, scriptures*, engineering |

(* also needs the prerequisite.) Note: barracks (era I) offers lancer only from era II.
Start state (`factions.js:10`, `64-69`): `START_RES = { wood: 200, gold: 100, meat: 200, stone: 100 }`, `age = 1`, castle placed done at `(base.x-2, base.y-1)`, 4 pawns + 1 warrior deployed idle.

---

## 6. Other simulation constants adjacent to the defs (`entities.js:5-33`, `factions.js:4-10`) [SIM]

Listed for completeness (their logic is specified in the entities/factions chapters):
`JOB_TOOL = {wood:'Axe', gold:'Pickaxe', stone:'Pickaxe', meat:'Knife'}` · `JOB_TIME = {wood:3.0, gold:3.4, stone:3.4, meat:20}` ·
`NODE_USERS = {tree:2, gold:3, stone:3, sheep:1}` · `BIG_USERS = 8` · `NODE_RES = {tree:'wood', gold:'gold', stone:'stone', sheep:'meat'}` ·
`CARRY = 10` · `SHEEP_MEAT = 15` · `TREE_WOOD = 40` · `GOLD_MAX = 240` · `STONE_MAX = 300` · `FARM_BREED = 9` · `FARM_SHEEP = 5` ·
`TREASURE = 50` · `BUILDING_ARMOR = 0.4` · `REPAIR_RATE = 0.035` · `ALERT_EVERY = 12` · `ALARM_TIME = 10` · `HOME_RADIUS = 1500` ·
`RAID_RADIUS = 900` · `SHELTER_CAP = 5` · `CALM_TIME = 4` · `HEALER_FOCUS = 0.6` · `KITE_RANGE = 110` · `MONK_SPACE = 150` ·
`ESCORT_GAP = 200` · `LINE_GAP = 44` · `POINT_INCOME = 0.5` · `CAPTURE_TIME = 8` · `CAPTURE_RADIUS = 300` · `POINT_SIGHT = 7` · `TRAIN_BONUS = 0.2` ·
`PAWN_MAX = 30` · `TROOP_MAX = 50` · `QUEUE_MAX = 5` · `RAID_SPACING = 120` · `WORK_RADIUS = 2600` · `DEFENSE_RADIUS = 12` · `BUILD_RADIUS = 40` · `OUTPOST_RADIUS = 9`.
Tree regrow 110 s after depletion; gold/stone veins regain +10 every 25 s up to max (`entities.js:144,166-167`).
Sight → pixels: `(def.sight + mods.sight) * TILE`; buildings under construction see only 2 cells (`control.js:57`, `factions.js:289`), building sight centre `(b.x, b.y - 30)`, point sight centre `(p.x, p.y - 64)`.
UI unit stat display (`ui.js:338-342`): attack `round(unitDmg)`, armour `unitArmor`, range in cells `round(unitRange/64)` only if `def.range > 100`, speed `round(unitSpeed/10)`, heal `round(heal*mods.heal)` + `` `  ·  riserva ${floor(faith)}/${def.faith}` `` for own monks.

---

## 7. Utility functions (`assets.js:598-611`) [SIM]

```js
rand(a,b)    = a + Math.random()*(b-a)
randInt(a,b) = floor(rand(a, b+1))          // inclusive both ends
pick(arr)    = arr[floor(random()*arr.length)]
clamp(v,a,b) = v<a ? a : v>b ? b : v
mulberry32(a) -> () => {                    // seeded PRNG used by map generation (world.js:312)
  a |= 0; a = (a + 0x6D2B79F5) | 0;
  let t = Math.imul(a ^ (a >>> 15), 1 | a);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
```
Rust: `a = a.wrapping_add(0x6D2B79F5)`; `t = (a ^ (a >> 15)).wrapping_mul(1 | a)` (u32 arithmetic, `>>>` = logical shift);
`t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t`; result `(t ^ (t >> 14)) as f64 / 4294967296.0`.
**Determinism note:** the JS simulation calls unseeded `Math.random()` everywhere (`rand`, `pick`, block rolls, AI timers,
spawn jitter…). The Rust sim must route all of these through one seeded per-match RNG so that server and solo runs are
reproducible; exact JS random sequences need not be matched.

---

## 8. Asset loading (`assets.js:11-44`, `71-103`, `manifest.js`) [RENDER]

- `ASSET_FILES` (`manifest.js:3-592`, generated by `tools/make_sprites.py`) lists **every PNG** under `assets/` (~588 files):
  the "Tiny Swords (Free Pack)" plus `assets/Extra`. Everything is preloaded before the game starts; progress = `loaded/total`.
  URL = `'assets/' + path segments URI-encoded` (paths contain spaces). A missing file rejects with `'Impossibile caricare assets/' + f`;
  `image(path)` throws `'Sprite mancante: ' + path` if not in the manifest.
- Font `Fredoka` from `fonts/Fredoka.ttf` (weights 300–700); failure is ignored (system fallback).
- After load, `buildDerived()` builds `Assets.barFill[color]`: a copy of `Bars/SmallBar_Fill.png` tinted with `COLOR_CSS[color]` (`source-atop` fill → solid colour where the fill is opaque).
- `sheet(path, fw, ax, ay, fps=10)` caches `{im, path, fw, fh: im.height, n: round(im.width/fw), ax, ay, fps}` — **first call wins** (later calls with different params return the cached record).
- `drawSheet(ctx, s, frame, x, y, flip=false, scale=1)`: `f = ((frame % n) + n) % n`; x, y rounded; draws source `(f*fw, 0, fw, fh)` at `(x - ax, y - ay)`; with flip/scale: translate to (x,y), scale `(flip ? -scale : scale, scale)`, draw at `(-ax, -ay)`.
- **Shadow pass** (`SHADOW`, `assets.js:50-69`, driven by `light.js:112-123`): while `SHADOW.on`, `img()`/`drawSheet` substitute a silhouette of the sprite filled with `#0e1230` (`source-in`), cached per path with the colour name replaced by `*` (shape is colour-independent). Selection rings, fire and HP overlays are skipped during the pass. In HD-2D this becomes real shadow casting of billboards; keep the colour as reference.

### 8.1 Manifest structure (colour folders ×5: Blue, Red, Yellow, Purple, Black)

| Folder | Content (per colour where applicable) | Size(s) |
|---|---|---|
| `Buildings/{C} Buildings/` | Archery, Barracks, Castle, House1-3, Monastery, Tower | see §3 |
| `Extra/Buildings/{C}/` | Camp, Depot, Farm, Market, Siege, Smith, Stable | see §3 |
| `Extra/Buildings/` (neutral) | Mine, Outpost, Quarry, Ranch, Sawmill, Wall | see §3/§4 |
| `Extra/Icons/` | 28 icons 64×64: stone, crown, anvil, horseshoe, flag, stop, arrows, shield, upgrade, hourglass, wheat, spyglass, sword, breastplate, pickaxe, axe, coins, scales, barrow, herbs, tower, boot, book, bell, tent, map, eye, banner (`EXTRA.icons`) | 64×64 |
| `Extra/Props/` | 49 static props (table §8.3) | various |
| `Extra/Resources/` | Gold_Big 134×108, Stone_Big 133×108, Stone_1 44×34, Stone_2 71×52, Stone_3 84×86, Stone_Resource 26×22 | |
| `Extra/Units/{C}/` | Knight_Idle/Run/Attack, Ram_Run/Attack, Catapult_Run/Attack, Scout_Idle/Run, `Pawn_Idle Stone`, `Pawn_Run Stone` | §2.5 |
| `Particle FX/` | Dust_01/02, Explosion_01/02, Fire_01/02/03, Water Splash | §8.2 |
| `Terrain/Decorations/Bushes/` | Bushe1-4, 1024×128 (8 frames of 128) | |
| `Terrain/Decorations/Clouds/` | Clouds_01-08, 576×256 | |
| `Terrain/Decorations/Rocks/` | Rock1-4, 64×64 | |
| `Terrain/Decorations/Rocks in the Water/` | `Water Rocks_01-04`, 1024×64 (16 frames of 64) | |
| `Terrain/Decorations/Rubber Duck/` | `Rubber duck.png` 96×32 (3 frames of 32) | |
| `Terrain/Resources/Gold/Gold Stones/` | `Gold Stone 1-6.png` 128×128 + `_Highlight` 768×128 (6 frames) | |
| `Terrain/Resources/Gold/Gold Resource/` | `Gold_Resource.png` 128×128, `Gold_Resource_Highlight.png` 768×128 | |
| `Terrain/Resources/Meat/` | `Meat Resource/Meat Resource.png` 64×64; `Sheep/Sheep_Idle` 768×128 (6f), `Sheep_Move` 512×128 (4f), `Sheep_Grass` 1536×128 (12f) | |
| `Terrain/Resources/Tools/` | Tool_01-04 64×64 (1 hammer, 2 axe, 3 knife, 4 pickaxe) — cursor icons (`ui.js:652`) | |
| `Terrain/Resources/Wood/Trees/` | Tree1, Tree2 1536×256; Tree3, Tree4 1536×192 (8 frames of 192); `Stump 1-4.png` 192×256 | |
| `Terrain/Resources/Wood/Wood Resource/` | `Wood Resource.png` 64×64 | |
| `Terrain/Tileset/` | `Tilemap_color1-5.png` 576×384, `Shadow.png` 192×192, `Water Background color.png` 64×64, `Water Foam.png` 3072×192 (16 frames of 192) | used by world.js (other chapter) |
| `UI Elements/UI Elements/` | Banners, Bars, Buttons (Big/Small/Tiny, Blue/Red, Regular/Pressed), Cursors 01-04, Human Avatars 01-25, Icons 01-12, Papers, Ribbons, Swords, Wood Table | §9 |
| `UI Elements/UI Banners from the store page/` | `Banner/Banner.png` 704×512, `Banner/Slots.png` 320×320, `Ribbons/Ribbon_{C}.png` 960×576 | §9 |
| `Units/{C} Units/` | Archer (+`Arrow.png` 64×64), Lancer, Monk (+`Heal_Effect.png` 2112×192, 11 frames), Pawn, Warrior | §2.5 |

UI pack icons by observed use: `Icon_01` build/hammer, `02` wood, `03` gold, `04` meat, `05` attack (sword), `06` armour,
`07` resume/play, `08` exit/menu, `09` close / demolish, `10` pause, `11` help, `12` sound.

### 8.2 Effects — `FX` (`assets.js:400-410`)

| key | file | fw | ax, ay | fps | frames | lifetime = n/fps |
|---|---|---|---|---|---|---|
| dust1 | `Particle FX/Dust_01.png` | 64 | 32, 46 | 14 | 8 | 0.571 s |
| dust2 | `Particle FX/Dust_02.png` | 64 | 32, 56 | 14 | 10 | 0.714 s |
| boom1 | `Particle FX/Explosion_01.png` | 192 | 96, 110 | 14 | 8 | 0.571 s |
| boom2 | `Particle FX/Explosion_02.png` | 192 | 96, 130 | 14 | 10 | 0.714 s |
| fire1 | `Particle FX/Fire_01.png` | 64 | 32, 62 | 10 | 8 | loop |
| fire2 | `Particle FX/Fire_02.png` | 64 | 32, 62 | 10 | 10 | loop |
| fire3 | `Particle FX/Fire_03.png` | 64 | 32, 62 | 10 | 12 | loop |
| splash | `Particle FX/Water Splash.png` | 192 | 96, 100 | 12 | 9 | 0.75 s |
| heal(color) | `Units/{C} Units/Monk/Heal_Effect.png` | 192 | 96, 134 | 12 | 11 | 0.917 s |

FX are presentation events (`emit(['fx', name, x, y, scale, dy, followId, color])`, `entities.js:58-88`); an FX is removed when `t*fps >= n` (`entities.js:1297`); `follow` makes it track a unit; `dy` only shifts draw order (`sy = y + dy`). Not part of the sim state.

### 8.3 Resources, nodes, props [RENDER] (`assets.js:412-427`, `extra.js:241-492`, `entities.js:1357-1445`)

- **Trees** `treeSheet(v)` = `Terrain/Resources/Wood/Trees/Tree{v+1}.png`, fw 192, ax 96, ay `TREE_DEF[v].ay`, 8 fps, 8 frames, frame `floor(time*8 + phase)` (phase random 0–8). Depleted → `Stump {v+1}.png` drawn at `(x - 96, y - stumpAy)`.
  `TREE_DEF = [{ay:240, stumpAy:238}, {ay:248, stumpAy:244}, {ay:170, stumpAy:231}, {ay:168, stumpAy:227}]`. Tree entity foot `(cellCenterX, cellCenterY + 22)`.
- **Gold** small node: stage `clamp(ceil(amount/40), 1, 6)` → `Gold Stone {stage}.png` drawn at `(x-64, y-64)`; when `shine > 0` play `Gold Stone {stage}_Highlight.png` (sheet fw 128, ax 64, ay 64, 10 fps) with frame `floor(shine*10)`; `shine` cycles: `+= dt`, when `> 0.6` reset to `-rand(3,7)` (cosmetic, not synced). Depleted node alpha 0.45.
- **Stone** small node: `Extra/Resources/Stone_{3 if amount>190, 2 if >80, else 1}.png` drawn bottom-centred at `(x - w/2, y + 22 - h)`. Depleted alpha 0.45.
- **Big deposits** (2×2): `Gold_Big` / `Stone_Big` drawn at `(x - w/2, cellCenterY(c) + 64*1.5 - 6 - h)`.
- **Sheep**: Sheep_Idle (fw 128, ax 64, ay 82, 8 fps), Sheep_Move (8 fps), Sheep_Grass (10 fps); flip when moving left.
- **Bushes**: `Bushe{v+1}.png` fw 128, ax 64, ay 80, 8 fps. **Rocks**: `Rock{v+1}.png` drawn at `(x-32, y-50)`.
- **Pickups** (scale 1.5, bob `|sin(t*3)|*4`, blink when `life < 5`): gold → `Gold_Resource_Highlight` (fw 128, ax 64, ay 70, 10 fps) for the first 0.6 s of each 2.2 s cycle else `Gold_Resource.png` at (-64,-70); stone → `Stone_Resource.png` at (-13,-20); wood/meat → `Wood Resource.png` / `Meat Resource.png` at (-32,-42). Chest → prop `chest` (y+8); opened → prop `chest_open`.
- **Projectiles**: arrow `Units/{C} Units/Archer/Arrow.png` centred (-32,-32) rotated to velocity; catapult rock `Stone_Resource.png` ×1.3 centred (-13,-11), spinning `ang += 9*dt`.
- **Props** (`drawProp`): `Extra/Props/{name}.png` anchored bottom-centre `(x - w/2, y - h)`. Sizes `[w, h]` (`EXTRA.props`):
  barrel 34×40, crate 39×40, logs 50×40, hay 51×44, well 60×76, signpost 45×58, campfire 49×44, cart 69×46, tent 80×84,
  mushrooms 44×40, flowers 56×36, statue 51×84, column 62×60, bones 51×34, deadtree 86×88, fence 65×40, scarecrow 58×68,
  grass 43×36, chest 45×38, chest_open 41×44, grave 55×50, boat 59×44, lilypads 42×34, reeds 49×52, ruin_arch 102×96,
  ruin_wall 95×60, menhir 63×84, stone_circle 73×56, boulder 58×56, shrine 64×80, obelisk 54×92, windmill 99×124,
  shipwreck 72×64, dock 64×48, wagon_wreck 70×50, beehive 38×46, flowers_red 51×34, flowers_blue 49×34, flowers_white 46×32,
  tall_grass 52×40, clover 39×26, pebbles 44×26, puddle 53×30, fallen_log 51×36, stump 49×34, pumpkins 56×38, ferns 55×40, sticks 41×24.
- `EXTRA.stones` (`Stone_1` 44×34, `Stone_2` 71×52, `Stone_3` 84×86, `Stone_Resource` 26×22) and `EXTRA.deposits` (`Gold_Big` 134×108, `Stone_Big` 133×108) are size records only.

---

## 9. UI drawing primitives (`assets.js:429-596`) [RENDER]

### 9.1 Slicing

- `nineSlice(ctx, im, xs, ys, x, y, w, h, s=1)`: `xs`/`ys` = three `[srcStart, srcLen]` pairs per axis. Corner sizes
  `lw = xs[0][1]*s, rw = xs[2][1]*s, th = ys[0][1]*s, bh = ys[2][1]*s`; if `lw+rw > w` both scale by `w/(lw+rw)` (same for height).
  Destination edges `X = [x, x+lw, x+w-rw, x+w]` (rounded), likewise `Y`; draws 9 pieces, centre/edges stretched, skipping zero-size cells.
- Grids: `G3 = [[0,64],[128,64],[256,64]]` (320-px sheets, separated cells) · `G7 = [[0,128],[192,64],[320,128]]` (448-px sheets) · `G1 = [[0,64],[64,64],[128,64]]` (192-px contiguous).
- `PANEL` table (`pad` = transparent margin `[left, top, right, bottom]` around the visible content; `drawPanel(def, x,y,w,h,s)` draws the 9-slice at `(x - l*s, y - t*s, w + (l+r)*s, h + (t+b)*s)` so that `(x,y,w,h)` is the **visible** rect):

| key | file | xs/ys | pad |
|---|---|---|---|
| banner | `UI Elements/UI Elements/Banners/Banner.png` | G7 | 28,60,44,17 |
| bannerSlots | `…/Banners/Banner_Slots.png` | G1 | 5,5,2,4 |
| wood | `…/Wood Table/WoodTable.png` | G7 | 44,43,44,25 |
| woodSlots | `…/Wood Table/WoodTable_Slots.png` | G1 | 12,11,12,11 |
| paper | `…/Papers/RegularPaper.png` | G3 | 12,20,12,19 |
| special | `…/Papers/SpecialPaper.png` | G3 | 9,20,9,21 |
| storeSlots | `UI Elements/UI Banners from the store page/Banner/Slots.png` | G3 | 43,8,40,17 |
| bigBlue / bigBlueDown | `…/Buttons/BigBlueButton_Regular.png` / `_Pressed.png` | G3 | 19,17,19,17 |
| bigRed / bigRedDown | `…/Buttons/BigRedButton_Regular.png` / `_Pressed.png` | G3 | 19,17,19,17 |

- `drawStoreBanner(x,y,w,h,s)` (store-page parchment, 704×512 sheet; bottom curls wider): with `c=64s, cr=44s, bh=98s, bl=192s, br=172s, mh=h-c-bh` (all rounded):
  top row src `(0,0,64,64)`→`(x,y,c,c)`; `(256,0,64,64)`→`(x+c,y,w-c-cr,c)`; `(640,0,44,64)`→`(x+w-cr,y,cr,c)`;
  middle src `(0,128,64,64)`, `(320,128,64,64)`, `(640,128,44,64)` → same columns, height `mh` at `y+c`;
  bottom src `(0,256,192,98)`→`(x, y+h-bh, bl, bh)`; `(256,256,64,98)`→`(x+bl, …, w-bl-br, bh)`; `(512,256,172,98)`→`(x+w-br, …, br, bh)`.
- `threeSlice(ctx, im, xs, sy, sh, x, y, w, s)`: one row (source y `sy`, height `sh`), left/right caps `xs[0][1]*s`, `xs[2][1]*s`, middle stretched if positive; height `sh*s`.

### 9.2 Text

`label(ctx, str, x, y, size, color='#fff', align='left', outline='#1b1f33')`: font `fontOf(size)`, baseline `middle`;
if outline: round joins, `lineWidth = max(2, size/5)`, stroke then fill. `textWidth(str, size)` measures with the same font.

### 9.3 Ribbons & banners (colour index rows; `assets.js:513-535`)

- `bigRibbon(colorIdx, cx, cy, w, s, str, size)`: `UIP+'Ribbons/BigRibbons.png'` (448×640), G7, row `y = colorIdx*128`, h 128; placed at `(cx - w/2, cy - 64s)`; label white centred at `(cx, cy - 2s)`.
- `smallRibbon(colorIdx, variant, …)`: `SmallRibbons.png` (320×640), G3, row `(colorIdx*2 + variant)*64`, h 64, at `cy - 33s`; label at `(cx, cy - 1)`. Castle name plate uses `smallRibbon(colorIdx, 0, b.x, top+4, 190, 0.8, f.name, 15)` (`entities.js:1464`).
- `swordBanner(colorIdx, …)`: `Swords/Swords.png` (448×640), G7, row `colorIdx*128`, at `cy - 64s`; label at `(cx + 16s, cy - 14s)`, colour `#4a3a33`, no outline.
- `storeRibbon(color, cx, cy, w, s, str, size)`: `STOREP+'Ribbons/Ribbon_{C}.png'`, G7, row 0 (h 128), at `cy - 68s`; label white at `(cx, cy)`. Used for menu titles `TINY SWORDS` / `MULTIGIOCATORE` (`ui.js:605,623`).
- `drawPennant(color, cx, y, s)`: from `Ribbon_{C}.png` src `(51,448,90,108)` → `(cx - 45s, y, 90s, 108s)`. Used for rally flags (s 0.45, at `rally.y - 50`), capture-point owner banner (s 0.5, at `top - 34`), menu colour choice.

### 9.4 Bars (gameplay-relevant overlays)

- `bigBar(x, y, w, ratio, s)`: base `BigBar_Base.png` G3 row 0 h 64, at `(x - 40s, y - 9s)`, width `w + 80s`; fill `BigBar_Fill.png` src `(0,22,64,21)` at `(x + 11s, y + 13s)`, width `(w - 22s)*clamp(ratio,0,1)`, height `21s`; skipped if width ≤ 0.5. Visible height 51s. Used for construction progress (`!done` and `w>1`: `bigBar(b.x-45, b.y-60, 90, progress/work, 0.4)`) and the selection panel HP.
- `smallBar(x, y, w, ratio, s, fill)`: base `SmallBar_Base.png` G3 row 0, at `(x - 49s, y - 22s)` width `w + 98s`; fill image src `(0,30,64,3)` at `(x + 10s, y + 8s)`, width `(w - 20s)*ratio`, height `6s`. Visible height 19s. `fill` = `Assets.barFill[color]` (faction-tinted).
  - Unit HP: shown when damaged or selected: `smallBar(u.x-24, u.y-def.barY, 48, hp/maxHp, 0.5, barFill[faction])`.
  - Building HP (done, damaged or selected): wide (`w>1`) `smallBar(b.x-44, top+18, 88, …, 0.7)` else `smallBar(b.x-22, top+18, 44, …, 0.5)`, `top = b.y - def.ay`.
  - Capture progress: `smallBar(p.x-44, top+22, 88, t/CAPTURE_TIME, 0.7, barFill[capturer])`.
- Sheltered pawns indicator: pawn avatar 34 px + label `'×' + n` (size 17, `#fff7c9`) at `(b.x+98, top-14)` for the castle else `(b.x-34, top-22)`.
- Rally line (own selected trainer building): dashed `[6,6]`, 2 px, `rgba(255,255,255,0.6)` from `(b.x, b.y1)` to rally point + pennant.
- Floating "+N" texts: icon 30 px + label size 18 `#fff7c9`, rise 28 px/s, life 1.2 s, alpha `clamp(1.6 - 1.4t, 0, 1)`.

### 9.5 Brackets, icons, thumbnails

- `drawBrackets(x, y, w, h, s)`: `Cursors/Cursor_04.png` (128×128) corners 32×32 at src `(0,0)`, `(96,0)`, `(0,96)`, `(96,96)` → drawn `32s` at the four corners. Selected building: `(x0-10, y0-14, w+20, h+28)` scale 1 (wide) / 0.7; selected resource: big `(x-74, y-80, 148, 148)`, small `(x-30, y-54, 60, 64, 0.7)`.
- `drawIcon(path, cx, cy, size)`: square `2*round(size/2)` centred.
- `drawBuildingThumb(color, kind, cx, cy, size)`: crops `def.crop`, fits `k = min(size/sw, size/sh)`, centred.

### 9.6 Icon specs — `drawSpec(ctx, spec, color, cx, cy, size)` (`assets.js:583-596`)

| spec | drawn as |
|---|---|
| `['ui', n]` | `ICON(n)`, size ×1.15 |
| `['x', name]` | `Extra/Icons/{name}.png`, size ×1.2 |
| `['res', kind]` | `RES_ICON[kind]`, size ×1.15 |
| `['unit', type]` | portrait at `(cx - 0.6size, cy - 0.62size)`, side `1.2size` |
| `['build', kind]` | building thumbnail in `size` |
| `['age', n]` | crown icon at `(cx, cy - 0.1size)` size ×1.1 + label `AGE_SHORT[n]` at `(cx, cy + 0.3size)`, size `0.46size`, colour `#ffe9a3`, centred |

---

## 10. HD-2D porting notes

- All sprite anchors above are pixel anchors for a 2D side view; when billboarding in Bevy, use pivot `(ax/fw, 1 - ay/fh)` and keep the
  world-to-sprite scale such that 64 sprite px ≈ 1 cell, so footprints (`w×h` cells) line up with voxel terrain blocks.
- Team colour is baked into separate PNG files per colour (no palette swap); only the small HP bar fill is runtime-tinted with `COLOR_CSS`.
- The sim crate needs from this chapter: all of §1 enums & orders, §2.1/§2.3/§2.4, the attack frame/fps table in §2.5, §3 gameplay
  columns + §3.1, §4 + constants, §5 + §5.1, §6, §7. Everything in §8–§9 is client-only.

## Open questions / ambiguities

1. Attack timing depends on sprite frame counts (§2.5). Proposal: keep these as constant data in the sim (do not derive from images at runtime).
2. `sheet()` caches by path with the first-call parameters; `Ram_Run`/`Catapult_Run` are requested with fps 0 (idle) **and** 8 (run) — because of the cache, whichever anim is requested first fixes the fps for both (in practice units spawn in `idle`, so the run animation of rams/catapults likely plays at **0 fps**, i.e. frozen). Same for any shared path. Decide whether to reproduce (probably not — treat it as a bug and animate run at 8 fps).
3. `fletching` description mentions towers/castle — this is implemented only through `mods.atk.archer` and `mods.range` being read by `entities.js:1209,1217`; keep that coupling. The sheet cache in item 2 is keyed per colour path, so the effect can differ per kingdom colour.
4. `shields` also halves **tower/castle** arrows (any projectile with `vx`), not only archer arrows; catapult rocks are not arrows.
5. The scout is trained at the castle but consumes a **troop** slot, not a pawn slot.

---

## Addenda (critic)

Completeness check: every top-level function/constant of `assets.js`/`extra.js` is referenced by some spec file
(checked by script). The open questions above are resolved here against the code.

**A1. Sprite-sheet cache (open question 2) — confirmed, and it is a real visible bug.**
`sheet(path, fw, ax, ay, fps)` (assets.js:80-88) caches by `path` only; the first call fixes `fw/ax/ay/fps`.
`UNIT_ANIM.ram/catapult` (assets.js:189-190) use the same file `Ram_Run`/`Catapult_Run` for `idle` (0 fps) and
`run` (8 fps). The first request for these files is always `idle`: units spawn idle (`spawnUnit`, entities.js:255
`anim:'idle'`) and portraits call `unitSheet(color, type, 'idle')` (assets.js:579, ui.js:584). So in practice the
ram/catapult **run animation is always frozen on frame 0** in the JS game.
Simulation impact: none. `tickAction` (entities.js:426-431) reads `sh.fps`/`sh.n` only of the *attack* sheet
(`Ram_Attack`/`Catapult_Attack`, distinct files, 7 fps), so hit timing is unaffected.
Decision for the port: animate run at 8 fps (the evident intent of the table); idle = frame 0 of run. Keep the
attack timing table as pure data in the sim (frames and fps per attack anim), independent of the art.

**A2. `shields` (open question 3) — exact rule.** `techMods`: `case 'shields': m.arrowRes = 0.5` (factions.js:90).
Applied in `hurt` (entities.js:494-497) only when the damaging projectile has `vx` defined, i.e. is an **arrow**
(`fireArrow` sets `vx/vy`, entities.js:444; rocks from `fireRock` have no `vx`, entities.js:452-455). Then:
`if t.type == 'ram' → ×0.2` (always, regardless of research) `else if t.def.block || t.type == 'knight' → ×tf.mods.arrowRes`.
`def.block` exists only for warrior (0.25) and lancer (0.2) (assets.js:129,135). The shooter does not matter:
archer arrows, tower arrows and castle arrows (`updateBuilding` uses `fireArrow`, entities.js:1219) are all halved
for warrior/lancer/knight of a faction with `shields`. Archers, monks, pawns, scouts, catapults never benefit.
Order inside `hurt`: arrow multiplier → `max(1, amount - unitArmor(t))` → block roll (×0.35). Buildings: only
`BUILDING_ARMOR`.

**A3. Scout slot (open question 4) — confirmed.** `roomFor(f,type)` (entities.js:1179) tests `type === 'pawn'`
only; every other type, scout included, uses `troopN < troopCap`. `census` (factions.js:204-208) puts scouts into
`f.soldiers` and counts them in `troopN`; `deploy` increments `troopN` (entities.js:1172). Consequences the port
must keep: the "Servono più accampamenti"/"Truppe al massimo" toasts apply to scouts; the HUD troop counter
(`f.soldiers.length`, ui.js:188) includes scouts and monks and siege; AI tests like `f.soldiers.length < 4`
(factions.js:857) count scouts too (AI never trains scouts, so only relevant after a human→AI takeover).

**A4. Helpers not described elsewhere (render-only, for completeness).**
- `stoneImg/stumpImg/goldImg/goldShine` (assets.js:418-427), `drawBigNode` (entities.js:1381-1384: big deposit image
  bottom-aligned at `World.cy(n.c) + TILE*1.5 - 6`), `SHEEP_SHEET` (entities.js:1395-1399: `Sheep_Idle`/`Sheep_Move`
  fw 128, anchor (64,82), 8 fps; `Sheep_Grass` 10 fps; frame = `floor(s.animT*fps)`), `BUSH_SHEET`/`drawBush`
  (entities.js:1443-1444: `Bushe{v+1}.png` fw 128 anchor (64,80) 8 fps, frame `floor(G.time*8 + d.phase)`),
  `drawRock` (entities.js:1445: `Rock{v+1}.png` at (x-32, y-50)). All [RENDER]; HD-2D billboards replace them.
- `avatarImg(color,type)` (assets.js:207-210): file `Human Avatars/Avatars_NN.png`, `NN = colorIdx*5 + def.avatar + 1`
  zero-padded to 2 digits.
