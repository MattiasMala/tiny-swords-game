# 05 — Controls, camera, fog of war, main loop

Porting spec for `js/control.js` (502 lines) and `js/main.js` (490 lines), plus the bits of `ui.js`, `panels.js`,
`light.js`, `net.js`, `factions.js`, `entities.js` they call into that matter for controls / loop behaviour.
Line references are `file:line` in the JS sources at commit `5d20a46`.

Everything in this file is **client-side** (presentation + local input) unless marked **[SIM]**. The client only
talks to the simulation through `command(cmd, ...args)` (`factions.js:425`): in solo/host it calls
`execCommand(G.player, ...)` synchronously and returns a refusal string or `null`; as a network client it sends
`{t:'c', c:cmd, a:args}` and **always returns `null`** (refusals then arrive as `toast` events from the host).

---

## 1. Constants

| Name | Value | Where | Meaning |
|---|---|---|---|
| `TILE` | 64 | assets.js:4 | world px per cell |
| `CAM_SPEED` | 1100 | control.js:5 | camera pan speed, **world px / second** (independent of zoom) = 17.19 cells/s |
| `CAM_MEDIUM` | 2 | control.js:6 | multiplier while Shift held |
| `CAM_FAST` | 4 | control.js:7 | multiplier while Ctrl held (wins over Shift) |
| `EDGE` | 8 | control.js:8 | CSS px from window border that triggers edge-scroll (× `G.dpr` → physical px) |
| `DRAG_MIN` | 8 | control.js:9 | CSS px (× dpr) of mouse travel after which a left-press becomes a box select |
| `START_SIGHT` | 15 | control.js:10 | radius (cells) revealed around own castle at match start |
| `MARK_COLOR` | move `#9be37a`, attack `#ff6a5a`, gather `#ffe27a`, rally `#ffffff` | control.js:11 | order marker colours |
| marker life | 0.6 s | control.js:361 | |
| ping life | 4 s | control.js:363 | minimap attack pings |
| double-click window | 0.35 s (on `G.clock`) | control.js:167 | |
| group double-tap window | 0.4 s (on `G.clock`) | control.js:332 | |
| fog recompute period | 0.2 s | control.js:54 | |
| fog fade rate | `k = min(1, dt*5)` | control.js:62 | exponential chase per frame |
| fog "explored" opacity | `Math.fround(0.42)` | control.js:63 | |
| fog colour | rgb(10,14,26) | control.js:35 | |
| `POINT_SIGHT` | 7 | entities.js:32 | cells seen around an owned capture point |
| `DEFENSE_RADIUS` / `BUILD_RADIUS` / `OUTPOST_RADIUS` | 12 / 40 / 9 cells | factions.js:9 | placement zones |
| `ZOOMS` | `0.5 + i*0.25`, i=0..16 → 0.5 … 4.5 | main.js:8 | allowed zoom values (physical px per sprite px) |
| fit reference | 1500 × 850 | main.js:150 | "design" viewport size used for default zoom and UI scale |
| UI scale `G.us` | `clamp(fit, 0.72*dpr, 1.5*dpr)` | main.js:152 | |
| camera bottom slack | `150 * G.us / G.zoom` world px | main.js:279 | extra scroll below the map (panel room) |
| `Quality.steps` | [1, 0.75, 0.5] | main.js:129 | render-resolution scale |
| perf watchdog | slow frame = dt > 0.03 s; trigger at accumulated 3 s; decay 2×dt; ignore dt > 0.25 | main.js:135-139, light.js:36-42 | |
| main-loop dt clamp | 0.05 s | main.js:463 | |
| host background clock | Worker `setInterval` 50 ms; only steps if no paint for ≥150 ms | net.js:155, main.js:481-486 | |
| `SNAP_DT` | 0.05 s | net.js:9 | host snapshot interval (20 Hz) |
| `FIELD_REFRESH` / `FIELD_NEW` | 2 / 2 per sim step | world.js:16-17 | per-step budgets set in `updateWorld` |
| toast duration | 2 s | main.js:36 | |
| end-of-match delay | 2.5 s (`G.endT`) | main.js:108 | before `state='over'` |
| demo restart delay | 8 s (`G.demoT`) | main.js:102 | |
| `TEST_GIFT` | 1000 | panels.js:388 | |
| `ALERT_EVERY` [SIM] | 12 s | entities.js:18 | host throttle for "under attack" alerts per faction |
| min wheel step | one zoom step per frame, sign only | control.js:376 | |

---

## 2. Global state touched here

### 2.1 `Input` (main.js:10-16) — raw per-frame input

| Field | Meaning |
|---|---|
| `keys: Set<code>` | KeyboardEvent.code values currently held |
| `hits: code[]` | codes pressed this frame (no auto-repeat), plus synthetic `'ZoomIn'`/`'ZoomOut'` |
| `navs: code[]` | NAV codes pressed this frame **including auto-repeats** |
| `mouse {x,y,inside}` | **physical** px inside canvas; `inside` false after `mouseleave` |
| `moved` | mouse moved > 2 physical px (Manhattan) since last frame |
| `lmb`, `rmb` | button held |
| `pressed`, `released` | left button went down / up this frame |
| `rpressed` | right button went down this frame (no `rreleased`) |
| `wheel` | sum of `sign(deltaY)` this frame |
| `downX/Y`, `rdownX/Y` | screen position of the last left / right press |

Per-frame flags `pressed, released, rpressed, moved, wheel` are reset in `finally` after update+render (main.js:475-476)
and after `stepUnseen` (only `pressed, released, rpressed`, main.js:485).

### 2.2 `G` (main.js:18-114), fields relevant here
`state` ('loading'|'menu'|'play'|'over'), `W,H` (canvas physical px), `fullW,fullH`, `zoom`, `zoomChosen`, `us`,
`viewW,viewH` (= W/zoom, H/zoom: view size in world px), `cam {x,y}` (world px of top-left), `dpr`,
`time` (sim time), `clock` (wall time, advances every update even when paused), `demo`, `paused`, `frozen`,
`spectate`, `placing`, `result` ('win'|'lose'|null), `endT`, `endTime`, `demoT`, `player` (local faction),
`toastT`, `toastText`, `banners[]`, `mouseWX/mouseWY` (mouse in world px), `splashT`, `clouds`, `waterFx`.

### 2.3 `Ctl` (control.js:80-96)

| Field | Meaning |
|---|---|
| `sel: Set<id>` | selected entity ids |
| `list: Entity[]` | resolved objects, rebuilt every frame by `refresh()` |
| `groups: {n: id[]}` | control groups 1..9 |
| `drag: {x,y}|null` | screen point where a potential box drag started |
| `mode: null|'amove'|'rally'|'pickmove'` | pending left-click action |
| `pickB` | building currently highlighted by arrows in `pickmove` |
| `enter` | Enter/Space pressed while placing (consumed by `place`) |
| `markers: {kind,x,y,t}[]` | ground order markers |
| `pings: {x,y,t}[]` | minimap attack pings |
| `alertAt {x,y}|null`, `alertT` | last attack location / `G.time` of it (init -99) |
| `lastPick {id,t}`, `lastGroup {n,t}` | for double-click / double-tap (init t = -9); **not** cleared by `reset()` |
| `idleI` | round-robin index for idle-pawn cycling |

`reset()` (control.js:93-96) clears sel and sets list=[], groups={}, drag=null, mode=null, pickB=null, enter=false,
markers=[], pings=[], alertAt=null, alertT=-99, idleI=0.

### 2.4 `G.placing` (control.js:214-215)
`{ kind, cx, cy, ok, move, last, kbd }` — `cx,cy` = top-left cell of footprint; `ok` = currently placeable;
`move` = building being relocated (or null for new construction); `last = [cx,cy]` = last cell placed during a wall
drag (or null); `kbd` = ghost driven by keyboard rather than mouse.

---

## 3. Raw input handling (main.js:178-216)

Browser listeners (port to Bevy input / web-sys; the semantics must stay):

* **keydown** (main.js:188-195): `Sfx.unlock()`. Zoom detection uses the **character** (`e.key`), not the physical
  key, because on the Italian keyboard + and − are elsewhere: `e.key === '+'` or (`e.key === '='` and `e.code === 'Equal'`)
  → `'ZoomIn'`; `e.key === '-'` → `'ZoomOut'`. `preventDefault()` if zoom key, any NAV code, or code in
  `Space, Tab, F1, F2, F3, F4, Backspace, Enter` (so Ctrl+W/A/S/D/+/− don't trigger browser actions; Ctrl+W itself
  cannot be blocked → see §15). If `!e.repeat`: push `e.code` to `hits`, and also the synthetic zoom code.
  If NAV code: push to `navs` (repeats included). Add code to `keys`.
* **keyup**: remove code from `keys`.
* **window blur**: `keys.clear()`, `lmb = rmb = false`.
* **mousemove** on canvas → `mouseAt(e)`: `x = clientX*dpr, y = clientY*dpr`; if `|dx|+|dy| > 2` → `moved = true`,
  `UI.kbd = false` (mouse takes over from keyboard focus). Set `mouse.x,y`, `inside = true`.
* **mouseleave**: `inside = false` (disables edge scroll and cursor drawing).
* **mousedown** on canvas: `Sfx.unlock()`, `mouseAt(e)`; button 0 → `lmb=true, pressed=true, downX/Y = mouse`;
  button 2 → `rmb=true, rpressed=true, rdownX/Y = mouse`.
