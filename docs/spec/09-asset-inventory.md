# 09 — Asset inventory, tileset anatomy & reference-map aesthetics (porting spec)

Sources read for this chapter: every PNG under `assets/` (588 files, dimensions read from the PNG `IHDR`, pixels
decoded and inspected: frame bounding boxes, palettes, seams, alpha), `js/manifest.js` (592 l.), `js/extra.js`
(493 l.), `js/assets.js` (611 l.), `js/world.js` (931 l., tile/decoration parts), the draw functions of
`js/entities.js` (1300-1445), `js/main.js` (render/water/clouds 55-63, 315-376), `js/ui.js` (buttons 125-155,
cursor 650-662), `tools/make_sprites.py` (380 l.), `tools/raw/SOURCES.md`, `Reference/Map-example.png`.

Machine-readable companion: **`docs/spec/assets.json`** (588 entries, one per PNG, see §1.2).

Conventions (same as chapter 01):
- `fw × fh` = frame size; `frames` = number of frames in a **horizontal strip**, frame `i` is the rect
  `(i·fw, 0, fw, fh)`. The JS derives it as `n = round(imageWidth / fw)`, `fh = imageHeight` (`assets.js:81-89`).
- `(ax, ay)` = anchor (pivot) in frame pixels; the sprite is drawn so that `(ax, ay)` lands on the entity's foot
  point `(x, y)` (`drawSheet`, `assets.js:91-103`). Flip = mirror around the anchor (`scale(-1,1)`).
  Billboard pivot in normalised UV (origin bottom-left) = `(ax/fw, 1 - ay/fh)`.
- `fps` = playback rate used by the JS. Frame index = `floor(t · fps + phase) mod frames` (looping) unless noted.
- **[RENDER]** = pure presentation (client only). **[SIM!]** = the simulation depends on a number that comes from a
  sprite sheet (frame counts/fps feeding attack timing, chapter 01/03a) and must be carried as data by the Rust sim.
- Pixel art: every sprite is drawn with nearest-neighbour sampling (`imageSmoothingEnabled = false`), positions
  rounded to integer pixels. Keep `ImageSampler::nearest()` in Bevy for all of these textures.

User-visible Italian strings that belong to asset loading (keep verbatim):
- `'Impossibile caricare assets/' + f` — load error (`assets.js:24`), shown centred on the loading screen.
- `'Sprite mancante: ' + path` — thrown for an unknown path (`assets.js:73`).
- `'Tiny Swords'` — loading-screen title (`main.js:430`); loading-screen background `#47aba9`.

---

## 0. Summary of findings

1. **588 PNG** = 410 from the *Tiny Swords (Free Pack)* + **178** generated for this game in `assets/Extra/`
   (README:6, 147). The five colour sets (Blue, Red, Yellow, Purple, Black) of units, buildings and Extra
   units/buildings have **identical dimensions** file-by-file (verified with a diff) — one table row per Blue file
   below stands for all five colours; `assets.json` lists all 588.
2. All animated sheets are horizontal strips. Frame sizes: units 192×192 (Lancer 320×320), Extra units 96/128/160,
   trees 192 wide (256 or 192 tall), bushes/sheep/gold-highlight 128, water rocks/dust/fire 64, foam/explosion/splash
   192, duck 32.
3. The terrain tileset is **only grass + rock cliff + grass ramps**. **There is no sand, dirt, path or water tile**
   in this version of the pack (water is a flat colour `#47aba9`). Sand/dirt for voxel faces must be derived
   (palette remap proposed in §3.7).
4. The 5 `Tilemap_colorN.png` are pixel-identical except the 3 grass colours (cliffs/foam/outline identical).
   The reference image uses them **by elevation tier** (low = color3, mid = color1, top = color2, forest floor =
   color5) — this is the single biggest aesthetic lever (§11).
5. Every semi-transparent pixel in the pack is a **baked ground shadow** (navy, alpha 69–119): units, trees,
   bushes, buildings, resources, clouds. For HD-2D billboards these pixels must be stripped (they would stand up
   vertically) and replaced by projected/blob shadows (§12).

---

## 1. Method, totals, JSON format

### 1.1 Totals per top folder

| Folder | Files | Notes |
|---|---|---|
| `assets/Units/<Color> Units/{Archer,Lancer,Monk,Pawn,Warrior}` | 5 × 45 = 225 | animated strips |
| `assets/Buildings/<Color> Buildings` | 5 × 8 = 40 | single images |
| `assets/Terrain/Tileset` | 8 | 5 tilesets, foam, shadow, water colour |
| `assets/Terrain/Decorations` | 21 | bushes 4, clouds 8, rocks 4, water rocks 4, duck 1 |
| `assets/Terrain/Resources` | 31 | gold 14, meat 4, tools 4, wood 9 |
| `assets/Particle FX` | 8 | |
| `assets/UI Elements` | 77 | 70 in `UI Elements/`, 7 in `UI Banners from the store page/` |
| `assets/Extra` (generated) | 178 | 41 buildings, 55 units, 6 resources, 48 props, 28 icons (see §10) |
| **Total** | **588** | |

### 1.2 `assets.json` format

```json
{"path":"assets/Units/Blue Units/Lancer/Lancer_Idle.png","w":3840,"h":320,"fw":320,"fh":320,"frames":12,
 "kind":"anim","ax":160,"ay":196,"fps":10}
```
- Required: `path` (repo-relative), `w`, `h`, `fw`, `fh`, `frames`.
- Extra fields: `kind` ∈ `anim` (strip), `single`, `tileset` (64×64 grid, `frames` = 54 cells = 9×6),
  `tileset-strips` (Shadow.png), `fill` (water colour), `nineslice`, `threeslice`, `rows-3slice`
  (BigRibbons/Swords: 5 rows of 128; SmallRibbons: 10 rows of 64 — `fw×fh` = one row, `frames` = rows),
  `atlas` (store ribbons, hand-cut regions), `portrait` (avatars), `icon` (single UI images, tools, cursors, small
  buttons, bar fills). `ax`, `ay`, `fps` are present where the JS defines them (copied from `assets.js`,
  `extra.js`, `entities.js`, `main.js`; props: `ax = floor(w/2)`, `ay = h`).

---

## 2. Inventory tables (grouped by folder)

### 2.1 `assets/Units/<Color> Units/` — 45 files per colour (×5)

Anchor for all 192-px unit frames: `(96, 134)`; Lancer 320-px frames: `(160, 196)` (`UNIT_DEF`, `assets.js:117-152`).
`fps` from `UNIT_ANIM` (`assets.js:177-200`). "content" = union bbox of opaque pixels over all frames (for billboard
quad sizing).

| File (`<Color> Units/…`) | w × h | frame | frames | fps | content bbox (x0,y0..x1,y1) | Purpose / anim key |
|---|---|---|---|---|---|---|
| `Archer/Archer_Idle.png` | 1152×192 | 192×192 | 6 | 8 | 56,47..127,135 | archer `idle` |
| `Archer/Archer_Run.png` | 768×192 | 192×192 | 4 | 10 | 57,46..129,135 | archer `run` |
| `Archer/Archer_Shoot.png` | 1536×192 | 192×192 | 8 | 14 | 53,46..139,135 | archer `attack` (hitFrame 5 = arrow release) [SIM!] |
| `Archer/Arrow.png` | 64×64 | 64×64 | 1 | – | 10,26..52,37 | projectile, drawn centred `(-32,-32)` and rotated by flight angle (`entities.js:1431-1441`) |
| `Lancer/Lancer_Idle.png` | 3840×320 | 320×320 | 12 | 10 | 114,48..183,197 | lancer `idle` |
| `Lancer/Lancer_Run.png` | 1920×320 | 320×320 | 6 | 10 | 110,54..181,202 | lancer `run` |
| `Lancer/Lancer_Up_Attack.png` | 960×320 | 320×320 | 3 | 9 | 122,46..173,196 | `attackUp` |
| `Lancer/Lancer_UpRight_Attack.png` | 960×320 | 320×320 | 3 | 9 | 131,88..259,194 | `attackUpRight` |
| `Lancer/Lancer_Right_Attack.png` | 960×320 | 320×320 | 3 | 9 | 121,123..306,197 | `attackRight` |
| `Lancer/Lancer_DownRight_Attack.png` | 960×320 | 320×320 | 3 | 9 | 121,131..242,273 | `attackDownRight` |
| `Lancer/Lancer_Down_Attack.png` | 960×320 | 320×320 | 3 | 9 | 141,137..187,301 | `attackDown` |
| `Lancer/Lancer_Up_Defence.png` | 1920×320 | 320×320 | 6 | 10 | 123,72..173,197 | `blockUp` |
| `Lancer/Lancer_UpRight_Defence.png` | 1920×320 | 320×320 | 6 | 10 | 133,105..242,195 | `blockUpRight` |
| `Lancer/Lancer_Right_Defence.png` | 1920×320 | 320×320 | 6 | 10 | 122,123..274,197 | `blockRight` |
| `Lancer/Lancer_DownRight_Defence.png` | 1920×320 | 320×320 | 6 | 10 | 126,132..228,257 | `blockDownRight` |
| `Lancer/Lancer_Down_Defence.png` | 1920×320 | 320×320 | 6 | 10 | 142,138..187,279 | `blockDown` |
| `Monk/Idle.png` | 1152×192 | 192×192 | 6 | 8 | 67,65..124,133 | monk `idle` |
| `Monk/Run.png` | 768×192 | 192×192 | 4 | 10 | 55,63..137,133 | monk `run` |
| `Monk/Heal.png` | 2112×192 | 192×192 | 11 | 12 | 35,63..155,133 | monk `attack` (= heal cast, hitFrame 6) [SIM!] |
| `Monk/Heal_Effect.png` | 2112×192 | 192×192 | 11 | 12 | 47,18..144,145 | FX `heal` on the healed unit, anchor (96,134), follows the target (`assets.js:409`, `entities.js:472`) |
| `Pawn/Pawn_Idle.png` | 1536×192 | 192×192 | 8 | 8 | 67,61..124,134 | pawn `idle` (empty-handed) |
| `Pawn/Pawn_Idle {Axe,Pickaxe,Knife,Hammer}.png` | 1536×192 | 192×192 | 8 | 8 | ≈56..141, 61..134 | `idleAxe` … holding the job tool |
| `Pawn/Pawn_Idle {Wood,Gold,Meat}.png` | 1536×192 | 192×192 | 8 | 8 | ≈67..131, 61..134 | `idleWood` … carrying a load |
| `Pawn/Pawn_Run.png` | 1152×192 | 192×192 | 6 | 10 | 65,58..130,134 | pawn `run` |
| `Pawn/Pawn_Run {Axe,Pickaxe,Knife,Hammer}.png` | 1152×192 | 192×192 | 6 | 10 | ≈54..144, 58..134 | `runAxe` … |
| `Pawn/Pawn_Run {Wood,Gold,Meat}.png` | 1152×192 | 192×192 | 6 | 10 | ≈68..137, 54..134 | `runWood` … |
| `Pawn/Pawn_Interact Axe.png` | 1152×192 | 192×192 | 6 | 10 | 56,60..153,134 | `workAxe` (chop) |
| `Pawn/Pawn_Interact Pickaxe.png` | 1152×192 | 192×192 | 6 | 10 | 52,60..152,134 | `workPickaxe` (mine gold/stone) |
| `Pawn/Pawn_Interact Knife.png` | 768×192 | 192×192 | 4 | 10 | 61,60..140,140 | `workKnife` (butcher sheep) |
| `Pawn/Pawn_Interact Hammer.png` | 576×192 | 192×192 | 3 | 10 | 71,65..154,137 | `workHammer` (build/repair) |
| `Warrior/Warrior_Idle.png` | 1536×192 | 192×192 | 8 | 8 | 59,48..142,136 | warrior `idle` |
| `Warrior/Warrior_Run.png` | 1152×192 | 192×192 | 6 | 10 | 53,46..145,136 | warrior `run` |
| `Warrior/Warrior_Attack1.png` | 768×192 | 192×192 | 4 | 12 | 43,44..162,155 | `attack1` (hitFrame 2) [SIM!] |
| `Warrior/Warrior_Attack2.png` | 768×192 | 192×192 | 4 | 12 | 41,48..158,151 | `attack2` (alternates with attack1) |
| `Warrior/Warrior_Guard.png` | 1152×192 | 192×192 | 6 | 10 | 57,44..126,136 | `block` |

