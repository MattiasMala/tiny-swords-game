#!/usr/bin/env python3
"""Converte le immagini generate (tools/raw/*.webp) negli sprite del gioco (assets/Extra/...).

Ogni immagine grezza è un soggetto, o una griglia di soggetti/fotogrammi, su sfondo trasparente,
magenta o bianco. Per ciascuna: rimozione dello sfondo, ritaglio, riduzione alla scala del
pacchetto, aggancio alla tavolozza del pacchetto, allineamento dei fotogrammi delle animazioni e
ricolorazione nei cinque colori dei regni (mappa ricavata dagli sprite originali).

Uso:  python3 tools/make_sprites.py        (richiede Pillow e numpy)
Scrive anche js/extra.js (misure degli sprite) e rigenera js/manifest.js.
"""
import collections
import glob
import json
import os

import numpy as np
from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AS = os.path.join(ROOT, "assets")
RAW = os.path.join(ROOT, "tools", "raw")
OUT = os.path.join(AS, "Extra")
COLORS = ["Blue", "Red", "Yellow", "Purple", "Black"]
UNIT_SHADOW = (15, 18, 26, 79)       # come l'ombra sotto le unità del pacchetto
BUILDING_SHADOW = (22, 28, 46, 69)   # come l'ombra alla base degli edifici del pacchetto


def rgba(path):
    return np.array(Image.open(path).convert("RGBA"))


# ---------------------------------------------------------------- tavolozza e colori di squadra

def pack_palette(min_count=40):
    count = collections.Counter()
    patterns = ["Buildings/*/*.png", "Units/*/*/*.png", "Terrain/Resources/*/*/*.png", "Terrain/Decorations/*/*.png",
                "Terrain/Tileset/Tilemap_color1.png", "UI Elements/UI Elements/Icons/*.png", "Particle FX/*.png"]
    for pattern in patterns:
        for path in glob.glob(os.path.join(AS, pattern)):
            a = rgba(path)
            cols, n = np.unique(a[a[..., 3] == 255][:, :3], axis=0, return_counts=True)
            for c, k in zip(map(tuple, cols), n):
                count[c] += int(k)
    return np.array([c for c, k in count.items() if k >= min_count], dtype=np.float32)


def team_maps():
    """Per ogni colore: {colore blu -> colore corrispondente}, confrontando pixel per pixel gli sprite gemelli."""
    votes = {c: collections.defaultdict(collections.Counter) for c in COLORS[1:]}
    files = glob.glob(os.path.join(AS, "Buildings/Blue Buildings/*.png")) + glob.glob(os.path.join(AS, "Units/Blue Units/*/*.png"))
    for path in files:
        a = rgba(path)
        for c in COLORS[1:]:
            b = rgba(path.replace("Blue Buildings", c + " Buildings").replace("Blue Units", c + " Units"))
            if b.shape != a.shape:
                continue
            m = (a[..., 3] == 255) & (b[..., 3] == 255) & (a[..., :3] != b[..., :3]).any(-1)
            pairs, n = np.unique(np.concatenate([a[m][:, :3], b[m][:, :3]], axis=1), axis=0, return_counts=True)
            for row, k in zip(pairs, n):
                votes[c][tuple(int(v) for v in row[:3])][tuple(int(v) for v in row[3:])] += int(k)
    maps = {c: {s: d.most_common(1)[0][0] for s, d in votes[c].items() if sum(d.values()) >= 30} for c in COLORS[1:]}
    common = set.intersection(*[set(m) for m in maps.values()])
    blue = {k for k in common if k[2] > k[0] + 8}  # solo i toni blu/teal: i marroni che variano sono dettagli, non la squadra
    return {c: {k: maps[c][k] for k in blue} for c in COLORS[1:]}


PALETTE = pack_palette()
TEAM = team_maps()