* **mouseup** on **window**: button 0 → `lmb=false, released=true`; button 2 → `rmb=false`.
* **wheel** on canvas: `preventDefault`, `wheel += sign(deltaY)`.
* **contextmenu**: `preventDefault`.

`NAV` (main.js:221-224): `ArrowLeft/KeyA = [-1,0]`, `ArrowRight/KeyD = [1,0]`, `ArrowUp/KeyW = [0,-1]`, `ArrowDown/KeyS = [0,1]`.

---

## 4. Main loop (main.js:333-345, 461-490)

### 4.1 Frame driver
```
frame(now):                         // requestAnimationFrame
  requestAnimationFrame(frame)      // first, so an exception never stops the loop
  if lastPaint: Light.watch((now-lastPaint)/1000); Quality.watch(same)   // real, UNclamped frame time
  lastPaint = now
  try { step(now); render() }
  finally { pressed = released = rpressed = moved = false; wheel = 0 }

step(now):
  dt = min(0.05, (now - lastFrame)/1000 || 0)     // first frame → 0
  lastFrame = now
  update(dt)

stepUnseen():                       // host only, called by a Worker every 50 ms (net.js:155-156)
  if performance.now() - lastPaint < 150: return
  step(now); pressed = released = rpressed = false   // no render
```
**There is no fixed timestep**: the sim receives the variable, clamped frame dt (≤ 0.05 s, i.e. effectively 60 Hz
steps on a normal display, 20 Hz steps when the host tab is hidden). The Rust sim will run at a fixed tick; see §17.

### 4.2 `update(dt)` order (main.js:333-345)
```
G.clock += dt
handleNav()          // §5.2
handleKeys()         // §5.3
Input.hits.length = 0
if Net.remote:                 Net.clientUpdate(dt)       // interpolation + updateView (net.js:313-330)
elif G.state=='menu' or Net.hosting or (G.state=='play' and !G.paused and !UI.help):
    if G.frozen: updateView(dt) else updateWorld(dt)
elif G.state != 'loading':     Ctl.refresh()
if Net.hosting: Net.hostTick(dt)                         // snapshot every SNAP_DT
```
Consequences (must be preserved):
* Solo + paused, solo + help open, solo + `state=='over'`: **nothing advances**, not even the camera (Ctl.update is
  not called); only the selection list is refreshed.
* Network (host or client): the match never stops — pause and help are just overlays; `Ctl.update` keeps running,
  so the camera still pans (keys and edge) while the pause menu is open.
* Menu (demo battle behind the menu) always simulates.
* Frozen (test mode): `updateView` only → camera, input/orders, fog, clouds, banners, toast run; sim time does not.

### 4.3 `updateWorld(dt)` [SIM + view] (main.js:290-307)
```
G.time += dt
World.refresh = FIELD_REFRESH (2); World.fresh = FIELD_NEW (2)     // per-step budgets
for f in factions: updateFaction(f, dt)
for u in units if !dead: updateUnit(u, dt)
separateUnits()
units = units.filter(!dead)
for b in buildings if !dead: updateBuilding(b, dt)
buildings = buildings.filter(!dead)
updateResources(dt); updatePoints(dt); updateArrows(dt); updatePickups(dt); updateFx(dt)
updateView(dt)                                   // client-side, see below
if G.demo and G.demoT > 0 and (G.demoT -= dt) <= 0: G.start({players: []})   // new demo battle
```
In the Rust split: everything up to `updatePickups` is the pure sim tick; `updateFx` and `updateView` are client.

### 4.4 `updateView(dt)` [client] (main.js:310-331)
```
if G.demo:   // menu background camera: slow Lissajous drift
  cam.x = (mw-viewW)/2 + cos(G.clock*0.04)  * max(0, (mw-viewW)/2 - 150)
  cam.y = (mh-viewH)/2 + sin(G.clock*0.055) * max(0, (mh-viewH)/2 - 150)
  updateMouse()
else Ctl.update(dt)            // §6-§11
Fog.update(dt)                 // §12
clouds: c.x += c.v*dt; if c.x > mw+100: c.x = -600; c.y = rand(-100, mh-150)
waterFx: t += dt; drop when t*fps >= n frames
if (G.splashT -= dt) <= 0: G.splashT = rand(0.4,1.2); c = random World.openWater cell; push splash fx at its centre
banners: if any, banners[0].t += dt; shift when t >= dur
G.toastT -= dt
if G.result and state=='play' and !spectate and (G.endT -= dt) <= 0: G.state = 'over'
```
(mw, mh = World.W*TILE, World.H*TILE.) Clouds are created in `G.scenery` (main.js:61-63): count
`round(8 * W*H / MAP_AREA)` with `MAP_AREA = 144*96` (→ 32 on 288×192), image `Clouds_0{i%8+1}.png`,
`x = rand(-300, mw)`, `y = rand(-100, mh-150)`, `v = rand(7,16)` px/s; drawn at alpha 0.6.

### 4.5 `render()` order (main.js:354-457) — reference for the HD-2D renderer
1. `state=='loading'` → loading screen only: background `#47aba9`, title `Tiny Swords`, bar (bg `#1b1f33`, fill
   `#e3c84e`, width `min(420*dpr, W*0.6)`), text `Caricamento sprite ${loaded}/${total}`; on error the error text.
2. `UI.begin()`; `renderWorld()`:
   1. world transform (scale zoom, translate `-round(cam*zoom)`), water fill `#47aba9`;
   2. animated water foam on `World.foam` cells, water rocks, water splash fx, rubber ducks
      (`r = pond?18:46; a = t*0.3+phase; x = d.x+sin(a)*r; y = d.y+cos(a*0.8)*r*0.4+sin(t*2+phase)*1.5; flip if cos(a)<0`);
   3. `World.drawGround`; `Light.tick()`;
   4. collect visible entities with culling margins (world px): decor 100, trees 260, golds 90, stones 110,
      points 260, sheep 90 (**and** `Fog.sees`), buildings 340 (own **or** `Fog.knows(b.x, b.y-20)`), units 200
      (not `inside`, own **or** `Fog.sees`), pickups 80 (`Fog.knows`), fx 200 (`Fog.sees`); sort by `sy` (y-sort);
   5. `Light.shadows`, draw entities, `drawArrows` (each arrow only if `Fog.sees`), clouds (alpha 0.6, culled at
      centre `(x+288, y+128)` margin 320), `Light.apply`;
   6. fog overlay: the 1-px-per-cell fog texture stretched over the whole map with **bilinear smoothing** (soft edges);
   7. `Light.post` in screen space (bloom/tilt-shift/vignette — only affects the world, not overlays);
   8. `drawOverlays` (HP bars, castle ribbon, sheltered-pawn count, rally line/pennant, selection brackets, floats),
      then `Ctl.drawWorld` (not in demo) — markers, pickmove brackets, placement ghost + range rings (§9.6).
3. Menu: `drawMenu`. Otherwise: `UI.locked = paused || help || state=='over' || !!UI.panel`; spectator HUD or HUD;
   `UI.locked=false`; `Ctl.drawScreen` (drag rectangle); then `drawGameOver` if over, else `drawPause` if paused, else
   `drawManage` if a panel is open (and not spectating).
4. Help overlay if `UI.help`; keyboard focus brackets; `UI.end()`; custom cursor last.

Note: trees, golds, stones, decor and points are drawn regardless of fog; the fog overlay (opacity 1 on unexplored
cells) hides them. Under explored-but-not-visible fog (0.42) their **current** state shows (see §12.4).

---

## 5. Keyboard

### 5.1 Complete binding table