Notes:
- Units face right in the art; left = horizontal flip (`u.flip`). Lancer attack/defence have 5 directions
  `['Up','UpRight','Right','DownRight','Down']` (`assets.js:172`); the 3 left directions are flips of the right ones.
- The pawn carrying **stone** uses Extra sheets `Pawn_Idle Stone` / `Pawn_Run Stone` (§2.8.2).
- Attack animations do not loop: while `state === 'attack'` the frame is clamped to `n-1` (`entities.js:1306`).
- Damaged-unit flash: `u.hurtT > 0` → drawn at alpha 0.5 (`entities.js:1314`).
- Colours: Blue/Red/Yellow/Purple/Black only differ by palette swap of the team colours.

### 2.2 `assets/Buildings/<Color> Buildings/` — 8 files per colour (×5), single images

Anchor = point placed on the centre of the bottom edge of the footprint (`BUILD_DEF`, `assets.js:224-293`).

| File | w × h | anchor (ax,ay) | crop (visible part, for thumbnails) | footprint (cells) | Purpose |
|---|---|---|---|---|---|
| `Castle.png` | 320×256 | 160,250 | 4,41,312,208 | 5×2 | `castle` |
| `House1.png` | 128×192 | 64,178 | 8,16,112,157 | 2×1 | `house` variant 0 |
| `House2.png` | 128×192 | 64,178 | (same crop) content 0,23..127,177 | 2×1 | `house` variant 1 |
| `House3.png` | 128×192 | 64,178 | (same crop) content 3,37..124,171 | 2×1 | `house` variant 2 |
| `Barracks.png` | 192×256 | 96,246 | 4,58,184,187 | 3×2 | `barracks` |
| `Archery.png` | 192×256 | 96,242 | 3,61,183,179 | 3×2 | `archery` |
| `Tower.png` | 128×256 | 64,232 | 4,46,120,184 | 2×1 | `tower` |
| `Monastery.png` | 192×320 | 96,312 | 16,45,160,265 | 3×2 | `monastery` |

Construction rendering (`entities.js:1335-1356`): unfinished → full sprite at alpha 0.3, then the bottom
`im.height · clamp(progress/work,0,1)` rows drawn opaque (builds "from the ground up"). Fire on damaged buildings: §8.

### 2.3 `assets/Terrain/Tileset/`

| File | w × h | frame | frames | Purpose |
|---|---|---|---|---|
| `Tilemap_color1.png` | 576×384 | 64×64 grid | 9×6 = 54 cells | yellow-green grass tileset; the JS uses it for **all level-0 ground** (`world.js:832-837`) |
| `Tilemap_color2.png` | 576×384 | 64×64 | 54 | green grass variant |
| `Tilemap_color3.png` | 576×384 | 64×64 | 54 | emerald grass variant |
| `Tilemap_color4.png` | 576×384 | 64×64 | 54 | olive / dry grass variant |
| `Tilemap_color5.png` | 576×384 | 64×64 | 54 | teal / swampy grass variant |
| `Water Foam.png` | 3072×192 | 192×192 | 16 | animated shore foam ring, fps 10, anchor (96,96) = cell centre (§4.2) |
| `Shadow.png` | 192×192 | 3 strips 64×192 | 1 | cliff-foot shadow blob (§5) |
| `Water Background color.png` | 64×64 | 64×64 | 1 | single colour `#47aba9` (RGBA 71,171,169,255) (§4.1) |

Full anatomy in §3.

### 2.4 `assets/Terrain/Decorations/`

| File | w × h | frame | frames | fps | anchor | content bbox | Purpose |
|---|---|---|---|---|---|---|---|
| `Bushes/Bushe1.png` | 1024×128 | 128×128 | 8 | 8 | 64,80 | 31,33..97,78 | large round yellow-green bush, wind sway |
| `Bushes/Bushe2.png` | 1024×128 | 128×128 | 8 | 8 | 64,80 | 41,42..86,75 | small round green bush |
| `Bushes/Bushe3.png` | 1024×128 | 128×128 | 8 | 8 | 64,80 | 23,25..101,83 | large spiky fern-bush with yellow sprigs |
| `Bushes/Bushe4.png` | 1024×128 | 128×128 | 8 | 8 | 64,80 | 40,37..85,78 | small spiky fern-bush |
| `Clouds/Clouds_01.png` | 576×256 | single | 1 | – | top-left | 38,42..532,198 | big bright cloud (with baked shadow) |
| `Clouds/Clouds_02.png` | 576×256 | single | 1 | – | top-left | 133,85..439,194 | medium bright cloud |
| `Clouds/Clouds_03.png` | 576×256 | single | 1 | – | top-left | 205,102..369,176 | small bright cloud |
| `Clouds/Clouds_04.png` | 576×256 | single | 1 | – | top-left | 234,115..339,171 | tiny bright cloud |
| `Clouds/Clouds_05.png` | 576×256 | single | 1 | – | top-left | 76,49..491,200 | big dull/cream cloud |
| `Clouds/Clouds_06.png` | 576×256 | single | 1 | – | top-left | 141,90..446,188 | medium dull cloud |
| `Clouds/Clouds_07.png` | 576×256 | single | 1 | – | top-left | 212,92..367,174 | small dull cloud |
| `Clouds/Clouds_08.png` | 576×256 | single | 1 | – | top-left | 238,109..325,163 | tiny dull cloud |
| `Rocks/Rock1.png` | 64×64 | single | 1 | – | 32,50 | 15,25..46,50 | small land rock |
| `Rocks/Rock2.png` | 64×64 | single | 1 | – | 32,50 | 7,17..56,52 | medium land rock |
| `Rocks/Rock3.png` | 64×64 | single | 1 | – | 32,50 | 14,23..52,51 | small land rock 2 |
| `Rocks/Rock4.png` | 64×64 | single | 1 | – | 32,50 | 2,14..60,55 | large land rock |
| `Rocks in the Water/Water Rocks_01.png` | 1024×64 | 64×64 | 16 | 10 | 32,36 | 9,25..49,53 | small rock + ripple ring |
| `Rocks in the Water/Water Rocks_02.png` | 1024×64 | 64×64 | 16 | 10 | 32,36 | 1,17..61,55 | medium rock pair + ripple |
| `Rocks in the Water/Water Rocks_03.png` | 1024×64 | 64×64 | 16 | 10 | 32,36 | 7,23..56,54 | small rock 2 + ripple |
| `Rocks in the Water/Water Rocks_04.png` | 1024×64 | 64×64 | 16 | 10 | 32,36 | 0,14..63,58 | large rock cluster + ripple |
| `Rubber Duck/Rubber duck.png` | 96×32 | 32×32 | 3 | 4 | 16,18 | 0,4..31,27 | easter-egg duck swimming on water (§4.4) |

### 2.5 `assets/Terrain/Resources/`

