# 02 — World: map, terrain, generation, passability, distance fields

Source: `js/world.js` (931 lines, the whole file is covered here). Helpers it uses live in `js/assets.js`
(`TILE = 64` at assets.js:4, `clamp` at assets.js:603, `mulberry32` at assets.js:604, `RES_KINDS` at assets.js:415,
`TERR = 'Terrain/'` at assets.js:48). Callers referenced for context: `js/main.js`, `js/entities.js`, `js/factions.js`,
`js/net.js`.

Legend used throughout:

- **[GAMEPLAY]** — behaviour the Rust simulation MUST reproduce (rules, numbers, guarantees, pathing semantics).
- **[VISUAL]** — purely presentational; will be redesigned for the HD-2D look (voxel terrain + billboard sprites).
- **[GEN-AESTHETIC]** — part of the current procedural *shape* of the map. The user explicitly wants map generation
  redesigned for aesthetics, so the exact algorithm need not be kept, BUT every gameplay *guarantee* listed in
  section 6 ("Generation invariants") must still hold in the new generator.
- **[SIM]** = belongs in the pure deterministic sim crate; **[RENDER]** = client only.

All user-visible strings in this file: there is exactly one, an exception message (section 5.1):
`'Generazione della mappa fallita (seme ' + seed + ')'`. Keep it verbatim.

---

## 1. Coordinates, map size, constants

### 1.1 Grid [GAMEPLAY][SIM]

| Item | Value | Ref |
|---|---|---|
| `World.W` | **288** cells | world.js:24 |
| `World.H` | **192** cells | world.js:24 |
| `N = W*H` | 55 296 cells | |
| `TILE` | **64** px per cell (world units are pixels) | assets.js:4 |
| World size in px | 18 432 x 12 288 | |

- Cell index: `c = y*W + x` (`idx`, world.js:34). Row-major, y grows downwards (south).
- `inside(x,y)`: `0 <= x < W && 0 <= y < H` (world.js:35).
- `cellAt(px,py)`: `x = floor(px/64), y = floor(py/64)`; returns `-1` if outside (world.js:36-39).
- Cell centre: `cx(c) = (c % W)*64 + 32`, `cy(c) = floor(c / W)*64 + 32` (world.js:40-41).
- `quad(c) = [c, c+1, c+W, c+W+1]`: the 2x2 footprint whose top-left is `c` (used for capture points and big deposits)
  (world.js:52).
- `isLand(x,y)`: inside and `land == 1` (mainland only; islets with `land == 2` are NOT land here) (world.js:42).

In Rust the sim will likely keep pixel units as `f32` (or fixed-point) for unit positions; the grid logic is all
integer. Keep `TILE = 64` as the conversion factor so speeds/ranges defined elsewhere in px stay valid.

### 1.2 Module constants (world.js:6-21)

| Const | Value | Meaning | Kind |
|---|---|---|---|
| `BASE_COUNT` | 5 | number of kingdoms / castles | GAMEPLAY |
| `BASE_GAP` | 64 | minimum Euclidean distance (cells) between two base centres | GAMEPLAY |
| `MAP_AREA` | 144*96 = 13 824 | the old map area; all counts are scaled by `scale = N / MAP_AREA` = **4** | GEN |
| `BASE_CLEAR` | `{x: 9, y: 7}` | half-sizes of the cleared rectangle kept free around each base (19 x 15 cells) | GAMEPLAY |
| `BASE_STOCK` | `[['trees',20,12],['golds',22,4],['stones',22,2],['trees',14,6],['golds',14,2],['stones',14,1]]` | min resources near every base: `[list, radius (cells), min count]` | GAMEPLAY |
| `DIRS8` | `[[1,0,1],[-1,0,1],[0,1,1],[0,-1,1],[1,1,1.414],[-1,1,1.414],[1,-1,1.414],[-1,-1,1.414]]` | 8-neighbour offsets in this exact order (E, W, S, N, SE, SW, NE, NW); 3rd value unused in world.js | SIM (order matters for tie-breaks) |
| `FIELD_CACHE` | 128 | max distance fields kept in cache | SIM (perf) |
| `FIELD_REFRESH` | 2 | stale-but-usable fields recomputed per sim step | SIM |
| `FIELD_NEW` | 2 | group members per step allowed to switch from the group's common path to their own slot path (used by `march`, entities.js:596; reset each step at main.js:293) | SIM |
| `CROSS_COST` | 30 | extra cost (tenths of cell) for stepping onto a cell of one's own building in "cross" fields | SIM |
| `POINT_MIX` | `['mine','sawmill','outpost','quarry','ranch','outpost']` | capture-point kinds, assigned cyclically by placement index | GAMEPLAY |
| `POINT_STOCK` | `{mine:'gold', quarry:'stone', sawmill:'forest', ranch:'meadow'}` | resource group spawned next to each capture point (outpost: none) | GAMEPLAY |

Related constants elsewhere: `START_SIGHT = 15` (control.js:10; comment at world.js:11 says 14 cells is inside the
area pre-explored at match start), `TREE_WOOD = 40`, `GOLD_MAX = 240`, `STONE_MAX = 300` (entities.js:13),
`TREASURE = 50` (entities.js:15, resources in a found chest), `RES_KINDS = ['wood','gold','meat','stone']`.

---

## 2. Per-cell layers (data model)

All are `Uint8Array(N)` unless noted. Allocated in `build()` (world.js:314-323) and in `load()` (world.js:917).