| Key (code) | Context | Effect | Ref |
|---|---|---|---|
| `KeyM` | always (any state) | toggle `Sfx.enabled` | main.js:242 |
| `F1` | always | toggle `UI.help` (help overlay; in solo it pauses the sim) | main.js:243 |
| `F2` | always | `Light.cycle()` (§14.1) | main.js:244 |
| `F3` | always | `Quality.cycle()` (§14.2) | main.js:245 |
| `Escape` | not playing or result set | close help only | main.js:246 |
| `Escape` | playing | cascade: help → panel (`'send'`→`'units'`, else close) → cancel placing → cancel `Ctl.mode` → clear selection (only if not paused) → toggle pause | main.js:247-254 |
| `KeyP` | playing, no result | toggle `G.paused` (in network: opens/closes the "MENU" overlay, game continues) | main.js:255 |
| *(everything below is skipped while paused or help is open)* | | | main.js:256 |
| `F4` | playing | `Net.playing` → toast `La modalità test c'è solo nelle partite da soli` + deny sound; else `UI.open('test')` (toggles) | main.js:257-259 |
| `KeyU/KeyL/KeyI/KeyK/KeyC` | playing (also while a panel is open) | `UI.open('units'/'workers'/'tree'/'market'/'build')` — toggles; switching closes placing & mode | panels.js:9-14,19-23 |
| `KeyV` | playing, no panel | `Ctl.moveMode()` (§10); refusal string → toast + deny | panels.js:15 |
| `Tab` | playing, no panel | `Ctl.selectArmy()` | panels.js:16 |
| `Backspace` | playing, no panel | `Ctl.recall()` | panels.js:17 |
| *(below: ignored while any panel is open)* | | | main.js:263 |
| `ZoomIn`/`ZoomOut` (+ / −) | | `setZoom(±1, G.W/2, G.H/2)` — around screen centre | main.js:264 |
| `Period` (`.`) | | `Ctl.nextIdle()` | main.js:265 |
| `KeyH` | | `Ctl.home()` | main.js:266 |
| `Space` | not placing and mode ≠ pickmove | `Ctl.centerOnSelection()` (while placing Space = confirm, see handleNav) | main.js:267 |
| `Digit1..Digit9` | | `Ctl.group(n, shiftHeld)` — Shift = assign, else recall | main.js:268 |
| `KeyX`,`KeyE`,`KeyF`,`KeyG`,`Delete` | | first context action (§8) whose `code` matches → `Ctl.act(a)` | main.js:270-271 |
| Arrows / WASD | modal (menu, over, paused, help, panel) | `UI.navigate(dx,dy)` (button focus; on the Send map moves the target by 3 cells) | main.js:231 |
| Arrows / WASD | placing or pickmove | `Ctl.keyMove(dx,dy)` (repeat-enabled) | main.js:232 |
| Arrows / WASD (held) | otherwise | camera pan (§11.1) | control.js:367-368 |
| `Enter`/`NumpadEnter`/`Space` | modal | `UI.press()` | main.js:235 |
| `Enter`/`NumpadEnter`/`Space` | placing or pickmove | `Ctl.keyConfirm()` | main.js:236 |
| Shift (held) | | camera ×2; add-to-selection on click/box; keep placing; wall drag line; group assign; remove unit from group in selection panel | |
| Ctrl (held) | | camera ×4 | control.js:374 |

Notes:
* The code comment in `Ctl.groups` says "Ctrl + numero", but the code (and README) uses **Shift + digit** to assign.
* `KeyV` context action in the selection panel has no `code` (only `key:'V'`) — the keyboard V always goes through the
  SHORTCUTS entry → `moveMode()`, which itself uses the selected building if exactly one own building is selected.
* WASD letters are both camera and NAV. `KeyE` (explore), `KeyF`, `KeyX`, `KeyG` do not collide with WASD.

### 5.2 `handleNav()` (main.js:225-237)
```
modal   = state=='menu' or state=='over' or G.paused or UI.help or UI.panel != null
placing = !modal and state=='play' and !spectate and !result and (G.placing or Ctl.mode=='pickmove')
enter   = hits contains 'Enter' or 'NumpadEnter' or 'Space'
if !modal: UI.kbd = false
for code in navs: if modal: UI.navigate(NAV[code]) elif placing: Ctl.keyMove(NAV[code])
navs.clear()
if enter and modal: UI.press() elif enter and placing: Ctl.keyConfirm()
```
### 5.3 `handleKeys()` (main.js:239-274)
Iterates `Input.hits` in order with `shift` = Shift held at that moment; logic exactly as table §5.1, in this order:
M, F1, F2, F3 → (if `state!='play' || result`: Escape closes help; continue) → Escape cascade → P →
(if paused or help: continue) → F4 → SHORTCUTS (panel ones always; run ones only without panel) →
(if panel: continue) → zoom / Period / H / Space / digits / context actions.

---

## 6. Per-frame controller: `Ctl.update(dt)` (control.js:357-407)

Called from `updateView` when not in demo. `free = !UI.hot && !UI.locked` (UI.hot = mouse was over HUD in the
previous rendered frame; UI.locked is false during update), `m = Input.mouse`, `k = Input.keys`.
```
refresh()                                       // §7.1
markers: t += dt, keep t < 0.6;  pings: t += dt, keep t < 4
// camera §11.1
keysAllowed = !UI.panel and !G.placing and Ctl.mode != 'pickmove'
mx = keysAllowed ? held(Right|D) - held(Left|A) : 0
my = keysAllowed ? held(Down|S)  - held(Up|W)   : 0
if m.inside and !drag and document.hasFocus():
   edge = EDGE*dpr
   if m.x < edge: mx = -1 elif m.x > G.W - edge: mx = 1
   if m.y < edge: my = -1 elif m.y > G.H - edge: my = 1
speed = CAM_SPEED * (ctrl ? 4 : shift ? 2 : 1)
cam += (mx, my) * speed * dt                    // diagonal is NOT normalised (×√2)
if Input.wheel and !UI.hot: setZoom(wheel < 0 ? +1 : -1, m.x, m.y)   // wheel up = zoom in, one step/frame
clampCamera(); updateMouse()
if spectate or !player or !player.alive or state!='play' or paused or UI.help: drag = null; return
shift = Shift held; lx,ly = cam + (downX,downY)/zoom      // orders go where the button was PRESSED
if G.placing: place(shift, free, lx, ly); return           // §9 — no selection/orders while placing
if Input.rpressed and free:
   rx,ry = cam + (rdownX, rdownY)/zoom
   b = single selected own building with def.trains (else null)
   if Ctl.mode: Ctl.mode = null                  // right-click cancels a pending mode, no order
   elif b and b.done: setRally(b, rx, ry)
   else: order(rx, ry)
if Input.pressed and free:
   if mode=='amove':  order(lx, ly, true); mode = null
   elif mode=='rally': if list[0] is building: setRally(list[0], lx, ly); mode = null
   elif mode=='pickmove':
        t = pick(lx, ly)
        why = (t is own building) ? startMoving(t) : 'Clicca un tuo edificio'
        if why: toast(why); play(why ? 'deny' : 'click')     // mode stays 'pickmove' on failure
   else: drag = {x: downX, y: downY}
if drag and Input.released:
   d = drag; drag = null
   if hypot(m - d) > DRAG_MIN*dpr: boxSelect(d.x, d.y, m.x, m.y, shift)
   else: clickSelect(lx, ly, shift)
```
`updateMouse()` (main.js:284-287): `mouseWX = cam.x + mouse.x/zoom`, `mouseWY = cam.y + mouse.y/zoom`.
Note: `lx/ly` are computed with the **current** camera, so if the camera scrolled between press and frame the point
shifts slightly; ignore in port (use the world point at press time — strictly better, equivalent intent).

---

## 7. Selection

### 7.1 `select(ids, add=false)` / `refresh()` / `mine()` (control.js:98-111)
* `select`: if `!add` clear; add ids; **`mode = null`**; `refresh()`.
* `refresh`: `list` = resolved entities for each id in `sel`; an id is **removed from `sel`** if the entity is
  missing from `G.index`, `dead`, or is a resource node (`NODE_RES[kind]`: tree/gold/stone/sheep) that is no longer
  valid (`nodeValid`: tree → wood > 0; sheep → !dead; gold/stone → amount > 0).
* `mine()`: `list` filtered to units (`isUnit`) of `G.player`.

### 7.2 `pick(wx, wy)` — what is under the mouse (control.js:126-149)
Priority classes, first class with a hit wins; **within units, buildings and trees the hit with the largest `y`
(front-most) wins**; within the other classes the first in list order wins.
1. **Units**: skip `dead`, `inside` (sheltered), and enemy units not `Fog.sees(u.x,u.y)`.
   Half-width `wide = (def.siege || type=='knight') ? 40 : 24`. Hit if `|wx-u.x| < wide` and
   `u.y - def.barY + 8 < wy < u.y + 10`.
2. **Buildings**: skip dead and those with `!Fog.knows(b.x, b.y-20)`. `top = def.w > 1 ? b.y - def.ay*0.85 : b.y0 - 6`.
   Hit if `b.x0-4 < wx < b.x1+4` and `top < wy < b.y1`.
3. **Capture points**: hit if `p.x0-4 < wx < p.x1+4`, `p.y - def.ay*0.85 < wy < p.y1`, and `Fog.knows(p.x, p.y-TILE)`.
4. **Sheep**: not dead, `|wx-s.x| < 26`, `s.y-44 < wy < s.y+8`, `Fog.sees(s.x,s.y)`.
5. **Gold then stone nodes**: `r = big ? 68 : 36`; `|wx-n.x| < r` and `|wy-n.y| < r`; `Fog.knows(n.x,n.y)`.
   (Exhausted nodes are not excluded here; they are filtered by `refresh` after selection.)
6. **Trees** with `wood > 0`: `|wx-t.x| < 30`, `t.y-100 < wy < t.y+8`, `Fog.knows`.

(x0,x1,y0,y1 = building/point footprint rect in world px; `barY`, `ay` from defs.) HD-2D port: these are
sprite-space hitboxes (the sprite extends upward from the foot point `x,y`). Reproduce them as billboard-space
rectangles relative to the foot position, keeping the priority order and the front-most rule.

### 7.3 `clickSelect(wx, wy, add)` (control.js:163-175)
```
e = pick(wx, wy); now = G.clock
if !e: if !add: select([]); return              // shift-click on nothing keeps selection; no sound
own = e.isUnit and e.faction == player
if own and lastPick.id == e.id and now - lastPick.t < 0.35:      // double-click
    select(all own units not dead, not inside, same type, onScreen(u.x,u.y,0)), add)
elif add and own and every entity in list is an own unit:          // shift-click toggles
    toggle e.id in sel; refresh()
else: select([e.id])                            // replaces (even with shift if mixed)
lastPick = {id: e.id, t: now}
Sfx.play('select')
```
`onScreen(x,y,m)` (entities.js:44-47): inside view rect expanded by `m`; **returns true always when hosting a network
match** (quirk: on the host a double-click selects that type on the whole map). Port: decide whether to keep
(see §18).