| File | w × h | frame | frames | fps | anchor | content bbox | Purpose |
|---|---|---|---|---|---|---|---|
| `Gold/Gold Resource/Gold_Resource.png` | 128×128 | single | 1 | – | 64,70 | 51,49..74,74 | gold nugget pickup (drawn ×1.5) |
| `Gold/Gold Resource/Gold_Resource_Highlight.png` | 768×128 | 128×128 | 6 | 10 | 64,70 | same | its shine (first 0.6 s of every 2.2 s) |
| `Gold/Gold Stones/Gold Stone 1.png` … `6.png` | 128×128 | single | 1 | – | 64,64 | 1: 48,50..79,78 · 2: 49,47..81,74 · 3: 34,36..91,87 · 4: 35,31..97,85 · 5: 28,20..101,99 · 6: 17,17..108,97 | gold node, **stage = clamp(ceil(amount/40),1,6)** (6 = full 240) |
| `Gold/Gold Stones/Gold Stone N_Highlight.png` (N=1..6) | 768×128 | 128×128 | 6 | 10 | 64,64 | as above | shine played when `shine ∈ (0, 0.6]` |
| `Meat/Meat Resource/Meat Resource.png` | 64×64 | single | 1 | – | 32,42 | 9,16..55,51 | meat pickup (×1.5) |
| `Meat/Sheep/Sheep_Idle.png` | 768×128 | 128×128 | 6 | 8 | 64,82 | 41,40..85,83 | sheep `idle` |
| `Meat/Sheep/Sheep_Move.png` | 512×128 | 128×128 | 4 | 8 | 64,82 | 38,37..89,83 | sheep `move` |
| `Meat/Sheep/Sheep_Grass.png` | 1536×128 | 128×128 | 12 | 10 | 64,82 | 40,40..87,83 | sheep `grass` (grazing) |
| `Tools/Tool_01.png` | 64×64 | single | 1 | – | centred | 20,17..47,44 | hammer — cursor `hammer` |
| `Tools/Tool_02.png` | 64×64 | single | 1 | – | centred | 18,20..44,42 | axe — cursor `axe` |
| `Tools/Tool_03.png` | 64×64 | single | 1 | – | centred | 23,22..41,41 | knife — cursor `knife` |
| `Tools/Tool_04.png` | 64×64 | single | 1 | – | centred | 21,19..43,43 | pickaxe — cursor `pick` (`assets.js:420`, `ui.js:652`) |
| `Wood/Trees/Tree1.png` | 1536×256 | **192×256** | 8 | 8 | 96,240 | 37,51..157,240 | dark teal pine |
| `Wood/Trees/Tree2.png` | 1536×256 | **192×256** | 8 | 8 | 96,248 | 43,5..150,248 | tall bright-green pine |
| `Wood/Trees/Tree3.png` | 1536×192 | 192×192 | 8 | 8 | 96,170 | 49,23..141,169 | yellow-green birch (deciduous) |
| `Wood/Trees/Tree4.png` | 1536×192 | 192×192 | 8 | 8 | 96,168 | 56,46..136,167 | autumn orange birch |
| `Wood/Trees/Stump 1.png` | 192×256 | single | 1 | – | 96,238 | 75,200..122,239 | stump of Tree1 |
| `Wood/Trees/Stump 2.png` | 192×256 | single | 1 | – | 96,244 | 73,210..115,244 | stump of Tree2 |
| `Wood/Trees/Stump 3.png` | 192×256 | single | 1 | – | 96,231 | 85,197..108,231 | stump of Tree3 |
| `Wood/Trees/Stump 4.png` | 192×256 | single | 1 | – | 96,227 | 86,202..111,227 | stump of Tree4 |
| `Wood/Wood Resource/Wood Resource.png` | 64×64 | single | 1 | – | 32,42 | 9,18..55,45 | wood pickup (×1.5) |

Caution: tree sheets are 1536 wide but the frame is **192** (not 256): `treeSheet` = `sheet(..., 192, 96, TREE_DEF[v].ay, 8)`
(`assets.js:421-424`). Tree1/2 frames are 192×256, Tree3/4 192×192.

### 2.6 `assets/Particle FX/` — FX table `assets.js:400-410`

| File | w × h | frame | frames | fps | anchor | content bbox | FX key / use |
|---|---|---|---|---|---|---|---|
| `Dust_01.png` | 512×64 | 64×64 | 8 | 14 | 32,46 | 15,13..49,45 | `dust1`: footstep/puff (unit trains, sheep killed, tree regrow, pawn builds) |
| `Dust_02.png` | 640×64 | 64×64 | 10 | 14 | 32,56 | 0,4..63,60 | `dust2`: tree felled, building placed/finished (one per footprint column), boulder impact (×1.6) |
| `Explosion_01.png` | 1536×192 | 192×192 | 8 | 14 | 96,110 | 63,60..126,123 | `boom1`: catapult rock impact, ram hit |
| `Explosion_02.png` | 1920×192 | 192×192 | 10 | 14 | 96,130 | 43,20..150,157 | `boom2`: building destroyed (random positions, scale 1–1.5 or 0.7) |
| `Fire_01.png` | 512×64 | 64×64 | 8 | 10 | 32,62 | 21,40..42,63 | small flame — building hp < 70% |
| `Fire_02.png` | 640×64 | 64×64 | 10 | 10 | 32,62 | 15,21..50,63 | medium flame — hp < 45% |
| `Fire_03.png` | 768×64 | 64×64 | 12 | 10 | 32,62 | 4,9..58,63 | big flame — hp < 25% |
| `Water Splash.png` | 1728×192 | 192×192 | 9 | 12 | 96,100 | 52,58..144,139 | random ambient splash on open water (§4.3) |

One-shot FX play once (removed when `t · fps ≥ frames`); FX are presentation events (`emit(['fx', name, x, y, scale,
dy, followId, color])`, `entities.js:85-88`) — **[RENDER]**, the sim only emits events.

### 2.7 `assets/UI Elements/` (77 files) — details and slice geometry in §9

| File | w × h | Kind | Purpose (JS use) |
|---|---|---|---|
| `UI Elements/Banners/Banner.png` | 448×448 | 9-slice G7 | parchment scroll panel (`PANEL.banner`, pad 28,60,44,17) — tooltips/side panels |
| `UI Elements/Banners/Banner_Slots.png` | 192×192 | 9-slice G1 | tan inset slot (`PANEL.bannerSlots`, pad 5,5,2,4) |
| `UI Elements/Wood Table/WoodTable.png` | 448×448 | 9-slice G7 | wooden board with mint corner brackets (`PANEL.wood`, pad 44,43,44,25) |
| `UI Elements/Wood Table/WoodTable_Slots.png` | 192×192 | 9-slice G1 | brown inset (`PANEL.woodSlots`, pad 12,11,12,11) |
| `UI Elements/Papers/RegularPaper.png` | 320×320 | 9-slice G3 | cream paper (`PANEL.paper`, pad 12,20,12,19) |
| `UI Elements/Papers/SpecialPaper.png` | 320×320 | 9-slice G3 | dark slate paper with gold corners (`PANEL.special`, pad 9,20,9,21) |
| `UI Elements/Buttons/BigBlueButton_{Regular,Pressed}.png` | 320×320 | 9-slice G3 | big teal button (`bigButton`, pad 19,17,19,17) |
| `UI Elements/Buttons/BigRedButton_{Regular,Pressed}.png` | 320×320 | 9-slice G3 | big red button |
| `UI Elements/Buttons/Small{Blue,Red}{Round,Square}Button_{Regular,Pressed}.png` | 128×128 | single | 8 files; `smallButton` (`ui.js:140-146`) |
| `UI Elements/Buttons/Tiny{Round,Square}{Blue,Red}Button.png` | 64×64 | single | 4 files; `tinyButton` (`ui.js:148-155`); round blue also = slider knob (`panels.js:240`) |
| `UI Elements/Bars/BigBar_Base.png` | 320×64 | 3-slice G3 | wooden progress-bar frame (`bigBar`) |
| `UI Elements/Bars/BigBar_Fill.png` | 64×64 | single | red fill; source rect `(0,22,64,21)` |
| `UI Elements/Bars/SmallBar_Base.png` | 320×64 | 3-slice G3 | thin HP-bar frame (`smallBar`) |
| `UI Elements/Bars/SmallBar_Fill.png` | 64×64 | single | red line; source rect `(0,30,64,3)`; tinted per kingdom at load (`buildDerived`) |
| `UI Elements/Cursors/Cursor_01.png` | 64×64 | single | arrow cursor, hotspot (23,18)·s |
| `UI Elements/Cursors/Cursor_02.png` | 64×64 | single | hand cursor, hotspot (27,19)·s |
| `UI Elements/Cursors/Cursor_03.png` | 64×64 | single | "no" (forbidden) cursor, hotspot (32,31)·s |
| `UI Elements/Cursors/Cursor_04.png` | 128×128 | 4 corners 32×32 | selection brackets (`drawBrackets`) |
| `UI Elements/Human Avatars/Avatars_01..25.png` | 256×256 | portrait | 5 colours × 5 unit types; portrait occupies centre `(40,40,176,176)` |
| `UI Elements/Icons/Icon_01..12.png` | 64×64 | icon | see §9.6 |
| `UI Elements/Ribbons/BigRibbons.png` | 448×640 | rows-3slice G7 | 5 rows × 128 px, one per colour (`bigRibbon`) |
| `UI Elements/Ribbons/SmallRibbons.png` | 320×640 | rows-3slice G3 | 10 rows × 64 px = 2 variants per colour (`smallRibbon`) |
| `UI Elements/Swords/Swords.png` | 448×640 | rows-3slice G7 | 5 rows × 128: sword-shaped banner per colour (`swordBanner`) |
| `UI Banners from the store page/Banner/Banner.png` | 704×512 | custom 9-slice | big menu scroll (`drawStoreBanner`) |
| `UI Banners from the store page/Banner/Slots.png` | 320×320 | 9-slice G3 | tan inset (`PANEL.storeSlots`, pad 43,8,40,17) |
| `UI Banners from the store page/Ribbons/Ribbon_{Black,Blue,Purple,Red,Yellow}.png` | 960×576 | atlas | title ribbon (row 0, 3-slice G7) + pennant `(51,448,90,108)` |