def snap(rgb):
    """Colori (N,3) -> colore più vicino della tavolozza del pacchetto."""
    out = np.empty((len(rgb), 3), np.uint8)
    weight = np.array([0.9, 1.2, 0.9], np.float32)
    for i in range(0, len(rgb), 20000):
        d = ((rgb[i:i + 20000][:, None, :].astype(np.float32) - PALETTE[None]) ** 2 * weight).sum(-1)
        out[i:i + 20000] = PALETTE[d.argmin(1)]
    return out


def teamify(im):
    """I blu/teal saturi che non sono fra i colori di squadra noti vengono portati al più vicino di quelli,
    così la ricolorazione li prende tutti (scudi, pennacchi, tegole)."""
    keys = np.array(list(TEAM["Red"]), np.float32)
    a = np.array(im)
    m = a[..., 3] == 255
    flat = a[m][:, :3].astype(np.float32)
    r, g, b = flat[:, 0], flat[:, 1], flat[:, 2]
    hi, lo = flat.max(1), flat.min(1)
    bluish = (b >= r + 25) & (g >= r) & ((hi - lo) / np.maximum(hi, 1) > 0.3) & (hi > 70)
    d = ((flat[bluish][:, None, :] - keys[None]) ** 2).sum(-1)
    near = d.min(1) < 70 ** 2
    out = flat.copy()
    idx = np.where(bluish)[0][near]
    out[idx] = keys[d.argmin(1)[near]]
    a[m, :3] = out.astype(np.uint8)
    return Image.fromarray(a)


def recolor(im, color):
    if color == "Blue":
        return im
    a = np.array(im)
    m = a[..., 3] == 255
    flat = a[m][:, :3]
    res = flat.copy()
    for src, dst in TEAM[color].items():
        res[(flat == np.array(src, np.uint8)).all(1)] = dst
    a[m, :3] = res
    return Image.fromarray(a)


# ---------------------------------------------------------------- sfondo e componenti

def components(mask):
    """Etichetta le regioni connesse di una maschera booleana. Ritorna (etichette, n)."""
    im = Image.fromarray(np.where(mask, 1, 0).astype(np.int32), "I")
    n = 0
    while True:
        a = np.array(im)
        rest = np.argwhere(a == 1)
        if not len(rest):
            return a - 1, n   # sfondo = -1, regioni 1..n
        n += 1
        y, x = rest[0]
        ImageDraw.floodfill(im, (int(x), int(y)), n + 1)


def foreground(path):
    """Ritorna (rgb, maschera del soggetto) qualunque sia lo sfondo: trasparente, magenta o bianco."""
    a = rgba(path)
    rgb, alpha = a[..., :3], a[..., 3]
    if (alpha < 128).mean() > 0.05:
        return rgb, alpha >= 110
    r, g, b = (rgb[..., k].astype(int) for k in range(3))
    corners = np.concatenate([rgb[:6, :6].reshape(-1, 3), rgb[:6, -6:].reshape(-1, 3), rgb[-6:, :6].reshape(-1, 3), rgb[-6:, -6:].reshape(-1, 3)])
    cr, cg, cb = np.median(corners, axis=0)
    if cg < 90 and cr > 180 and cb > 180:  # magenta
        return rgb, ~(((r + b) / 2 - g > 90) & (np.abs(r - b) < 70) & (r > 120))
    # bianco: è sfondo ciò che è quasi bianco e tocca il bordo, più i buchi chiusi abbastanza grandi
    # (fra le zampe, dentro un manico); i riflessi bianchi dentro il disegno sono piccoli e restano
    whiteish = np.minimum(np.minimum(r, g), b) >= 244
    small = whiteish[::2, ::2]
    lab, n = components(small)
    bg = np.zeros_like(small)
    for k in range(1, n + 1):
        region = lab == k
        ys, xs = np.where(region)
        border = ys.min() == 0 or xs.min() == 0 or ys.max() == small.shape[0] - 1 or xs.max() == small.shape[1] - 1
        if border or region.sum() >= 60:
            bg |= region
    bg = np.kron(bg, np.ones((2, 2), bool))[:rgb.shape[0], :rgb.shape[1]]
    return rgb, ~(bg & whiteish)