Any entity can be selected (own/enemy units and buildings, points, sheep, nodes, trees) — only own units receive orders.

### 7.4 `boxSelect(x0,y0,x1,y1, add)` (control.js:151-161) — screen coordinates
```
ids = own units, not dead, not inside, whose screen point ((u.x-cam.x)*zoom, (u.y-26-cam.y)*zoom)
      lies inside the normalised rectangle
army = ids filtered to type != 'pawn'
if ids.length or !add:
   select( (army.length and army.length < ids.length) ? army : ids, add )
```
I.e. mixed pawns+soldiers → soldiers only; empty box without Shift deselects all; empty box with Shift does nothing.
Box only selects own units (never buildings or enemies). HD-2D: project the point 26 px (≈0.4 cell) above the foot.

Drag rectangle drawing (control.js:495-501): only once travel > `DRAG_MIN*dpr`; fill `rgba(255,255,255,0.12)`,
stroke `#ffffff` width `round(2*dpr)`.

### 7.5 Selection panel interactions (ui.js:353-363)
Clicking a portrait in the multi-unit panel: without Shift → `select([id])`; with Shift → remove that unit from
`sel` and `refresh()`. Portrait grid shows up to `cols*3-1` units then `+N`.

---

## 8. Right-click orders and context actions

### 8.1 `order(wx, wy, forceAttackMove=false)` (control.js:178-204)
```
own = mine(); if empty: return
t = pick(wx, wy)
pawns = own where type=='pawn'; army = own where def.dmg truthy; monks = own where type=='monk'
except(g) = own minus g
send(units, kind, target) = if units non-empty:
     command('order', ids, kind, round(wx), round(wy), target ? target.id : 0)
mark = 'move'
if t and (t.isUnit or t.isBuilding) and t.faction != player:      // enemy
    band = army ∪ monks          // monks follow attackers instead of walking onto the target
    send(band, 'attack', t); send(except(band), 'move')
    mark = army non-empty ? 'attack' : 'move'
elif forceAttackMove:            // X then left click
    send(own, 'amove'); mark = 'attack'
elif t and NODE_RES[t.kind]:     // tree / gold / stone / sheep
    send(pawns, 'gather', t); send(except(pawns), 'move')
    mark = pawns non-empty ? 'gather' : 'move'
elif t and t.isBuilding:         // own building
    work = !t.done or t.hp < t.maxHp
    send(pawns, work ? 'build' : (t.def.drop ? 'drop' : 'move'), t); send(except(pawns), 'move')
    mark = (pawns non-empty and work) ? 'gather' : 'move'
elif t and t.isUnit and t.hp < t.maxHp and monks non-empty:        // wounded own unit
    send(monks, 'heal', t); send(except(monks), 'move')            // mark stays 'move'
else: send(own, 'move')
mark(mark, wx, wy); Sfx.play('ack')
```
Note: forceAttackMove is checked **after** the enemy-target case (X + click on an enemy = normal attack).
Capture points are neither units nor buildings → plain move. Units with `dmg`: warrior, lancer, archer, knight, ram,
catapult (not pawn, scout, monk). Order kinds sent from here: `attack, move, amove, gather, build, drop, heal`; from
actions: `explore, stop`; plus `recall` command. Sim side caps id arrays at 200 (factions.js:340).

### 8.2 `setRally(b, wx, wy)` (control.js:206-212)
```
t = pick(wx, wy)
ok = t and (NODE_RES[t.kind] or (t.isBuilding and t.faction==player and !t.done))
command('rally', b.id, round(wx), round(wy), ok ? t.id : 0)
b.rally = {x: wx, y: wy, t: ok ? t : null}       // immediate local feedback, also on clients
mark('rally', wx, wy); Sfx.play('ack')
```
Rally is set by right-click when exactly one own **finished** building with `def.trains` is selected (castle,
barracks, archery, stable, monastery, siege), or via G + left click. Right-click with an unfinished trainer selected
falls through to `order()` which does nothing (no own units).

Rally visual (entities.js:1466-1471): when such a building is selected, dashed line `[6,6]` `rgba(255,255,255,0.6)`
width 2 from `(b.x, b.y1)` to rally point, and a pennant at `(rally.x, rally.y-50)` scale 0.45.

### 8.3 Context actions `actions()` (control.js:281-308)
Each `{code, key, icon, title, desc, on, run}`; shown in the selection panel (right side, ui.js:256-266) and triggered
by key `code`. `act(a)` (control.js:309-313): `why = a.run() || null`; if why → toast(why); sound deny if why else click.

If the selection contains own units (`mine()` non-empty):
1. if any has `dmg`: `KeyX` / `X` — **Attacco in marcia**, `on: mode=='amove'`, run: `mode='amove'`.
   desc: `Poi clic sul terreno: le unità vanno lì in formazione, al passo del più lento, attaccando chi incontrano; poi proseguono da sole sui nemici vicini.`
2. if any `def.scout`: `KeyE` / `E` — **Esplora**, run: `command('order', scoutIds, 'explore', 0, 0, 0)`.
   desc: `L'Esploratore va da solo, una dopo l'altra, nelle zone mai viste più vicine, finché ce ne sono.`
3. always: `KeyF` / `F` — **Ferma**, desc `Annulla l'ordine in corso.`, run: `command('order', ownIds, 'stop', 0, 0, 0)`.
→ return (no building actions when units are selected).

Else if exactly one own building `b` is selected:
1. if `b.done && b.def.trains`: `KeyG` / `G` — **Punto di raccolta**, `on: mode=='rally'`, run: `mode='rally'`.
   desc: `Poi clic sul terreno o su una risorsa (anche col tasto destro, direttamente).`
2. if `b.kind != 'castle'`:
   * (no code) / `V` — **Sposta**, desc `Scegli la nuova casella col mouse o con le frecce (Invio per posare), dentro il raggio in cui si può costruire.`, run: `startMoving(b)`.
   * `Delete` / `Canc` — title `b.done ? 'Demolisci' : 'Annulla cantiere'`, desc
     `b.done ? 'Abbatte l\'edificio, senza rimborso.' : 'Rimuove il cantiere e restituisce le risorse.'`,
     run: `command('demolish', b.id); select([])`. (Sim: unfinished → full refund; castle refused with
     `Il castello non si demolisce`.)

Icons (for the asset mapping): X `['ui',5]`, E `['x','eye']`, F `['x','stop']`, G `['x','flag']`, V `['x','barrow']`,
Delete `['ui',9]`.

### 8.4 Cursor (control.js:449-466; drawn ui.js:653-662)
```
if placing: return placing.ok ? 'arrow' : 'no'
if mode=='amove': 'attack'
if mode=='pickmove': b = UI.hot ? null : pick(mouseW); return (b own building, not castle) ? 'hand' : 'arrow'
own = mine(); if none or UI.hot: 'arrow'
t = pick(mouseW); pawn = any own pawn
if !t: 'arrow'
if t is enemy unit/building: any own has dmg ? 'attack' : 'arrow'
if pawn and t.kind=='tree': 'axe'
if pawn and t.kind in (gold, stone): 'pick'
if pawn and t.kind=='sheep': 'knife'
if pawn and t.isBuilding and (!t.done or t.hp<t.maxHp): 'hammer'
'arrow'
```
Applied only when `state=='play' && !paused && !help && !UI._hot` (ui.js:479); UI sets `'hand'` over buttons.
Sprites: arrow `Cursors/Cursor_01.png` (offset −23,−18 ×s), hand `Cursor_02.png` (−27,−19), no `Cursor_03.png` (−32,−31),
size `64*s` with `s = max(dpr, us)`; attack = icon `ICON(5)` at +10,+10 size 46s; axe `TOOL(2)`, pick `TOOL(4)`,
knife `TOOL(3)`, hammer `TOOL(1)` at +6,+6 size 96s. Not drawn when mouse outside canvas.

---

## 9. Placement mode (new building)

### 9.1 Entry: `startPlacing(kind)` (control.js:216-221)
Called by the Build panel (panels.js:346, after closing the panel). Ghost starts centred on the view:
`cx = round((cam.x + viewW/2)/TILE - w/2)`, `cy = round((cam.y + viewH/2)/TILE - h/2)`;
`{ok:false, move:null, last:null, kbd: UI.kbd}` (keyboard-chosen → ghost driven by arrows); `mode = null`.