### 2.8 `assets/Extra/` (178 generated files) — see §10 for provenance

#### 2.8.1 `Extra/Buildings/` — 7 per colour ×5 = 35 + 6 neutral = 41

Anchor/crop from `EXTRA.buildings` (`extra.js:4-161`). All have a 6-px transparent pad and a baked ellipse shadow.

| File | w × h | anchor | crop | Footprint | Purpose |
|---|---|---|---|---|---|
| `<Color>/Camp.png` | 200×189 | 100,179 | 6,6,188,177 | 3×2 | `camp` (Accampamento) |
| `<Color>/Depot.png` | 136×118 | 68,108 | 6,6,124,106 | 2×1 | `depot` (Deposito) |
| `<Color>/Farm.png` | 202×167 | 101,157 | 6,6,190,155 | 3×2 | `farm` (Fattoria) |
| `<Color>/Market.png` | 202×207 | 101,197 | 6,6,190,195 | 3×2 | `market` (Mercato) |
| `<Color>/Siege.png` | 202×192 | 101,182 | 6,6,190,180 | 3×2 | `siege` (Officina d'assedio) |
| `<Color>/Smith.png` | 196×184 | 98,174 | 6,6,184,172 | 3×2 | `smith` (Fucina) |
| `<Color>/Stable.png` | 200×174 | 100,164 | 6,6,188,162 | 3×2 | `stable` (Scuderia) |
| `Wall.png` (neutral) | 76×78 | 38,68 | 6,6,64,66 | 1×1 | `wall` (Palizzata) — also sliced as a single stake, §2.8.6 |
| `Outpost.png` (neutral) | 168×230 | 84,220 | 6,6,156,218 | 2×2 point | capture point `outpost` |
| `Mine.png` | 168×150 | 84,140 | 6,6,156,138 | 2×2 point | `mine` |
| `Quarry.png` | 168×161 | 84,151 | 6,6,156,149 | 2×2 point | `quarry` |
| `Sawmill.png` | 168×162 | 84,152 | 6,6,156,150 | 2×2 point | `sawmill` |
| `Ranch.png` | 168×145 | 84,135 | 6,6,156,133 | 2×2 point | `ranch` |

Count check: 7 coloured kinds × 5 colours = 35, plus Wall, Outpost, Mine, Quarry, Sawmill, Ranch = 6 → 41.

#### 2.8.2 `Extra/Units/<Color>/` — 11 per colour ×5 = 55 (`EXTRA.units`, `extra.js:162-240`)

| File | w × h | frame | frames | fps | anchor | content bbox | Purpose |
|---|---|---|---|---|---|---|---|
| `Knight_Idle.png` | 768×128 | 128×128 | 6 | 8 | 64,112 | 13,10..114,119 | knight `idle` |
| `Knight_Run.png` | 768×128 | 128×128 | 6 | 12 | 64,113 | 13,10..113,120 | knight `run` |
| `Knight_Attack.png` | 960×128 | **160×128** | 6 | 12 | 80,113 | 33,13..148,120 | knight `attack` (hitFrame 3) [SIM!] |
| `Ram_Run.png` | 512×128 | 128×128 | 4 | 8 (idle: 0 = frame 0 frozen) | 64,117 | 8,30..119,125 | ram `run`/`idle` |
| `Ram_Attack.png` | 640×128 | **160×128** | 4 | 7 | 80,118 | 24,32..146,126 | ram `attack` (hitFrame 2) [SIM!] |
| `Catapult_Run.png` | 384×96 | 96×96 | 4 | 8 (idle: 0) | 48,85 | 4,32..92,92 | catapult `run`/`idle` |
| `Catapult_Attack.png` | 384×96 | 96×96 | 4 | 7 | 48,86 | 4,7..92,93 | catapult `attack` (hitFrame 2) [SIM!] |
| `Scout_Idle.png` | 576×96 | 96×96 | 6 | 8 | 48,86 | 18,18..78,93 | scout `idle` |
| `Scout_Run.png` | 576×96 | 96×96 | 6 | 10 | 48,86 | 18,18..78,93 | scout `run` |
| `Pawn_Idle Stone.png` | 1536×192 | 192×192 | 8 | 8 | 96,134 | 67,61..130,134 | pawn `idleStone` (gold-carry sheet with grey nugget) |
| `Pawn_Run Stone.png` | 1152×192 | 192×192 | 6 | 10 | 96,134 | 68,58..132,134 | pawn `runStone` |

Units without `avatar` (scout, knight, ram, catapult) get their UI portrait from the first idle frame
(`drawPortrait`, `assets.js:577-581`): square of side `min(fw,fh)·0.86` centred horizontally, top at `ay − side·0.9`.

#### 2.8.3 `Extra/Resources/` — 6 files (`EXTRA.stones`, `EXTRA.deposits`)

| File | w × h | anchor | Purpose |
|---|---|---|---|
| `Stone_1.png` | 44×34 | (w/2, h−22) | stone node, amount ≤ 80 |
| `Stone_2.png` | 71×52 | (w/2, h−22) | stone node, 80 < amount ≤ 190 |
| `Stone_3.png` | 84×86 | (w/2, h−22) | stone node, amount > 190 (max 300) |
| `Stone_Resource.png` | 26×22 | 13,20 (pickup, ×1.5) / 13,11 (catapult rock, ×1.3, rotated) | stone pickup + catapult projectile |
| `Gold_Big.png` | 134×108 | bottom-centre on `cy(c) + 1.5·TILE − 6` | big gold deposit, 2×2 cells (1500) |
| `Stone_Big.png` | 133×108 | same | big stone deposit, 2×2 cells (1800) |

Stone draw: `drawImage(im, round(s.x − w/2), round(s.y + 22 − h))` (`entities.js:1371-1379`); entity `y = cy(c)+6`.
Big node: `x = n.x − w/2`, `y = World.cy(n.c) + 96 − 6 − h` (`entities.js:1381-1384`). Depleted nodes
(`amount ≤ 0`) are drawn at alpha 0.45.

#### 2.8.4 `Extra/Props/` — 48 single images, anchor = bottom-centre `(w/2, h)` (`drawProp`, `entities.js:1390-1393`)

| Name | w×h | Name | w×h | Name | w×h | Name | w×h |
|---|---|---|---|---|---|---|---|
| barrel | 34×40 | beehive | 38×46 | boat | 59×44 | bones | 51×34 |
| boulder | 58×56 | campfire | 49×44 | cart | 69×46 | chest | 45×38 |
| chest_open | 41×44 | clover | 39×26 | column | 62×60 | crate | 39×40 |
| deadtree | 86×88 | dock | 64×48 | fallen_log | 51×36 | fence | 65×40 |
| ferns | 55×40 | flowers | 56×36 | flowers_blue | 49×34 | flowers_red | 51×34 |
| flowers_white | 46×32 | grass | 43×36 | grave | 55×50 | hay | 51×44 |
| lilypads | 42×34 | logs | 50×40 | menhir | 63×84 | mushrooms | 44×40 |
| obelisk | 54×92 | pebbles | 44×26 | puddle | 53×30 | pumpkins | 56×38 |
| reeds | 49×52 | ruin_arch | 102×96 | ruin_wall | 95×60 | scarecrow | 58×68 |
| shipwreck | 72×64 | shrine | 64×80 | signpost | 45×58 | statue | 51×84 |
| sticks | 41×24 | stone_circle | 73×56 | stump | 49×34 | tall_grass | 52×40 |
| tent | 80×84 | wagon_wreck | 70×50 | well | 60×76 | windmill | 99×124 |

Roles in generation (`world.js:696-777`): *solid landmarks* (block their cell): well, statue, column, ruin_arch,
deadtree, boulder, scarecrow, grave, menhir, signpost, ruin_wall, fence, stone_circle, cart, hay, logs, barrel, shrine,
obelisk, wagon_wreck, beehive, windmill; *abandoned camp*: tent + campfire + barrel/crate/logs + hay/cart/crate + chest;
*walk-over details*: grass, tall_grass, flowers(+_red/_blue/_white), clover, ferns, pebbles, sticks, mushrooms, stump,
fallen_log, bones, puddle, pumpkins; *shore/water*: reeds, lilypads, boat, dock, shipwreck. `chest` = treasure pickup
sprite (`drawPickup`, y+8); `chest_open` is present but unused by the current code.

#### 2.8.5 `Extra/Icons/` — 28 icons 64×64 (content ≤ 48 px, centred)

`anvil, arrows, axe, banner, barrow, bell, book, boot, breastplate, coins, crown, eye, flag, herbs, horseshoe,
hourglass, map, pickaxe, scales, shield, spyglass, stone, stop, sword, tent, tower, upgrade, wheat`
(`XICON(name)`, `assets.js:413`). Used as tech icons (`TECH_DEF.icon`), command icons (`['x', name]`), stats
(`arrows`, `boot`, `herbs`), resource icon for stone (`RES_ICON.stone`), age crown, etc.

#### 2.8.6 Palisade slicing

`drawWall` (`entities.js:1321-1334`): `STAKE = { sx: 31, sw: 14, n: 5 }` — a vertical run of palisades is drawn by
repeating the single stake column `(31, 0, 14, 78)` of `Wall.png` `n` times per cell, at
`y = b.y0 + (i+1)·64/5 − 72`; the front view (`Wall.png` whole, anchor 38,68) is used when the wall has a
horizontal neighbour or no neighbour. In a 3D port the palisade can become a real mesh/billboard row; keep the
neighbour rule (front vs side) if billboards are kept.

---

## 3. Terrain tileset anatomy (`Tilemap_color1.png`, 576×384 = 9 columns × 6 rows of 64 px)

Cell `(col,row)` = pixel rect `(col·64, row·64, 64, 64)`.

```
        col0      col1      col2      col3   | col4 | col5      col6      col7      col8
row0  [ FLAT 3x3 autotile + strips        ] | ---- | [ ELEVATED TOP 3x3 autotile + strips   ]
row1  [  (grass, mint shore rim)          ] | ---- | [  (grass, rock-teal rim)              ]
row2  [                                    ] | ---- | [  bottom row ends in 10px rock band   ]
row3  [ horizontal 1-high strip | single ] | ---- | [ horizontal strip      | single        ]
row4  ramp L-hi   ----   ----  ramp R-hi  | ---- | CLIFF L   CLIFF mid  CLIFF R   CLIFF 1-wide  (foot on grass)
row5  ramp L-lo   ----   ----  ramp R-lo  | ---- | CLIFF L   CLIFF mid  CLIFF R   CLIFF 1-wide  (foot in water)
```

Opacity per cell (color1; % of opaque pixels): row0 `93 95 93 89 0 94 98 93 90`; row1 `98 100 99 97 0 100 100
100 100`; row2 `97 99 97 94 0 100 100 100 100`; row3 `92 94 91 88 0 94 98 94 92`; row4 `50 0 0 49 0 89 94 89 83`;
row5 `89 0 0 91 0 85 92 83 78`. Column 4 and cells (1..2, 4..5) are fully transparent.

### 3.1 Autotile index rule (how the JS picks cells) — `world.js:789-825`

`edge(before, after)`: `before && after → 1` (centre), `!before && after → 0` (start), `before && !after → 2`
(end), neither → `3` (isolated). Column index = `edge(W, E)`, row index = `edge(N, S)` (W/E/N/S = neighbour is
"same surface").

- **Flat ground (level 0)**: `flat[c] = edge(ground(x−1), ground(x+1)) | edge(ground(y−1), ground(y+1)) << 2`
  where `ground` = `land > 0` (includes unreachable islets `land = 2`). Cell = `(col, row)` in cols 0-3, rows 0-3.
  **Always from `Tilemap_color1`** (`tiles[0]`, `world.js:837`).
- **Elevated surface of level L (1 or 2)**: `(5 + edge(up(x−1), up(x+1)), edge(up(y−1), up(y+1)))`, `up = level ≥ L`,
  from `Tilemap_color{plateau id}` where `plateau id = hill index % 4 + 1` ∈ 1..4 (`world.js:546-548`), i.e.
  hills cycle through tilesets 1,2,3,4 (color5 is never used by the JS).
- **Cliff wall**: a cell with `cliff = 1` directly below a level-L cell: `(5 + edge(wall(x−1), wall(x+1)), land ? 4 : 5)`
  — row 4 when the foot stands on land, row 5 when the cliff drops into water (no shadow then). Tileset = the
  plateau id of the cell above.
- **Stairs/ramps** (`stair` codes: 1 = left low, 2 = left high, 3 = right low, 4 = right high, `world.js:13`):
  `(st ≤ 2 ? 0 : 3, st % 2 == 0 ? 4 : 5)` → left ramp = column 0, right ramp = column 3, high half = row 4, low half
  = row 5. A "left" stair stands to the **west** of the plateau's bottom-row end cell (its high side is on the
  right in the art, against the plateau); "right" stair mirrors it. `hi` cell gets `level = L`, `lo` stays at L−1;
  movement between `hi` and `lo` (vertically adjacent) is the only allowed level change (`world.js:55-58`).