def split_grid(fg, cols, rows):
    """Maschera di ogni cella della griglia: le regioni connesse vanno alla cella che contiene il loro centro."""
    q = 4
    small = fg[::q, ::q]
    lab, n = components(small)
    h, w = small.shape
    cells = [np.zeros_like(small) for _ in range(cols * rows)]
    for k in range(1, n + 1):
        region = lab == k
        if region.sum() < 3:
            continue
        ys, xs = np.where(region)
        cx, cy = int(xs.mean() * cols / w), int(ys.mean() * rows / h)
        cells[min(rows - 1, cy) * cols + min(cols - 1, cx)] |= region
    return [np.kron(c, np.ones((q, q), bool))[:fg.shape[0], :fg.shape[1]] & fg for c in cells]


# ---------------------------------------------------------------- riduzione a sprite

def bbox(mask):
    ys, xs = np.where(mask)
    return xs.min(), ys.min(), xs.max() + 1, ys.max() + 1


def shrink(rgb, fg, scale):
    """Soggetto (ritagliato sul suo riquadro) ridotto di `scale` e agganciato alla tavolozza."""
    x0, y0, x1, y1 = bbox(fg)
    rgb, fg = rgb[y0:y1, x0:x1], fg[y0:y1, x0:x1]
    w, h = max(1, round(rgb.shape[1] * scale)), max(1, round(rgb.shape[0] * scale))
    cover = np.array(Image.fromarray((fg * 255).astype(np.uint8)).resize((w, h), Image.BOX)).astype(np.float32) / 255
    pre = np.stack([np.array(Image.fromarray((rgb[..., k] * fg).astype(np.uint8)).resize((w, h), Image.BOX)).astype(np.float32) for k in range(3)], -1)
    solid = cover > 0.5
    out = np.zeros((h, w, 4), np.uint8)
    out[solid, :3] = snap(pre[solid] / np.maximum(cover[solid][:, None], 1e-3))
    out[solid, 3] = 255
    return Image.fromarray(out)


def save(im, *parts):
    path = os.path.join(OUT, *parts)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    im.save(path)


def with_shadow(im, rx, ry, cx, cy, color):
    """Aggiunge sotto lo sprite un'ellisse d'ombra (non si somma ai pixel già opachi)."""
    shadow = Image.new("RGBA", im.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).ellipse([cx - rx, cy - ry, cx + rx, cy + ry], fill=color)
    shadow.alpha_composite(im)
    return shadow


META = {"buildings": {}, "units": {}, "props": {}, "icons": [], "stones": [], "deposits": {}}


def building(raw, name, width, team=True):
    rgb, fg = foreground(os.path.join(RAW, raw + ".webp"))
    x0, _, x1, _ = bbox(fg)
    sprite = shrink(rgb, fg, width / (x1 - x0))
    pad = 6
    canvas = Image.new("RGBA", (sprite.width + pad * 2, sprite.height + pad * 2), (0, 0, 0, 0))
    canvas.alpha_composite(sprite, (pad, pad))
    if team:
        canvas = teamify(with_shadow(canvas, sprite.width / 2 + 3, 13, canvas.width / 2, pad + sprite.height - 10, BUILDING_SHADOW))
        for c in COLORS:
            save(recolor(canvas, c), "Buildings", c, name + ".png")
    else:
        save(canvas, "Buildings", name + ".png")
    # ax, ay = punto dello sprite che va sul centro del bordo inferiore dell'ingombro
    META["buildings"][name] = {"w": canvas.width, "h": canvas.height, "ax": canvas.width // 2, "ay": pad + sprite.height - 4,
                               "crop": [pad, pad, sprite.width, sprite.height]}