### 9.2 `place(shift, free, lx, ly)` each frame (control.js:412-446)
```
p = G.placing; d = BUILD_DEF[p.kind]; f = player
click = Input.pressed and free; key = Ctl.enter; press = click or key; Ctl.enter = false
if p.move and p.move.dead: G.placing = null; return
if Input.moved or click: p.kbd = false                  // mouse takes the ghost back
if !p.kbd:
    p.cx = round((click ? lx : mouseWX)/TILE - d.w/2)
    p.cy = round((click ? ly : mouseWY)/TILE - d.h/2)
seen(cx,cy) = Fog.knows((cx + d.w/2)*TILE, (cy + d.h/2)*TILE)     // footprint centre explored
why(cx,cy) =
    p.move ? moveBlock(f, p.move, cx, cy)
    : !canAfford(f, d.cost) ? 'Risorse insufficienti'
    : zoneBlock(f, p.kind, cx, cy) || ((canPlace(p.kind, cx, cy) and seen(cx,cy)) ? null : 'Qui non si può costruire')
no = why(p.cx, p.cy); p.ok = (no == null)
if Input.rpressed: G.placing = null; return             // right-click cancels (even over HUD)
if press:
    refused = no || (p.move ? command('move', p.move.id, p.cx, p.cy)
                            : command('place', p.kind, p.cx, p.cy, builders(p.kind, p.cx, p.cy)))
    if refused: Sfx.play('deny'); toast(refused); return            // ghost stays
    Sfx.play('click'); p.last = [p.cx, p.cy]
    if p.move or !shift: G.placing = null                          // Shift keeps placing more
    return
// palisade drag line (release frame still counts)
if p.kind=='wall' and !p.move and shift and free and p.last and (Input.lmb or Input.released)
   and (p.last != [p.cx,p.cy]):
    [x0,y0] = p.last; n = max(|p.cx-x0|, |p.cy-y0|)               // Chebyshev length
    mineSel = any selected own pawn
    for i in 1..n:
        cx = round(x0 + (p.cx-x0)*i/n); cy = round(y0 + (p.cy-y0)*i/n)
        if why(cx,cy) == null: command('place','wall',cx,cy, mineSel ? builders('wall',cx,cy) : [])
    p.last = [p.cx, p.cy]
if !Input.lmb: p.last = null
```
Helper rules (factions.js:147-176, documented in the sim spec): `canPlace` — all footprint cells inside, passable (or
own cells when moving), not stairs, not `noBuild`, all on the same terrain `level`. `zoneBlock` — footprint centre
within `DEFENSE_RADIUS` (towers/walls: `def.defense`) or `BUILD_RADIUS` cells of the castle footprint centre, or within
`OUTPOST_RADIUS` of an owned point with `def.zone` (centre `p.gx,p.gy` in cells); message
`` `${d.defense ? 'Torri e palizzate' : 'Troppo lontano'}: entro ${r} caselle dal castello o ${OUTPOST_RADIUS} da un tuo avamposto` ``.
`moveBlock` — `'Il castello non si sposta'` / zoneBlock / `'Qui non può stare'`. The sim's own `placeBlock` also
checks the age lock (client check does not; the sim refusal comes back as the string).
Client-only extra rule: the footprint centre must be **explored** (`Fog.knows`) for new buildings (not for moves).
Not checked client-side in the wall line: cumulative cost (the sim refuses individually when money runs out).

Sounds: `'deny'`, `'click'`. Rendering-only: none of this changes sim state except via commands.

### 9.3 Builder choice `builders(kind, cx, cy)` (control.js:268-277)
```
sel = selected own pawns; if non-empty: return their ids
x,y = footprint centre in px
pawns = all own live pawns
if kind=='wall' and any pawn with act==6 (Costruisce) within 600 px of (x,y): return []   // they chain to next site
free = pawns with act in {0 (Inattivo), 8 (Porta il carico)} sorted by distance to (x,y)
busy = pawns with act in {3,4,5,11} (gathering wood/gold/meat/stone, RES_ACT panels.js:28) sorted by distance
return (free ++ busy).take(d.work > 15 ? 2 : 1).ids
```
(Sim: a gatherer sent to build returns to its gathering when done.) `work` values: e.g. house 9, camp 12, depot 12,
farm 14, barracks 18, wall 3, tower 16, monastery/siege 22 — see the defs spec.

### 9.4 Keyboard ghost: `keyMove(dx,dy)` while placing (control.js:237-246)
```
p.kbd = true
p.cx = clamp(p.cx+dx, 0, World.W - d.w); p.cy = clamp(p.cy+dy, 0, World.H - d.h)
x,y = footprint centre px; mx = viewW*0.3; my = viewH*0.3
if x < cam.x+mx or x > cam.x+viewW-mx or y < cam.y+my or y > cam.y+viewH-my: lookAt(x, y)   // recentre
```
Enter/NumpadEnter/Space → `keyConfirm()` → `Ctl.enter = true` → handled by `place()` in the same frame.
Camera keys are disabled while placing (arrows/WASD move the ghost instead); edge scroll still works.

### 9.5 Hints shown above the selection panel while placing / in a mode (ui.js:242-254)
Exact strings:
* moving: `Clic, Invio o Spazio: sposta qui  ·  Frecce o WASD: muovi la sagoma  ·  Destro / Esc: annulla`
* wall: `Clic, Invio o Spazio: piazza  ·  Maiusc + trascina: una fila  ·  Frecce o WASD: muovi la sagoma  ·  Destro / Esc: annulla`
* other: `Clic, Invio o Spazio: piazza (Maiusc: più d'uno)  ·  Frecce o WASD: muovi la sagoma  ·  Destro / Esc: annulla`
* amove: `Clic sul punto da attaccare  ·  Esc: annulla`
* rally: `Clic sul punto di raccolta  ·  Esc: annulla`
* pickmove: `Clic sull'edificio da spostare, oppure Frecce (o WASD) e Invio  ·  Esc: annulla`
Otherwise, if `toastT > 0`, the toast text at alpha `clamp(toastT*2, 0, 1)`, colour `#ffd9cf`. Hint colour `#fff`.

### 9.6 Ghost and range circles (control.js:476-493)
* Range rings: dashed `[14,10]`, `rgba(255,255,255,0.75)`, width 3 (world px). Around the castle footprint centre
  (`c.cx + w/2, c.cy + h/2` cells) with radius `DEFENSE_RADIUS` (12) if the placed kind has `def.defense`
  (tower, wall) else `BUILD_RADIUS` (40) cells; plus around each own capture point with `def.zone` (outposts) at
  `(p.gx, p.gy)` with `OUTPOST_RADIUS` (9). Shown both for new placement and for moving.
* Footprint rectangle fill: ok `rgba(120,255,140,0.28)`, blocked `rgba(255,70,60,0.38)`.
* Building sprite (player colour; variant of the moved building or 0) at alpha 0.65, anchored at
  `(x0 + w/2 - d.ax, y0 + h - 4 - d.ay)`; corner brackets `(x0-8, y0-8, w+16, h+16)`, scale `d.w>1 ? 1 : 0.6`.
* HD-2D: render the ghost as a translucent billboard over a tinted footprint on the voxel ground, rings as dashed
  ground decals.

---

## 10. Building move mode (V)

* `moveMode()` (control.js:223-228): if exactly one **own** building is selected → `startMoving(b)` (returns
  `'Il castello non si sposta'` for the castle → toast + deny). Otherwise `G.placing = null; mode = 'pickmove';
  pickB = null`.
* `startMoving(b, kbd=false)` (control.js:229-234): castle → `'Il castello non si sposta'`; else
  `G.placing = {kind: b.kind, cx: b.cx, cy: b.cy, ok:false, move: b, last:null, kbd}`, `mode = null`.
* In `pickmove`: left click on own building → `startMoving(t)` (click sound) else toast `Clicca un tuo edificio`
  (deny). Arrows → `keyMove` picks a building (below); Enter/Space → `keyConfirm()`:
  `b = pickB alive ? pickB : null; why = b ? startMoving(b, true) : 'Scegli un edificio con le frecce'`; toast if why;
  sound deny/click.
* `keyMove` in pickmove (control.js:247-256):
```
cur = pickB if alive else null
(x0,y0) = cur ? (cur.x, cur.y) : view centre
best = null; bd = ∞
for b in buildings: skip if not own, dead, castle, or b == cur
   along  = (b.x-x0)*dx + (b.y-y0)*dy
   across = |(b.x-x0)*dy| + |(b.y-y0)*dx|
   if cur and along < 8: continue
   d = cur ? along + across*2 : hypot(b - (x0,y0))      // first press: nearest to view centre, any direction
   if d < bd: best = b
if best: pickB = best; lookAt(best.x, best.y - 60)
```
  pickB highlight: brackets `(x0-10, y0-14, x1-x0+20, y1-y0+28)`, scale `w>1 ? 1 : 0.7`.
* Then placement as §9 with `p.move` set: validation via `moveBlock`; confirm sends `command('move', id, cx, cy)`;
  placing always ends after a successful move (Shift ignored). Sim: same-cell move is a silent no-op; the move is
  instant and keeps HP and queue (`relocateBuilding`). If the moved building dies while placing → placing cancelled.
* The V shortcut button shows brackets while `mode=='pickmove'` or `placing.move` (panels.js:38).

---

## 11. Camera

### 11.1 Pan
See §6. Keys: arrows/WASD (disabled when a panel is open, while placing, or in pickmove). Edge scroll when the mouse
is inside the canvas, the window has focus and no box-drag is in progress; it **overrides** the key direction per axis.
Speed 1100 world px/s × (Ctrl 4 | Shift 2 | 1), multiplied by frame dt (real dt, also when frozen). Not normalised on
diagonals. Camera does not move in solo while paused/help/over (Ctl.update not called).

### 11.2 Zoom
* `zoomSteps()` (main.js:121-124): `ZOOMS` filtered to `[0.5*dpr, 1.5*dpr]`; if empty → `[0.5]`.
* `setZoom(dir, sx, sy)` (main.js:156-164): next step index `clamp(idx(zoom)+dir, 0, n-1)`; if unchanged return;
  keep world point under screen point (sx,sy) fixed: `w = cam + s/zoom_old; cam = w - s/zoom_new`;
  `zoomChosen = true`; update viewW/H; `clampCamera()`.
