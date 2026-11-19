//! Top-down preview of a generated map (owner S1): surfaces by tier, cliffs, stairs, roads, water depth, trees,
//! resources, capture points, chests and decorations as small glyphs.
//!
//! cargo run -p ts-sim --release --example mapgen_png -- --seed 42 --out map.png [--scale 4] [--crop x,y,w,h]
//! cargo run -p ts-sim --release --example mapgen_png -- --seeds 1,2,3 --out dir/  (one PNG per seed + timings)
//!
//! Minimal PNG writer: RGB, zlib "stored" (uncompressed) deflate blocks, CRC-32 and Adler-32 — no dependencies.

use std::time::Instant;
use ts_sim::defs::{NodeKind, PointKind, PropKind};
use ts_sim::map::{DecorKind, GeneratedWorld, Surface};
use ts_sim::mapgen::{generate_report};
use ts_sim::world::MatchSetup;

struct Img {
    w: usize,
    h: usize,
    px: Vec<[u8; 3]>,
}

impl Img {
    fn new(w: usize, h: usize) -> Img {
        Img { w, h, px: vec![[0, 0, 0]; w * h] }
    }
    fn put(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }
    fn blend(&mut self, x: i64, y: i64, c: [u8; 3], a: f32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let p = &mut self.px[y as usize * self.w + x as usize];
            for i in 0..3 {
                p[i] = (p[i] as f32 * (1.0 - a) + c[i] as f32 * a) as u8;
            }
        }
    }
    fn rect(&mut self, x: i64, y: i64, w: i64, h: i64, c: [u8; 3]) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.put(xx, yy, c);
            }
        }
    }
    fn disc(&mut self, cx: f32, cy: f32, r: f32, c: [u8; 3]) {
        let (x0, x1) = ((cx - r).floor() as i64, (cx + r).ceil() as i64);
        let (y0, y1) = ((cy - r).floor() as i64, (cy + r).ceil() as i64);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    self.put(x, y, c);
                }
            }
        }
    }

    fn png(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity((self.w * 3 + 1) * self.h);
        for y in 0..self.h {
            raw.push(0);
            for x in 0..self.w {
                raw.extend_from_slice(&self.px[y * self.w + x]);
            }
        }
        let mut z = vec![0x78, 0x01];
        let mut i = 0;
        while i < raw.len() || raw.is_empty() {
            let n = (raw.len() - i).min(65535);
            let last = i + n >= raw.len();
            z.push(last as u8);
            z.extend_from_slice(&(n as u16).to_le_bytes());
            z.extend_from_slice(&(!(n as u16)).to_le_bytes());
            z.extend_from_slice(&raw[i..i + n]);
            i += n;
            if last {
                break;
            }
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &v in &raw {
            a = (a + v as u32) % 65521;
            b = (b + a) % 65521;
        }
        z.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&(self.w as u32).to_be_bytes());
        ihdr.extend_from_slice(&(self.h as u32).to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"IDAT", &z);
        chunk(&mut out, b"IEND", &[]);
        out
    }
}

fn crc32(data: &[&[u8]]) -> u32 {
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = 0xffff_ffffu32;
    for d in data {
        for &b in *d {
            c = table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
        }
    }
    c ^ 0xffff_ffff
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
}