- Draw order (`world.js:828-861`): water colour → foam → water rocks/splashes/ducks → flat tiles → for L = 1, 2:
  cliff shadows (Shadow.png) then that level's tiles → sprites.

### 3.2 Flat-ground block (cols 0-3, rows 0-3)

3×3 autotile in cols 0-2 × rows 0-2 (corners at (0,0) (2,0) (0,2) (2,2), edges (1,0) (0,1) (2,1) (1,2), centre (1,1)),
col 3 = 1-wide vertical strip (top end, middle, bottom end in rows 0-2), row 3 = 1-high horizontal strip
(left end, middle, right end in cols 0-2), (3,3) = single isolated tile. Outer edges: navy scalloped outline
`#161c2e`/`#354f59` plus a **2–4 px mint rim `#c6f0db`** — this rim is the wet shoreline; it matches the foam colour.

### 3.3 Elevated block (cols 5-8, rows 0-3)

Same 4×4 layout. Centre (6,1) is **pixel-identical** to (1,1). Differences: the rim has no mint (rock-teal
`#669d9d`/`#8cc3c4` peeking under the scalloped grass edge on the left/right sides), and the bottom-edge cells
(rows 2 and 3) end with a ~10 px rock band (y 54-63) that continues seamlessly into the cliff cells (seam
(6,2)→(6,4) = 60, the same as (6,3)→(6,4)).

### 3.4 Cliff cells (cols 5-8, rows 4-5)

Rock face in `#669d9d` (base), `#506a69` (shade), `#8cc3c4` (highlight), `#161c2e` (outline), `#354f59`.
Row 4 bottom ≈ 12 px: dark-green grass tufts (`#3c7662`) with transparent gaps (foot on lower ground). Row 5 bottom:
mint foam `#c6f0db` lip (foot in water). Column 5 = left end (rounded left side), 6 = middle, 7 = right end, 8 =
1-wide pillar. Rows 0..51 of each cliff cell are fully opaque. The face reads as 2 vertical stone slabs per cell.

### 3.5 Ramps (cols 0 and 3, rows 4-5)

(0,4) = left ramp high half: grass triangle, diagonal from bottom-left to top-right, transparent top-left;
(0,5) = left ramp low half: grass slope with a rock wedge at the bottom-right showing the cliff it climbs;
(3,4)/(3,5) = mirror images (right ramp). Grass is drawn as "hanging" vertical strands (a slope covered in grass).

### 3.6 Colour variants (all 5 tilesets differ only in the 3 grass colours)

Grass centre cell (1,1) uses exactly 3 colours (counts 2659/956/481 px):

| Tileset | base | dark | highlight | Character | Reference use (§11) |
|---|---|---|---|---|---|
| color1 | `#9bb94e` | `#80ac5e` | `#b8b958` | yellow-green, sunny | **mid tier** (main plateau) |
| color2 | `#85b156` | `#789e5f` | `#93ba4f` | fresh green | **top tier** |
| color3 | `#5fab62` | `#5da067` | `#74b363` | emerald | **low/coastal tier** |
| color4 | `#839a5e` | `#77915c` | `#969f64` | olive / dry | not used in the reference |
| color5 | `#569b8d` | `#528e89` | `#63a486` | teal / shaded forest floor | **pine-forest floor** |

Shared (all variants): outline `#161c2e`, `#354f59`; grass shadow `#3c7662`, `#618559`; cliff `#669d9d`, `#506a69`,
`#8cc3c4`; mint shore/foam `#c6f0db`. Water `#47aba9`.

Minimap colours used by the JS (`world.js:874-887`): cliff `#5c8f92`, stair `#c9b87a`, level ≥ 2 `#d9e58a`, level 1
`#b9cf62`, level 0 `#8fb24c`, trees `#4f7f3a`, gold `#f2d24b`, stone `#9aa6ad`, big deposits 2×2 with `#1b1f33`
border and `#f2d24b` / `#c3ccd1` fill; sea = Water Background pattern (fallback `#47aba9`). Minimap scale k = 2 px/cell.

### 3.7 Best "pure" textures for voxel block faces (HD-2D terrain)

Seam measurements (mean RGBA L1 difference across the joining edge; internal column-to-column difference of the
grass is ≈ 10): grass (1,1) wraps horizontally 7.3 and vertically 15.8 → **seamlessly tileable**; cliff (6,4)
self-wraps horizontally 36 (acceptable), but **not vertically** (bottom tufts are transparent; 686).

| Voxel face | Source cell (col,row) → pixel rect | Notes |
|---|---|---|
| **Grass top** (any tier) | (1,1) → `(64,64,64,64)` of `Tilemap_colorN` (identical to (6,1) `(384,64)`) | fully opaque, tileable both ways. Pick N per tier/biome (§3.6, §11). |
| Grass top with rim (edge/corner blocks) | flat set cols 0-3 rows 0-3 for blocks touching water (mint shore rim); elevated set cols 5-8 rows 0-3 for plateau rims | use the §3.1 autotile rule on the top face; keeps the hand-drawn scalloped outline |
| **Grass side / lip** | strip `y 40..63` of (6,2) → `(384,168,64,24)` (left end `(320,168,64,24)`, right end `(448,168,64,24)`) | scalloped grass overhang + 10 px rock band; place as the top 24 px of a grass-block side, or as a separate overhang quad slightly in front of the cliff face (more HD-2D) |
| **Cliff rock wall side** | (6,4) → `(384,256,64,64)` foot on ground; (6,5) → `(384,320,64,64)` foot in water; ends (5,4)/(7,4), pillar (8,4) | horizontally tileable middle; for a wall more than one block high use rows 0..51 (opaque, no foot) for the upper blocks and the full cell (with tufts/foam foot) for the bottom block |
| East/west faces of cliff blocks | (5,4) (left/west end) and (7,4) (right/east end) | already contain the rounded corner shading |
| Ramp (stairs) | (0,4)/(0,5) left, (3,4)/(3,5) right | in 3D a ramp is a sloped quad: texture with (1,1) grass + the hanging-strand look of (0,4) |
| **Sand top** | **not in the pack** | derive: recolour (1,1) by mapping its 3 colours `#9bb94e→base`, `#80ac5e→dark`, `#b8b958→light`. Proposed (pack-adjacent tones: Banner_Slots `#ccb88d`/`#c9aa8e`, reference walls `#e1d6b8`): base `#d8c28e`, dark `#c4a77a`, light `#e8d9a8`. Shore rim: keep mint `#c6f0db`. |
| **Dirt** (paths, farm plots, under buildings) | **not in the pack** | same 3-colour remap: base `#805a53`, dark `#70504e`, light `#9a6f5e` (WoodTable_Slots browns `#7e5552`, `#805a53`, `#70504e`); or use color4 (olive) for "worn grass" |
| Water surface | `Water Background color.png` = flat `#47aba9` | a plane with that colour (no texture); foam ring and rocks are sprites (§4) |

