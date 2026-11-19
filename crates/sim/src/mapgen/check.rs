//! Gameplay invariants of spec 02 §6 (+ addenda) and the aesthetic outline rules, checked on a generated world.
//! Used by the tests (50 seeds) and by `examples/mapgen_png.rs --check`.

use super::grid::Mask;
use super::*;
use crate::map::Surface;

/// Checks the invariants of 02 §6 (+ addenda) on a generated world. `Err` describes the first violation.
pub fn check_invariants(g: &GeneratedWorld) -> Result<(), String> {
    let m = &g.map;
    let n = m.len();
    if m.w != W || m.h != H {
        return Err("map size".into());
    }
    // ---- 2. bases ----
    if g.bases.len() != BASE_COUNT {
        return Err(format!("{} bases", g.bases.len()));
    }
    for i in 0..g.bases.len() {
        for j in 0..i {
            let (a, b) = (g.bases[i], g.bases[j]);
            let d = (((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f64).sqrt();
            if d < BASE_GAP as f64 {
                return Err(format!("bases {i},{j} only {d:.1} apart"));
            }
        }
        let (bx, by) = g.bases[i];
        for y in by - BASE_LAND.1..=by + BASE_LAND.1 {
            for x in bx - BASE_LAND.0..=bx + BASE_LAND.0 {
                if x >= 2 && y >= 2 && x < W - 2 && y < H - 3 && m.land[m.idx(x, y) as usize] != 1 {
                    return Err(format!("base {i}: no land at ({x},{y})"));
                }
            }
        }
        let lvl = m.level[m.idx(bx, by) as usize];
        for y in by - BASE_CLEAR.1..=by + BASE_CLEAR.1 {
            for x in bx - BASE_CLEAR.0..=bx + BASE_CLEAR.0 {
                let c = m.idx(x, y) as usize;
                if m.land[c] != 1
                    || m.static_block[c] != 0
                    || m.cliff[c] != 0
                    || m.stair[c] != 0
                    || m.no_build[c] != 0
                    || m.level[c] != lvl
                {
                    return Err(format!("base {i}: clear zone not clear at ({x},{y})"));
                }
            }
        }
        if !m.can_place(BuildingKind::Castle, bx - 2, by - 1, &[]) {
            return Err(format!("base {i}: castle not placeable"));
        }
    }
    // ---- 3. reachability (static obstacles present) ----
    let mut goals: Vec<i32> = g.bases.iter().map(|b| m.idx(b.0, b.1 + 2)).collect();
    for p in &g.plateaus {
        goals.push(m.idx(p.landing.0, p.landing.1));
        if let Some(t) = p.top {
            goals.push(m.idx(t.0, t.1));
        }
    }
    let f = compute_field(m, &[goals[0]], 0, false);
    if let Some(c) = goals.iter().find(|c| f[**c as usize] == FIELD_INF) {
        return Err(format!("goal {:?} unreachable", m.xy(*c)));
    }
    // ---- 4. start stock ----
    let open = |c: i32| [c - 1, c + 1, c - W, c + W].iter().any(|k| m.valid(*k) && f[*k as usize] < FIELD_INF);
    for (i, &(bx, by)) in g.bases.iter().enumerate() {
        for (kind, r, need) in BASE_STOCK {
            let cells: Vec<i32> = match kind {
                NodeKind::Tree => g.trees.iter().map(|t| t.cell).collect(),
                NodeKind::Gold => g.golds.iter().map(|t| t.cell).collect(),
                NodeKind::Stone => g.stones.iter().map(|t| t.cell).collect(),
            };
            let cnt = cells
                .iter()
                .filter(|c| {
                    let (x, y) = m.xy(**c);
                    ((((x - bx).pow(2) + (y - by).pow(2)) as f64).sqrt() as f32) < r && open(**c)
                })
                .count();
            if cnt < need {
                return Err(format!("base {i}: {cnt} {kind:?} within {r} < {need}"));
            }
        }
        if !g.meadows.iter().any(|p| {
            let (dx, dy) = (p.x / 64.0 - bx as f32, p.y / 64.0 - by as f32);
            (dx * dx + dy * dy).sqrt() < BASE_MEADOW
        }) {
            return Err(format!("base {i}: no meadow"));
        }
    }
    // ---- 5. elevation semantics ----
    for c in 0..n {
        let ci = c as i32;
        let (x, y) = m.xy(ci);
        let l = m.level[c];
        if m.land[c] == 1 && l > 0 && m.stair[c] == 0 && y + 1 < H {
            let s = c + W as usize;
            if m.level[s] < l && m.cliff[s] == 0 {
                return Err(format!("raised cell ({x},{y}) without cliff below"));
            }
        }
        let st = m.stair[c];
        if st != 0 {
            if m.land[c] != 1 || m.cliff[c] != 0 {
                return Err(format!("stair on bad ground ({x},{y})"));
            }
            if st % 2 == 0 {
                let lo = c + W as usize;
                if m.stair[lo] != st - 1 || m.level[lo] + 1 != l || m.no_build[c] == 0 {
                    return Err(format!("broken stair at ({x},{y})"));
                }
            } else if m.stair[c - W as usize] != st + 1 {
                return Err(format!("lower stair half without upper at ({x},{y})"));
            }
        }
        if m.cliff[c] != 0 && y > 0 && m.level[c - W as usize] <= m.level[c] && m.land[c] == 1 {
            return Err(format!("cliff ({x},{y}) not below a raised cell"));
        }
    }
    // ---- 6. mineral sites (W4) ----
    if g.plateaus.is_empty() {
        return Err("no plateau".into());
    }
    let gold_on_first = g.golds.iter().filter(|d| m.plateau[d.cell as usize] == 1).count();
    if gold_on_first < 3 {
        return Err(format!("only {gold_on_first} gold nodes on the central plateau"));
    }
    let high_gold = g.golds.iter().filter(|d| m.level[d.cell as usize] > 0).count();
    if high_gold < 16 {
        return Err(format!("only {high_gold} gold nodes on elevated ground"));
    }
    // ---- 7-10. objects: in margin, reachable, never on bad ground ----
    let inner = |c: i32| {
        let (x, y) = m.xy(c);
        x >= 2 && y >= 2 && x < W - 2 && y < H - 2
    };
    let walk = {
        let mut mm = m.clone();
        // resources do not block for the prune rule
        for c in g.trees.iter().map(|t| t.cell).chain(g.golds.iter().map(|d| d.cell)).chain(g.stones.iter().map(|d| d.cell)) {
            mm.block[c as usize] = 0;
        }
        compute_field(&mm, &[goals[0]], 0, false)
    };
    let all_nodes = g
        .trees
        .iter()
        .map(|t| t.cell)
        .chain(g.golds.iter().map(|d| d.cell))
        .chain(g.stones.iter().map(|d| d.cell))
        .chain(g.chests.iter().map(|d| d.cell));
    for c in all_nodes {
        let ci = c as usize;
        if !inner(c) || m.land[ci] != 1 || m.cliff[ci] != 0 || m.stair[ci] != 0 {
            return Err(format!("object on bad cell {:?}", m.xy(c)));
        }
        if walk[ci] == FIELD_INF {
            return Err(format!("object unreachable {:?}", m.xy(c)));
        }
    }
    for p in &g.points {
        let (x, y) = m.xy(p.cell);
        for b in &g.bases {
            if ((((b.0 - x).pow(2) + (b.1 - y).pow(2)) as f64).sqrt()) <= POINT_BASE_GAP as f64 {
                return Err("point too close to a base".into());
            }
        }
        for yy in y - 1..=y + 2 {
            for xx in x - 1..=x + 2 {
                if m.no_build[m.idx(xx, yy) as usize] == 0 {
                    return Err("point without noBuild margin".into());
                }
            }
        }
        if m.quad(p.cell).iter().any(|k| m.static_block[*k as usize] == 0) {
            return Err("point not blocking".into());
        }
    }
    for (_, b) in &g.bigs {
        let (x, y) = m.xy(b.cell);
        if g.bases.iter().any(|q| ((((q.0 - x).pow(2) + (q.1 - y).pow(2)) as f64).sqrt()) <= BIG_BASE_GAP as f64) {
            return Err("big deposit too close to a base".into());
        }
    }
    // 2x2 objects: inside the 2-cell margin (W6), on one level, spaced like `spread()` (points ≥ 11 from points,
    // bigs ≥ 9 from every earlier mark)
    let quads: Vec<(i32, f64)> = g
        .points
        .iter()
        .map(|p| (p.cell, POINT_MARK_GAP as f64))
        .chain(g.bigs.iter().map(|b| (b.1.cell, BIG_MARK_GAP as f64)))
        .collect();
    for (i, &(c, gap)) in quads.iter().enumerate() {
        let (x, y) = m.xy(c);
        if !(inner(c) && inner(c + W + 1)) {
            return Err(format!("2x2 object in the border margin {:?}", (x, y)));
        }
        if m.quad(c).iter().any(|k| m.level[*k as usize] != m.level[c as usize] || m.land[*k as usize] != 1) {
            return Err(format!("2x2 object on bad ground {:?}", (x, y)));
        }
        for &(o, _) in &quads[..i] {
            let (ox, oy) = m.xy(o);
            if ((((ox - x).pow(2) + (oy - y).pow(2)) as f64).sqrt()) < gap {
                return Err(format!("2x2 objects {:?} and {:?} closer than {gap}", (x, y), (ox, oy)));
            }
        }
    }
    // capture points carry their POINT_STOCK group(s) next to them
    for p in &g.points {
        let (x, y) = m.xy(p.cell);
        let near = |c: i32, r: i32| {
            let (cx, cy) = m.xy(c);
            (cx - x - 1).abs() <= r && (cy - y - 1).abs() <= r
        };
        let ok = match p.kind {
            PointKind::Mine => g.golds.iter().any(|d| near(d.cell, 8)),
            PointKind::Quarry => g.stones.iter().any(|d| near(d.cell, 8)),
            PointKind::Sawmill => g.trees.iter().any(|t| near(t.cell, 9)),
            PointKind::Ranch => g.meadows.iter().any(|q| near(m.cell_of(*q), 8)),
            PointKind::Outpost => true,
        };
        if !ok {
            return Err(format!("{:?} point {:?} without its resources", p.kind, (x, y)));
        }
    }
    // chests ≥ 14 cells from every base (02 §6.9)
    for ch in &g.chests {
        let (x, y) = m.xy(ch.cell);
        if g.bases.iter().any(|b| (((b.0 - x).pow(2) + (b.1 - y).pow(2)) as f64) <= (FAR_FROM_BASES as f64).powi(2)) {
            return Err(format!("chest {:?} too close to a base", (x, y)));
        }
    }
    // stair access zones (3x4 around every stair top) are noBuild (02 §6.5)
    for c in 0..n {
        let st = m.stair[c];
        if st != 0 && st % 2 == 0 {
            let (hx, hy) = m.xy(c as i32);
            for yy in hy - 1..=hy + 2 {
                for xx in hx - 1..=hx + 1 {
                    if super::land::in_margin(xx, yy) && m.no_build[m.idx(xx, yy) as usize] == 0 {
                        return Err(format!("stair ({hx},{hy}) access cell ({xx},{yy}) buildable"));
                    }
                }
            }
        }
    }
    // mineral sites (W4): the central plateau holds 3 gold groups + a stone group; 16 sites in all hold gold and
    // stone on elevated ground; every plateau carries trees
    let stone_on_first = g.stones.iter().filter(|d| m.plateau[d.cell as usize] == 1 && m.level[d.cell as usize] > 0).count();
    if stone_on_first < 1 {
        return Err("no stone on the central plateau".into());
    }
    let high_stone = g.stones.iter().filter(|d| m.level[d.cell as usize] > 0).count();
    if high_stone < 16 {
        return Err(format!("only {high_stone} stone nodes on elevated ground"));
    }
    let want_sites = g.plateaus.len().min(16) as u8;
    for id in 1..=want_sites {
        let on = |c: i32| m.plateau[c as usize] == id && m.level[c as usize] > 0;
        if !g.golds.iter().any(|d| on(d.cell)) || !g.stones.iter().any(|d| on(d.cell)) {
            return Err(format!("plateau {id} (of the 16 nearest the centre) lacks its gold + stone site"));
        }
    }
    for id in 1..=g.plateaus.len() as u8 {
        let t = g.trees.iter().filter(|t| m.plateau[t.cell as usize] == id && m.level[t.cell as usize] > 0).count();
        if t < 3 {
            return Err(format!("plateau {id} ({:?}) has only {t} trees", g.plateaus[id as usize - 1]));
        }
    }
    // ---- counts (economy ≈ JS) ----
    if !(1200..=2800).contains(&g.trees.len()) {
        return Err(format!("{} trees", g.trees.len()));
    }
    if !(120..=320).contains(&g.golds.len()) || !(80..=220).contains(&g.stones.len()) {
        return Err(format!("{} golds / {} stones", g.golds.len(), g.stones.len()));
    }
    if g.points.len() < 18 || g.bigs.len() < 10 || g.chests.len() < 60 || g.meadows.len() < 30 {
        return Err(format!(
            "points {} bigs {} chests {} meadows {}",
            g.points.len(),
            g.bigs.len(),
            g.chests.len(),
            g.meadows.len()
        ));
    }
    if !(18..=40).contains(&g.plateaus.len()) {
        return Err(format!("{} plateaus", g.plateaus.len()));
    }
    // ---- 11. unreachable land pockets are noBuild ----
    let empty = {
        let mut mm = m.clone();
        mm.block.iter_mut().for_each(|v| *v = 0);
        compute_field(&mm, &[goals[0]], 0, false)
    };
    for c in 0..n {
        if m.land[c] == 1 && m.cliff[c] == 0 && empty[c] == FIELD_INF && m.no_build[c] == 0 {
            return Err(format!("unreachable pocket cell {:?} buildable", m.xy(c as i32)));
        }
        // solid props / points / bigs never seal off walkable ground (resources may: they can be cut)
        if m.static_block[c] == 0 && empty[c] != FIELD_INF && walk[c] == FIELD_INF {
            return Err(format!("cell {:?} sealed off by static props", m.xy(c as i32)));
        }
    }
    // ---- aesthetics: rectilinear tiers (no one-cell spurs on the coast / plateau outlines) ----
    let mut land = Mask::new(W, H);
    for c in 0..n {
        land.v[c] = (m.land[c] == 1) as u8;
    }
    let spurs = land.v.iter().zip(&land.open(3).v).filter(|(a, b)| **a != 0 && **b == 0).count();
    if spurs > 40 {
        return Err(format!("{spurs} coast spur cells"));
    }
    let mut high = Mask::new(W, H);
    for c in 0..n {
        // raised ground incl. terrace stairs (inside their plateau); main stairs stick out beside the cliff
        let st = m.stair[c];
        let terrace_stair = (st % 2 == 0 && m.level[c] == 2) || (st % 2 == 1 && m.level[c] >= 1);
        high.v[c] = (m.level[c] >= 1 && (st == 0 || terrace_stair) && m.land[c] == 1) as u8;
    }
    let opened = high.open(3);
    let spur_cells: Vec<usize> = (0..n).filter(|c| high.v[*c] != 0 && opened.v[*c] == 0).collect();
    if !spur_cells.is_empty() {
        return Err(format!("{} plateau spur cells, first {:?}", spur_cells.len(), m.xy(spur_cells[0] as i32)));
    }
    // ---- render layers ----
    if m.surface.len() != n || m.depth.len() != n {
        return Err("render layers".into());
    }
    for c in 0..n {
        let s = m.surface[c];
        if (m.land[c] == 0 && m.cliff[c] == 0) != (s == Surface::Water) {
            return Err(format!("surface {s:?} on land {} at {:?}", m.land[c], m.xy(c as i32)));
        }
        if m.land[c] == 0 && m.cliff[c] == 0 && m.depth[c] == 0 {
            return Err("water without depth".into());
        }
    }
    Ok(())
}
