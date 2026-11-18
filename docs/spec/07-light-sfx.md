# 07 — Lighting, Day/Night, World Rendering Pipeline, Particle FX and Sound Effects

Porting spec for: `js/light.js` (all, 285 lines), `js/sfx.js` (all, 79 lines), the render parts of
`js/main.js` (draw order, culling, y-sort, clouds, water animation, quality levels), and the entity draw
code in `js/entities.js` / `js/control.js` / `js/assets.js` that the render pipeline uses (shadows,
selection rings, health bars, damage blink, particle FX, overlays, sound triggers).

Everything in this document is **presentation**. The only thing the deterministic sim crate has to provide
is: `time` (sim seconds), entity state (hp, hurtT, flags...), and a **presentation event queue**
(`fx`, `snd`, `play`, `float`, `toast`, `alert`, `say`, `prop`; see section 9). The day/night cycle is
a pure function of sim `time`, so in multiplayer it is identical on every client with no extra traffic.

Coordinates: "world px" = original 2D map pixels, `TILE = 64` px per cell (assets.js:4). In the new
voxel/HD-2D renderer 1 cell = 1 block, so divide world px by 64 to get block units. "Screen px" = physical
canvas pixels. `G.zoom` = screen px per world px.

---------------------------------------------------------------------------------------------------------

## 1. Time sources (main.js:18-22, 333-345, 461-486; net.js:235, 308-325)

| Name | Meaning | Advances when | Used by |
|---|---|---|---|
| `G.time` | sim time in seconds | `updateWorld` (`G.time += dt`, main.js:291) — i.e. solo play not paused/help, menu demo, host always; on clients `Net.clientUpdate` adds dt (net.js:310) and snaps to the host value if `|s.time - G.time| > 0.25` (net.js:235) | day/night phase, light flicker, all sprite animation frames tied to world (foam, water rocks, ducks, trees, bushes, fires), FX lifetimes |
| `G.clock` | real (wall) seconds since load | every frame (`update`, main.js:334) | sun shafts drift, demo-camera orbit, double-click timing, menu animations |
| frame dt | `min(0.05, (now - lastFrame)/1000)` (main.js:463) | | update step (capped at 50 ms) |
| paint dt | `(now - lastPaint)/1000` uncapped (main.js:469) | | `Light.watch`, `Quality.watch` auto-degrade |

- `G.reset()` sets `G.time = 0` at every match start (main.js:42) → every match (and every demo battle)
  starts at day phase 0.1 (bright morning, see 2.2).
- Pause (`G.paused`) and help (`UI.help`) freeze `G.time` in solo → the day cycle and all world animations
  freeze. Test mode `G.frozen` also freezes `G.time` (only `updateView` runs: clouds, water splashes,
  camera, fog keep moving because they use `dt` directly — main.js:341, 310-331).
- In network play the game never pauses (main.js:338-340).

---------------------------------------------------------------------------------------------------------

## 2. Day/night cycle (light.js:6-17, 72-86)

### 2.1 Constants

```
DAY_LENGTH = 480            // sim seconds per full day (8 minutes)
DAY_KEYS = [                // [phase 0..1, ambient RGB, shadow strength]
  [0.00, [255, 200, 172], 0.20],   // alba (dawn)
  [0.07, [255, 251, 240], 0.30],   // giorno (day)
  [0.50, [255, 251, 240], 0.30],
  [0.60, [255, 206, 150], 0.28],   // ora dorata (golden hour)
  [0.67, [208, 142, 150], 0.20],   // tramonto (sunset)
  [0.73, [108, 124, 196], 0.14],   // notte (night): darker and bluer, but readable
  [0.95, [108, 124, 196], 0.14],
  [1.00, [255, 200, 172], 0.20],
]
```

### 2.2 `Light.tick()` — called once per rendered frame, right after the ground is drawn (main.js:382)

```
phase = (G.time / 480 + 0.1) % 1                       // offset 0.1: time 0 = morning
i = 1; while DAY_KEYS[i].phase < phase: i++             // first key with phase >= p (p==0 -> i=1, k=0)
(p0,c0,s0) = DAY_KEYS[i-1]; (p1,c1,s1) = DAY_KEYS[i]; k = (phase - p0)/(p1 - p0)
ambient[j] = round(c0[j] + (c1[j]-c0[j])*k)              // integer RGB, JS Math.round (half up)
shade      = s0 + (s1-s0)*k                              // NOT rounded
lum   = (ambient.r*0.3 + ambient.g*0.59 + ambient.b*0.11) / 255
night = clamp((0.92 - lum) / 0.42, 0, 1)
// sun: moves east->west during the day; shadows swing left->right, shortest at noon.
day = clamp(phase / 0.7, 0, 1); sun = (day - 0.5) * 2         // -1 .. +1
lx = night > 0.9 ? -0.55 : sun * 1.15                          // moon: fixed direction, soft shadows
ly = night > 0.9 ?  0.34 : 0.26 + 0.2 * sun * sun
```

Stored state on `Light`: `phase, ambient[3], night, shade, lx, ly` (initial values light.js:26:
`phase 0, ambient [255,255,255], night 0, shade 0.3, lx 0, ly 0.4`).

### 2.3 Timeline in sim seconds from match start (t = ((phase - 0.1) mod 1) * 480)

| Phase | Sim time (first day) | Ambient | shade | night | Event |
|---|---|---|---|---|---|
| 0.10 | 0 s | 255,251,240 | 0.30 | 0 | match start (full day) |
| 0.50 | 192 s (3:12) | 255,251,240 | 0.30 | 0 | day ends, golden ramp starts |
| 0.60 | 240 s (4:00) | 255,206,150 | 0.28 | ≈0.187 | golden hour |
| 0.67 | 273.6 s (4:33.6) | 208,142,150 | 0.20 | ≈0.672 | sunset |
| ≈0.711 | ≈292.6 s | — | — | crosses 0.9 | shadows switch to moon direction (hard switch) |
| 0.73 | 302.4 s (5:02.4) | 108,124,196 | 0.14 | 1.0 | full night |
| 0.95 | 408 s (6:48) | 108,124,196 | 0.14 | 1.0 | dawn ramp starts |
| ≈0.956 | ≈410.9 s | — | — | crosses 0.9 | shadows leave moon direction |
| 1.00/0.00 | 432 s (7:12) | 255,200,172 | 0.20 | ≈0.198 | dawn |
| 0.07 | 465.6 s (7:45.6) | 255,251,240 | 0.30 | 0 | full day again |
| 0.10 | 480 s | | | | cycle repeats |

Derived luminances: day 0.98427 (night 0), dawn 0.83694 (night 0.1978), golden 0.84133 (0.1873),
sunset 0.63796 (0.6715), night 0.49851 (night = 1.0035 → clamped 1).