Suggested sand/dirt values are **proposals** (marked so); they are not in the original game.

---

## 4. Water

### 4.1 Water colour

`Water Background color.png` is 64×64 of a single colour RGBA(71,171,169,255) = `#47aba9`. On screen the JS fills
the view with `WATER_COLOR = '#47aba9'` (`main.js:349-358`; "una tinta unita costa molto meno del motivo ripetuto");
the minimap uses the image as a repeating pattern (`world.js:868`).

### 4.2 Water Foam (`Water Foam.png`, 3072×192, 16 frames of 192×192, fps 10, anchor (96,96))

Each frame = a mint `#c6f0db` rounded square ≈ 66–84 px wide centred on the cell, plus dark-teal `#458597` shadow
specks below it; frames 0-6 show the square slightly shrinking/settling, frames 7-15 an expanding ripple ring that
breaks into dashes (loop = 1.6 s). Frame content bbox union x 52..137, y 58..148 (≈ 12 px beyond the 64-px tile
on each side, ~20 px below).
Usage (`main.js:363-367`): drawn **under the ground tiles**, centred on every **foam cell**, all with the **same
frame** `floor(t·10)` (synchronised surf). Foam cells (`world.js:575-585`): every land cell (`land > 0`, i.e. also
islets) with at least one of its 8 neighbours not land, plus every water cell that is a cliff (cliff dropping into
the sea). Only the ~12 px ring outside the land tile stays visible → animated shoreline.
HD-2D port: a flat quad slightly above the water plane along the coastline, or a shader ring; keep synchronised
16-frame/10 fps animation.

### 4.3 Water Splash (Particle FX, 9 frames, fps 12, anchor 96,100)

Ambient: `G.splashT -= dt`; when ≤ 0 → `splashT = rand(0.4, 1.2)` and a splash plays at the centre of a random
`World.openWater` cell (`main.js:319-326`). `openWater` = water cells (x,y in 1..W−2/H−2) with **no** land within
Chebyshev radius 1 and **some** land within radius 5 (`world.js:779-782`). [RENDER]

### 4.4 Rocks in the water and the duck

- Water rocks (`world.js:753-759`): up to `100·scale` placements, 14000·scale tries; cell must be water, not `used`,
  no land within radius 1, land within radius 3. Stored `{x: cx, y: cy, v: 0..3, phase: rng·16}`; drawn with frame
  `floor(t·10 + phase)` (16-frame loop, desynchronised) (`main.js:368-372`). Each frame = rock + expanding ripple.
- Ducks (`world.js:760-768`): up to `9·scale` in open sea (water, land within radius 4 but not 1, ≥ 1100 px apart)
  + one per pond (`pond: true`). Motion (`main.js:374-378`): `r = pond ? 18 : 46`, `a = t·0.3 + phase`,
  `x = d.x + sin(a)·r`, `y = d.y + cos(a·0.8)·r·0.4 + sin(t·2 + phase)·1.5`, flipped when `cos(a) < 0`, frame
  `floor(t·4 + phase)` (3 frames).

---

## 5. `Shadow.png` (192×192)

Single colour RGBA(22,28,46,80) (≈31 % navy), a ~79×80 blob at x 57..135, y 56..135 with dithered edges, made of 3
columns: left edge piece (x 0..63, only its right 7 px used), middle (x 64..127), right edge piece (x 128..191).
Usage (`world.js:840-853`): for each cliff cell whose foot is on land (and each low stair half), per level: draw the
middle column `(64,0,64,192)` at `(x·64, y·64 − 42)`; if the shadow run starts here (no shadow to the left) also the
left column at `x·64 − 64`; if it ends here, the right column at `x·64 + 64`. Net effect: a soft dark band from
y·64+14 to y·64+93 → visible ≈ 29 px onto the lower ground under the cliff foot, softened at both ends of a run.
Precomputed flags: `shadow[c] = 1 | (left neighbour has none ? 2 : 0) | (right neighbour has none ? 4 : 0)`
(`world.js:823`). HD-2D: replace by real shadow mapping from the directional light or an AO decal at cliff feet.

---

## 6. Clouds (`Clouds_01..08`, 576×256 single images)

Two families: 01-04 bright white-cream (big → tiny), 05-08 duller cream (big → tiny). Each image includes a baked
semi-transparent drop shadow RGBA(36,42,59,85) under the cloud.
Usage (`main.js:60-63`, `319`, `410-412`): `count = round(8 · W·H / MAP_AREA)` (= 32 on the 288×192 map), image
`Clouds_0{i%8+1}`, start `x = rand(−300, mapW)`, `y = rand(−100, mapH − 150)`, speed `v = rand(7, 16)` px/s to the
east; when `x > mapW + 100` → `x = −600`, new random y. Drawn on top of the world at **alpha 0.6**, top-left at
(x, y). The light pass also projects each cloud's silhouette on the ground at `(x + 90 + lx·60, y + 260)` with alpha
0.5 (`light.js:127`) — moving cloud shadows. HD-2D: cloud billboards high above the terrain + their projected shadow.

---

## 7. Bushes, land rocks, trees, gold, stone, sheep, pickups

- **Bushes** (`Bushe1..4`): 8-frame sway loop at 8 fps, per-instance `phase = rng·8`, frame `floor(t·8 + phase)`
  (`entities.js:1443-1444`). Placement (`world.js:745-752`): random free low cells (`freeLow`), position
  `x = col·64 + 16 + rng·32`, `y = row·64 + 30 + rng·26`, `v = 0..3`; the first 200 placements are bushes, the rest
  (up to a total of `330·scale`) are land rocks. Islets also get one bush (60 %) or one rock (`world.js:572`).
- **Land rocks** (`Rock1..4`): static, drawn at `(x − 32, y − 50)` (`entities.js:1445`). Not obstacles.
- **Trees**: entity `x = cx(c)`, `y = cy(c) + 22`; per-tree `phase = random·8`; while `wood > 0` animate
  `floor(t·8 + phase)` (8-frame sway); when felled (`wood ≤ 0`) draw `Stump v` at `(x − 96, y − stumpAy)`
  (`entities.js:106-111`, `1357-1360`). Variant `v = floor(rng·4)` per tree at generation; all four variants are mixed
  randomly in the current generator.
- **Gold node**: `stage = clamp(ceil(amount/40), 1, 6)`; `shine` timer starts at `rand(−6,0)`, `+= dt`; when it
  exceeds 0.6 → `shine = −rand(3,7)`. While `0 < shine ≤ 0.6` and `amount > 0`, draw `Gold Stone {stage}_Highlight`
  frame `floor(shine·10)` (6 frames), else the static `Gold Stone {stage}` at `(x−64, y−64)`
  (`entities.js:113-118`, `155-156`, `1362-1369`). Entity `y = cy(c)`, depth key `sy = y + 18` (big: +56).
- **Stone node**: Extra stones (§2.8.3); entity `y = cy(c) + 6`, `sy = y + 20` (big: +56).
- **Sheep**: states `idle` / `move` / `grass` → sheets of §2.5, frame `floor(animT · fps)`, flip by `s.flip`
  (`entities.js:1395-1403`).
- **Pickups** (`drawPickup`, `entities.js:1406-1424`): chest → prop `chest` at `y + 8`; else blink (skip every other
  1/6 s) when `life < 5`; bob `y = p.y − hop − |sin(t·3)|·4`; scale 1.5; gold = highlight frames for the first 0.6 s of
  each 2.2 s cycle then static `Gold_Resource` at `(−64,−70)`; stone `Stone_Resource` at `(−13,−20)`; wood/meat at
  `(−32,−42)`.
- **Arrow / rock projectiles** (`entities.js:1431-1441`): translated to `(a.x, a.y)`, rotated by `a.ang`; arrow centred
  `(−32,−32)`; catapult rock = `Stone_Resource` scaled 1.3 at `(−13,−11)`. Only drawn where the fog shows.
- Depth sorting: everything (decor, trees, gold, stone, points, sheep, buildings, units, pickups, fx) is sorted by
  `sy` (foot y) ascending (`main.js:381-394`). In 3D this becomes natural depth (billboards at their world z).

---

## 8. Fire on damaged buildings (`entities.js:1349-1355`)

When `hp/maxHp = r < 0.7` (and not in the shadow pass): sheet = `r < 0.25 ? Fire_03 : r < 0.45 ? Fire_02 : Fire_01`,
count `n = r < 0.25 ? 4 : r < 0.45 ? 3 : 2`. For `i in 0..n`:
`h1 = |sin(fireSeed + i·12.9898)·43758.5453| mod 1`, `h2 = |sin(fireSeed + i·78.233)·12543.123| mod 1`;
position `x = b.x + (h1 − 0.5)·w·64·0.7`, `y = b.y − 24 − h2·ay·0.45`; frame `floor(t·fps + i·3)`; scale 1.4.
`fireSeed` is per building (chapter 03b). [RENDER]

---

## 9. UI Elements — slice geometry (`assets.js:429-566`)