* Wheel: around the mouse, one step per frame, ignored if `UI.hot`. `+`/`−` keys: around screen centre `(W/2, H/2)`.
* Default zoom (on resize while `!zoomChosen`): largest step ≤ `fit = min(W/1500, H/850)`, else smallest step.
  After the player has zoomed, resize only clamps zoom into the new step range.
* HD-2D port: keep the discrete step count/semantics (min ≈ ⅓ of max, 5 steps at dpr 1: 0.5..1.5) mapped onto the
  orthographic scale / camera distance.

### 11.3 Limits `clampCamera()` (main.js:278-282)
```
mw = World.W*TILE; mh = World.H*TILE; bottom = 150 * G.us / G.zoom
cam.x = mw > viewW ? clamp(cam.x, 0, mw - viewW) : (mw - viewW)/2
cam.y = mh > viewH ? clamp(cam.y, 0, mh - viewH + bottom) : (mh - viewH)/2
```
### 11.4 `lookAt(x, y)` (control.js:112): `cam = (x - viewW/2, y - viewH/2)`; clamp.
Used by: minimap, home (castle y−80), nextIdle, group double-tap, centerOnSelection (buildings y−80), alert bell,
keyboard ghost follow, pickmove (y−60), match start (castle), Units panel "select" button.

### 11.5 Minimap (ui.js:310-323; drawing ui.js:273-308)
Rect `mw = 270*us, mh = 180*us`, at `x = W - mw - 24*us, y = H - mh - 26*us` (bottom-right), framed panel 10*us around.
`k = mw / (World.W*TILE)`; mouse → world `wx = (m.x - x)/k, wy = (m.y - y)/k`.
* Mouse over frame → marks UI hot (world clicks ignored). Left press inside → `UI.mapDrag = true`; while lmb held
  (even if the mouse leaves the minimap) `lookAt(wx, wy)` every frame; released → `mapDrag = false`.
* Right press inside (not spectating) → `Ctl.order(clamp(wx,0,mapW), clamp(wy,0,mapH))` (normal right-click order
  semantics incl. `pick` at that world point; not rally).
* Content: minimap terrain image; buildings (own or `Fog.knows(b.x, b.y-20)`) as squares of size 9/5/3 ×us
  (castle / w>1 / small) with a `#1b1f33` 1-px border, positioned at `(b.x, b.y1)` bottom-centred; capture points
  (`Fog.knows(p.x, p.y-TILE)`) as diamonds r 6.5us dark + 4.5us owner colour or `#f4ecd2` if free; units (not inside;
  own or `Fog.sees`) as 2.5us squares, white if selected else faction colour; fog texture (smoothed); view rectangle
  `rgba(255,255,255,0.9)` width 1.5us clamped to the map; pings: red `#ff4a3d` circles width 2.5us, radius
  `(5 + (t*14) % 16)*us`, alpha `clamp(2 - t/2, 0, 1)`.

### 11.6 Spectator (ui.js:483-490)
After defeat in a network match the player can choose `Guarda` → `spectate = true`, `state = 'play'`. Camera keys,
edge scroll and minimap work; no selection/orders. Banner `Spettatore · frecce o minimappa per guardare`.

---

## 12. Fog of war

### 12.1 Client fog `Fog` (control.js:17-76) — per local player, presentation + input gating
Arrays over `World.W*World.H` cells: `vis` (seen now, u8), `seen` (explored, u8), `shade` (f32 current opacity).
* `reset(on)`: allocate; `shade.fill(on ? 1 : 0)`; build a W×H RGBA texture with RGB (10,14,26), alpha `shade*255`;
  `t = 0`. Called at `G.start` with `on = !demo`, and by network clients at `loadMatch` with `true`.
* `sees(x,y)` = `!on || vis[cellAt(x,y)] == 1` (out-of-map → false when on).
  `knows(x,y)` = `!on || seen[cell] == 1`.
* `reveal(x, y, r)` (cells): `cx = floor(x/TILE), cy = floor(y/TILE)`; for every cell (i,j) in the clamped square
  `[cx-r, cx+r] × [cy-r, cy+r]` with `(i-cx)² + (j-cy)² <= r*r + r` → `vis = 1, seen = 1`.
* `update(dt)`:
```
if on and player and (t -= dt) <= 0:
    t = 0.2
    vis.fill(0)
    for u in units of player, !dead:              reveal(u.x, u.y,      u.def.sight + f.mods.sight)   // sheltered pawns too
    for b in buildings of player, !dead:          reveal(b.x, b.y - 30, (b.done ? b.def.sight : 2) + f.mods.sight)
    for p in points with owner == player:         reveal(p.x, p.y - TILE, POINT_SIGHT + f.mods.sight)
k = min(1, dt*5)
for each cell c:
    want = (!on or vis[c]) ? 0 : seen[c] ? f32(0.42) : 1
    if shade[c] == want: continue
    s = shade[c] + (want - shade[c])*k; if |want - s| < 0.001: s = want
    shade[c] = s; alpha[c] = s*255; changed = true
if changed: upload texture
```
  `seen` is never cleared during a match (explored memory). Turning `Fog.on` off (test mode, end of game) fades
  everything to clear; turning it back on fades unexplored back to black.
* Vision radii (cells) — units: pawn 5, scout 10, warrior 6, lancer 6, archer 7, knight 7, monk 6, ram 5, catapult 8;
  buildings (finished): castle 10, house 4, camp 5, depot 5, farm 5, barracks 5, wall 2, archery 5, stable 5, smith 5,
  market 5, tower 9, monastery 5, siege 5; **any unfinished building 2**; owned capture point 7.
  `f.mods.sight` = 0, or **+2** after the `maps` tech (factions.js:93) — applies to all of the above.
  Reveal origins: units at foot `(x,y)`; buildings at `(x, y-30)`; points at `(x, y-TILE)`.
* Initial: `Fog.reveal(castle.x, castle.y, START_SIGHT=15)` at start (host/solo in `G.start`, clients on first snapshot).
* At match end (`checkEnd` sets a result): `Fog.on = false` → whole map revealed.

### 12.2 What is gated by fog (client)
| Thing | Shown/pickable if |
|---|---|
| own units / buildings | always |
| enemy units, sheep, fx, arrows | `sees` (currently visible) |
| enemy buildings | `knows(b.x, b.y-20)` (explored) — drawn with **live** state (see 12.4) |
| capture points (pick, minimap) | `knows(p.x, p.y-TILE)`; drawn regardless |
| gold/stone nodes, trees | pick needs `knows`; drawn regardless (hidden by opaque fog only) |
| pickups (treasure, drops) | `knows` |
| new building placement | footprint centre `knows` |
| attack alert banner | shown only if alert point is off-screen |

### 12.3 [SIM] Faction exploration memory (factions.js:269-296) — must live in the sim crate
Independent from the client Fog, used by the sim for auto-work (pawns only choose resource nodes in explored
territory) and scout exploring. Only for `f.human` factions (AI: `explored()` always true).
```
updateFaction: if human and (f.seeT -= dt) <= 0: f.seeT = 0.25; explore(f)
explore(f):
  first = f.seen is null; allocate u8[W*H] if needed
  if first and f.castle: mark(castle.x, castle.y, START_SIGHT)
  look(e, x, y, r): key = (floor(y/TILE)*W + floor(x/TILE))*32 + r
                    if e.seenKey != key: e.seenKey = key; mark(x, y, r)      // only when cell or radius changed
  same three source loops / radii / origins as Fog.update, using mark (same circle formula)
explored(f, x, y) = !f.human or !f.seen or f.seen[cellAt(x,y)] == 1
```
Note the sim memory and the client `Fog.seen` are computed identically but separately (client every 0.2 s, sim every
0.25 s with change-detection). In Rust the client may simply mirror the sim's per-faction `seen` (smaller payload: send
deltas), but `vis` (currently visible) is client-computed in JS.

### 12.4 Faithfulness note
The README claims "il resto resta come l'hai visto l'ultima volta", but the code does not keep snapshots: explored
enemy buildings are drawn with their current state (new enemy buildings in explored areas appear, destroyed ones
vanish, HP bars update), trees/nodes show current depletion. Port as-is (or open question §18).

---

## 13. Markers, pings, alerts, toasts, banners

* `mark(kind, x, y)` → marker `{kind,x,y,t:0}`; drawn as ellipse radii `26×11` scaled by `(1 - 0.6k)` with
  `k = t/0.6`, alpha `1-k`, line width 3, colour `MARK_COLOR[kind]`; removed at t ≥ 0.6.
* `alert(x, y)` (control.js:116-123), triggered by sim event `['alert', factionIdx, x, y]` for the local player
  (host throttles to once per `ALERT_EVERY = 12` s per faction; also emitted when a capture point is lost):
  push ping; `alertAt = {x,y}; alertT = G.time`; if `!onScreen(x, y, 0)`: if `banners.length < 4` push banner
  `Il tuo regno è sotto attacco!` (player colour, dur 2.6 s) and play `'horn'`.
  (Quirk: `onScreen` is always true on a network host, so the host never gets banner/horn.)