**Quirks to preserve (or consciously fix — see Open questions):**
- Between phase ≈0.956 and 1.0 `night <= 0.9` again but `day = clamp(p/0.7) = 1` → `lx = +1.15`
  (evening direction), then at the wrap to phase 0 it jumps to `lx = -1.15`. Same between 0.70 and ≈0.711
  (`lx = 1.15` then hard switch to moon). Shadows are weak then (shade 0.14–0.2) so it is barely visible.
- `lx/ly` jump discontinuously when `night` crosses 0.9.

### 2.4 Mapping to a 3D (HD-2D) renderer

The 2D shadow is a shear of the sprite (section 3): a sprite pixel at height `h` above its ground base lands
on the ground at offset `(lx*h, ly*h)` (x = east, y = south / toward the camera). In 3D coordinates
(X east, Y up, Z south) the light ray direction is `∝ (lx, -1, ly)`, i.e. **direction toward the sun =
normalize(-lx, 1, -ly)**. Sun elevation = `atan(1 / hypot(lx, ly))`:
noon (lx 0, ly 0.26) ≈ 75.4°; early morning/late evening (|lx| 1.15, ly 0.46) ≈ 38.9°; moon (−0.55, 0.34) ≈ 57.1°.
The sun is always on the far (north) side so shadows fall toward the viewer.
Recommended mapping: directional light colour = `ambient/255`, shadow opacity ≈ `shade` (the 2D game draws
shadows at alpha `shade` with colour `#0e1230`), point lights from section 4, scene multiply tint = ambient.

---------------------------------------------------------------------------------------------------------

## 3. Sprite shadows (light.js:18-19, 88-136; assets.js:50-76, 91-103)

### 3.1 Shadow silhouettes — `SHADOW` (assets.js:51-69)

- `SHADOW.on` global flag; while true, `img(path)` and `drawSheet` return a **silhouette** of the image:
  same alpha, RGB replaced by `SHADOW.color = '#0e1230'` (canvas `source-in` fill).
- Cache key: the path with any faction colour name (`Blue|Red|Yellow|Purple|Black`) replaced by `*` — one
  silhouette per shape regardless of team colour.
- Draw functions check `SHADOW.on` to skip non-shape decorations: selection ring (entities.js:1308), building
  fires (entities.js:1349). Everything else (including `globalAlpha` tweaks: hurt blink 0.5, construction
  ghost 0.3 + partially revealed sprite, depleted gold/stone 0.45, wall construction alpha) is drawn into the
  shadow too, so those shadows are equally faded.

### 3.2 Ground base per entity — `Light.base(e)` (light.js:89-101)

Returns the world y of the line the shadow is hinged on (null = no shadow):

| Entity | Base |
|---|---|
| unit / building / capture point (`isUnit`, `isBuilding`, `isPoint`) | `e.sy` (units: `y`; buildings: `(cy+h)*64-4` = `y`; points: `y`) |
| tree (`drawTree`) | `wood > 0 ? e.y : null` (stumps cast none) |
| gold (`drawGold`) | `big ? e.sy : e.y + 30` |
| stone (`drawStone`) | `big ? e.sy : e.y + 20` |
| sheep | `e.y` |
| bush | `e.y + 2` |
| rock (land decoration) | `e.y + 8` |
| prop | `FLAT_PROPS.has(name) ? null : e.y` |
| anything else (pickups, fx, arrows) | null |

```
FLAT_PROPS = {'bones','flowers','grass','lilypads','mushrooms','reeds','boat','flowers_red','flowers_blue',
  'flowers_white','clover','pebbles','puddle','sticks','dock','shipwreck','tall_grass','ferns'}  // on ground/water: no shadow
```

### 3.3 `Light.shadows(ctx, list)` (light.js:105-136) — level ≥ 1 only

1. Offscreen layer at **half** screen resolution (`ceil(W/2) × ceil(H/2)`), cleared, nearest-neighbour.
2. For every entity of the y-sorted visible list with a non-null base `by`:
   world→layer transform `scale(z/2)` with offset `-round(cam*z)/2`, then
   `translate(e.x, by) · matrix(a=1, b=0, c=-lx, d=-ly, e=0, f=0) · translate(-e.x, -by)` and call the
   entity's normal draw function (it draws silhouettes because `SHADOW.on = true`).
   Matrix meaning: for a local point (dx, dy) relative to the base, `x' = dx - lx*dy`, `y' = -ly*dy` —
   the sprite is flipped down onto the ground, squashed vertically by `ly` and sheared by `lx`.
3. **Cloud shadows** on the same layer: alpha 0.5, silhouette of each cloud image drawn **unsheared** at
   `(round(c.x + 90 + lx*60), round(c.y + 260))` (world px, top-left).
4. Layer composited once over the scene (screen space) at `globalAlpha = shade`, bilinear upscale ×2.
   Because all shadows are in one layer first, overlapping shadows do not darken twice.

The shadow pass is drawn after the ground and **before** the entities (main.js:397-398).

---------------------------------------------------------------------------------------------------------

## 4. Point lights (light.js:138-153)

`Light.lights(list)` builds lights from the **visible** entity list (so enemy buildings remembered under fog
also glow; fog is drawn on top afterwards). `t = G.time`.

```
add(x, y, r, power, seed, flick):
  power_eff = power * (1 - flick + flick * (0.5 + 0.5 * sin(t*9 + seed) * sin(t*5.3 + seed*1.7)))
```

| Source | Condition | Position (world px) | Radius r | power | seed | flick |
|---|---|---|---|---|---|---|
| Building windows | `isBuilding && done && kind != 'wall'` | `(x, y - def.ay*0.32)` | castle 340; `def.w > 2` 260; else 200 | 1.0 | `e.id` | 0.12 |
| Burning building (extra) | same and `hp < maxHp*0.7` | `(x, y - def.ay*0.4)` | 240 | 1.2 | `e.id*3` | 0.5 |
| Capture point held | `isPoint && owner` | `(x, y - def.ay*0.3)` | 220 | 0.9 | `e.id` | 0.12 |
| Campfire prop | `draw==drawProp && name=='campfire'` | `(x, y - 18)` | 290 | 1.25 | `e.x` | 0.35 |
| Shrine prop | `name=='shrine'` | `(x, y - 18)` | 150 | 0.7 | `e.x` | 0.35 |
| Gold node | `draw==drawGold && amount > 0` (small and big) | `(x, y)` | 120 | 0.4 | `e.id` | 0.2 |

Building width classes: castle w5; 3-wide = camp, farm, barracks, archery, stable, smith, market, monastery,
siege → 260; 2-wide = house, depot, tower → 200; wall excluded. Unfinished buildings emit no light.

Glow sprite (light.js:61), 256×256 radial gradient centre→edge:
`0: rgba(255,220,160,1) · 0.35: rgba(255,176,92,0.55) · 0.7: rgba(255,140,60,0.16) · 1: rgba(255,120,40,0)`.

---------------------------------------------------------------------------------------------------------

## 5. Scene lighting — `Light.apply(ctx, list)` (light.js:157-196), level ≥ 1