### 9.1 Slice grids

| Name | Source cells `[offset, size]` (same for x and y) | Sheets |
|---|---|---|
| `G3` | `[[0,64],[128,64],[256,64]]` — 320-px sheets with separated 64-px cells | papers, big buttons, store Slots, bar bases |
| `G7` | `[[0,128],[192,64],[320,128]]` — 448-px sheets | Banner, WoodTable, BigRibbons, Swords, store Ribbon row 0 |
| `G1` | `[[0,64],[64,64],[128,64]]` — 192-px contiguous | Banner_Slots, WoodTable_Slots |

`nineSlice(ctx, im, xs, ys, x, y, w, h, s)`: corner sizes = source sizes × s; if `lw + rw > w` (or `th + bh > h`) both
are scaled by `w/(lw+rw)`; edges and centre are **stretched** (not tiled). `drawPanel(def, x, y, w, h, s)` expands the
visible rect by `pad · s` on each side before slicing (pads in §2.7). `threeSlice(im, xs, sy, sh, x, y, w, s)`: one
source row `sy..sy+sh`, end caps `xs[0]`, `xs[2]` scaled by s, middle stretched.

### 9.2 Ribbons and swords

- `bigRibbon(colorIdx, cx, cy, w, s)`: row `sy = colorIdx·128`, `sh = 128`, G7, top-left `(cx − w/2, cy − 64s)`, text at
  `(cx, cy − 2s)` white. Rows in COLORS order: 0 Blue, 1 Red, 2 Yellow, 3 Purple, 4 Black.
- `smallRibbon(colorIdx, variant, …)`: row `(colorIdx·2 + variant)·64`, `sh = 64`, G3, top `cy − 33s`; **variant 0 =
  swallow-tail (notched) ends, variant 1 = pointed (arrow) ends**.
- `swordBanner`: Swords.png row `colorIdx·128`, G7 (hilt with coloured guard on the left, parchment blade, pointed
  tip on the right); text dark `#4a3a33` at `(cx + 16s, cy − 14s)`, no outline.
- `storeRibbon(color)`: `Ribbon_<Color>.png`, row 0 (`sy 0, sh 128`), G7, top `cy − 68s` (the folded title ribbon with
  notched tails). `drawPennant(color, cx, y, s)`: source `(51, 448, 90, 108)` = hanging swallow-tail pennant,
  drawn `90s × 108s` centred on cx. The rest of the 960×576 atlas (curved ribbon arcs at y 128-448, scroll+ribbon
  combos at x ≥ 512) is unused.

### 9.3 Store banner (`drawStoreBanner`, `assets.js:470-484`)

`c = 64s`, `cr = 44s`, `bh = 98s`, `bl = 192s`, `br = 172s`, `mh = h − c − bh`. Source pieces from
`Banner/Banner.png` (704×512): top-left `(0,0,64,64)`, top-middle `(256,0,64,64)` stretched, top-right `(640,0,44,64)`;
middle-left `(0,128,64,64)`, centre `(320,128,64,64)`, middle-right `(640,128,44,64)`; bottom-left curl `(0,256,192,98)`,
bottom-middle `(256,256,64,98)`, bottom-right curl `(512,256,172,98)` — the bottom corners are wider (rolled scroll).

### 9.4 Bars

- `bigBar(x, y, w, ratio, s)`: base `threeSlice(BigBar_Base, G3, 0, 64, x − 40s, y − 9s, w + 80s, s)`; fill width
  `(w − 22s)·clamp(ratio)`; if > 0.5 px draw `BigBar_Fill (0,22,64,21)` at `(x + 11s, y + 13s, fw, 21s)`. Visible
  height 51·s. Used for construction progress (`w 90, s 0.4`).
- `smallBar(x, y, w, ratio, s, fill)`: base `threeSlice(SmallBar_Base, G3, 0, 64, x − 49s, y − 22s, w + 98s, s)`; fill
  `(w − 20s)·ratio` from `(0,30,64,3)` at `(x + 10s, y + 8s, fw, 6s)`. Visible height 19·s. `fill` = SmallBar_Fill
  recoloured with `COLOR_CSS[color]` via `source-atop` (`buildDerived`, `assets.js:31-43`). Unit HP bar:
  `smallBar(e.x − 24, e.y − barY, 48, hp/maxHp, 0.5)`; building: `(e.x − 44|22, top + 18, 88|44, …, 0.7|0.5)`.

### 9.5 Buttons and cursors

- `bigButton` (`ui.js:128-138`): 9-slice of Big{Blue|Red}Button_{Regular|Pressed} (pressed while mouse down), label
  24·s white, optional `ICON(n)` 40·s left of the text; text y offset `+3s` pressed / `−4s` normal.
- `smallButton` (`ui.js:140-146`): `Small{Red|Blue}{Round|Square}Button_{Pressed|Regular}.png` drawn `size × size`
  centred; hit box = central `0.7·size` square; content offset `dy = +0.04·size` pressed / `−0.03·size`.
- `tinyButton` (`ui.js:148-155`): `Tiny{Round|Square}{Red|Blue}Button.png`, drawn at 0.92 scale when pressed, icon
  at `0.74·size`.
- Cursor (`ui.js:652-662`), size `d = 64·s` with `s = max(dpr, uiScale)`: `hand` Cursor_02 at `(m − (27,19)s)`,
  `no` Cursor_03 at `(m − (32,31)s)`, `attack` = Icon_05 at `m + 10s` size 46s, tool cursors (`axe` Tool_02, `pick`
  Tool_04, `knife` Tool_03, `hammer` Tool_01) at `m + 6s` size 96s, default Cursor_01 at `(m − (23,18)s)`.
- `drawBrackets` (`assets.js:554-561`): Cursor_04 corners `(0,0)`, `(96,0)`, `(0,96)`, `(96,96)`, 32×32 each, drawn
  at `32·s`.

### 9.6 Icons and avatars

`Icon_01..12` (64×64): 01 hammer (build — `['ui',1]`), 02 wood log (`RES_ICON.wood`), 03 gold coin
(`RES_ICON.gold`), 04 meat (`RES_ICON.meat`), 05 sword (attack, `['ui',5]`, attack cursor, damage stat), 06 shield
(armour stat), 07 green right arrow (play: `Riprendi`, `Rigioca`, `Guarda`, `GIOCA`, `INIZIA`), 08 orange back arrow
(`Menu`, `Esci`), 09 red X (close, `Demolisci`/`Annulla cantiere`), 10 gear (pause), 11 "i" info (help), 12 music
note (sound toggle).

Avatars (`assets.js:208-215`): `n = COLORS.indexOf(color)·5 + avatar + 1`, file `Avatars_{n:02}`; `avatar` 0 warrior
(plumed great-helm), 1 lancer (round helmet), 2 archer (pointed hood), 3 monk (tonsure), 4 pawn (curly hair).
Rows of 5 per colour: 01-05 Blue, 06-10 Red, 11-15 Yellow, 16-20 Purple, 21-25 Black. Draw source `(40,40,176,176)`.

---

## 10. `assets/Extra` and `tools/make_sprites.py`

`assets/Extra/` holds 178 PNGs generated for this game "nello stile del pacchetto": AI images (`tools/raw/*.webp`,
models listed in `tools/raw/SOURCES.md`: mostly gpt-image-2; mine/quarry gemini-3-pro-image; sawmill
gemini-3.1-flash-image) converted by `tools/make_sprites.py` (Python + Pillow + numpy). Pipeline:
1. `pack_palette()` — every RGB colour that appears ≥ 40 times (opaque) in the pack's buildings, units, resources,
   decorations, `Tilemap_color1`, UI icons and FX = the target palette.
2. `team_maps()` — for each non-blue colour, a blue→colour RGB map voted pixel-by-pixel over the twin pack sprites;
   only bluish keys (`b > r + 8`) kept. `teamify` snaps saturated blue/teal pixels to the nearest known team colour
   (distance < 70) so recolouring catches them; `recolor` applies the map.
3. `foreground()` — background removal (transparent, magenta or white; white holes ≥ 60 px or touching the border).
4. `shrink()` — crop to bbox, BOX-downscale to the pack scale, alpha threshold 0.5, snap every colour to the palette
   (weighted RGB distance, weights 0.9/1.2/0.9).
5. `building(raw, name, width)` — 6-px pad, baked ellipse shadow `BUILDING_SHADOW = (22,28,46,69)`
   (`rx = w/2 + 3`, `ry = 13`), 5 colours (or neutral), metadata `ax = W/2`, `ay = pad + spriteH − 4`, crop.
   Widths: Smith 184, Market 190, Stable 188, Siege 190, Depot 124, Farm 190, Wall 64 (neutral), Camp 188, points 156.
6. `animation(raw, name, cols, rows, height, …)` — split grid into frames, scale so the reference frame is `height`
   px tall (Knight 100, Ram 86, Catapult 52, Scout 66 = pawn height), align frames on the body (search ±18/±14 px) or
   keep grid positions (`grid=True`, scouts) or synthesise from one frame with vertical `bob` offsets
   (Ram_Run/Catapult_Run: `[0,−1,0,1]`); frame size rounded up to multiples of 32; shadow ellipse
   `UNIT_SHADOW = (15,18,26,79)`; metadata `fw, fh, n, ax, ay`.
7. `stone_carry()` — Pawn_{Idle,Run} Gold → Stone by recolouring the 5 nugget colours to greys.
8. `sheet()` — props (4 sheets of 12, given heights 24-124 px), stones, deposits, icons (48 px in a 64-px box).
9. Writes `js/extra.js` (`EXTRA` = buildings/units/props/icons/stones/deposits metadata) and regenerates
   `js/manifest.js` (sorted list of every PNG under `assets/`, loaded at start-up).