| Layer | Values | Meaning | Kind |
|---|---|---|---|
| `land` | 0 water, **1 mainland**, 2 islet (unreachable scenery) | only `land == 1` can ever be walked/built | GAMEPLAY |
| `level` | 0, 1, 2 | elevation: 0 ground, 1 plateau ("altura"), 2 terrace on a plateau | GAMEPLAY |
| `cliff` | 0/1 | rock wall cell (the cell just south of a raised cell's south edge). Never walkable. Can be on land OR on water (cliff dropping into the sea) | GAMEPLAY (blocking) |
| `stair` | 0..4 | `1` left stair lower half, `2` left stair upper half, `3` right stair lower half, `4` right stair upper half (comment world.js:13). Odd = lower half, even = upper half. | GAMEPLAY |
| `plateau` | 0..4 | tileset index (1..4) of the plateau the cell belongs to (also on its stairs) | VISUAL (but `plateau` id ≠ 0 doubles as "part of a hill"; not used by sim rules) |
| `noBuild` | 0/1 | buildings cannot be placed here (stair access, capture-point surroundings, unreachable pockets) | GAMEPLAY |
| `block` | count (0..255) | number of static/dynamic obstacles on the cell (trees, ores, solid props, points, bigs, buildings). `setBlock` adds/subtracts | GAMEPLAY |
| `wallOf` | 0 or side (kingdom idx+1) | the cell holds a palisade (`wall`) of that kingdom | GAMEPLAY |
| `ownOf` | 0 or side | the cell holds a building (palisades included) of that kingdom | GAMEPLAY |
| `reserved` | 0/1 | (build-time only, local) cleared base zone + unreachable pockets; not serialized | GEN |

Other World state:

- `version` (int): bumped on every map change (`setBlock`, `setOwner`, `resetBlock`, `build`, `load`). Used by the
  field cache and by net snapshots (net.js:163 "slow" snapshot when `World.version` changed).
- `opened` (int): value of `version` when a cell was last *freed* (block decremented or owner removed).
- `refresh` (int): stale-field recomputes still allowed this step; set to `FIELD_REFRESH` at the start of each sim
  step (main.js:292).
- `fresh` (int): set to `FIELD_NEW` at each step (main.js:293); consumed in `march` (entities.js:596).
- `fields: Map<string, {v, d: Float32Array}>` distance-field cache (insertion-ordered).
- `moves: Map<int, {v, block, m: Uint8Array}>` move-mask cache keyed by `side*2 + cross`.
- `walls: [6 ints]` count of standing palisade cells per side (index 0 unused, 1..5 kingdoms).
- `crossed` (bool): out-parameter of `getField` (see 4.6).
- Lists (all produced by generation; see section 5): `bases`, `plateaus`, `ponds`, `foam`, `trees`, `golds`, `stones`,
  `meadows`, `props`, `treasures`, `bushes`, `rocks`, `waterRocks`, `ducks`, `openWater`, `points`, `bigs`.
- [RENDER] `tFlat`, `tTop`, `tShadow`, `tileImgs`, `minimap` (section 7).

### 2.1 Object records produced by generation

| List | Record | Notes |
|---|---|---|
| `bases` | `{x, y}` integer cell coords | castle is placed at cells `(x-2 .. x+2, y-1 .. y)` (5x2 footprint, factions.js:65; castle def w=5,h=2 assets.js:226). `(x, y+2)` is the canonical "base reachability cell". |
| `plateaus` | `{id, x, y, n, top}` | `id` tileset 1..4; `(x,y)` = landing cell at the top of its first stair; `n` = cell count; `top` = `{x,y}` landing of the terrace's first stair, or `null` |
| `ponds` | `{x, y}` float cell coords (ellipse centre) | |
| `trees` | `{c, v}` | `v` = sprite variant 0..3 [VISUAL]. Wood per tree = `TREE_WOOD` 40 (set in entities.js:108). |
| `golds` | `{c, amount: 240}` | |
| `stones` | `{c, amount: 300}` | |
| `meadows` | `{x, y}` pixel coords = centre of a cell | sheep spawn: 3 sheep per meadow (main.js:76). Meadows do NOT occupy cells. |
| `points` | `{c, kind}` | capture point occupying `quad(c)` |
| `bigs` | `{c, kind:'gold'|'stone', amount}` | big deposit occupying `quad(c)`; gold 1500, stone 1800 |
| `treasures` | `{c, kind}` | chest; `kind = RES_KINDS[index % 4]` → wood, gold, meat, stone, wood, ... ; becomes a pickup of `TREASURE` (50) of that resource (main.js:77). **Not serialized** (client gets `[]`). |
| `props` | `{name, c, solid, x, y}` | scenery object; `solid` ones block their cell (`c`). Shore/pond props have `c = -1`, `solid = false`. |
| `bushes`, `rocks` | `{x, y, v, phase}` px | non-blocking decoration [VISUAL] |
| `waterRocks` | `{x, y, v, phase}` px | [VISUAL] |
| `ducks` | `{x, y, phase, pond?}` px | [VISUAL] |
| `foam` | cell indices | coast foam [VISUAL] |
| `openWater` | cell indices | candidate cells for water splash effects (main.js:324) [VISUAL] |

---

## 3. Passability rules [GAMEPLAY][SIM]

`side` = kingdom index + 1 of whoever moves (0 = nobody / neutral). (world.js:43-58)

```
passable(c, side=0)  = c >= 0 && land[c]==1 && cliff[c]==0 && (block[c]==0 || (side!=0 && wallOf[c]==side))
crossable(c, side=0) = c >= 0 && land[c]==1 && cliff[c]==0 && (block[c]==0 || (side!=0 && ownOf[c]==side))
```

- **passable** = a unit may *stand/stop* here: free cell, or a cell holding a palisade of its own kingdom (own walls
  are walkable for their owner, like gates).
- **crossable** = a unit may *pass through* here: free cell, or any building cell of its own kingdom (palisades
  included). Enemy buildings are never crossable — "troops are not locked in among their own houses, enemy ones are".
- `c` out of range (`-1` or `>= N`) → false (JS reads `undefined`; in Rust bounds-check explicitly).

### 3.1 Elevation linking

```
linkOK(a, b) = level[a]==level[b]
            || (stair[a]!=0 && stair[b]!=0 && |a-b|==W)       // vertically adjacent stair halves
```

You change level ONLY between the lower and upper half of the same stair (vertical step). Any same-level
neighbours are linked. Cliff cells are never passable, so the plateau edge is sealed on the south by cliffs, and
on the other sides simply by level mismatch (a plateau's north/east/west border is an invisible "step" that cannot be
crossed: different levels, no cliff cell needed).

Stair geometry (built in `stairSpots`/`raise`, section 5.8): a stair is 2 cells stacked vertically **outside** the
plateau, beside its south-west or south-east corner:

```
   row y   : [hi]  [c = plateau bottom-edge cell, level L]       hi has level L, stair 2 (left) / 4 (right)
   row y+1 : [lo]  [cliff cell]                                    lo keeps lower level, stair 1 / 3
   row y+2 : [ap]                                                  approach cell, lower level
```
(shown for `dir = -1`, i.e. stair on the left; mirrored for `dir = +1`). `hi` connects horizontally to `c`
(same level) and vertically to `lo` (stair rule); `lo` connects to `ap` and lateral lower-level cells.

### 3.2 Pixel-level stepping (unit movement)

`stepOK(x0, y0, x1, y1, side=0, cross=false)` (world.js:61-67):

```
a = cellAt(x0,y0); b = cellAt(x1,y1)
if a == b: return true
targetOK = (cross || !passable(a, side)) ? crossable(b, side) : passable(b, side)
if !targetOK: return false
if !crossable(a, side): return true          // stuck inside a blocked cell: can always get out (ignores levels!)
return linkOK(a, b)
```

`tryMove(e, dx, dy)` (world.js:68-72) — axis-separated sliding with a 9 px probe:

```
r = 9; side = e.faction ? e.faction.idx+1 : 0; cross = (e.cross === true)
if dx != 0 && stepOK(e.x, e.y, e.x + dx + sign(dx)*r, e.y, side, cross): e.x += dx
if dy != 0 && stepOK(e.x, e.y, e.x, e.y + dy + sign(dy)*r, side, cross): e.y += dy   // uses the already-updated x
```

Note: only the point 9 px ahead along the axis of motion is tested; the unit is a point with a 9 px look-ahead, not a
circle. Diagonal moves can therefore "slide" along walls. Sheep also use `tryMove` (entities.js:173), with no
faction → side 0.

`clearLine(x0, y0, x1, y1, side=0)` (world.js:74-83) — straight-line walkability test used by `approach`
(entities.js:322, only when distance < 280 px):

```
d = hypot(x1-x0, y1-y0); n = ceil(d / 24)       // n = 0 → returns true
px, py = x0, y0
for i in 1..=n:
  x = x0 + (x1-x0)*i/n; y = y0 + (y1-y0)*i/n
  if !stepOK(px,py, x,py, side, true) || !stepOK(x,py, x,y, side, true): return false
  px, py = x, y
return true
```

AMBIGUITY: the comment says "without cutting through buildings (not even one's own)", but the code passes
`cross = true`, which uses `crossable` → own buildings ARE crossable by the line test. Port the code behaviour
(own buildings crossable in `clearLine`) unless the gameplay spec owner decides otherwise.

### 3.3 Block / owner mutation (world.js:85-101)

```
setBlock(cells, delta):         // delta = +1 when a building is placed, -1 when removed (entities.js:1094)
  for c in cells: block[c] += delta
  version++
  if delta < 0: opened = version

setOwner(cells, side, wall):    // side = idx+1, or 0 to clear; wall = it's a palisade (entities.js:1095)
  for c in cells:
    if wallOf[c] != 0: walls[wallOf[c]]--
    ownOf[c]  = side
    wallOf[c] = wall ? side : 0
    if wall && side != 0: walls[side]++
  version++
  if side == 0: opened = version
```

Trees are never removed from `block`: a felled tree remains a blocking stump that regrows (see entities spec).
Gold/stone nodes: check entities spec for depletion; world.js itself never unblocks them after generation.

`resetBlock()` (world.js:893-901): clears `block`, `wallOf`, `ownOf`, `walls = [0;6]`; sets `block[o.c] = 1` for
every tree, gold, stone and every prop with `solid !== false` (trees/golds/stones have no `solid` field → they
block); sets `block = 1` on `quad(c)` of every point and big; `version++`. Buildings are then re-added via
`setBlock`/`setOwner` (net.js:304-305 does exactly this on clients after a full snapshot).

### 3.4 `nearestPassable(px, py)` (world.js:104-121)

Neutral (`side 0`) passability, levels ignored.

```
x0 = clamp(floor(px/64), 0, W-1); y0 = clamp(floor(py/64), 0, H-1)
if passable(idx(x0,y0)): return (px, py)            // the point itself, unchanged
for r in 1..=10:                                    // Chebyshev rings
  best = -1; bd = inf
  for y in y0-r..=y0+r: for x in x0-r..=x0+r:
    if max(|x-x0|,|y-y0|) != r || !inside(x,y): continue
    c = idx(x,y); if !passable(c): continue
    d = hypot(cx(c)-px, cy(c)-py)
    if d < bd: bd = d; best = c                     // strict <: first in y-then-x scan order wins ties
  if best >= 0: return (cx(best), cy(best))
return (px, py)                                     // nothing within 10 rings
```

---

## 4. Movement graph and distance fields [GAMEPLAY][SIM]

Distances are integers in **tenths of a cell**: 10 per orthogonal step, 14 per diagonal step (+30 per own-building
cell entered in cross mode). Unreachable = `Infinity` (Float32 in JS). In Rust use `u32` with `u32::MAX` as the
unreachable sentinel (a `u16` could overflow in long mazes with cross surcharges).

### 4.1 `neighborOK(c, x, y, dx, dy, side, cross)` (world.js:125-136)

Returns neighbour index `n` or -1:

```
nx, ny = x+dx, y+dy; if !inside: -1
n = idx(nx, ny)
ok(k) = cross ? crossable(k, side) : passable(k, side)
if !ok(n): -1
if dx!=0 && dy!=0:                                   // diagonal: no corner cutting
  o1 = c+dx; o2 = c+dy*W
  if !(ok(o1) && ok(o2)): -1
  if !linkOK(c,o1) || !linkOK(c,o2) || !linkOK(o1,n) || !linkOK(o2,n): -1
else if !linkOK(c, n): -1
return n
```

Note: `c` itself is NOT required to be ok.

### 4.2 `moveMask(side=0, cross=false)` (world.js:142-161)

Precomputes the same rule for all cells as a bitmask (8 bits per cell):

```
own = cross ? ownOf : wallOf
open[c] = land[c]==1 && cliff[c]==0 && (block[c]==0 || (side!=0 && own[c]==side))
link(a,b) = level[a]==level[b] || (stair[a]!=0 && stair[b]!=0 && |a-b|==W)
for each c (x = c%W, y = c/W):
  w = x>0   && open[c-1] && link(c,c-1)        -> bit 1   (0x01)
  e = x<W-1 && open[c+1] && link(c,c+1)        -> bit 2   (0x02)
  n = y>0   && open[c-W] && link(c,c-W)        -> bit 4   (0x04)
  s = y<H-1 && open[c+W] && link(c,c+W)        -> bit 8   (0x08)
  for k in 0..4:                               // k=0 NW, 1 NE, 2 SW, 3 SE
    dx = (k&1) ? +1 : -1; dy = (k&2) ? +W : -W
    require (dx<0 ? w : e) && (dy<0 ? n : s)
    t = c+dx+dy
    if open[t] && link(c+dx, t) && link(c+dy, t): bits |= 16 << k
  m[c] = bits
```

Bit order equals the `step` table of `computeField`: `[-1, +1, -W, +W, -1-W, +1-W, -1+W, +1+W]`.
Equivalent to `neighborOK` (the source cell's own openness is not checked; a closed seed cell can still expand).

`movesFor(side, cross)` (world.js:165-171): cache keyed by `side*2 + (cross?1:0)`, valid while
`entry.v == version` and the `block` array identity is unchanged (JS detail: a fresh `block` array after
`build`/`load`). In Rust: invalidate on `version` change.

### 4.3 `computeField(seeds, side=0, cross=false, moves=moveMask(side,cross), reuse=null)` (world.js:180-205)

Exact multi-source shortest path using Dial's algorithm with 64 circular buckets (max edge cost 14+30 = 44 < 64 so no
aliasing):

```
extra = cross ? CROSS_COST(30) : 0
dist[*] = INF
for s in seeds: if dist[s] != 0: dist[s] = 0; push s into bucket[0]; pending++
d = 0
while pending > 0:
  q = bucket[d & 63]
  while q not empty:
    c = q.pop()                         // LIFO; order does not affect results
    pending--
    if dist[c] != d: continue           // stale entry
    bits = moves[c]
    for k in 0..8 where bits has bit k:
      t = c + step[k]
      nd = d + (k<4 ? 10 : 14) + (extra!=0 && block[t]!=0 && wallOf[t]!=side ? extra : 0)
      if nd < dist[t]: dist[t] = nd; push t into bucket[nd & 63]; pending++
  d++
return dist
```

- In cross mode the surcharge applies to cells that are blocked AND not own palisades (i.e. own non-wall buildings;
  enemy buildings are not open at all). Own palisades cost nothing extra in either mode.
- The result is unique (exact shortest distances), so any correct Dijkstra/Dial in Rust gives identical numbers →
  deterministic across server and client.
- `reuse` is a memory-recycling detail (reuse the evicted array). Optional in Rust.

### 4.4 `ring(cells, side=0)` (world.js:245-256)

Walkable cells around a footprint, on the same level as the footprint's first cell:

```
lvl = level[cells[0]]
out = {}  (insertion-ordered set)
for c in cells: for (dx,dy) in DIRS8: if inside(x+dx,y+dy):
  n = c+dx+dy*W
  if n not in cells && passable(n, side) && level[n]==lvl: out.add(n)
```

### 4.5 Field targets (world.js:257-258)

- `fieldToCell(c, side, from)`: key `'c'+c`; seeds = `passable(c, s) ? [c] : ring([c], s)`.
- `fieldFor(e, side, from)`: key `'e'+e.id`; seeds = `ring(e.cells, s)` (entity footprint: building, tree, ore node,
  point...). Note the seed function receives the *effective* side `s` chosen by `getField` (see below), not
  necessarily the caller's side.

Consumers (for orientation; their behaviour is in the entities spec): `navTo`/`fieldOf` (entities.js:302-316),
`approach` (entities.js:320-334; `pathLeft = max(d, f[a]*64/10)` px), AI/stat trip estimate (factions.js:236-238:
field value at the node cell, or min over the 8-neighbours of the node's cells, `trip = max(0, d/10 - 0.7)` cells).

### 4.6 `getField(key, seedFn, side=0, from=-1)` and the cache (world.js:217-243)

```
getField(key, seedFn, side, from):
  walls = this.walls[side] > 0 ? side : 0          // separate fields only for kingdoms that own palisades
  d = cached(key + ':' + walls, seedFn, walls, cross=false, from)
  crossed = (from >= 0 && side != 0 && d[from] == INF)
  if crossed: return cached(key + ':x' + side, seedFn, side, cross=true, from)
  return d
```

`crossed` is stored in `World.crossed` and copied by the caller into `u.cross` (entities.js:304, 326): when the normal
field cannot reach the unit (it is enclosed by its own buildings, or the target is), the unit follows the "cross" field
that passes through its own buildings, and `tryMove`/`stepFrom` then use `crossable` instead of `passable`.

```
cached(key, seedFn, side, cross, from):
  e = fields.get(key)
  if e:
    if e.v == version: return e.d                                  // fresh
    if e.v >= opened || (from >= 0 && e.d[from] < INF):            // stale but still "good"
      if refresh <= 0: return e.d                                  // budget exhausted: keep using stale field
      refresh--
    // else: cells were freed since, and the requester has no path in the old field → recompute NOW (no budget)
  old = e ? e.d : null
  fields.delete(key)
  if old == null && fields.size >= FIELD_CACHE(128):
    evict the oldest-inserted entry (Map insertion order), reuse its array
  d = computeField(seedFn(side), side, cross, movesFor(side, cross), old)
  fields.set(key, {v: version, d})                                 // re-inserted at the end (FIFO-by-computation)
  return d
```

Rationale (comments world.js:207-216): if since the field was made only *building* happened (no cell freed,
`e.v >= opened`), old paths are still valid except freshly occupied cells, which per-step checks avoid anyway. If
cells were freed, the old field is still fine for whoever already has a path (at most a shorter one exists now). Those
are refreshed at most `FIELD_REFRESH` = 2 per sim step. Requesters without a path get an immediate recompute.

Determinism note for Rust: cache hits vs. recomputes change which (possibly stale) field a unit follows, so the cache
policy (128 entries, insertion-order eviction, refresh budget 2/step, `fresh` budget 2/step) is part of the simulation
behaviour. Because the authoritative sim runs only on the server (or locally in solo), bit-exact parity between
machines is not required, but keep the policy to keep the same performance profile and behaviour.

Memory: 128 fields x 221 KB (Float32 x 55 296) ≈ 28 MB in JS; same with `u32` in Rust. (A `u16` with saturation
at 65 534 would halve it; typical distances are a few thousand tenths, but only use it if saturation is handled.)

### 4.7 `stepFrom(field, c, side=0, cross=false)` (world.js:261-278)

Next cell to move to; -1 if none (already at minimum / unreachable).

```
x, y = c % W, c / W
if !(cross ? crossable(c,side) : passable(c,side)):     // standing in a cell where one cannot stand
  bd = INF; best = -1
  for (dx,dy) in DIRS8: if inside(x+dx,y+dy):
    n = c+dx+dy*W
    if crossable(n, side) && field[n] < bd: bd = field[n]; best = n
  return best                                          // NB: ignores linkOK and diagonal corner rules
best = -1; bd = field[c]
for (dx,dy) in DIRS8:
  n = neighborOK(c, x, y, dx, dy, side, cross)
  if n >= 0 && field[n] < bd: bd = field[n]; best = n   // strict <: tie → first in DIRS8 order (E,W,S,N,SE,SW,NE,NW)
return best
```

Callers move the unit toward `(cx(n), cy(n))` with `moveUnit` → `tryMove`.

---

## 5. Generation pipeline

### 5.1 Driver: `generate(seed)` (world.js:282-289) [SIM, but see 9]

```
for attempt in 0..400:
  ok = build(seed + attempt*7919, calm = attempt >= 30) && sound()
  if ok: break
if !ok: throw Error('Generazione della mappa fallita (seme ' + seed + ')')
buildTiles(); renderMinimap()          // RENDER only
```

- `seed` comes from `Math.random()*1e9 | 0` (main.js:74) — NOT deterministic across games (a new map each match).
- The first 30 attempts are "bold" (gulfs, lakes, very wavy coasts); later attempts are "calm" simple islands.
- Measured with the JS (40 seeds, headless run): attempts needed min 1, avg 3.1, max 25. Never reached calm mode in
  that sample; comment says never failed on 2000 seeds.
- RNG: `mulberry32` (assets.js:604-611):
  ```
  a |= 0; a = (a + 0x6D2B79F5) | 0
  t = imul(a ^ (a >>> 15), 1 | a)
  t = (t + imul(t ^ (t >>> 7), 61 | t)) ^ t
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  ```
  (wrapping i32/u32 arithmetic). Port it as-is to the sim crate so any seeded logic is reproducible.

### 5.2 `sound()` — map acceptance test (world.js:293-308) [GAMEPLAY guarantee]

Runs AFTER `build()` returned, when `block` already contains trees, ores, solid props, points and bigs (the last
`resetBlock` in build). Uses a neutral field (side 0, no cross).

```
goals = [idx(b.x, b.y+2) for b in bases]
for p in plateaus: goals += idx(p.x, p.y); if p.top: goals += idx(p.top.x, p.top.y)
f = computeField([goals[0]])
if any goal has f == INF: reject
open(c) = f[c-1] < INF || f[c+1] < INF || f[c-W] < INF || f[c+W] < INF     // reachable orthogonal neighbour
for b in bases:
  for [list, r, need] in BASE_STOCK:
    n = count of o in this[list] with hypot(o.c%W - b.x, floor(o.c/W) - b.y) < r && open(o.c)
    if n < need: reject
  if no meadow m with hypot(m.x/64 - b.x, m.y/64 - b.y) < 14: reject       // meadow coords are px (cell centre → +0.5)
accept
```

So every accepted map guarantees: all 5 bases mutually reachable on foot; every plateau and every terrace reachable;
around each base, within the given Euclidean radii (cells, from base centre) and adjacent to a reachable cell:
**≥12 trees within 20, ≥6 trees within 14, ≥4 gold nodes within 22, ≥2 within 14, ≥2 stone nodes within 22, ≥1
within 14, and ≥1 meadow within 14.**

### 5.3 `build(seed, calm)` — overview (world.js:311-784)

Order of steps (each detailed below):

1. Allocate layers, clear lists, `fields.clear()`, `version++` (314-329).
2. Island shape from superellipse + waves + value noise, minus gulfs/lakes (331-367). [GEN-AESTHETIC]
3. Two smoothing passes (368-377). [GEN-AESTHETIC]
4. Pick 5 bases (388-407). [GAMEPLAY rules: min gap 64, clear zone]
5. Force land around bases, mark reserved zones (408-412).
6. Small ponds (414-430). [GEN-AESTHETIC]
7. Keep only land 4-connected to base 0 (432-447).
8. Plateaus (hills) with cliffs, stairs and optional terraces (449-550). [GAMEPLAY: elevation/choke points; shape is GEN]
9. Mark unreachable pockets reserved/noBuild (552-554).
10. Islets in the sea (556-573). [VISUAL]
11. Foam list (575-585). [VISUAL]
12. Resources: base stock, hill resources, capture points, big deposits, scattered groups (587-694). [GAMEPLAY]
13. Props: abandoned camps, chests, landmarks, ground details (696-729). [chests GAMEPLAY; solid props block → GAMEPLAY-ish; look VISUAL]
14. Prune unreachable objects (730-743). [GAMEPLAY]
15. Decorations: bushes/rocks, water rocks, ducks, pond lilypads, shore props, open water (745-782). [VISUAL]

`scale = N / MAP_AREA = 4`. `solid` = number of `land==1` cells after step 7. `rich = solid / (N*0.71)` (0.71 = land
share of the old island). Measured `solid` ≈ 31 000–40 000 (avg 37 000) → `rich` ≈ 0.79–1.02 (avg 0.94).

### 5.4 Step 2 — island shape [GEN-AESTHETIC] (world.js:331-367)

Value noise (world.js:332-339): `lattice` = 1024 rng values (32x32, wrapping with `& 31`); bilinear with smoothstep
`u = fx²(3-2fx)`:
`noise(x,y) = a + (b-a)u + (c-a)w + (a-b-c+d)uw` with a,b,c,d the 4 lattice corners.

Parameters (rng drawn in this order):
- `squareness = 2.2 + rng*1.4` (2 = ellipse, >3 ≈ rectangle)
- `waves`: for k in [2,3,4,5]: `{k, a: (calm ? 0.03 : 0.14) * rng / sqrt(k/2), ph: rng*2π}`
- `rough = 0.4 + rng*0.45`, `grain = 0.09 + rng*0.07`
- if not calm:
  - gulfs: `floor(rng * 4 * sqrt(scale))` = 0..7; each: angle `a = rng*2π`, centre `(W/2 + cos(a)*(W/2-3), H/2 + sin(a)*(H/2-3))`, `rx = 13 + rng*14`, `ry = 10 + rng*11`
  - lakes: `floor(rng * 3 * sqrt(scale))` = 0..5; centre `(W*(0.2+rng*0.6), H*(0.2+rng*0.6))`, `rx = 6 + rng*9`, `ry = 5 + rng*7`

`inMargin(x,y) = x>=2 && y>=2 && x<W-2 && y<H-3` (land never touches the border; bottom margin 3).

For each cell in margin:
```
nx = (x+0.5 - W/2) / (W/2 - 2.5); ny = (y+0.5 - H/2) / (H/2 - 2.5); a = atan2(ny, nx)
reach = 1 + Σ w.a * cos(w.k*a + w.ph)
d = (|nx|^sq + |ny|^sq) / reach^sq
if d + (noise(x*grain, y*grain) - 0.5)*rough >= 0.8: water
wobble = (noise(x*0.21+7, y*0.21+3) - 0.5) * 0.7
land = 1 unless some water ellipse q has ((x-q.x)/q.rx)² + ((y-q.y)/q.ry)² + wobble < 1
```

### 5.5 Step 3 — smoothing (world.js:368-377) [GEN-AESTHETIC]

2 passes, in place (reads already-updated neighbours), x in 1..W-2, y in 1..H-2:
`n` = count of 4-neighbours that are land; land with `n < 2` → water; water with `n >= 3` and `inMargin` → land.

### 5.6 Step 4–5 — bases [GAMEPLAY] (world.js:378-412)

Helpers: `fill(xa,ya,xb,yb,fn)` applies `fn` to every in-margin cell of the inclusive rectangle;
`landShare(xa,ya,xb,yb)` = fraction of the inclusive rectangle that is land (out-of-map counts as water).

Candidate spots: `y` from `BASE_CLEAR.y+7 = 14` to `H - BASE_CLEAR.y - 9 = 176` step 2; `x` from
`BASE_CLEAR.x+7 = 16` to `W - BASE_CLEAR.x - 8 = 271` step 2; keep if
`landShare(x-11, y-9, x+11, y+9) > 0.92` (23x19 rectangle). No spots → fail.

Selection:
```
bases = [random spot]
while bases.len < 5:
  best = null; bd = -1
  for t in 0..400 while (t < 14 || bd < 64):          // at least 14 tries; keep trying up to 400 until gap ≥ 64
    s = random spot; d = min distance (Euclidean, cells) from s to existing bases
    if d > bd: bd = d; best = s
  if bd < 64: fail
  bases.push(best)
```
(Farthest-of-a-handful sampling: spread out, but never the same pattern.) Bases are assigned to kingdoms in random
order later (main.js:80-81).

Then for each base:
- force `land = 1` on rectangle `(x-12, y-10) .. (x+12, y+10)` (BASE_CLEAR + 3), in-margin only;
- `reserved = 1` on `(x-9, y-7) .. (x+9, y+7)` (BASE_CLEAR): no resources, decorations, ponds, stairs here.

Note the reserved zone is NOT `noBuild` — players build there.

### 5.7 Step 6–7 — ponds and connectivity (world.js:414-447)

Ponds [GEN-AESTHETIC]: up to `900*scale = 3600` tries while `ponds < 9*scale = 36`:
`px = 6 + rng*(W-12)`, `py = 6 + rng*(H-12)`, `rx = 1.6 + rng*1.6`, `ry = 1.2 + rng*1.0`. Reject if any cell in
`floor(px-rx-3)..px+rx+3` x `floor(py-ry-3)..py+ry+3` is outside, water or reserved; or another pond centre is within
14. Carve: cells in `floor(px-rx)..px+rx` x `floor(py-ry)..py+ry` with `((x+0.5-px)/rx)² + ((y+0.5-py)/ry)² <= 1` →
`land = 0`. Push `{x: px, y: py}`. (Measured: always 36 ponds.) Ponds are impassable water.

Connectivity [GAMEPLAY]: 4-connected flood fill over `land` from `(bases[0].x, bases[0].y)`; every unvisited cell →
`land = 0`. `landNear(x,y,r)` = any `isLand` cell in the (2r+1)² square.

### 5.8 Step 8 — plateaus, cliffs, stairs, terraces (world.js:449-550)

GAMEPLAY essence (must survive the redesign): raised areas (level 1) reachable only through stairs, with impassable
cliffs on their south side; some carry a second level (terrace, level 2) reached by stairs from level 1; they hold
gold/stone/trees; the plateau closest to the centroid of the bases holds **triple gold**; stair access cells are
`noBuild`; buildings must be on a single level (factions.js:155) and never on stairs or `noBuild`. The exact blob shapes
are [GEN-AESTHETIC].

`keep` mask: initially `(b.x-13, b.y-11) .. (b.x+13, b.y+12)` around each base (BASE_CLEAR + 4, +5 south).

**blob(cx, cy, n, [wMin,wMax,hMin,hMax], ok, bottom=false)** → Set of cells or null:
```
cells = {} (insertion-ordered); ax, ay = cx, cy
for k in 0..n:
  w = wMin + floor(rng*(wMax-wMin+1)); h = hMin + floor(rng*(hMax-hMin+1))
  x0 = ax - floor(rng*w)
  y0 = (bottom && k==0) ? ay - h + 1 : ay - floor(rng*h)     // rect contains the anchor; bottom: anchor on last row
  if any cell of rect [x0,x0+w) x [y0,y0+h) is outside or !ok: if k==0 return null else continue
  add all rect cells
  anchor = random element of cells (in insertion order)
return cells
```

**stairSpots(M, ground)**: for every `c` in M with `c+W` not in M (south-edge cells), for `dir` in [-1, +1]:
`hi = c+dir, lo = hi+W, ap = lo+W`; skip if any of `hi, lo, ap, hi-W, hi+dir, lo+dir` is in M; accept
`{hi, lo, ap, dir}` if `ground(hi) && ground(lo) && ground(ap)`.

**raise(M, L, spots, count)**:
```
for c in M: level[c] = L
for c in M: if c+W not in M: cliff[c+W] = 1
put = []
gap(a,b) = Manhattan distance between a.hi and b.hi
while put.len < count && spots non-empty:
  pick = floor(rng*spots.len)
  if put non-empty:
    far = -1
    for (i, s) in spots: d = min over q in put of gap(q, s); if d > far: far = d; pick = i
    if far < 5: break
  s = spots.remove(pick)
  stair[s.lo] = dir<0 ? 1 : 3; stair[s.hi] = dir<0 ? 2 : 4; level[s.hi] = L
  noBuild on rect (hi.x-1, hi.y-1) .. (hi.x+1, hi.y+2)       // 3 wide x 4 tall, in-margin: stair access
  put.push(s)
return put
```
(First stair random, each next the farthest from those already placed; stop if the farthest is < 5 apart.)

**Hill loop**:
```
wantHills = round(solid/360 * (0.8 + rng*0.5))
hillOK(c) = x in [3, W-4] && y in [3, H-6] && land[c]==1 && !keep[c]
cape = all c in [W, N-W) with land[c]==1 && land[c+W]==0        // land with sea directly south (computed once)
for t in 0..3000*scale(12000) while hills.len < wantHills:
  edge = (cape non-empty && rng < 0.35) ? random cape cell : -1
  (cx, cy) = edge<0 ? (4 + floor(rng*(W-8)), 4 + floor(rng*(H-10))) : cell of edge
  M = blob(cx, cy, 2 + floor(rng*4) /*2..5 rects*/, [6,14,4,9], hillOK, bottom = edge>=0)
  if !M || M.size < 24: continue
  spots = stairSpots(M, c => land==1 && level==0 && !cliff && !stair && !reserved)
  if spots empty: continue
  hill = {cells: M, x: cx, y: cy, stairs: raise(M, 1, spots, M.size > 90 ? 3 : 2), top: null}
  if M.size >= 50 && rng < 0.8:                                  // terrace
    inner(c) = for dy in -2..=3, dx in -2..=2: (c+dx+dy*W) in M && !noBuild   (all)
    for k in 0..30 while !hill.top:
      p = random cell of M
      M2 = blob(p.x, p.y, 1 + floor(rng*2) /*1..2*/, [3,8,2,5], inner)
      if !M2 || M2.size < 8: continue
      sp = stairSpots(M2, c => M.has(c) && level==1 && !cliff && !stair)
      if sp non-empty: hill.top = {cells: M2, x: p.x, y: p.y, stairs: raise(M2, 2, sp, M2.size > 24 ? 2 : 1)}
  // resource anchor: the free level-1 cell of M nearest to (cx, cy)
  bd = INF
  for c in M: if !cliff && !noBuild && level==1: d = hypot(c.x-cx, c.y-cy); if d < bd: hill.x, hill.y = c
  for c in M: keep on rect (c.x-3, c.y-3) .. (c.x+3, c.y+4)
  hills.push(hill)
if hills.len < 4: fail
```

Cape hills: the first rectangle's bottom row sits on a land cell whose south neighbour is sea, so its cliff is placed
on a water cell (`land 0, cliff 1`) — "a cliff dropping straight into the sea". Terrace cliffs land on level-1 cells
inside M (guaranteed by `inner`'s dy up to +3).

Ordering & ids:
```
mid = centroid of the 5 bases
sort hills by Euclidean distance of (hill.x, hill.y) to mid, ascending      // hills[0] = most contested, richest
landing(s) = cell (s.hi - s.dir)          // the plateau cell beside the stair top; always kept clear
for (i, h) in hills:
  h.id = i % 4 + 1
  plateau[c] = h.id for c in h.cells; plateau[s.hi] = plateau[s.lo] = h.id for s in h.stairs
  plateaus.push({id, landing(h.stairs[0]).x, .y, n: h.cells.size, top: h.top ? landing(h.top.stairs[0]) : null})
```

Measured (40 seeds): 81–134 plateaus (avg 105), 7–21 with terraces (avg 13); ~9 700 level-1 cells, ~180 level-2
cells, ~1 400 cliff cells, ~460 stair cells. (The old map had far fewer; the count scales with land area.)

### 5.9 Step 9 — unreachable pockets [GAMEPLAY] (world.js:552-554)

`walk = computeField([idx(b0.x, b0.y+2)])` with the current (empty) `block`; every `land==1 && !cliff` cell with
`walk == INF` → `reserved = noBuild = 1` (stays scenery: no resources, no building).

### 5.10 Step 10 — islets [VISUAL] (world.js:556-573)

`wantIsles = 6 + floor(rng*6)` (6..11); up to 900 tries: `x = 6 + floor(rng*(W-12))`, `y = 6 + floor(rng*(H-13))`,
`rx = 1.3 + rng*2`, `ry = 1.1 + rng*1.4`, `r = ceil(max(rx,ry))`. Reject if `land[x,y] != 0`, mainland within Chebyshev
`r+2`, no mainland within `r+9`, or another islet centre within 10. Cells with `(dx/rx)²+(dy/ry)² <= 1` (dx,dy in
-r..r) → `land = 2`. If `ry > 1.9` it is a raised rock: tileset `1 + floor(rng*4)`; every islet cell whose south
neighbour is also islet gets `level 1, plateau = tile`; then islet cells with level 0 whose north neighbour has level
→ `cliff = 1`. Each islet gets one decoration: 60% bush else rock, at px
`(x*64 + 16 + rng*32, (y - (raised?1:0))*64 + 30 + rng*26)`, `v = floor(rng*4)`, `phase = rng*8`.
Islets are `land 2` → never walkable.

### 5.11 Step 11 — foam [VISUAL] (world.js:575-585)

`ground(x,y) = inside && land > 0`. Foam cell list: every water cell with `cliff` (cliff in sea), and every
`land > 0` cell having any 8-neighbour that is not ground (outside counts as not ground).

### 5.12 Step 12 — resources [GAMEPLAY] (world.js:587-694)

`used` = build-time occupancy (one object per cell).

```
freeLow(c)  = land==1 && level==0 && !cliff && !stair && !reserved && !noBuild && !used
freeHigh(c) = land==1 && level!=0 && !cliff && !stair && !noBuild && !used
scatter(cx, cy, n, r, ok):                      // up to n cells, n*14 tries
  out = []
  for t in 0..n*14 while out.len < n:
    x = round(cx + (rng*2-1)*r); y = round(cy + (rng*2-1)*r)
    if inside && ok(idx): used = 1; out.push(idx)
```

**place(kind, px, py, ok=freeLow, strict=false)** → bool success:
- `forest`: `scatter(px,py, 8 + floor(rng*4) /*8..11*/, 2.8, ok)`; `grove` (dense wood, ~2x): `20 + floor(rng*8)`
  (20..27) cells, radius 4.4. Group minimum 5: if strict and fewer than 5 were placed, they are un-`used` and nothing
  is placed. Each cell → `trees.push({c, v: floor(rng*4)})`. Success = `cells >= 5`.
- `gold`: 3 cells, `stone`: 2 cells, radius 1.6; minimum 2 (strict rollback as above). Each →
  `golds.push({c, amount: 240})` / `stones.push({c, amount: 300})`. Success = `cells >= 2`.
- anything else (`meadow`): `scatter(px,py, 1, 1.5, ok)`; if none → false; else the cell is un-`used` again (meadows
  don't occupy) and `meadows.push({x: cx(c), y: cy(c)})` (px).
- Non-strict placement keeps partial groups (e.g. 1 gold) but reports failure.

**Base stock — `near(b, kind, angle, pad, far=0)`** (fair start: same directions for every base relative to the map
centre):
```
a0 = atan2(H/2 - b.y, W/2 - b.x) + angle
for turn in [0, 0.4, -0.4, 0.8, -0.8, 1.3, -1.3, 1.9, -1.9, 2.5, -2.5, 3.14]:
  ca = cos(a0+turn); sa = sin(a0+turn)
  edge = min(9 / max(1e-6,|ca|), 7 / max(1e-6,|sa|))     // distance to the clear-zone border along that ray
  d0 = edge + pad + far
  for d = d0; d <= max(d0, 12.5 + far); d += 1:
    if place(kind, b.x + ca*d, b.y + sa*d, freeLow, strict=true): return
// silently gives up if nothing fits (then sound() rejects the map)
```
Per base, in order:

| kind | angle (rad) | pad | far |
|---|---|---|---|
| meadow | 0.2 | 2 | 0 |
| forest | 1.9 | 2.5 | 0 |
| forest | -2.3 | 2.5 | 0 |
| gold | -1.05 | 1.5 | 0 |
| gold | 0.85 | 1.5 | 5 |
| stone | -0.25 | 1.5 | 0 |

**Hill resources** (for each hill `h` at sorted index `i`; `ok = c in h.cells && freeHigh(c)`, non-strict):
- `i == 0` (closest to the bases' centroid): **3 gold groups** at `(h.x-3, h.y)`, `(h.x, h.y)`, `(h.x+3, h.y)`.
- `0 < i < 4*scale (=16)`: 1 gold group at `(h.x, h.y)`.
- `i < 16`: 1 stone group at `(h.x + (i==0 ? 0 : 3), h.y + (i==0 ? 2.5 : 1))`.
- every hill: `scatter(h.x, h.y, 3 + floor(rng*4) /*3..6*/, sqrt(h.cells.size)/2, ok)` trees (direct, no min group).

Note `freeHigh` allows level 1 and level 2 cells of the hill (terrace included).

**Capture points and big deposits**:
```
roomy(c) = x in [3, W-5] && y in [3, H-6] && freeLow on all 16 cells (dx,dy in -1..=2)   // 2x2 + 1-cell margin
marks = copy of bases
spread(fromBase, gap):
  best = -1; bd = -1
  for t in 0..80:
    x = 4 + floor(rng*(W-8)); y = 4 + floor(rng*(H-9)); c = idx(x,y)
    if !roomy(c) || some base within fromBase (Euclidean): continue
    d = min distance to marks; if d > bd: bd = d; best = c
  if best < 0 || bd < gap: return -1
  marks.push(best); used on quad(best); return best

for i in 0..round(9*scale*rich):                 // ≈ 36*rich
  c = spread(20, 11); if c < 0: continue
  kind = POINT_MIX[i % 6]                        // i counts skipped attempts too
  points.push({c, kind})
  noBuild on 4x4 (dx,dy in -1..=2)               // must always be reachable
  if POINT_STOCK[kind]:                          // centre cell (x,y) = (c.x+1, c.y+1)
    place(POINT_STOCK[kind], x+4.5, y+0); place(POINT_STOCK[kind], x-4.5, y+1)   // non-strict, freeLow
for i in 0..round(5*scale*rich):                 // ≈ 20*rich
  c = spread(18, 9)
  if c >= 0: bigs.push({c, kind: i%2 ? 'stone' : 'gold', amount: i%2 ? 1800 : 1500})
```
Capture-point resource neighbours: mine → 2 gold groups, quarry → 2 stone groups, sawmill → 2 forests, ranch → 2
meadows (sheep), outpost → nothing. Capture-point behaviour itself (POINT_DEF, assets.js:311-317) is in the
entities/factions spec.

**Scattered groups** (proportional to land), `farFromBases(x,y)` = every base farther than 14 cells:
```
for [kind, count] in [['grove',3], ['forest',26], ['gold',9], ['stone',10], ['meadow',10]]:
  i = 0
  for t in 0..500*scale(2000) while i < round(count*rich*scale):
    x = 4 + rng*(W-8); y = 4 + rng*(H-8)        // floats
    if farFromBases(x,y) && place(kind, x, y): i++
```
→ targets ≈ 12 groves, 104 forests, 36 gold groups, 40 stone groups, 40 meadows at `rich = 1`.

Measured totals after pruning (40 seeds): trees 1 566–2 063 (avg 1 848), gold nodes 199–239 (avg 217), stone nodes
121–146 (avg 135), meadows 45–57 (avg 52), capture points 29–37 (avg 34), big deposits 16–20 (avg 19).

### 5.13 Step 13 — props, camps, chests (world.js:696-729)

```
prop(name, c, solid=false): used[c]=1; props.push({name, c, solid,
     x: cx(c) + (solid ? 0 : round((rng-0.5)*28)), y: cy(c) + 20})
chest(c): used[c]=1; treasures.push({c, kind: RES_KINDS[treasures.len % 4]})
spot(test=freeLow): up to 300 tries x = 3 + floor(rng*(W-6)), y = 3 + floor(rng*(H-6));
                    return idx if farFromBases && test, else -1
```

- **Abandoned camps** ("accampamenti abbandonati"), `i in 0..9*scale (36)` attempts: `c = spot(room3)` where `room3`
  = all 9 cells of the 3x3 around c are `freeLow`. Place solid: `tent` at `c-W`, `campfire` at `c+1`,
  `['barrel','crate','logs'][i%3]` at `c-W+1`, `['hay','cart','crate'][i%3]` at `c-1`; and a chest at `c`.
- **Loose chests**: `16*scale = 64` attempts of `spot()` → chest. Measured total treasures ≈ 98–100.
- **Landmarks** (solid, block movement): `54*scale = 216` attempts of `spot()`, name = `landmarks[i % 28]` with
  `['well','statue','column','ruin_arch','deadtree','boulder','deadtree','scarecrow','grave','menhir','signpost','ruin_wall','fence','stone_circle','cart','hay','logs','barrel','shrine','column','obelisk','grave','wagon_wreck','beehive','signpost','fence','windmill','boulder']`.
  (These are the "ruins, menhirs, mills (windmill), wrecks (wagon_wreck)" of the task list.) Measured: 360 solid props
  total (camps + landmarks).
- **Ground details** (non-solid, walkable), for each `[name, count]`, `count*scale` tries of a random cell
  (`x = 3 + floor(rng*(W-6))`, same for y), placed only if `freeLow`:
  `grass 110, tall_grass 70, flowers 60, flowers_red 34, flowers_blue 34, flowers_white 34, clover 44, ferns 44,
  pebbles 44, sticks 30, mushrooms 34, stump 18, fallen_log 14, bones 14, puddle 12, pumpkins 8`.

GAMEPLAY parts: chests (resource pickups) and the fact that solid props block movement/building. The specific prop
types and the ground details are [VISUAL] and can be redesigned freely (the new generator may place different sprites)
as long as the chest count/spread and "no blocked chokepoints" (step 14) are preserved.

### 5.14 Step 14 — prune unreachable objects [GAMEPLAY] (world.js:730-743)

```
resetBlock()
for o in trees ∪ golds ∪ stones: block[o.c] = 0         // resources don't count as obstacles here: a forest can be cut through
way = computeField([idx(b0.x, b0.y+2)]); lost(c) = way[c] == INF
shut(o) = none of the 8 cells around the 2x2 quad is reachable:
          [c-1, c+2, c-W, c+2W, c+W-1, c+W+2, c-W+1, c+2W+1]
trees, golds, stones, treasures: drop those with lost(o.c)
meadows: drop those with lost(cellAt(m.x, m.y))
points, bigs: drop those with shut(o)
resetBlock()
```
Solid props in unreachable spots are kept (scenery). After this the `block` layer is the final static layer that
`sound()` then tests against (with trees blocking again).

### 5.15 Step 15 — decorations [VISUAL] (world.js:745-782)

- Bushes/rocks: up to `12000*scale` tries while `bushes+rocks < 330*scale (1320)`; random cell, must be `freeLow`;
  `used = 1`; px `(x*64+16+rng*32, y*64+30+rng*26)`, `v = floor(rng*4)`, `phase = rng*8`; goes to `bushes` while
  `bushes.len < 200` else `rocks`. (Measured: always 200 bushes, 1 120 rocks.) Non-blocking.
- Water rocks: up to `14000*scale` tries, max `100*scale = 400`; cell `x in [1,W-2]`, `y in [1,H-2]`, water, not
  used, no mainland within Chebyshev 1, mainland within 3; `{x: cx, y: cy, v: floor(rng*4), phase: rng*16}`.
- Ducks: max `9*scale = 36` in the sea (same cell rule but mainland within 4, and ≥ 1100 px from other ducks),
  `phase = rng*6`; plus one duck per pond at `(p.x*64, p.y*64)` with `pond: true`, and a `lilypads` prop per pond at
  `(round(p.x*64 + (rng-0.5)*60), round(p.y*64 + 30))`, `c = -1`, non-solid.
- Shore props: shore cells = water, non-cliff, non-used cells with mainland directly north, x in [2,W-3], y in
  [2,H-3]. `70*scale = 280` picks without replacement; by pick index `i`: `i%9==0` → `boat`, else `i%13==5` →
  `dock`, else `i%17==8` → `shipwreck`, else `i%4==0` → `lilypads`, else `reeds`. Position
  `(cx + round((rng-0.5)*30), cy + (big ? 14 : 6))`, big = boat/dock/shipwreck; `c = -1`, non-solid. (These are the
  "piers (dock) and wrecks (shipwreck)".)
- `openWater`: cells `x in [1,W-2]`, `y in [1,H-2]` with no mainland within Chebyshev 1 and mainland within 5.

---

## 6. Generation invariants the new (aesthetic) generator MUST keep [GAMEPLAY]

1. 288x192 cells (same scale of distances, sight radii, build radii, unit speeds). Changing it would rebalance the game.
2. Exactly 5 bases on mainland, pairwise ≥ 64 cells apart; each base has a fully land, obstacle-free
   19x15 zone (`BASE_CLEAR` ±9/±7) with land guaranteed ±12/±10; castle 5x2 at `(x-2, y-1)`.
3. All bases, all plateau stair landings and terrace landings mutually reachable on foot with static obstacles present.
4. Per-base start stock (`BASE_STOCK` + a meadow within 14), placed in the same relative directions toward the map
   centre for fairness (`near()` table) — "partenza equa".
5. Elevation semantics: levels 0/1/2, cliffs impassable, level changes only via 2-cell stairs (`linkOK`), buildings
   only on one level, not on stairs, not on `noBuild`; stair access zones (3x4 around stair top) are `noBuild`.
6. The plateau closest to the base centroid holds 3 gold groups (+1 stone); the next 15 closest hold 1 gold + 1 stone;
   all hills hold 3–6 trees.
7. Capture points (≈36*rich, kinds cycling `POINT_MIX`, ≥20 cells from bases, ≥11 from other marks, 4x4 `noBuild`,
   2x2 footprint, each with its `POINT_STOCK` resources) and big deposits (≈20*rich, alternating gold 1500 / stone
   1800, ≥18 from bases, ≥9 from other marks, 2x2).
8. Scattered resource density ≈ as in 5.12; node amounts gold 240, stone 300, tree 40 wood.
9. Chests ≈100 (36 camps + 64 loose attempts), kinds cycling wood/gold/meat/stone, ≥14 cells from bases, 50 resources each.
10. Nothing gameplay-relevant (resource, chest, meadow, point, big) in an unreachable spot (step 14 rules).
11. Unreachable land pockets: `noBuild`.
12. Ponds/lakes/gulfs/islets/foam/decor/prop sprites: free to redesign. The user's goal: a more beautiful, less random
    map using the full asset set; HD-2D voxel terrain (Minecraft-like blocks for ground/mountains) with 2D billboards
    for trees, characters, structures. Elevation in the new renderer can be shown as block stacks; keep the logical
    `level` 0..2 (or extend, if the sim spec is updated accordingly).

---

## 7. Rendering-only code [VISUAL][RENDER] (world.js:786-889)

To be replaced by the HD-2D renderer; documented for reference of what information the old view showed.

- `edge(before, after)` → tileset strip index: `0` start edge, `1` centre, `2` end edge, `3` isolated
  (both→1, !before&after→0, before&!after→2, neither→3).
- `buildTiles()`: `tFlat[c] = col | row<<2` from `edge` of ground (land>0) neighbours horizontally/vertically (tileset
  `Tilemap_color1`). For L in 1..2: `tTop[L-1][c] = tileset<<8 | col<<4 | row`, `tShadow[L-1][c]` bits (1 shadow, 2
  ends left, 4 ends right):
  - wall of this level (`cliff && level[c-W]==L && level[c]<L`): tileset `plateau[c-W]`, col `5 + edge(wall left,
    wall right)`, row `4` on land (with shadow) / `5` on water (wet foot, no shadow);
  - stair rising to this level (`stair != 0 && level == (stair odd ? L-1 : L)`): col `0` (left, stair ≤2) or `3`
    (right), row `4` upper half / `5` lower half; shadow on lower half;
  - raised surface (`level >= L`): col `5 + edge(up left, up right)`, row `edge(up above, up below)`.
- `drawGround`: draws visible flat tiles, then per level shadow (`Shadow.png`, 64x192 slices at `y*64-42`) then tops.
  Tiles: `Terrain/Tileset/Tilemap_color{1..5}.png`.
- `renderMinimap()`: 2 px per cell; sea = pattern `Terrain/Tileset/Water Background color.png` or `#47aba9`; cliff
  `#5c8f92`, stair `#c9b87a`, level 2 `#d9e58a`, level 1 `#b9cf62`, ground `#8fb24c`; trees `#4f7f3a`, gold
  `#f2d24b`, stone `#9aa6ad`; bigs 2x2 with `#1b1f33` 1-px border, fill gold `#f2d24b` / stone `#c3ccd1`. The minimap is
  a UI feature [GAMEPLAY-UI: keep a minimap], but its look can change.

---

## 8. Network serialization (world.js:903-930)

Old model: the host generates the world and sends it to clients (they cannot regenerate it identically because the
JS generator is seeded but not shipped as authoritative state).

`serialize()`: `cells[c] = land | level<<2 | cliff<<4 | noBuild<<5 | stair<<6 | plateau<<9` (land 2 bits, level 2
bits, cliff 1, noBuild 1, stair 3 bits, plateau 3+ bits), plus lists `bases, plateaus, ponds, trees, golds, stones,
props, meadows, bushes, rocks, waterRocks, ducks, foam, openWater, points, bigs`. NOT sent: `treasures`
(pickups are synchronized as entities), `reserved`, `block`/`wallOf`/`ownOf` (rebuilt).

`load(data)`: re-allocate layers, unpack bits (`land = v&3, level = v>>2&3, cliff = v>>4&1, noBuild = v>>5&1,
stair = v>>6&7, plateau = v>>9`), assign lists, `treasures = []`, `resetBlock()`, `fields.clear()`, `version++`,
then `buildTiles()`, `renderMinimap()`.

Rust recommendation: since the generator becomes a pure deterministic Rust function `generate(seed) -> WorldGen`
compiled into both server and WASM client, the server could send only the seed (+ generator version) and the client
regenerates — verify by a hash of the cell layers. Alternatively keep sending the packed layers (55 296 cells ×
2 bytes ≈ 110 KB, compresses well). Either way entity ids for trees/ores/points must be assigned in the same order on
both sides (main.js:40: "host and client number trees and deposits the same way: orders refer to them by id"; order
in `G.scenery()`: trees, golds (small then big), stones (small then big), points). Reconnect support (user request)
means the server must be able to resend the world to a rejoining client at any time.

---

## 9. Sim vs. render split, determinism notes

- Pure sim (Rust `sim` crate): grid layers, passability, `linkOK`, `stepOK`/`tryMove`/`clearLine`,
  `nearestPassable`, `moveMask`, `computeField`, `ring`, field cache with budgets, `stepFrom`, `setBlock`/`setOwner`/
  `resetBlock`, the whole generator (including gameplay-relevant object lists), `sound()`.
- Generation contains `Math.random()` only in the seed choice (main.js:74) and in sheep/kingdom shuffles (main.js:76,
  80) — outside world.js. Everything in world.js is driven by `mulberry32(seed)` → deterministic given the same
  floating-point math. Rust `f64` math matches JS for `+ - * / sqrt`; `sin/cos/atan2/pow/hypot` may differ in the
  last ulp across platforms — acceptable because the map is either generated once on the server or regenerated
  identically by the same Rust code (to be safe across native vs. wasm, prefer `libm` for transcendental functions in
  the generator).
- Render-only: tiles, shadows, minimap image, foam/openWater/ducks/waterRocks/bushes/rocks/non-solid props positions.
- Performance notes from comments: a field costs ~1 ms in JS ("30 fields in one step = 30 ms", "40 units = 30 fields =
  40 ms"); `moveMask` costs about as much as a field and halves field cost when cached. Rust should be much faster,
  but keep the budgets (they also define behaviour).

---

## 10. Open questions / ambiguities

1. `clearLine` comment vs. code (own buildings crossable in code) — see 3.2.
2. `stepFrom` from a non-standable cell ignores `linkOK` and diagonal-corner rules (can "exit" to another level). This
   mirrors `stepOK`'s "can always get out of a blocked cell". Intentional escape hatch; port as-is.
3. `nearestPassable` ignores levels and side (own walls are not considered passable for it).
4. Hill count is large (≈80–130 plateaus) because `wantHills` scales with land area; the new aesthetic generator
   will likely want fewer, larger, nicer mountains. That changes only invariant 6's "next 15" set; decide with the
   design owner.
5. `plateaus[].id` is just a tileset choice (1..4) and isn't used by sim rules; safe to drop or repurpose
   (e.g. biome/material for voxel blocks).
6. `walls` per side decides whether a kingdom gets its own field keys; in Rust key fields by
   `(target, effective_side, cross)` with the same rule.

---

## Addenda (critic)

Completeness check: every `World` method and top-level constant of `world.js` is referenced in this file
(checked by script). Resolutions of §10 open questions, verified against the code:

**W1. `clearLine` — port the code, not the comment.** world.js:73-83: the comment says "senza tagliare attraverso
gli edifici (nemmeno i propri)", but both `stepOK` calls pass `cross = true` (world.js:79), so `crossable` applies:
the line passes through cells of the mover's **own** buildings (`ownOf[c] == side`), never through enemy/neutral
blocked cells, cliffs, water, or across a level change outside a stair (`linkOK`). Only caller: `approach`
(entities.js:322, only when `d < 280`). Sampling step: `n = ceil(d/24)`; each sample is checked as an X move then a
Y move (L-shaped), so a diagonal squeeze between two blocked cells fails.

**W2. Chests do reach clients.** `World.serialize()` does not include `treasures`, but chests are not world data at
runtime: `G.start` turns every treasure into a pickup (`dropPickup(kind, cx, cy+6, TREASURE, Infinity, true)`,
main.js:77) and pickups are replicated in **every** snapshot with a chest flag (net.js:180 row
`[id, x, y-hop, kind, fading, chest]`, applied net.js:276). Opening one emits `['prop','chest_open',x,y]`
(entities.js:1283), a permanent decor event for everyone. Rust: either send chests as entities in the snapshot (as
now), or regenerate them from the seed on the client and replicate only "opened" ids; the first is simpler.

**W3. Seeds.** Confirmed: `World.generate((Math.random()*1e9)|0)` (main.js:74); generation itself is fully driven
by `mulberry32(seed + attempt*7919)` (world.js:285, 312) — no `Math.random` inside `build`. Rust: the server picks
the seed (any RNG), the generator must be a pure function `seed → World`; send seed + world hash to clients
(see 08-net R5). Note the base→faction assignment (main.js:80, `COLORS.slice().sort(() => Math.random()-0.5)`) is
*not* part of the world and must use the match RNG with a real Fisher–Yates shuffle (see 05 addenda, D2).

**W4. Plateau count vs. "next 15" rule — proposed decision.** world.js:541-549 sorts `hills` by distance to the
centroid of the 5 bases (`mid`); resources are then assigned in that order (§5). The redesigned generator may
produce fewer, larger mountains. Keep the *economy* invariant, not the plateau count: exactly **1 central mineral
site with 3 gold groups + 1 stone** (on the elevated area nearest `mid`) and **15 further mineral sites with
1 gold + 1 stone each**, placed on elevated ground (level ≥ 1), ordered by distance to `mid`; when there are fewer
than 16 mountains, put several sites on the same mountain (separate shelves/terraces, each reachable through a
stair, ≥ 6 cells apart). Trees on elevated ground: keep the overall count (≈ 3–6 per 50–100 elevated cells).
Plateau ids (1–4) are purely a tileset choice (no sim reads `plateau[]` except rendering and the minimap), so they can
be replaced by the HD-2D height/biome palette (see 09 §3, recommended reference mapping).

**W5. Field-cache budgets.** `World.refresh` / `World.fresh` (`FIELD_REFRESH`/`FIELD_NEW` = 2 each per step, set in `updateWorld`, main.js:292-293) are
per-step budgets; with a fixed Rust tick of 20 Hz instead of ~60 fps, use 6 per tick to keep the same per-second
throughput (or run the sim at 60 Hz and keep 2). This changes only how fast large groups start moving, not where.

**W6. Bounds of neighbour lookups.** Several JS loops index `field[k + dx + dy*W]` without bounds checks (e.g.
`estimateRates`, factions.js:238): JS returns `undefined` (comparisons false). In Rust use `get()` with
`f32::INFINITY`/`u16::MAX` as default; generation guarantees resources are never on border rows/columns so
row-wrap (`x = 0, dx = -1`) never happens in practice (the island is surrounded by sea) — keep that guarantee in the new
generator: no resource node or building cell on the outermost 2 rows/columns.