def align(frames):
    """Sposta ogni fotogramma perché combaci il più possibile col primo (il corpo resta fermo, si muove il resto)."""
    base = np.array(frames[0])
    placed = [(0, 0)]
    for f in frames[1:]:
        a = np.array(f)
        best, off = -1, (0, 0)
        # partenza: allineati in basso al centro, poi ricerca attorno
        sx, sy = (base.shape[1] - a.shape[1]) // 2, base.shape[0] - a.shape[0]
        for dy in range(sy - 14, sy + 15):
            for dx in range(sx - 18, sx + 19):
                ax0, ay0 = max(0, dx), max(0, dy)
                ax1, ay1 = min(base.shape[1], dx + a.shape[1]), min(base.shape[0], dy + a.shape[0])
                if ax1 <= ax0 or ay1 <= ay0:
                    continue
                p = base[ay0:ay1, ax0:ax1]
                q = a[ay0 - dy:ay1 - dy, ax0 - dx:ax1 - dx]
                score = ((p[..., 3] == 255) & (q[..., 3] == 255) & (p[..., :3] == q[..., :3]).all(-1)).sum()
                if score > best:
                    best, off = score, (dx, dy)
        placed.append(off)
    return placed


def animation(raw, name, cols, rows, height, ref=0, shadow=(30, 9), bob=None, grid=False):
    """Foglio di fotogrammi -> una striscia orizzontale per colore. `height` = altezza del fotogramma `ref`
    nel gioco; `bob` = spostamenti verticali per ricavare un'animazione dal solo primo fotogramma;
    `grid` = ogni fotogramma tiene la posizione che ha nella sua cella del foglio grezzo (piedi a terra
    come li ha disegnati il modello) invece di essere allineato al primo sul corpo."""
    rgb, fg = foreground(os.path.join(RAW, raw + ".webp"))
    cells = split_grid(fg, cols, rows)
    x0, y0, x1, y1 = bbox(cells[ref])
    scale = height / (y1 - y0)
    frames = [shrink(rgb, c, scale) for c in cells]
    if bob:
        frames, offsets = [frames[0]] * len(bob), [(0, dy) for dy in bob]
    elif grid:
        h, w = fg.shape
        pos = [(bbox(c)[0] - (i % cols) * w / cols, bbox(c)[1] - (i // cols) * h / rows) for i, c in enumerate(cells)]
        offsets = [(round((x - pos[0][0]) * scale), round((y - pos[0][1]) * scale)) for x, y in pos]
    else:
        offsets = align(frames)
    # tela comune: il piede del primo fotogramma è la linea di terra, al centro
    left = min(dx for dx, _ in offsets)
    top = min(dy for _, dy in offsets)
    right = max(dx + f.width for (dx, _), f in zip(offsets, frames))
    bottom = max(dy + f.height for (_, dy), f in zip(offsets, frames))
    ground_x, ground_y = frames[0].width / 2 - left, frames[0].height - top
    half = int(max(ground_x, right - left - ground_x)) + 6
    fw = (half * 2 + 31) // 32 * 32
    fh = (int(bottom - top) + 14 + 31) // 32 * 32
    ax, ay = fw // 2, fh - 10 - int(bottom - top - ground_y)
    strip = Image.new("RGBA", (fw * len(frames), fh), (0, 0, 0, 0))
    for i, (f, (dx, dy)) in enumerate(zip(frames, offsets)):
        cell = Image.new("RGBA", (fw, fh), (0, 0, 0, 0))
        cell.alpha_composite(f, (int(ax - ground_x + dx - left), int(ay - ground_y + dy - top)))
        strip.alpha_composite(with_shadow(cell, shadow[0], shadow[1], ax, ay - 2, UNIT_SHADOW), (i * fw, 0))
    strip = teamify(strip)
    for c in COLORS:
        save(recolor(strip, c), "Units", c, name + ".png")
    META["units"][name] = {"fw": fw, "fh": fh, "n": len(frames), "ax": ax, "ay": ay}


def sheet(raw, cols, rows, items, folder, box=None):
    """Griglia di oggetti distinti -> un file per oggetto. items = [(nome, altezza)]; con `box` ogni oggetto
    viene centrato in una tela quadrata di quel lato (icone)."""
    rgb, fg = foreground(os.path.join(RAW, raw + ".webp"))
    out = []
    for (name, height), cell in zip(items, split_grid(fg, cols, rows)):
        x0, y0, x1, y1 = bbox(cell)
        scale = height / (y1 - y0) if not box else min(height / (y1 - y0), height / (x1 - x0))
        sprite = shrink(rgb, cell, scale)
        if box:
            canvas = Image.new("RGBA", (box, box), (0, 0, 0, 0))
            canvas.alpha_composite(sprite, ((box - sprite.width) // 2, (box - sprite.height) // 2))
            sprite = canvas
        save(sprite, folder, name + ".png")
        out.append((name, sprite))
    return out


def stone_carry():
    """Pedone che porta la pietra: come quello che porta l'oro, con la pepita ricolorata in grigio.
    Questi cinque colori compaiono solo nella pepita (il pedone senza carico non li ha)."""
    nugget = {(255, 255, 255): (214, 220, 220), (251, 255, 168): (190, 198, 200), (233, 240, 68): (146, 158, 166),
              (206, 165, 84): (104, 118, 132), (172, 122, 88): (74, 84, 98)}
    nugget = {k: tuple(int(v) for v in snap(np.array([g], np.float32))[0]) for k, g in nugget.items()}
    for anim in ["Idle", "Run"]:
        blue = rgba(os.path.join(AS, "Units/Blue Units/Pawn", f"Pawn_{anim} Gold.png"))
        masks = {k: (blue[..., 3] == 255) & (blue[..., :3] == np.array(k, np.uint8)).all(-1) for k in nugget}
        for c in COLORS:  # la forma è la stessa in tutti i colori: la maschera si prende dal blu, che non ha altri gialli
            a = rgba(os.path.join(AS, f"Units/{c} Units/Pawn", f"Pawn_{anim} Gold.png"))
            for k, grey in nugget.items():
                a[masks[k], :3] = grey
            save(Image.fromarray(a), "Units", c, f"Pawn_{anim} Stone.png")
        META["units"][f"Pawn_{anim} Stone"] = {"fw": 192, "fh": 192, "n": 8 if anim == "Idle" else 6, "ax": 96, "ay": 134}


def main():
    building("smith", "Smith", 184)
    building("market", "Market", 190)
    building("stable", "Stable", 188)
    building("siege", "Siege", 190)
    building("depot", "Depot", 124)
    building("farm", "Farm", 190)
    building("wall", "Wall", 64, team=False)
    building("camp", "Camp", 188)
    for raw in ["outpost", "mine", "quarry", "sawmill", "ranch"]:  # strutture neutrali, senza colori di squadra
        building(raw, raw.capitalize(), 156, team=False)

    animation("knight_idle", "Knight_Idle", 3, 2, 100)
    animation("knight_run", "Knight_Run", 3, 2, 100, ref=0)
    animation("knight_attack", "Knight_Attack", 3, 2, 100)
    animation("ram", "Ram_Attack", 2, 2, 86, shadow=(46, 10))
    animation("ram", "Ram_Run", 2, 2, 86, shadow=(46, 10), bob=[0, -1, 0, 1])
    animation("catapult", "Catapult_Attack", 2, 2, 52, shadow=(44, 9))
    animation("catapult", "Catapult_Run", 2, 2, 52, shadow=(44, 9), bob=[0, -1, 0, 1])
    animation("scout_idle", "Scout_Idle", 3, 2, 66, grid=True)  # 66 px = altezza visibile del pedone (Pawn_Idle)
    animation("scout_run", "Scout_Run", 3, 2, 66, grid=True)
    stone_carry()

    props = sheet("props_a", 4, 3, [("barrel", 40), ("crate", 40), ("logs", 40), ("hay", 44), ("well", 76), ("signpost", 58),
                                    ("campfire", 44), ("cart", 46), ("tent", 84), ("mushrooms", 40), ("flowers", 36), ("statue", 84)], "Props")
    props += sheet("props_b", 4, 3, [("column", 60), ("bones", 34), ("deadtree", 88), ("fence", 40), ("scarecrow", 68), ("grass", 36),
                                     ("chest", 38), ("chest_open", 44), ("grave", 50), ("boat", 44), ("lilypads", 34), ("reeds", 52)], "Props")
    props += sheet("props_c", 4, 3, [("ruin_arch", 96), ("ruin_wall", 60), ("menhir", 84), ("stone_circle", 56), ("boulder", 56), ("shrine", 80),
                                     ("obelisk", 92), ("windmill", 124), ("shipwreck", 64), ("dock", 48), ("wagon_wreck", 50), ("beehive", 46)], "Props")
    props += sheet("props_d", 4, 3, [("flowers_red", 34), ("flowers_blue", 34), ("flowers_white", 32), ("tall_grass", 40), ("clover", 26),
                                     ("pebbles", 26), ("puddle", 30), ("fallen_log", 36), ("stump", 34), ("pumpkins", 38), ("ferns", 40), ("sticks", 24)], "Props")
    META["props"] = {name: [im.width, im.height] for name, im in props}
    stones = sheet("res_stone", 2, 2, [("Stone_1", 34), ("Stone_2", 52), ("Stone_3", 86), ("Stone_Resource", 22)], "Resources")
    META["stones"] = {name: [im.width, im.height] for name, im in stones}
    deposits = sheet("deposits", 2, 1, [("Gold_Big", 108), ("Stone_Big", 108)], "Resources")
    META["deposits"] = {name: [im.width, im.height] for name, im in deposits}
    icons = sheet("icons_a", 4, 3, [(n, 48) for n in ["stone", "crown", "anvil", "horseshoe", "flag", "stop", "arrows", "shield", "upgrade", "hourglass", "wheat", "spyglass"]], "Icons", box=64)
    icons += sheet("icons_b", 4, 3, [(n, 48) for n in ["sword", "breastplate", "pickaxe", "axe", "coins", "scales", "barrow", "herbs", "tower", "boot", "book", "bell"]], "Icons", box=64)
    icons += sheet("icons_c", 2, 2, [(n, 48) for n in ["tent", "map", "eye", "banner"]], "Icons", box=64)
    META["icons"] = [name for name, _ in icons]

    # the Rust client reads docs/spec/assets.json; these two files are only a by-product for reference
    os.makedirs(os.path.join(ROOT, "js"), exist_ok=True)
    with open(os.path.join(ROOT, "js", "extra.js"), "w") as f:
        f.write("'use strict';\n// Generato da tools/make_sprites.py: misure degli sprite aggiuntivi in assets/Extra/.\n")
        f.write("const EXTRA = " + json.dumps(META, indent=1) + ";\n")
    files = sorted(os.path.relpath(os.path.join(d, n), AS) for d, _, names in os.walk(AS) for n in names if n.endswith(".png"))
    with open(os.path.join(ROOT, "js", "manifest.js"), "w") as f:
        f.write("'use strict';\n// Generato da tools/make_sprites.py: ogni PNG in assets/ (pacchetto \"Tiny Swords (Free Pack)\" + assets/Extra).\n")
        f.write("const ASSET_FILES = " + json.dumps(files, indent=1) + ";\n")
    print(len(files), "sprite in assets/;", sum(len(v) for v in META.values()), "voci in js/extra.js")


if __name__ == "__main__":
    main()