Port notes: the Rust client should consume `docs/spec/assets.json` (or an equivalent generated manifest) instead of
`manifest.js`/`extra.js`; the generator is an offline tool and does not need porting. Extra art is visibly more
detailed/"painterly" than the pack (higher-frequency detail at the same scale); place it with the same anchors.

---

## 11. Aesthetic analysis of the reference (`Reference/Map-example.png`, 926×710)

It is the official Tiny Swords store artwork (older pack variant: pines, birches, blue-roof houses, multi-tier
cliffs, the "Tiny Swords" sign). Scale ≈ 0.5–0.55 of native (castle 312 px → ~172 px), i.e. one 64-px tile ≈ 32–35
screen px, so the whole picture is a hand-built map of only ≈ 27×21 tiles. Pixel classification (exact palette
matches, visible pixels): water 31 %, grass 17 % (of which mid tier color1 48 %, low tier color3 28 %, top tier
color2 16 %, forest color5 8 %), cliff faces 4.6 %, foam rim 0.6 %, the rest sprites/outlines/sign.

What makes it look good — each point is a rule to encode in the aesthetic generator:

1. **Tiers, not noise.** Land is organised in 3 nested elevation tiers (coast level → main plateau → top terrace),
   each a clean region with long straight edges and occasional 1-tile steps. No single-cell jaggies, no isolated
   1-cell holes. Outlines are rectilinear unions of rectangles (≥ 3 tiles per run), corners rounded only by the tile
   art.
2. **Colour by tier.** Each tier uses a different grass tileset: low/coastal = color3 (emerald), main plateau =
   color1 (yellow-green, brightest, most area), top terrace = color2 (green); a shaded biome region (pine forest) =
   color5 (teal). Elevation reads instantly from hue; brighter = more important ground. color4 is unused.
3. **Cliffs only where they read.** Rock walls are exactly 1 tile high per tier step and appear on south-facing
   edges (the 3/4 camera hides north faces). Runs are long (3-10 tiles), straight, with rounded end caps. Several
   cliffs drop straight into the sea (wet-foot row 5 tiles + foam), giving dramatic coast sections; elsewhere a narrow
   grass strip of the lower tier separates cliff and water.
4. **Ramps at cliff ends.** Each tier connects to the one below with 1 (sometimes 2) grassy ramps placed at the end of
   a cliff run, never in the middle; ramps are flanked by bushes.
5. **Coastline.** Shore follows the tier outline with 1-2 tile notches and small bays; a constant mint rim + a second
   dashed ripple line ~4-8 px outside it (the animated foam). 4-5 tiny islets (1×1, 3×1, 3×2 tiles) sit 1-3 tiles off
   the coast, each with a bush, a tower or nothing — they punctuate the water. Open water is never empty: small grey
   water rocks (≈ 12, singles and pairs) within 1-2 tiles of the shore, more near cliff corners.
6. **Trees in clusters on edges.** Deciduous trees (yellow/orange birch, mixed autumn variants) grow in clumps of
   3-6 overlapping crowns hugging plateau edges, corners and coast tips, slightly overhanging cliffs/water — never
   scattered evenly in open meadows. Pines form one dense forest (≈ 12 trees, overlapping, 2-3 rows deep) on its own
   darker (color5) ground at the map border, partly cut off by the frame. Varieties are grouped by region (pines with
   pines, birches with birches), with mixed autumn tints only inside birch clumps.
7. **Bushes and rocks as transition glue.** Bushes (1-2, sometimes a small + a large) sit at cliff feet, beside ramps,
   at building corners and on islets; land rocks sit at cliff feet and coast corners. They mark transitions, they
   do not fill meadows.
8. **Big calm open spaces.** ≈ 60 % of each tier's surface is empty grass where units stand/walk; buildings get 1-2
   tiles of clearance. Detail density is high only along edges (edge-weighted placement).
9. **Settlement logic.** Castle on the top tier near its edge; towers on promontories and on islets (defensive
   silhouettes against water); a village of 4 houses clustered with 1-tile gaps on the open plateau; 5-6 sheep grazing
   in loose groups (1-3 tiles apart) next to the houses.
10. **Palette discipline.** Few hues: teal water `#47aba9` (dominant), 3-4 grasses, teal-grey rock `#669d9d`, navy
    outline `#161c2e` around everything, mint foam `#c6f0db`, cream/blue accents on buildings. Shadows are navy
    translucent, never black.
11. **Framing.** Land touches/cuts the image border on several sides (top-left forest, right) so the world feels
    bigger than the frame; water occupies a third of the picture and frames the land masses.

Implications for the procedural generator (proposal, to be refined in the map-generation chapter):
- Build an integer height field with 3 tiers via nested rectilinear blobs (rectangle unions snapped to a coarse
  grid, then morphological open/close to remove 1-cell spurs/holes), not value-noise coastlines.
- Assign tileset by tier/biome (low = color3, mid = color1, top = color2, forest floor = color5; optional dry
  region = color4).
- Place ramps at cliff-run ends; enforce reachability as the current `sound()` does.
- Place vegetation with edge-weighted Poisson-disc sampling: trees in clusters seeded on tier edges and corners
  (cluster size 3-6, radius 1.5-2.5 tiles), forests as dense regions of one species, bushes/rocks at cliff feet and
  ramp sides, water rocks along shores, islets of 1-6 tiles off the coast.
- Keep ≥ 50-60 % of each tier free of decoration; keep gameplay constraints (bases, stocks around bases, points)
  from chapter 02/world generation.

---

## 12. HD-2D porting notes (Octopath-style: voxel terrain + billboards)

- **Billboard pivots**: use `(ax, ay)` from the tables/JSON; quad size = frame size × (world units per pixel);
  billboards should be camera-facing around the vertical axis (cylindrical) so they stay upright on the tilted camera.
- **Baked shadows must be removed**: every pixel with `0 < alpha < 255` in the pack is a baked ground shadow
  (colours/alpha found: `22,28,46,69` units/buildings/trees/bushes/resources; `15,18,26,79` Extra units + some pack
  units; `26,32,53,69` and `22,28,46,119` in Terrain/Resources; `36,42,59,85` clouds; `22,28,46,80` Shadow.png). No
  other semi-transparent pixels exist (Particle FX and UI have none). Rule: at load time, set alpha 0 for every pixel
  with alpha < 255 in world sprites, and draw a ground-projected shadow (blob decal of the same footprint or a real
  shadow map with alpha-tested billboards).
- **Team colours**: all five colour variants exist as files; no runtime tinting needed except the HP-bar fill
  (`COLOR_CSS`). The JS shadow pass caches silhouettes by path with the colour name replaced by `*`
  (`assets.js:51-69`) — irrelevant if real shadows are used.
- **Animation clocks**: decorations use global time `G.time` + per-instance phase (trees `random·8`, bushes `rng·8`,
  water rocks `rng·16`, ducks `rng·6`); foam is globally synchronised. Units/sheep use their own `animT`.
- **[SIM!] data from sheets**: `frames` and `fps` of attack sheets (Warrior_Attack1/2 4@12, Lancer_*_Attack 3@9,
  Archer_Shoot 8@14, Heal 11@12, Knight_Attack 6@12, Ram_Attack 4@7, Catapult_Attack 4@7) together with `hitFrame`
  and `cd` drive attack timing in the simulation (chapter 01/03a). The sim crate must embed these numbers (it never
  loads images).
- **Texture atlasing (optimisation)**: 588 files ≈ 5.6 MB of PNG. The tileset faces, foam and decorations are small;
  pack per-category atlases (terrain, decorations+resources, FX, UI, units per colour) to cut draw calls; Lancer
  sheets (3840 px wide) need splitting into ≤ 2048/4096-wide atlas pages for WebGL2 limits.

---

## 13. Open questions / ambiguities

1. Sand and dirt textures do not exist in the pack; §3.7 proposes palette remaps — needs art approval (or new art).
2. The reference uses an older Tiny Swords version (pines/birches identical to ours, but houses with blue roofs and a
   different castle); our buildings differ — only the terrain/vegetation composition rules transfer directly.
3. The JS draws all level-0 ground with `Tilemap_color1` and hills with colors 1-4 cyclically; the reference suggests
   tier-based colours (3/1/2 + 5 for forest). Decide the mapping for the port (recommended: reference mapping).
4. Store `Ribbon_<Color>.png` atlas regions other than row 0 and the pennant, `chest_open.png`, and color4 are unused
   by the current game; keep or drop.
5. Extra (AI-generated) sprites are more detailed than the pack art; in HD-2D close-ups the mismatch may be more
   visible — consider whether to keep them as is.

---

## Addenda (critic)

Corrections verified in the code (they change §13 open questions 3 and 4):

**I1. Tileset colours actually used by the JS.** `drawGround` loads `tileImgs = [color1 … color5]` (world.js:832);
flat level-0 ground uses `tiles[0]` = **color1** (world.js:837); raised surfaces, walls and stairs use
`tiles[t >> 8]` where `t >> 8` is the plateau id `1..4` (world.js:811-819, ids assigned `h.id = i % 4 + 1`,
world.js:546, and `1 + floor(rng()*4)` for raised islets, world.js:567). So hills cycle **color2, color3, color4,
color5** — not colors 1-4 — and **color4 is used** (every 4th plateau). Recommendation unchanged: adopt the reference
tier/biome mapping in the port (§3, §11).

**I2. `chest_open.png` is used.** Picking up a map chest emits `['prop', 'chest_open', x, y]` (entities.js:1283),
which every client turns into a permanent decor sprite via `makeProp`/`drawProp` (entities.js:70, 92, 1390-1393;
`PROP(name)` = `Extra/Props/${name}.png`, assets.js:414). Keep it: it is the "opened chest" state of the chest
billboard.