fn hex(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn shade(c: [u8; 3], f: f32) -> [u8; 3] {
    [
        (c[0] as f32 * f).min(255.0) as u8,
        (c[1] as f32 * f).min(255.0) as u8,
        (c[2] as f32 * f).min(255.0) as u8,
    ]
}

fn surface_color(s: Surface) -> [u8; 3] {
    match s {
        Surface::Water => hex(0x47aba9),
        Surface::Sand => hex(0xd8c28e),
        Surface::GrassLow => hex(0x5fab62),
        Surface::GrassMid => hex(0x9bb94e),
        Surface::GrassTop => hex(0x85b156),
        Surface::GrassDry => hex(0x839a5e),
        Surface::ForestFloor => hex(0x569b8d),
        Surface::Dirt => hex(0x805a53),
        Surface::Path => hex(0xb08a62),
    }
}

fn render(g: &GeneratedWorld, s: usize, crop: (i32, i32, i32, i32)) -> Img {
    let m = &g.map;
    let (cx0, cy0, cw, ch) = crop;
    let mut img = Img::new(cw as usize * s, ch as usize * s);
    let sf = s as f32;
    let to_px = |x: f32, y: f32| ((x / 64.0 - cx0 as f32) * sf, (y / 64.0 - cy0 as f32) * sf);
    for cy in cy0..cy0 + ch {
        for cx in cx0..cx0 + cw {
            if !m.inside(cx, cy) {
                continue;
            }
            let c = m.idx(cx, cy) as usize;
            let mut col = surface_color(m.surface[c]);
            if m.land[c] == 0 && m.cliff[c] == 0 {
                let d = m.depth[c].min(8) as f32;
                col = shade(col, 1.12 - d * 0.035);
            } else {
                col = shade(col, 1.0 + m.level[c] as f32 * 0.07);
            }
            let (x0, y0) = (((cx - cx0) as usize * s) as i64, ((cy - cy0) as usize * s) as i64);
            img.rect(x0, y0, s as i64, s as i64, col);
            if m.cliff[c] != 0 {
                // rim (top strip, tier colour) + rock face
                let rock = if m.land[c] == 0 { hex(0x5c8f92) } else { hex(0x669d9d) };
                let rim = (s / 4).max(1) as i64;
                img.rect(x0, y0 + rim, s as i64, s as i64 - rim, rock);
                img.rect(x0, y0 + s as i64 - 1, s as i64, 1, if m.land[c] == 0 { hex(0xc6f0db) } else { hex(0x3c7662) });
            }
            if m.stair[c] != 0 {
                let st = m.stair[c];
                let ramp = hex(0xc9b87a);
                for k in 0..s as i64 {
                    // diagonal hatch rising toward the plateau
                    let kx = if st <= 2 { k } else { s as i64 - 1 - k };
                    img.put(x0 + kx, y0 + (s as i64 - 1 - k), ramp);
                    img.put(x0 + kx, y0 + (s as i64 - 1 - k).max(0) - 1, ramp);
                }
            }
            // level outlines (navy) on the edges between different levels / land-water
            let edge = hex(0x161c2e);
            for (dx, dy) in [(1, 0), (0, 1)] {
                let (nx, ny) = (cx + dx, cy + dy);
                if !m.inside(nx, ny) {
                    continue;
                }
                let k = m.idx(nx, ny) as usize;
                let a = (m.land[c] != 0 || m.cliff[c] != 0, m.level[c]);
                let b = (m.land[k] != 0 || m.cliff[k] != 0, m.level[k]);
                if a != b && m.stair[c] == 0 && m.stair[k] == 0 && m.cliff[c] == 0 && m.cliff[k] == 0 {
                    if dx == 1 {
                        img.rect(x0 + s as i64 - 1, y0, 1, s as i64, edge);
                    } else {
                        img.rect(x0, y0 + s as i64 - 1, s as i64, 1, edge);
                    }
                }
            }
        }
    }
    // shore foam (mint) on water cells touching land
    for cy in cy0..cy0 + ch {
        for cx in cx0..cx0 + cw {
            if !m.inside(cx, cy) {
                continue;
            }
            let c = m.idx(cx, cy) as usize;
            if m.land[c] == 0 && m.cliff[c] == 0 && m.depth[c] == 1 {
                let (x0, y0) = (((cx - cx0) as usize * s) as i64, ((cy - cy0) as usize * s) as i64);
                for k in 0..s as i64 {
                    for j in 0..s as i64 {
                        if (k + j) % 3 == 0 {
                            img.blend(x0 + k, y0 + j, hex(0xc6f0db), 0.35);
                        }
                    }
                }
            }
        }
    }
    let r = sf * 0.42;
    // decorations
    for d in &m.decor {
        let (x, y) = to_px(d.x, d.y);
        match d.kind {
            DecorKind::Bush => img.disc(x, y - sf * 0.2, r * 0.8, hex(0x3f8f3a)),
            DecorKind::Rock => img.disc(x, y - sf * 0.2, r * 0.6, hex(0x9aa6ad)),
            DecorKind::WaterRock => img.disc(x, y, r * 0.6, hex(0x7f8f96)),
            DecorKind::Duck => img.disc(x, y, r * 0.6, hex(0xffd84a)),
            DecorKind::Cloud => {}
            DecorKind::Tree => img.disc(x, y - sf * 0.35, r * 1.1, hex(0x2f6f4a)),
            DecorKind::Prop(p) => {
                let col = match p {
                    PropKind::Boat | PropKind::Dock | PropKind::Shipwreck => hex(0x7a4a2a),
                    PropKind::Reeds | PropKind::Lilypads => hex(0x2e8a4e),
                    PropKind::Flowers | PropKind::FlowersRed => hex(0xe0506a),
                    PropKind::FlowersBlue => hex(0x5a7ae0),
                    PropKind::FlowersWhite | PropKind::Clover => hex(0xf0f0e0),
                    PropKind::Pumpkins => hex(0xf08a20),
                    PropKind::Grass | PropKind::TallGrass | PropKind::Ferns => hex(0x4a8a3a),
                    PropKind::Pebbles | PropKind::Puddle | PropKind::Bones => hex(0xb0b8bc),
                    PropKind::Mushrooms | PropKind::Sticks | PropKind::FallenLog | PropKind::Stump => hex(0x8a6a4a),
                    // ruins / stones: pale stone
                    PropKind::Statue
                    | PropKind::Column
                    | PropKind::RuinArch
                    | PropKind::RuinWall
                    | PropKind::Menhir
                    | PropKind::StoneCircle
                    | PropKind::Boulder
                    | PropKind::Shrine
                    | PropKind::Obelisk
                    | PropKind::Grave => hex(0xd8d4c8),
                    // farm / village wood
                    PropKind::Windmill
                    | PropKind::Scarecrow
                    | PropKind::Hay
                    | PropKind::Fence
                    | PropKind::Well
                    | PropKind::Beehive
                    | PropKind::Signpost => hex(0xe0b060),
                    // camp / wreck
                    PropKind::Tent | PropKind::Campfire => hex(0xff5030),
                    PropKind::Barrel
                    | PropKind::Crate
                    | PropKind::Logs
                    | PropKind::Cart
                    | PropKind::WagonWreck
                    | PropKind::Deadtree => hex(0x9a6a3a),
                    _ => hex(0xc040c0),
                };
                if d.solid {
                    img.rect((x - r * 0.7) as i64, (y - sf * 0.55 - r * 0.7) as i64, (r * 1.4) as i64 + 1, (r * 1.4) as i64 + 1, col);
                } else {
                    img.disc(x, y - sf * 0.2, r * 0.45, col);
                }
            }
        }
    }
    // resources
    for t in &g.trees {
        let p = m.center(t.cell);
        let (x, y) = to_px(p.x, p.y);
        let col = match t.variant {
            0 => hex(0x1f5a4a),
            1 => hex(0x2f8a3a),
            2 => hex(0xa6c43a),
            _ => hex(0xe08a2a),
        };
        img.disc(x, y, r * 1.45, shade(col, 0.7));
        img.disc(x - sf * 0.1, y - sf * 0.12, r * 1.05, col);
    }
    for d in &g.golds {
        let p = m.center(d.cell);
        let (x, y) = to_px(p.x, p.y);
        img.disc(x, y, r * 0.9, hex(0xf2d24b));
    }
    for d in &g.stones {
        let p = m.center(d.cell);
        let (x, y) = to_px(p.x, p.y);
        img.disc(x, y, r * 0.9, hex(0xdde3e6));
    }
    for (k, d) in &g.bigs {
        let p = m.center(d.cell);
        let (x, y) = to_px(p.x, p.y);
        let col = if *k == NodeKind::Gold { hex(0xf2d24b) } else { hex(0xc3ccd1) };
        img.rect((x - sf * 0.5) as i64, (y - sf * 0.5) as i64, (sf * 2.0) as i64, (sf * 2.0) as i64, hex(0x1b1f33));
        img.rect((x - sf * 0.5) as i64 + 1, (y - sf * 0.5) as i64 + 1, (sf * 2.0) as i64 - 2, (sf * 2.0) as i64 - 2, col);
    }
    for p in &g.points {
        let q = m.center(p.cell);
        let (x, y) = to_px(q.x, q.y);
        let col = match p.kind {
            PointKind::Mine => hex(0xe0b020),
            PointKind::Quarry => hex(0x9aa6ad),
            PointKind::Sawmill => hex(0x8a5a2a),
            PointKind::Ranch => hex(0xf0f0f0),
            PointKind::Outpost => hex(0xd03030),
        };
        img.rect((x - sf * 0.5) as i64, (y - sf * 0.5) as i64, (sf * 2.0) as i64, (sf * 2.0) as i64, hex(0x000000));
        img.rect((x - sf * 0.5) as i64 + 1, (y - sf * 0.5) as i64 + 1, (sf * 2.0) as i64 - 2, (sf * 2.0) as i64 - 2, col);
    }
    for ch in &g.chests {
        let q = m.center(ch.cell);
        let (x, y) = to_px(q.x, q.y);
        img.rect((x - r * 0.6) as i64, (y - r * 0.5) as i64, (r * 1.2) as i64 + 1, (r) as i64 + 1, hex(0x6b3a1a));
    }
    for mw in &g.meadows {
        let (x, y) = to_px(mw.x, mw.y);
        for (dx, dy) in [(-0.6, 0.0), (0.5, -0.4), (0.3, 0.6)] {
            img.disc(x + dx * sf, y + dy * sf, r * 0.55, hex(0xffffff));
        }
    }
    // castles
    for &(bx, by) in &g.bases {
        let (x, y) = to_px(((bx - 2) * 64) as f32, ((by - 1) * 64) as f32);
        img.rect(x as i64, y as i64, (5.0 * sf) as i64, (2.0 * sf) as i64, hex(0x2a3a8a));
        img.rect(x as i64 + 1, y as i64 + 1, (5.0 * sf) as i64 - 2, (2.0 * sf) as i64 - 2, hex(0xe8e0c8));
    }
    img
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let seeds: Vec<u32> = if let Some(s) = get("--seeds") {
        s.split(',').filter_map(|v| v.trim().parse().ok()).collect()
    } else {
        vec![get("--seed").and_then(|v| v.parse().ok()).unwrap_or(42)]
    };
    let out = get("--out").unwrap_or_else(|| "map.png".into());
    let scale: usize = get("--scale").and_then(|v| v.parse().ok()).unwrap_or(4);
    let crop = get("--crop").map(|v| {
        let p: Vec<i32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (p[0], p[1], p[2], p[3])
    });
    let setup = MatchSetup::default();
    if args.iter().any(|a| a == "--why") {
        for &seed in &seeds {
            for k in 0..12 {
                match ts_sim::mapgen::attempt(seed, k) {
                    Ok(_) => {
                        println!("seed {seed} attempt {k}: ok");
                        break;
                    }
                    Err(e) => println!("seed {seed} attempt {k}: {e}"),
                }
            }
        }
        return;
    }
    if args.iter().any(|a| a == "--profile") {
        for &seed in &seeds {
            let mut t = Instant::now();
            let mut line = format!("seed {seed}:");
            ts_sim::mapgen::profile(seed, 0, &mut |stage| {
                line += &format!(" {stage} {:.1}", t.elapsed().as_secs_f64() * 1000.0);
                t = Instant::now();
            });
            println!("{line}");
        }
        return;
    }
    if let Some(n) = get("--check").and_then(|v| v.parse::<u32>().ok()) {
        // invariants + timing over n seeds (the test seeds), no images
        let (mut worst, mut total, mut bad) = (0.0f64, 0.0f64, 0);
        for k in 0..n {
            let seed = k.wrapping_mul(2_654_435_761).wrapping_add(17);
            let t = Instant::now();
            let (g, attempts, sound) = generate_report(seed, &setup);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            worst = worst.max(ms);
            total += ms;
            let res = ts_sim::mapgen::check::check_invariants(&g);
            if res.is_err() || !sound {
                bad += 1;
            }
            println!("seed {seed}: {ms:.0} ms, attempts {attempts}, sound {sound}, {:?}", res);
        }
        println!("{n} seeds: {bad} bad, avg {:.0} ms, worst {worst:.0} ms", total / n.max(1) as f64);
        return;
    }
    let forced = get("--attempt").and_then(|v| v.parse::<u32>().ok());
    for &seed in &seeds {
        let t = Instant::now();
        let (g, attempts, checked) = match forced {
            Some(k) => match ts_sim::mapgen::attempt_unchecked(seed, k) {
                Some(g) => (g, k + 1, false),
                None => {
                    println!("seed {seed} attempt {k}: shape failed");
                    continue;
                }
            },
            None => generate_report(seed, &setup),
        };
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let m = &g.map;
        if let Some(v) = get("--dump") {
            // --dump x,y,w,h : per-cell "level/plateau/stair/cliff/land" grid
            let p: Vec<i32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if p.len() == 4 {
                let b0 = m.idx(g.bases[0].0, g.bases[0].1 + 2);
                let f = ts_sim::path::compute_field(m, &[b0], 0, false);
                for y in p[1]..p[1] + p[3] {
                    let row: Vec<String> = (p[0]..p[0] + p[2])
                        .map(|x| {
                            let c = m.idx(x, y) as usize;
                            let r = if m.static_block[c] != 0 {
                                'B'
                            } else if f[c] < u32::MAX {
                                '*'
                            } else {
                                ' '
                            };
                            format!("{}{:>2}{}{}{}{r}", m.level[c], m.plateau[c], m.stair[c], if m.cliff[c] != 0 { 'C' } else { '.' }, m.land[c])
                        })
                        .collect();
                    println!("{y:>3}: {}", row.join(" "));
                }
            }
        }
        let land = m.land.iter().filter(|v| **v == 1).count();
        let l1 = (0..m.len()).filter(|c| m.land[*c] == 1 && m.level[*c] == 1).count();
        let l2 = (0..m.len()).filter(|c| m.land[*c] == 1 && m.level[*c] == 2).count();
        // road cells touching another road cell only by a corner (should be 0: tracks are 4-connected)
        let path = |x: i32, y: i32| m.inside(x, y) && m.surface[m.idx(x, y) as usize] == Surface::Path;
        let mut corner_only = 0;
        for y in 0..m.h {
            for x in 0..m.w {
                if !path(x, y) {
                    continue;
                }
                for (dx, dy) in [(1, 1), (-1, 1)] {
                    if path(x + dx, y + dy) && !path(x + dx, y) && !path(x, y + dy) {
                        corner_only += 1;
                    }
                }
            }
        }
        if corner_only > 0 {
            println!("seed {seed}: {corner_only} corner-only road contacts");
        }
        let props = m.decor.iter().filter(|d| matches!(d.kind, DecorKind::Prop(_))).count();
        let solid = m.decor.iter().filter(|d| d.solid).count();
        let bushes = m.decor.iter().filter(|d| d.kind == DecorKind::Bush).count();
        let rocks = m.decor.iter().filter(|d| d.kind == DecorKind::Rock).count();
        let wr = m.decor.iter().filter(|d| d.kind == DecorKind::WaterRock).count();
        println!(
            "seed {seed}: {ms:.0} ms, attempts {attempts}, sound {checked}, hash {:016x} | land {land} L1 {l1} L2 {l2} plateaus {} | trees {} golds {} stones {} bigs {} points {} chests {} meadows {} | props {props} (solid {solid}) bushes {bushes} rocks {rocks} waterrocks {wr}",
            m.hash(),
            g.plateaus.len(),
            g.trees.len(),
            g.golds.len(),
            g.stones.len(),
            g.bigs.len(),
            g.points.len(),
            g.chests.len(),
            g.meadows.len(),
        );
        let crop = crop.unwrap_or((0, 0, m.w, m.h));
        let img = render(&g, scale, crop);
        let path = if seeds.len() > 1 || out.ends_with('/') || out.ends_with('\\') {
            let dir = out.trim_end_matches(['/', '\\']);
            let _ = std::fs::create_dir_all(dir);
            format!("{dir}/map_{seed}.png")
        } else {
            out.clone()
        };
        if let Err(e) = std::fs::write(&path, img.png()) {
            eprintln!("cannot write {path}: {e}");
        }
    }
}