* HUD bell button (ui.js:213-218) shown while `G.time - alertT < 30`: tooltip title `Sotto attacco`, desc
  `Porta la visuale dove il regno è stato colpito.`; click → `lookAt(alertAt)`.
* HUD idle-pawn button (ui.js:205-212): shown when any own pawn has `act == 0`; tooltip `Pedoni inattivi`, key `.`,
  desc `Seleziona il prossimo pedone senza un compito.`; click → `nextIdle()`.
* `G.toast(text)`: `toastText = text; toastT = 2`.
* Banners (`G.announce` → `'say'` event): queue, only the first animates; max 4 queued.

### 13.1 Shortcut functions (control.js:316-354)
* `nextIdle()`: idle = own live pawns with `act == 0`; if none return; `u = idle[idleI++ % len]`; `select([u.id])`;
  `lookAt(u)`. (No sound.)
* `home()`: if castle alive → `select([castle.id])`, `lookAt(c.x, c.y - 80)`.
* `group(n, assign)`: assign → `groups[n] = mine().ids` (own units only; assigning with nothing selected clears the
  group). Recall → ids filtered to those still in `G.index`; if none return; `select(ids)`; if `lastGroup.n == n` and
  `G.clock - lastGroup.t < 0.4` → `centerOnSelection()`; `lastGroup = {n, t: G.clock}`.