Runs after entities, arrows and clouds; before fog (main.js:404).

1. **Ambient light map** (only if `r+g+b < 750` — day colour sums to 746, so in practice always on):
   - map at 1/8 screen resolution (`ceil(W/8) × ceil(H/8)`), filled with `rgb(ambient)`.
   - each light: glow sprite drawn **additive** (`lighter`) centred on the light, radius `r` world px
     (`l.r*z/8` map px), alpha `clamp(power_eff * (0.25 + 0.75*night), 0, 1)` → in daylight lights still add
     25 %, but against a near-white ambient that saturates and is invisible.
   - upscale: smooth to half resolution, then nearest ×2 to full; composite with **multiply** over the scene.
2. **Night halo** (only if `night > 0.02`): glow sprite drawn **additive** directly on the scene, radius
   `r*0.62` world px, alpha `clamp(0.3 * night * power_eff, 0, 1)`.

In a 3D renderer: ambient = global tint/ambient light, each light = warm point light (colour ≈ #FFDCA0 core
→ #FF7828 edge), range ≈ r/64 blocks, intensity ∝ power_eff·(0.25+0.75·night), plus an emissive bloom
halo ∝ 0.3·night·power_eff.

---------------------------------------------------------------------------------------------------------

## 6. Post pass "diorama" — `Light.post(ctx)` (light.js:198-282), level 2 only

Runs in screen space after fog, before overlays/UI (main.js:411-415), so health bars, ribbons, markers and
the UI stay sharp and unlit. `w = ceil(W/4)`, `h = ceil(H/4)`, `POST_GLOW = 0.34`.

1. `A` = whole frame downscaled to quarter resolution (bilinear).
2. **Bright pass** `C = A × A × A` (copy, then multiply A twice) — per channel `(c/255)^3`.
3. **Sun shafts** added into `C` (6.1).
4. **Tilt-shift bands**: `top = round(h*0.22)`, `bottom = round(h*0.18)` (quarter-res rows).
   `B` = copy of `A`, then `A` drawn again at offset (+0.5, +0.5) with alpha 0.6 (extra blur), then
   `destination-in` a vertical gradient mask: for `i = 0..8`, `t = i/8`, alpha `1 - t²` placed at stop
   `t*top/h` and at stop `1 - t*bottom/h` (opaque at the screen edge, transparent at the band's inner edge).
   Draw B's top `top` rows stretched ×4 over the screen top, and its bottom `bottom` rows over the screen
   bottom (bilinear, source-over). → top 22 % and bottom 18 % of the screen fade into a blurred copy.
5. **Bloom**: `C` upscaled smooth ×2, then nearest ×2, composited **additive** at alpha 0.34.
6. **Vignette**: 256² radial gradient `0: rgba(8,10,28,0) · 0.62: rgba(8,10,28,0) · 0.86: rgba(8,10,28,0.42)
   · 1: rgba(8,10,28,0.85)`, stretched (elliptical) over rect `(-0.12W, -0.12H, 1.24W, 1.24H)`, cached per
   screen size; drawn at alpha `0.75 + 0.2*night`.

### 6.1 Sun shafts — `Light.shafts(C, w, h)` (light.js:201-224)

```
sun = (1 - night) * (0.45 + 0.55 * |lx|)       // stronger with a low sun
if sun <= 0.05: return
lean = -lx * 0.5; t = G.clock (REAL time)
for i in 0..5:
  x  = ((((i*0.19 + t*0.006 + sin(i*12.7)*0.08) mod 1.2) + 1.2) mod 1.2 - 0.1) * W   // JS % then +1.2 %1.2
  sw = (0.1 + 0.07*sin(i*3.1)) * W
  alpha = 0.085 * sun * (0.6 + 0.4*sin(t*0.35 + i*2.1)) / 0.34        // ÷POST_GLOW: compensated at bloom
  // full-res shear: X = (x - lean*H*0.5) + px + lean*py, Y = py; drawn at quarter scale
  draw shaft sprite at local rect (-sw/2, -0.05H, sw, 1.1H)
then: erase shafts where fog is (destination-out with the fog canvas in world space)
C += shafts (additive)
```
Shaft sprite 64×256: horizontal gradient `rgba(255,244,210)` alpha 0→1→0 (0, 0.5, 1), multiplied by a
vertical mask alpha 1 (top) → 0.5 (70 %) → 0 (bottom). Net on-screen alpha = `0.085*sun*(0.6..1.0)`.

---------------------------------------------------------------------------------------------------------

## 7. Quality levels, F2 / F3 and auto-degrade (light.js:22-42; main.js:125-154, 242-245, 467-470)

`Light.level`: **2** = full (shadows + light map + halos + post: bands, bloom, shafts, vignette);
**1** = shadows + light map + halos, no post pass; **0** = nothing from light.js (no shadows either).
`Light.on = level > 0`.

- `Light.cycle()` (F2): `level = (level + 2) % 3` (2→1→0→2), `auto = false`, toast by new level:
  - 0: `'Luci spente'`
  - 1: `'Luci leggere: senza sfocatura, bagliore e raggi'`
  - 2: `'Luci complete'`
- `Light.watch(dt)` (every painted frame, real paint dt):
  ```
  if !auto || level == 0 || dt > 0.25: return          // >0.25 s = tab was hidden
  slow = dt > 0.03 ? slow + dt : max(0, slow - dt*2)    // 0.03 s ≈ under 33 fps
  if slow < 3: return
  level--; slow = 0
  toast(level ? 'Il gioco rallentava: luci alleggerite (F2 per cambiarle)'
              : 'Il gioco rallentava: luci spente (F2 per riaccenderle)')
  ```
- `Quality` (render resolution scale, F3): `steps = [1, 0.75, 0.5]`, index `i`, `auto`, `slow`.
  - `cycle()`: `auto = false`, `i = (i+1)%3`, toasts `'Risoluzione piena'`, `'Risoluzione al 75%'`,
    `'Risoluzione al 50%'`.
  - `watch(dt)`: returns if `!auto || i == 2 || (Light.auto && Light.level > 0) || dt > 0.25` — i.e. only
    after the lights are already off (or the player chose the light level manually); same `slow` accumulator;
    at `slow >= 3` → `i+1`, toast `'Il gioco rallentava: risoluzione ridotta (F3 per cambiarla)'`.
  - `set` calls `resize()`: canvas = `round(fullW*scale) × round(fullH*scale)`, CSS `image-rendering`
    `pixelated` at scale 1 else `auto`.
- Toast: `G.toast(text)` shows for 2 s (`toastT = 2`, main.js:36).
- Help text line (ui.js:17): `['M · F1 · F2 · F3 · F4', 'audio · aiuto · luci · risoluzione · modalità test (risorse, tempo, nebbia)']`.
- Neither setting is persisted.

Bevy: level 2 → bloom + tilt-shift DoF + vignette + god rays; level 1 → lights/shadows only; level 0 → unlit
sprites, no shadows. Keep the same auto-degrade rule (3 s accumulated over 33 ms frames) and toasts.

---------------------------------------------------------------------------------------------------------

## 8. World render pipeline (main.js:349-457)

### 8.1 Camera, zoom (main.js:6-8, 121-164, 278-287, 310-316)

- `ZOOMS = [0.5, 0.75, …, 4.5]` (17 steps of 0.25). Allowed steps: `z ∈ [0.5*dpr, 1.5*dpr]` (else `[0.5]`).
- `fit = min(W/1500, H/850)`; initial zoom = largest allowed step `<= fit` (else the smallest); after the
  player zooms (`zoomChosen`) the zoom is only clamped. `G.us = clamp(fit, 0.72*dpr, 1.5*dpr)` (UI scale).
  `viewW = W/zoom`, `viewH = H/zoom`.
- Wheel / `+` `-`: one step, keeping the world point under the cursor (keys: screen centre) fixed.
- `clampCamera`: x in `[0, mapW - viewW]` (centred if map smaller); y in `[0, mapH - viewH + 150*us/zoom]`
  (extra room under the map for the bottom panel).
- World transform: `scale(z)`, translate `-round(cam*z)` (camera snapped to whole screen pixels).
- Menu demo camera: `cam.x = (mw-viewW)/2 + cos(clock*0.04)*max(0,(mw-viewW)/2-150)`,
  `cam.y = (mh-viewH)/2 + sin(clock*0.055)*max(0,(mh-viewH)/2-150)`.

### 8.2 Draw order of one frame (`renderWorld`, main.js:354-419; `render`, 436-457)

1. Clear to water colour `WATER_COLOR = '#47aba9'` (flat fill of the view rect + 3 px).
2. **Water foam** at every `World.foam` cell (land cells touching non-ground in 8 directions, plus cliff
   cells in water — world.js:578-584): sheet `Terrain/Tileset/Water Foam.png`, frame 192 px, anchor (96,96),
   10 fps, 16 frames; **all foam tiles share the same frame** `floor(G.time*10)`; drawn centred on the cell
   centre before the ground, so the ground covers the inner part and only the ring on water shows. Cull margin 100.
3. **Water rocks** (`World.waterRocks`, ≤100*scale, in water 2-3 cells off shore): sheet
   `Terrain/Decorations/Rocks in the Water/Water Rocks_0{v+1}.png`, fw 64, anchor (32,36), 10 fps, 16 frames,
   frame `floor(t*10 + phase)` (phase ∈ [0,16)). Cull 60.
4. **Water splashes** `G.waterFx` (8.4). Cull 100.
5. **Rubber ducks** (`World.ducks`; ≤9*scale at sea ≥1100 px apart, plus one per pond with `pond:true`):
   sheet `Terrain/Decorations/Rubber Duck/Rubber duck.png` fw 32, anchor (16,18), 4 fps, 3 frames.
   ```
   r = pond ? 18 : 46; a = t*0.3 + phase
   x = d.x + sin(a)*r;  y = d.y + cos(a*0.8)*r*0.4 + sin(t*2 + phase)*1.5
   frame = floor(t*4 + phase); flip = cos(a) < 0
   ```
   Cull 40.
6. **Ground tiles** `World.drawGround` (flat grass, per level: wall-foot shadows then tops/stairs/cliffs —
   see the map spec; replaced by voxel terrain).
7. `Light.tick()`.
8. **Visible list** (cull margin in world px around the view; fog filter):
   | List (in this push order) | margin | extra condition | sets `sy` |
   |---|---|---|---|
   | `G.decor` (bushes, rocks, props) | 100 | — | (own) |
   | trees | 260 | — | |
   | golds | 90 | — | |
   | stones | 110 | — | |
   | points | 260 | — | |
   | sheep | 90 | `Fog.sees` | `sy = y` |
   | buildings | 340 | own, or `Fog.knows(x, y-20)` | |
   | units | 200 | `!inside` and (own or `Fog.sees`) | `sy = y` |
   | pickups | 80 | `Fog.knows` | `sy = y` |
   | fx | 200 | `Fog.sees` | (fixed at spawn: `y + dy`) |
   Sorted ascending by `sy` (JS sort is stable: ties keep the push order above).
   Sort keys of static things: tree `cy+22` (= y), small gold `y+18`, big gold/stone `y+56`, small stone
   `y+20` (y = cell centre + 6), building/point `y` (bottom edge − 4), decor `y`.
9. `Light.shadows(ctx, visible)` (section 3).
10. Draw every visible entity (`e.draw(ctx, e)`, 8.3).
11. **Arrows/rocks** (not y-sorted, on top of entities), only where `Fog.sees`: arrow sprite
    `Units/{color} Units/Archer/Arrow.png` centred (−32,−32) rotated by `ang`; catapult rock =
    `Extra/Resources/Stone_Resource.png` at (−13,−11) scaled 1.3, rotated (`ang += 9 rad/s`).
12. **Clouds** alpha 0.6, drawn at `(round(c.x), round(c.y))` top-left, cull `inView(c.x+288, c.y+128, 320)`.
13. `Light.apply` (section 5).
14. **Fog of war**: 1 px per cell canvas (RGB 10,14,26; alpha = per-cell `shade`), stretched over the map with
    bilinear smoothing (soft edges). Shade targets: 0 visible, `fround(0.42)` explored, 1 never seen; chases
    with `k = min(1, dt*5)` per frame, snaps when within 0.001 (control.js:51-75). Visibility recomputed
    every 0.2 s.
15. `Light.post` (screen space, section 6).
16. `drawOverlays` (world space, 8.5) — after post: crisp, unlit, not fogged.
17. `Ctl.drawWorld` (order markers, placement ghost; not in demo) (8.6).
18. Screen-space UI (menu / HUD / spectator / drag box / game-over / pause / manage panel / help / focus /
    cursor) — see UI spec.

Loading screen (`renderLoading`, main.js:421-434; d = dpr): fill `#47aba9`; on error label `G.error` size 18d
centred; else title `'Tiny Swords'` size 34d at `y-50d`; bar back `#1b1f33` rect `(x-3d, y-3d, w+6d, 22d)`,
fill `#e3c84e` `(x, y, w*loaded/total, 16d)` with `w = min(420d, 0.6W)`, `y = H/2`; text
`` `Caricamento sprite ${loaded}/${total}` `` size 14d at `y+40d`. Errors: `'Impossibile caricare assets/' + f`,
`'Sprite mancante: ' + path`.

### 8.3 Entity draw functions (entities.js:1304-1445; assets.js:81-103)

`sheet(path, fw, ax, ay, fps)`: frames laid horizontally, `n = round(imgW/fw)`, frame height = image height,
anchor (ax, ay) = pixel of the frame placed on the entity's (x, y). `drawSheet` wraps frame index
`((f % n) + n) % n`, rounds x, y to integers, optional horizontal flip and uniform scale around the anchor.

- **Unit** (`drawUnit`): sheet from `unitSheet(color, type, anim)`; frame `floor(animT*fps)`, clamped to
  `n-1` when `state == 'attack'` (attack animations don't loop).
  - **Selection ring** (drawn *before* the sprite, in y-sort order, skipped in shadow pass): ellipse at
    `(x, y+1)`, radii `38×13` if siege or knight else `24×10`, stroke 2.5 world px, colour `'#ffffff'` if
    own unit else `'#ff5a4a'`.
  - **Damage blink**: if `hurtT > 0` the sprite is drawn at `globalAlpha 0.5` (semi-transparent, not a
    white flash). Sim sets `hurtT = 0.12` on every hit (entities.js:504) and decrements it each tick
    (1001). Network clients receive flag bit 2 = `hurtT > 0` and set `hurtT = 1` until the next snapshot
    (net.js:263; snapshots every 0.05 s).
- **Building** (`drawBuilding`): image anchored `(x - ax, y - ay)`.
  - Under construction: whole sprite at alpha 0.3, plus the bottom `round(imgH * progress/work)` rows at
    full alpha (builds up from the ground).
  - Done and `hp/maxHp < 0.7` (not in shadow pass): fires — `r < 0.25`: `fire3` ×4; `r < 0.45`: `fire2` ×3;
    else `fire1` ×2. For i in 0..n-1:
    ```
    h1 = |sin(fireSeed + i*12.9898) * 43758.5453| mod 1
    h2 = |sin(fireSeed + i*78.233)  * 12543.123|  mod 1
    pos = (x + (h1-0.5)*w*64*0.7,  y - 24 - h2*ay*0.45); frame floor(G.time*fps + i*3); scale 1.4
    ```
    `fireSeed = Math.random()*100` per building **per client** (entities.js:1089) — fire placement differs
    between clients (cosmetic).
  - **Wall** (`drawWall`): alpha `0.35 + 0.65*clamp(progress/work)` while building; front view if a wall
    neighbours left/right or it has no vertical neighbours; else a column of stakes cut from the sprite
    (`STAKE = {sx:31, sw:14, n:5}`: n stakes (n−1 if also front) at `y0 + (i+1)*64/5 - 72`).
- **Tree**: alive → sheet `Terrain/Resources/Wood/Trees/Tree{v+1}.png` fw 192 anchor (96, TREE_DEF[v].ay)
  8 fps, frame `floor(G.time*8 + phase)`; felled → `Stump {v+1}.png` at `(x-96, y - stumpAy)`.
  `TREE_DEF = [{ay:240,stumpAy:238},{ay:248,stumpAy:244},{ay:170,stumpAy:231},{ay:168,stumpAy:227}]`.
- **Gold**: `stage = clamp(ceil(amount/40), 1, 6)`; depleted → alpha 0.45; big → `Extra/Resources/Gold_Big.png`
  bottom-centred at `cy(c) + 64*1.5 - 6`; small → if `shine > 0 && amount > 0` the highlight sheet
  `Gold Stone {stage}_Highlight.png` (fw 128, anchor 64,64, frame `floor(shine*10)`) else static
  `Gold Stone {stage}.png` at `(x-64, y-64)`. **Twinkle** (entities.js:153-158, presentation only, uses
  `Math.random`): `shine += dt; if shine > 0.6: shine = -rand(3,7)`; initial `rand(-6,0)`.
- **Stone**: depleted alpha 0.45; big → `Stone_Big.png` like gold; small → `Stone_{3 if amount>190, 2 if >80,
  else 1}.png` bottom-centred at `y + 22`.
- **Capture point**: `Extra/Buildings/{Sprite}.png` at `(x - ax, y - ay)`.
- **Prop**: `Extra/Props/{name}.png` bottom-centred at (x, y).
- **Sheep**: sheets `Sheep_Idle` / `Sheep_Move` (8 fps) / `Sheep_Grass` (10 fps), fw 128, anchor (64,82),
  frame `floor(animT*fps)`, flip.
- **Bush**: `Terrain/Decorations/Bushes/Bushe{v+1}.png` fw 128 anchor (64,80) 8 fps frame `floor(G.time*8+phase)`.
- **Rock**: `Terrain/Decorations/Rocks/Rock{v+1}.png` at `(x-32, y-50)`.
- **Pickup**: chest → prop `'chest'` at `y+8`. Else blink when `life < 5`: hidden when `floor(life*6)` is odd;
  `y' = y - hop - |sin(t*3)|*4`; drawn scaled 1.5 around (x, y'); gold: every 2.2 s cycle the first 0.6 s shows
  `Gold_Resource_Highlight.png` (fw 128, anchor 64,70, frame `floor(ph*10)`), else `Gold_Resource.png` at
  (−64,−70); stone `Extra/Resources/Stone_Resource.png` at (−13,−20); wood/meat
  `Wood Resource.png` / `Meat Resource.png` at (−32,−42). Spawn hop: first 0.4 s lerp from drop point to
  target with `hop = sin(k*π)*30`.
- **FX** (`drawFx`): at `follow ? (follow.x, follow.y) : (x, y)` (a followed unit that died keeps its last
  position), frame `floor(t*fps)`, scale `f.scale`.

### 8.4 Particle FX catalogue (assets.js:400-410; frame counts from image widths)

| key | file | fw | anchor | fps | frames | life (s) |
|---|---|---|---|---|---|---|
| dust1 | `Particle FX/Dust_01.png` (512×64) | 64 | 32,46 | 14 | 8 | 0.571 |
| dust2 | `Particle FX/Dust_02.png` (640×64) | 64 | 32,56 | 14 | 10 | 0.714 |
| boom1 | `Particle FX/Explosion_01.png` (1536×192) | 192 | 96,110 | 14 | 8 | 0.571 |
| boom2 | `Particle FX/Explosion_02.png` (1920×192) | 192 | 96,130 | 14 | 10 | 0.714 |
| fire1 | `Particle FX/Fire_01.png` (512×64) | 64 | 32,62 | 10 | 8 | loop |
| fire2 | `Particle FX/Fire_02.png` (640×64) | 64 | 32,62 | 10 | 10 | loop |
| fire3 | `Particle FX/Fire_03.png` (768×64) | 64 | 32,62 | 10 | 12 | loop |
| splash | `Particle FX/Water Splash.png` (1728×192) | 192 | 96,100 | 12 | 9 | 0.75 |
| heal | `Units/{color} Units/Monk/Heal_Effect.png` (2112×192) | 192 | 96,134 | 12 | 11 | 0.917 |

FX instance (`showEvent 'fx'`, entities.js:65-68): `{sh, x, y, t:0, scale, follow, sy: y + dy, draw: drawFx}`;
removed when `t*fps >= n` (`updateFx`, entities.js:1295-1297). `addFx(name, x, y, {scale, dy, follow, color})`
rounds x, y and `scale` to 2 decimals and emits `['fx', name, x, y, scale, dy, followId|0, color]`.

Every spawn site (sim emits these; with `onScreen` gating — see 9.2):

| Trigger | file:line | FX | position / options |
|---|---|---|---|
| tree felled (wood → 0) | entities.js:144 | dust2 | (t.x, t.y) |
| sheep killed | 149 | dust1 | (s.x, s.y) |
| tree regrown (after 110 s) | 162 | dust1 | (t.x, t.y), only if onScreen |
| wild sheep respawn | 188 | dust1 | (s.x, s.y), onScreen |
| point captured | 208 | dust2 | (p.x, p.y), scale 1.6 |
| monk heal | 472 | heal | (t.x, t.y), follow target, dy 4, color = healer faction colour |
| unit killed | 519 | boom1 | (x, y-22), dy 60 |
| pawn enters / leaves shelter | 917, 929 | dust1 | (x, y+4), dy −6, onScreen |
| unit running | 1023-1026 | dust1 | every `rand(0.7,1.5)` s while moving, (x, y+4), dy −6, onScreen |
| building relocated | 1100, 1107 | dust2 | one per footprint column `(x0+(i+0.5)*64, y+4)`, before and after the move |
| building completed | 1131 | dust2 | per column, scale 1.5 if w>1 else 1 |
| building destroyed | 1145-1147 | boom2 | `w+1` explosions at `(x + rand(-0.4,0.4)*w*64, y - rand(10, ay*0.5))`, dy 80, scale `w>1 ? rand(1,1.5) : 0.7` |
| unit deployed from building | 1165 | dust2 | (x, y+2), onScreen |
| catapult rock impact | 1231-1232 | boom1 + dust2 | boom1 (tx, ty−10) dy 30; dust2 (tx, ty+6) scale 1.6 |

**Water splashes** (main.js:320-326, local only, never networked): every `rand(0.4, 1.2)` s (timer
`G.splashT`, initial 2) pick a random `World.openWater` cell (water cells with no land within 1 cell but land
within 5 — world.js:780-782) and push `{sh: splash, x: cx, y: cy, t: 0}`; removed when `t*12 >= 9`.
Drawn under the ground layer (step 4).

**Clouds** (main.js:60-63, 319): count `round(8 * W*H / MAP_AREA)` with `MAP_AREA = 144*96`; image
`Terrain/Decorations/Clouds/Clouds_0{i%8+1}.png` (576×256); `x = rand(-300, mapW)`, `y = rand(-100, mapH-150)`,
`v = rand(7, 16)` px/s eastward; when `x > mapW + 100` → `x = -600`, new `y = rand(-100, mapH-150)`.
Local/random per client.

### 8.5 Overlays (`drawOverlays`, entities.js:1448-1490), world space, after post

Bar primitives (assets.js:537-551):
- `smallBar(x, y, w, ratio, s, fill)`: base `UI Elements/UI Elements/Bars/SmallBar_Base.png` 3-slice
  (cols `[0,64],[128,64],[256,64]`, row 0 height 64) at `(x-49s, y-22s)` width `w+98s`; fill image rows
  (0,30,64,3) stretched to `(x+10s, y+8s, (w-20s)*clamp(ratio), 6s)` if width > 0.5.
  Fill = `SmallBar_Fill.png` tinted per faction (`source-atop` with `COLOR_CSS`: Blue `#46a5bb`, Red
  `#e6645c`, Yellow `#e3c84e`, Purple `#b981c6`, Black `#7887a0`).
- `bigBar(x, y, w, ratio, s)`: `BigBar_Base.png` 3-slice at `(x-40s, y-9s)` width `w+80s`; fill
  `BigBar_Fill.png` src (0,22,64,21) → `(x+11s, y+13s, (w-22s)*ratio, 21s)`.

Per visible entity:
- **Unit**: if `hp < maxHp` or selected → `smallBar(x-24, y - def.barY, 48, hp/maxHp, 0.5, factionFill)`.
  barY: pawn 78, scout 78, warrior 92, lancer 92, archer 92, knight 106, monk 78, ram 100, catapult 74.
- **Building** (`top = y - def.ay`, `wide = def.w > 1`):
  - own pawns sheltered inside (counted from own `inside` units whose position is inside the rect): avatar
    (`drawAvatar(color,'pawn', bx, by, 34)`) + label `'×' + n` at `(bx+38, by+18)` size 17 `#fff7c9`; castle
    `bx = x+98, by = top-14`, else `bx = x-34, by = top-22`.
  - not done and wide → `bigBar(x-45, y-60, 90, progress/work, 0.4)`.
  - done and (damaged or selected) → `smallBar(x - (wide?44:22), top+18, wide?88:44, hp/maxHp, wide?0.7:0.5)`.
  - castle → small ribbon (colour index, variant 0) centred `(x, top+4)`, width 190, scale 0.8, text =
    faction name, size 15.
  - selected → corner brackets (`Cursor_04.png` corners 32 px) around `(x0-10, y0-14, w+20, h+28)` scale
    `wide ? 1 : 0.7`; if own, done and trains units → dashed line (`[6,6]`, `rgba(255,255,255,0.6)`, width 2)
    from `(x, y1)` to the rally point and a pennant (`Ribbon_{color}.png` src (51,448,90,108)) at
    `(rally.x, rally.y-50)` scale 0.45.
- **Capture point** (`top = y - ay`): owner pennant at `(x, top-34)` scale 0.5; if `by && t > 0` →
  `smallBar(x-44, top+22, 88, t/8, 0.7, fill of by's colour)`; selected → brackets scale 1.
- **Other selected** (resource / sheep): big → brackets `(x-74, y-80, 148, 148, 1)`, else
  `(x-30, y-54, 60, 64, 0.7)`.
- **Floating texts** (`G.floats`, from `float` events; rise 28 px/s, life 1.2 s, entities.js:1298-1299):
  alpha `clamp(1.6 - t*1.4, 0, 1)`, icon (resource icon) size 30 centred `(x-18, y)`, label `str` (e.g.
  `'+10'`) at `(x, y)` size 18 colour `#fff7c9`, left aligned, dark outline `#1b1f33`.

### 8.6 Order markers and placement ghost (control.js:469-501)

- Markers (`Ctl.mark`, life 0.6 s): ellipse at the order point, radii `26*(1-0.6k) × 11*(1-0.6k)` with
  `k = t/0.6`, stroke 3, alpha `1-k`, colour `MARK_COLOR = {move:'#9be37a', attack:'#ff6a5a',
  gather:'#ffe27a', rally:'#ffffff'}`.
- Placement ghost: dashed (`[14,10]`, `rgba(255,255,255,0.75)`, width 3) circles of the build radius around the
  castle centre (`DEFENSE_RADIUS` for defense buildings else `BUILD_RADIUS`) and `OUTPOST_RADIUS` around own
  outposts; footprint fill `rgba(120,255,140,0.28)` if ok else `rgba(255,70,60,0.38)`; sprite at alpha 0.65;
  brackets around `(x0-8, y0-8, w+16, h+16)` scale `w>1 ? 1 : 0.6`.
- Drag box (screen space): fill `rgba(255,255,255,0.12)`, stroke `#ffffff` width `round(2*dpr)`, shown once the
  drag exceeds `DRAG_MIN*dpr = 8*dpr`.

---------------------------------------------------------------------------------------------------------

## 9. Presentation events and the sim boundary

### 9.1 Event queue (entities.js:56-89; net.js:21, 186-188, 280)

`emit(ev)` = `showEvent(ev)` locally **and**, when hosting, push into `Net.events` (sent with the next
snapshot; clients call `showEvent` for each). Recipient index = faction idx, −1 = everyone;
`mine(i) = i < 0 || (player && player.idx == i)`.

| Event | Payload | Client effect |
|---|---|---|
| `fx` | name, x, y, scale, dy, followId, color | spawn FX (8.4) |
| `prop` | name, x, y | add a prop to `G.decor` (e.g. `'chest_open'` when a chest is taken) |
| `snd` | name, x, y | `Sfx.hear` (positional, everyone) |
| `play` | factionIdx, name | `Sfx.play(name)` at full volume if mine |
| `float` | factionIdx, x, y, str, resKind | floating text if mine |
| `toast` | factionIdx, text | toast if mine |
| `alert` | factionIdx, x, y | `Ctl.alert` (minimap ping, banner, horn) if exactly mine |
| `say` | toIdx, text, colourFactionIdx, dur | banner if mine and fewer than 4 queued |

Rust: the sim crate should output a `Vec<PresentationEvent>` per tick with exactly these variants; the server
broadcasts them; solo mode consumes them directly.

### 9.2 Simulation code that depends on the camera / randomness (must be decided in the port)

- `onScreen(x, y, m=80)` (entities.js:44-47) gates several `addFx` and the `chop` sound on the **local camera**;
  when hosting it always returns true. On a dedicated Rust server always emit (and let clients cull), or
  emit unconditionally and cull in the client.
- `Ctl.alert` plays `horn` and shows the banner `'Il tuo regno è sotto attacco!'` (dur 2.6) only if the
  alert point is **not** on screen (`onScreen(x, y, 0)`, control.js:116-123). Client-side logic.
- `twinkle` (gold shine) runs in `updateResources` but is purely cosmetic and uses `Math.random` → move to
  the client. `fireSeed` likewise cosmetic per client.
- `hurtT` is sim state (0.12 s) but only used for the blink → include a `hurt` flag in snapshots.

---------------------------------------------------------------------------------------------------------

## 10. Sound effects (sfx.js; all sounds are synthesized, the asset pack has no audio)

### 10.1 Engine (sfx.js:4-41)

- State: `ctx` (AudioContext, created lazily), `enabled = true`, `last = {}` (per-name last play time, ms).
- `unlock()` — on every `keydown` and canvas `mousedown` (main.js:189, 206): create the context if missing,
  `resume()` if suspended (browser autoplay policy; the WASM build needs the same "first gesture" resume).
- No master gain/compressor: every voice connects straight to the output (sums may exceed 1.0 → clip).
- **`tone(freq, dur, type, vol, slideTo?)`** — one oscillator voice starting now:
  - waveform `type` ∈ `sine | square | triangle | sawtooth` (WebAudio built-ins: band-limited, built from
    sine-only Fourier series and normalised to peak 1: square `b_n = 4/(nπ)` odd n; sawtooth
    `b_n = (−1)^(n+1)·2/(nπ)`; triangle `b_n = 8·sin(nπ/2)/(nπ)²`; all start at phase 0).
  - frequency: `freq` at start; if `slideTo` given, exponential ramp to `slideTo` at `dur`:
    `f(τ) = freq·(slideTo/freq)^(τ/dur)`; phase = 2π∫f.
  - gain: `vol` at start, exponential ramp to `0.0001` at `dur`: `g(τ) = vol·(0.0001/vol)^(τ/dur)`.
  - voice stops at `dur`.
- **`noise(dur, vol, cutoff)`** — white-noise burst:
  - buffer length `max(1, floor(sampleRate*dur))`, sample `i` = `(rand*2−1)·(1 − i/len)` (linear fade-out
    baked in), fresh random noise every call.
  - → BiquadFilter `lowpass`, `frequency = cutoff`, default `Q = 1` (WebAudio lowpass Q is in dB:
    `alpha = sin(w0) / (2·10^(Q/20))`), → constant gain `vol` → output.
- **`play(name, vol = 1)`** (sfx.js:43-69):
  ```
  if !enabled || !ctx || ctx.state != 'running' || vol <= 0.02: return
  now = performance.now()           // real ms
  if now - (last[name] || 0) < 45: return      // per-name throttle 45 ms
  last[name] = now
  v = 0.07 * vol
  ```
  Arpeggio notes are scheduled with `setTimeout` (real ms) and are not re-checked against `enabled`.
- **Positional**: `Sfx.at(name, x, y)` = `emit(['snd', name, round(x), round(y)])` (sim side).
  `Sfx.hear(name, x, y)`: return if `!enabled || !ctx || G.demo` (**menu demo battle is silent**);
  `d = hypot(x − (cam.x + viewW/2), y − (cam.y + viewH/2))` (world px from the view centre);
  `play(name, 1 − d/(viewW*0.75))` → audible within 0.75 view-widths, linear falloff, skipped below 0.02.
  The throttle is only consumed by sounds that pass the volume check.
- **Mute**: key `M` toggles `Sfx.enabled` (main.js:242), also the round speaker buttons (ui.js:222, 486, 616).
  Not persisted.

### 10.2 Sound table (v = 0.07 × vol). All parts start at t = 0 unless an offset is given.

| Name | Voices |
|---|---|
| `hit` | noise(0.09 s, 2.2v, lowpass 1800) + tone(180 Hz, 0.08 s, square, 0.6v, slide→90) |
| `block` | tone(900, 0.07, square, 0.6v, →500) + noise(0.05, 1.5v, 5000) |
| `shoot` | tone(700, 0.12, triangle, v, →250) |
| `chop` | noise(0.06, 1.6v, 900) |
| `coin` | tone(990, 0.07, square, 0.7v) + tone(1480, 0.12, square, 0.6v) (simultaneous) |
| `heal` | tone(520, 0.25, sine, 1.4v, →1040) |
| `build` | tone(330, 0.1, triangle, 1.4v) + tone(495, 0.18, triangle, 1.2v) |
| `death` | tone(260, 0.22, sawtooth, 0.7v, →70) |
| `boom` | noise(0.5, 3.5v, 500) + tone(90, 0.4, sine, 2.5v, →40) |
| `click` | tone(600, 0.05, square, 0.7v) |
| `select` | tone(520, 0.04, triangle, 0.9v) |
| `ack` | tone(440, 0.05, triangle, v) + tone(660, 0.07, triangle, 0.8v) |
| `research` | 660 / 880 / 1100 Hz at 0 / 90 / 180 ms, each tone(f, 0.16, triangle, 1.3v) — total 0.34 s |
| `age` | 392 / 523 / 659 / 784 / 1047 Hz at i·140 ms, each tone(f, 0.35, sawtooth, 1.1v) — total 0.91 s |
| `deny` | tone(160, 0.14, square, 0.8v, →110) |
| `horn` | tone(196, 0.5, sawtooth, 0.9v) + tone(294, 0.6, sawtooth, 0.6v) |
| `win` | 523 / 659 / 784 / 1047 Hz at i·130 ms, each tone(f, 0.3, triangle, 1.6v) — total 0.69 s |
| `lose` | 392 / 330 / 262 / 196 Hz at i·180 ms, each tone(f, 0.4, sawtooth, v) — total 0.94 s |

Rust: pre-render each name once at `vol = 1` into a mono PCM buffer (e.g. 44.1/48 kHz f32) following the
formulas above (noise from a seeded RNG is acceptable; optionally keep 2-4 noise variants), play with a
volume scalar `vol`, keep the 45 ms per-name throttle and the 0.02 cut-off. Peak voice gain is
`3.5·0.07 = 0.245` (boom noise).

### 10.3 When each sound plays

Positional (`Sfx.at` → everyone near the camera hears it):

| Sound | Trigger | file:line | Position |
|---|---|---|---|
| `shoot` | arrow fired (archer, tower, castle) | entities.js:447 | arrow origin |
| `shoot` | catapult rock fired | entities.js:456 | rock origin `(x ± 20, y − 50)` |
| `heal` | monk heal lands | entities.js:473 | target |
| `hit` | melee hit lands (non-ram) | entities.js:477 | **attacker** |
| `boom` | ram hit lands | entities.js:477 | ram |
| `block` | shield block (warrior/lancer, chance `def.block`) | entities.js:502 | target |
| `death` | unit killed | entities.js:520 | unit |
| `chop` | pawn working (gather any resource *or* build/repair) each time `workT` crosses a multiple of 0.6 s; gated by `onScreen` | entities.js:792 (via 842, 885) | pawn |
| `coin` | human faction's pawn deposits resources | entities.js:801 | pawn (heard by anyone nearby) |
| `build` | building relocated | entities.js:1108 | building |
| `boom` | building destroyed | entities.js:1148 | building |
| `boom` | catapult rock impact | entities.js:1233 | impact point |
| `hit` | arrow hits a unit (no sound when an arrow hits a building) | entities.js:1252 | arrow |

Faction-only (`emit(['play', idx, name])`, full volume, never in the demo):

| Sound | Trigger | file:line |
|---|---|---|
| `build` | point captured (capturer) | entities.js:209 |
| `build` | building completed (owner, not walls) | entities.js:1134 |
| `coin` | pickup / chest collected | entities.js:1286 |
| `age` | age2 / age3 reached | factions.js:107 |
| `research` | other tech completed | factions.js:122 |
| `horn` | AI launches an attack wave on a human | factions.js:745 |
| `win` | a human's attack eliminates a faction | factions.js:911 |

Local UI (`Sfx.play` direct):

| Sound | Trigger | file:line |
|---|---|---|
| `win` / `lose` | match result decided (`G.checkEnd`) | main.js:111 |
| `horn` | `alert` event for an off-screen point | control.js:121 |
| `select` | click selection; select-all-army | control.js:174, 345 |
| `ack` | right-click order; set rally; recall; send-army map confirm | control.js:203, 211, 353; panels.js:195 |
| `click` / `deny` | building placed / refused; pick-move / key confirm; context action; UI buttons; build menu | control.js:263, 312, 398, 429-430; panels.js:22, 40, 76, 78, 107, 346; ui.js:135, 153, 211, 217, 413, 572, 588 |
| `deny` | F4 in network game (toast `'La modalità test c\'è solo nelle partite da soli'`); `'Non hai truppe'` | main.js:258; control.js:343, 350 |

---------------------------------------------------------------------------------------------------------

## 11. Open questions / ambiguities

1. **Shadow direction discontinuities** (2.3): keep bit-exact (hard switches at night 0.9 and at the phase
   wrap) or smooth them in the 3D port? Recommendation: keep the formulas but lerp the sun direction over
   ~1 s of sim time when it jumps.
2. **Sun shafts use real time** (`G.clock`) while everything else uses sim time — they keep drifting when
   paused. Keep as is (cosmetic).
3. **Camera-dependent FX/sound gating in sim** (`onScreen`, 9.2) must move out of the authoritative sim;
   proposed: always emit, cull on client by view distance (margin 80 world px for FX).
4. 2D-specific techniques (half-res shadow layer, 1/8 light map, quarter-res post) are performance tricks;
   the HD-2D renderer should reproduce the *look* (values: ambient colours, shade, light radii/powers,
   flicker, 22 %/18 % blur bands, bloom of `c³` at 0.34, vignette stops, shaft parameters), not the method.
5. Cloud shadows are offset by a fixed (+90, +260) plus `lx*60` and are not sheared; with real 3D clouds they
   would come naturally from the sun direction — acceptable deviation?
6. WebAudio band-limited oscillators vs naive waveforms: minor timbre differences are acceptable; noise is
   re-randomised per play in JS.

---

## Addenda (critic)

**L1. Camera-dependent effects and sounds — decision: always emit, client culls.** Full list of sim call sites that
test `onScreen` (entities.js:44-47): 162 (tree regrow `dust1`), 188 (wild sheep respawn `dust1`), 792 (`chop` sound,
once per 0.6 s of work), 917 / 929 (enter / leave shelter `dust1`, dy −6), 1025 (running `dust1` every
`rand(0.7,1.5)` s), 1165 (deploy `dust2`). On a network host `onScreen` is always true, so in multiplayer they are
already always emitted; the Rust sim emits them unconditionally and the client drops events outside its view
(margin 80 px world as in `onScreen`'s default `m = 80`). The `chop` sound additionally goes through the client's
distance falloff and 45 ms repeat limit (§10).

**L2. Shadow direction jumps — decision: smooth.** The HD-2D port uses a real directional light; interpolate the sun
direction continuously over the day/night timeline (§2) instead of the 2D jumps at night > 0.9 and at the dawn
wrap. Colour keyframes and timings stay exact. Cloud shadows: may follow the sun direction (not gameplay).

**L3. Sun rays on real time** — use presentation time that stops when the solo game is paused (the only place where
real vs sim time differ visibly).

**L4. `bandMask`** (light.js:228-239, used at :262; diorama top/bottom blur mask: vertical gradient, 9 stops `t = i/8`, alpha `1 - t²` placed at `t·top/h` and `1 - t·bottom/h`;
cached by `h:top:bottom`) is a Canvas technique; in Bevy reproduce it as the screen-space tilt-shift
weight (see bevy notes open question). [RENDER]