* `centerOnSelection()`: if list non-empty, `lookAt(e.x, e.y - (e.isBuilding ? 80 : 0))` for `e = list[0]`.
* `army()`: own live units with `type != 'pawn'` (includes scouts, monks, siege).
* `selectArmy()` (Tab): none → toast `Non hai truppe` + deny; else select all + `'select'` sound.
* `recall(units = army())` (Backspace / Units panel): if no units or no castle → toast `Non hai truppe` + deny; else
  `command('recall', ids)`; marker `'move'` at `(c.x, c.y1 + 130)`; `'ack'`. (Sim: move order to that point for the
  listed non-pawn units; README: they don't stop to fight.)

---

## 14. Performance auto-degrade and resolution

### 14.1 Lights `Light` (light.js:23-43)
`level`: 2 = full (shadows, point lights, post pass: bloom/tilt-shift blur/vignette/sun shafts), 1 = light
(shadows and lights, no post pass or shafts), 0 = off. `auto = true` initially.
* `cycle()` (F2): `level = (level + 2) % 3` (2→1→0→2), `auto = false`, toast
  `['Luci spente', 'Luci leggere: senza sfocatura, bagliore e raggi', 'Luci complete'][level]`.
* `watch(dt)` (real frame time): return if `!auto || level == 0 || dt > 0.25`;
  `slow = dt > 0.03 ? slow + dt : max(0, slow - 2*dt)`; if `slow >= 3`: `level--`, `slow = 0`, toast
  `level ? 'Il gioco rallentava: luci alleggerite (F2 per cambiarle)' : 'Il gioco rallentava: luci spente (F2 per riaccenderle)'`.

### 14.2 Render resolution `Quality` (main.js:128-140)
`steps = [1, 0.75, 0.5]`, `i = 0`, `auto = true`, `slow = 0`.
* `set(i, note)`: `i = i; slow = 0; resize(); toast(note)`.
* `cycle()` (F3): `auto = false`; `set((i+1) % 3, ['Risoluzione piena', 'Risoluzione al 75%', 'Risoluzione al 50%'][new i])`.
* `watch(dt)`: return if `!auto || i == last || (Light.auto && Light.level > 0) || dt > 0.25` — i.e. resolution only
  drops after auto-lights reached 0 (or if lights were set by hand); same accumulator; at 3 s →
  `set(i+1, 'Il gioco rallentava: risoluzione ridotta (F3 per cambiarla)')`.

### 14.3 `resize(w?, h?)` (main.js:143-154)
`fullW/H` = exact device-pixel size if provided by ResizeObserver(`device-pixel-content-box`) and within 1.5 px of
`inner*dpr`, else `round(innerWidth/Height * devicePixelRatio)`. Canvas = `round(full * Quality.scale)`;
CSS `image-rendering: pixelated` at scale 1, `auto` below. `G.dpr = G.W / innerWidth` (so dpr includes the quality
scale). Then zoom (§11.2), `G.us = clamp(min(W/1500, H/850), 0.72*dpr, 1.5*dpr)`, `viewW/H = W/H / zoom`.
Bevy: map to window scale factor + a render-target scale; keep the three toasts and the watchdog thresholds.

---

## 15. Pause, help, test mode, page unload

### 15.1 Pause (main.js:253-255; ui.js:515-527)
`G.paused` toggled by P, by Escape at the end of its cascade, by the HUD pause button (top-right, sets true), and
cleared by the overlay's `Riprendi`.
* **Solo**: sim, camera, fog all stop (§4.2). Overlay ribbon `PAUSA`; buttons `Riprendi` and `Menu` (→ `G.toMenu()`).
* **Network** (`Net.playing`): nothing stops (host keeps simulating, client keeps interpolating; camera still pans).
  Overlay ribbon `MENU`; buttons `Riprendi` and `Esci`; note text host:
  `La partita continua. Se esci finisce per tutti.`, client: `La partita continua mentre il menu è aperto.`
* While paused (or help open): orders/selection disabled (`Ctl.update` early return), all game keys except
  M/F1/F2/F3/Escape/P ignored, arrows/Enter drive the overlay buttons.
* Help (F1, HUD `?` button) behaves like pause for the sim in solo only.

### 15.2 Test mode (F4) (main.js:257-259; panels.js:384-397; ui.js:474-477)
Solo only. Panel `'test'` (toggled by F4) titled `Modalità test`, subtitle
`Solo nelle partite da soli. Si apre e si chiude con F4.`. Contents:
* per resource (wood, gold, meat, stone): icon, current amount `floor(res)`, button `+1000` → `f.res[k] += 1000`
  (direct local mutation — allowed because it's solo; in Rust make it a sim command `debug_gift`, rejected in network);
* `+1000 di tutte e quattro` → +1000 to each;
* `Ferma il tempo` / (when frozen) `Tempo fermo: fallo ripartire` → toggle `G.frozen`;
* `Togli la nebbia: tutta la mappa in vista` / (when off) `Nebbia tolta: rimettila` → toggle `Fog.on`;
* footer `A tempo fermo puoi guardare in giro e dare ordini: partono quando lo fai ripartire.`
Frozen = `updateView` only: commands still execute immediately (orders are stored, units don't move until time
resumes). `G.frozen` reset to false at each `G.reset()`. HUD status line (yellow `#ffe27a`) while
`frozen || (!Fog.on && !result)`:
`` `Modalità test: ${frozen ? 'tempo fermo' : ''}${frozen && !Fog.on ? ' · ' : ''}${Fog.on ? '' : 'nebbia tolta'}  (F4)` ``.
Network refusal toast: `La modalità test c'è solo nelle partite da soli`.

### 15.3 beforeunload (main.js:198-200)
If `G.state == 'play' && !G.result` → `preventDefault(); returnValue = ''` (browser confirm dialog). Rationale: Ctrl+W
(fast pan up with WASD) would close the tab. README: Ctrl+arrow is the safe fast pan on Windows/Linux. Port with
web-sys `beforeunload` (wasm). Note: the new Rust version must additionally support rejoining after a reload
(out of scope of this file).

---

## 16. Match lifecycle hooks in main.js

* `G.reset()` (main.js:38-46): `UI.panel = null`; `NEXT_ID = 1` (host and clients must number trees/nodes
  identically); clear index, time, paused, frozen, placing, result, endT, demoT, spectate, toastT, `sheepT = 5`,
  pickT, pointT, player, and entity lists.
* `G.scenery()` (main.js:49-64): trees, golds (normal + big), stones (normal + big), points, decor (bushes, rocks,
  props), clouds from `World`.
* `G.start(setup)` (main.js:68-95): `setup = {players: [{color, local, conn}], level}`; no players = demo battle behind
  the menu (`state = 'menu'`), else `'play'`. `reset`; `World.generate(random seed)`; scenery; 3 sheep per meadow;
  treasure pickups; factions: `COLORS` shuffled (`sort(() => random-0.5)`) → base i; `makeFaction(color, base,
  isHuman, level || 'normal')`, `idx = i`, `conn`; local player; `setupFaction` each; `Ctl.reset()`;
  `Fog.reset(!demo)`; `lookAt(focus castle)`; `Fog.reveal(castle, 15)`; if not demo announce
  `Distruggi i castelli nemici!` (all, 3.5 s).
* `G.toMenu()`: `UI.help = false; Net.leave(); start({players: []})`.
* `G.checkEnd()` (main.js:100-113): demo → when ≤1 alive and `demoT <= 0` set `demoT = 8`. Spectating and ≤1 alive →
  `spectate = false; state = 'over'`. If no result yet: player dead → `'lose'`; else exactly one alive → `'win'`. On
  result: `endT = 2.5`, `endTime = time`, `placing = null`, `select([])`, `Fog.on = false`, play `'win'`/`'lose'`.
  After `endT` (§4.4) `state = 'over'` → game-over overlay (`VITTORIA!` / `SCONFITTA`, documented in the UI spec).
* Startup (main.js:488-490): `resize()`, start rAF, `Assets.load()` then `G.start({players: []})`; on error
  `G.error = err.message` (shown on the loading screen).

---

## 17. Porting guidance (Rust / Bevy / WASM)

1. **Crate split.** Sim crate: `updateWorld` minus `updateFx`/`updateView`, `execCommand`, `explore`/`explored`,
   end detection (`checkEnd` result part), demo restart timer. Client: everything else in this file (Input, Ctl, Fog,
   camera, placement, cursor, markers, HUD, Light/Quality, test-mode UI). Client-side validation (`why`, `p.ok`) needs
   read access to sim state (`canPlace`, `zoneBlock`, `moveBlock`, `canAfford`) — expose these as pure functions of the
   sim state in the sim crate so client and server agree.
2. **Fixed tick.** JS steps with variable dt ≤ 0.05 s (typically ≈1/60 s). Choose a fixed sim tick (e.g. 30 Hz or 60 Hz;
   the server snapshot rate was 20 Hz). Per-step budgets `FIELD_REFRESH = 2` and `FIELD_NEW = 2` are **per sim step**
   in JS at ~60 steps/s → ~120/s each; if the tick rate differs, scale them (e.g. ×2 at 30 Hz) to keep throughput.
   All other sim timers are dt-based seconds and port unchanged. Client presentation (camera, fog fade, markers,
   clouds, toasts, double-click timers on `G.clock`) uses real frame dt; keep the 0.05 clamp for presentation timers.
3. **Pause semantics.** Solo: pause/help/game-over stop the sim (and the JS also stops the camera; acceptable to let
   the camera move in the port, but the sim must stop). Network: never stops. Frozen (test): sim stops, commands
   still applied immediately to the sim state.
4. **Commands are the only client→sim channel**; the local feedback that bypasses it (rally pre-set `b.rally`,
   markers, sounds, toasts of local refusals) must be kept client-side. As a network client, command refusals are not
   returned synchronously; the host emits `toast` events.
5. **Coordinates.** Sim uses world px (TILE = 64) with y down; sprite hitboxes are y-up offsets from the foot point.
   With an HD-2D camera (tilted perspective), convert mouse → ground point by ray-casting onto the voxel terrain
   (respecting terrain `level` heights), then apply the 2-D `pick` rules in sim coordinates; for sprite-height
   hitboxes (units/buildings/trees), prefer screen-space tests against the billboard rect projected from the foot.
   Box select projects the point 26 px above the foot.
6. **Input mapping in Bevy**: use `KeyCode` (physical) for everything except zoom, which must use the logical key
   character `+`, `=`(on the Equal key), `-` (Bevy `Key::Character`). Prevent browser defaults for the same keys
   (wasm canvas `prevent_default_event_handling`). Keep auto-repeat for NAV keys only (`navs`).
7. **Fog texture**: one texel per cell, linearly filtered, alpha from `shade`; on the voxel terrain project it as a
   ground/fragment mask (and hide billboards per §12.2). Upload only when changed.

---

## 18. Open questions / quirks to decide

1. `onScreen()` returns true on a network host → host double-click selects that unit type map-wide, and the host
   never gets the off-screen attack banner/horn. Likely a bug; recommend the port use a pure view-rect test for these
   two client uses (keep "always" only for host-side fx/sound emission decisions, if any remain).
2. Camera keys/edge-scroll keep working under the network pause menu while arrows also navigate the menu buttons.
   Keep or block?
3. In solo pause the camera is frozen (side effect of loop structure). Keep or allow panning?
4. Enemy buildings under explored fog show live state (no "last seen" memory) despite README wording. Keep, or
   implement true last-seen ghosts (would be a behaviour change).
5. Diagonal camera pan is not normalised (×√2); keep for faithfulness.
6. Wheel produces at most one zoom step per frame regardless of delta; with Bevy's pixel-delta trackpads, accumulate
   sign per frame the same way.
7. Comment in `Ctl.groups` says Ctrl+digit; the implementation and README use Shift+digit. Spec follows the code.
8. Client placement ignores the age lock (the sim refuses with its own message); the ghost may show green for an
   age-locked building only if the Build panel let you pick it (it normally doesn't).
9. Test-mode resource gift mutates faction state directly from the UI; in Rust it must become a (solo-only) command.

---

## Addenda (critic)

**C1. `onScreen` on the network host (Q1) — confirmed bug, FIX.** `onScreen` (entities.js:44-47) returns `true`
whenever `Net.hosting`. Client-side consequences on the host only: double-click selects every own unit of that type
on the whole map (control.js:168), and the "under attack" banner `'Il tuo regno è sotto attacco!'` + horn never fire
(control.js:119-122). Rust: the sim never calls a camera test (03a addenda U5); the client's `on_screen(x,y,m)` always
uses the local camera, so every player (including the lobby leader) gets the solo behavior.

**C2. Camera while paused / help open (Q2, Q3) — decision.** Verified: in solo, `update()` (main.js:333-345) does not
call `updateWorld`/`updateView` while `G.paused || UI.help`, only `Ctl.refresh()`, so the camera freezes; in network
games `updateView` keeps running → `Ctl.update` pans (keys/edge) under the pause menu, and arrow keys move both the
camera and the menu focus. Rust: camera panning by keys and screen edge is suspended whenever a modal overlay is open
(pause menu, help, management screen with keyboard focus), in solo and network alike; world simulation pause stays
solo-only.

**C3. Fog shows current state, not last-seen state (Q4) — confirmed.** Buildings are drawn if `Fog.knows`
(main.js:392) with their current hp/flags; resources likewise (`Fog.knows` for nodes in pick, control.js:143-146).
Since the Rust server will filter snapshots by visibility (08 R6), the client must keep **last-seen ghosts** of enemy
buildings (kind, cell, variant, last hp) to reproduce what the player sees today in explored-but-not-visible cells;
neutral resources (trees/gold/stone amounts) can stay globally replicated (cheap, not secret).

**C4. Fixed-step budgets (Q5)** — see 02-world addenda W5.

**C5. Test mode (Q7).** The F4 screen's "+1000" etc. become sim commands `DebugAddRes{kind, amount}` etc., accepted
only by the in-browser solo sim; the server rejects them (no string needed: silently ignored, as JS ignores malformed
commands).

**D1. Determinism — complete list of `Math.random` call sites (sim vs presentation).** Helpers: `rand`, `randInt`,
`pick` (assets.js:600-602) all use `Math.random`. In the Rust sim, one `SimRng` (seeded per match from the server
seed; e.g. mulberry32 or PCG32) is used **only** for the [SIM] draws below, in this order of occurrence per tick;
[PRES] draws move to the client (seeded by entity id).
| Site | What | Class |
|---|---|---|
| main.js:74 | world seed | SIM (server picks) |
| main.js:80 | base ↔ colour assignment (shuffle) | SIM — Fisher–Yates |
| main.js:62, 319, 323-324 | clouds, water splash | PRES |
| entities.js:97 | `dropPickup` scatter angle/radius | SIM |
| entities.js:109, 117, 156 | tree `phase`, gold `shine`, `twinkle` | PRES |
| entities.js:130-131 | sheep spawn offset, initial timer, flip | SIM (flip PRES ok) |
| entities.js:172, 175, 178 | sheep idle/graze choice, wander target | SIM |
| entities.js:185 | wild meadow pick for respawn | SIM |
| entities.js:255 | unit `animT` | PRES |
| entities.js:256, 264 | unit `thinkT`, `fearT` staggering | SIM |
| entities.js:499 | block roll | SIM |
| entities.js:618 | alarm guard post | SIM |
| entities.js:655, 689 | soldier / monk think interval | SIM |
| entities.js:927 | shelter exit spot | SIM |
| entities.js:1024 | running-dust timer | PRES |
| entities.js:1089 | building `fireSeed` | PRES |
| entities.js:1117 | tower/castle first shot `cd` | SIM |
| entities.js:1146 | destruction fx scatter | PRES |
| entities.js:1155 | loot x jitter | SIM |
| entities.js:1163, 1169 | deploy spot, rally jitter | SIM |
| factions.js:54 | faction `tickT` phase | SIM |
| factions.js:59 | AI `thinkT`, `waveT` initial | SIM |
| factions.js:192 | `findSpot` ring shuffle | SIM — Fisher–Yates |
| factions.js:666 | AI weighted target-kingdom draw (`chooseTarget`) | SIM |
| factions.js:743 | AI next wave gap | SIM |
(`Net.pick` at net.js:100-127 and `Ctl.pick` at control.js:126-458 are lobby colour picking and mouse picking, not
random.) `world.js` uses only its own `mulberry32(seed)` stream.

**D2. Sorting.** JS `Array.prototype.sort` is stable; use stable `sort_by` with `f64::total_cmp` in Rust for all
comparator sorts (entities.js:375, 562; factions.js:317, 490, 526, 645, 650, 721, 850; world.js:542) so ties keep
insertion order (e.g. `Object.keys(want)` order = `UNIT_TYPES` order). Never use a random comparator (Rust ≥ 1.81 may
panic on non-total orders).

**D3. Iteration order.** Many decisions pick "the first minimum" while iterating `G.units`, `G.buildings`, node lists
in **creation order** (e.g. `dropSiteAt`, `shelterFor`, `acquireTarget`, `updateBuilding` target choice). Rust must
iterate entities in creation order (e.g. `Vec` append + tombstones, or `BTreeMap<id>`), not hash order.
