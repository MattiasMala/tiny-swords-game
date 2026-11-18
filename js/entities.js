'use strict';
// Unità, edifici, risorse, proiettili, effetti: logica e disegno.

let NEXT_ID = 1;
const JOB_TOOL = { wood: 'Axe', gold: 'Pickaxe', stone: 'Pickaxe', meat: 'Knife' };
// secondi di lavoro per un carico. La pecora è a due passi dalla fattoria, alberi e giacimenti no: il lavoro più
// lungo sul cibo porta la resa di un pedone al livello delle altre risorse (circa 38 al minuto)
const JOB_TIME = { wood: 3.0, gold: 3.4, stone: 3.4, meat: 20 };
const NODE_USERS = { tree: 2, gold: 3, stone: 3, sheep: 1 };         // pedoni che possono lavorare insieme sullo stesso nodo
const BIG_USERS = 8;                                                 // ...e su un grande giacimento (2x2 celle)
const NODE_RES = { tree: 'wood', gold: 'gold', stone: 'stone', sheep: 'meat' };
const LOAD_NAME = { wood: 'Wood', gold: 'Gold', meat: 'Meat', stone: 'Stone' };
const CARRY = 10, SHEEP_MEAT = 15, TREE_WOOD = 40, GOLD_MAX = 240, STONE_MAX = 300;
const FARM_BREED = 9, FARM_SHEEP = 5; // una fattoria fa nascere una pecora ogni FARM_BREED secondi, fino a FARM_SHEEP attorno a sé
const TREASURE = 50;            // risorse in un forziere trovato sulla mappa
const BUILDING_ARMOR = 0.4;     // gli edifici subiscono solo una frazione del danno
const REPAIR_RATE = 0.035;      // frazione di vita al secondo riparata da un pedone
const ALERT_EVERY = 12;         // secondi fra due avvisi "sotto attacco" allo stesso regno
const ALARM_TIME = 10;          // secondi per cui, dopo un colpo subito in casa, le truppe ferme accorrono
const HOME_RADIUS = 1500;       // px dal castello entro cui un colpo a un soldato vale come attacco al regno
const RAID_RADIUS = 900;        // px entro cui un assalto prosegue da solo sul prossimo nemico
const SHELTER_CAP = 5;          // pedoni che una casa può nascondere (il castello non ha limite)
const CALM_TIME = 4;            // secondi senza truppe nemiche in vista prima che i pedoni tornino al lavoro
const HEALER_FOCUS = 0.6;       // un Monaco nemico a portata conta come se fosse a questa frazione della sua distanza
const KITE_RANGE = 110;         // px: un tiratore con un nemico da mischia più vicino di così arretra mentre ricarica
const MONK_SPACE = 150;         // px: un Monaco con un soldato nemico più vicino di così si scansa
const ESCORT_GAP = 200;         // px: distanza a cui un Monaco al seguito sta dai soldati che accompagna
const LINE_GAP = 44;            // px fra due unità in formazione
const POINT_INCOME = 0.5;       // risorse al secondo che un punto conquistato rende a chi lo tiene
const CAPTURE_TIME = 8;         // secondi da passare soli attorno a un punto per conquistarlo
const CAPTURE_RADIUS = 300;     // px attorno al punto entro cui le truppe contano
const POINT_SIGHT = 7;          // caselle viste da un punto conquistato
const TRAIN_BONUS = 0.2;        // ogni fabbrica dello stesso tipo oltre la prima accelera di tanto l'addestramento (fino al doppio)
const ACT_LABEL = ['Inattivo', 'In marcia', 'In combattimento', 'Taglia legna', 'Estrae oro', 'Macella pecore',
  'Costruisce', 'Ripara', 'Porta il carico', 'Cura i feriti', 'Di guardia', 'Estrae pietra', 'Al riparo', 'In fuga', 'In esplorazione'];

// ---------------------------------------------------------------- helper

function rectDist(px, py, b) {
  return Math.hypot(Math.max(b.x0 - px, 0, px - b.x1), Math.max(b.y0 - py, 0, py - b.y1));
}
function distTo(u, t) { return t.isBuilding ? rectDist(u.x, u.y, t) : Math.hypot(t.x - u.x, t.y - u.y); }
// Sempre true quando ospito una partita in rete: effetti e suoni servono anche agli altri giocatori.
function onScreen(x, y, m = 80) {
  if (Net.hosting) return true;
  return x > G.cam.x - m && y > G.cam.y - m && x < G.cam.x + G.viewW + m && y < G.cam.y + G.viewH + m;
}
function dir5(dx, dy) {
  const a = Math.atan2(-dy, Math.abs(dx));
  return a > 1.18 ? 'Up' : a > 0.39 ? 'UpRight' : a > -0.39 ? 'Right' : a > -1.18 ? 'DownRight' : 'Down';
}
function setAnim(u, a) { if (u.anim !== a) { u.anim = a; u.animT = 0; } }
// Ogni entità con un id è rintracciabile: serve agli ordini (anche arrivati dalla rete) e alla selezione.
function track(e) { G.index.set(e.id, e); return e; }

// Eventi di presentazione (effetti, suoni, avvisi): mostrati in locale e, quando ospito una partita
// in rete, accodati per i client. Il destinatario è l'indice di una fazione; -1 = tutti.
function emit(ev) {
  showEvent(ev);
  if (Net.hosting) Net.events.push(ev);
}
function showEvent(ev) {
  const mine = i => i < 0 || (G.player !== null && G.player.idx === i);
  switch (ev[0]) {
    case 'fx': {
      const [, name, x, y, scale, dy, followId, color] = ev;
      G.fx.push({ sh: FX[name](color), x, y, t: 0, scale, follow: followId ? G.index.get(followId) || null : null, sy: y + dy, draw: drawFx });
      break;
    }
    case 'prop': G.decor.push(makeProp(ev[1], ev[2], ev[3])); break;
    case 'snd': Sfx.hear(ev[1], ev[2], ev[3]); break;
    case 'play': if (mine(ev[1])) Sfx.play(ev[2]); break;
    case 'float': if (mine(ev[1])) G.floats.push({ x: ev[2], y: ev[3], str: ev[4], icon: RES_ICON[ev[5]], t: 0 }); break;
    case 'toast': if (mine(ev[1])) G.toast(ev[2]); break;
    case 'alert': if (G.player !== null && G.player.idx === ev[1]) Ctl.alert(ev[2], ev[3]); break;
    case 'say': {
      const faction = G.factions[ev[3]] || G.player; // senza colore indicato: quello di chi legge
      if (faction && mine(ev[1]) && G.banners.length < 4) G.banners.push({ text: ev[2], faction, t: 0, dur: ev[4] });
      break;
    }
  }
}

// name = chiave di FX; opt: scale, dy (ordine di disegno), follow (unità da seguire), color.
function addFx(name, x, y, opt = {}) {
  const scale = +(opt.scale || 1).toFixed(2);
  emit(['fx', name, Math.round(x), Math.round(y), scale, opt.dy || 0, opt.follow ? opt.follow.id : 0, opt.color]);
}
function addFloat(f, x, y, str, kind) { emit(['float', f.idx, Math.round(x), Math.round(y), str, kind]); }

// Oggetto di scena (sprite fermo, appoggiato a terra nel punto x, y).
function makeProp(name, x, y) { return { name, x, y, sy: y, draw: drawProp }; }

// Risorse a terra: le prende la prima unità che ci passa sopra. chest = forziere della mappa
// (non scade); altrimenti bottino di un edificio abbattuto, che dopo `life` secondi sparisce.
function dropPickup(kind, x, y, amount, life, chest = false) {
  const a = rand(0, Math.PI * 2), r = chest ? 0 : rand(24, 54);
  G.pickups.push({
    id: NEXT_ID++, kind, amount, chest, x, y, sx: x, sy0: y, tx: x + Math.cos(a) * r, ty: y + Math.sin(a) * r * 0.6 + (chest ? 0 : 10),
    t: chest ? 1 : 0, life, hop: 0, sy: y, draw: drawPickup,
  });
}

// ---------------------------------------------------------------- risorse

function makeTree(c, v) {
  return track({
    id: NEXT_ID++, kind: 'tree', c, cells: [c], v, x: World.cx(c), y: World.cy(c) + 22, sy: World.cy(c) + 22,
    wood: TREE_WOOD, regrow: 0, users: 0, phase: Math.random() * 8, draw: drawTree,
  });
}
// big = grande giacimento: 2x2 celle a partire da c, molte più risorse e più posti di lavoro.
function makeGold(c, amount, big = false) {
  const x = World.cx(c) + (big ? TILE / 2 : 0), y = World.cy(c) + (big ? TILE / 2 : 0);
  return track({
    id: NEXT_ID++, kind: 'gold', c, cells: big ? World.quad(c) : [c], x, y, sy: y + (big ? 56 : 18),
    amount, max: amount, big, users: 0, shine: rand(-6, 0), regen: 0, draw: drawGold,
  });
}
function makeStone(c, amount, big = false) {
  const x = World.cx(c) + (big ? TILE / 2 : 0), y = World.cy(c) + (big ? TILE / 2 : 6);
  return track({
    id: NEXT_ID++, kind: 'stone', c, cells: big ? World.quad(c) : [c], x, y, sy: y + (big ? 56 : 20),
    amount, max: amount, big, users: 0, regen: 0, draw: drawStone,
  });
}
// home = punto attorno a cui pascola (un prato o il recinto di una fattoria).
function makeSheep(home) {
  const s = track({
    id: NEXT_ID++, kind: 'sheep', home, x: home.x + rand(-90, 90), y: home.y + rand(-60, 60), sy: 0,
    state: 'idle', t: rand(0.5, 3), animT: 0, flip: Math.random() < 0.5, tx: 0, ty: 0, users: 0, dead: false, draw: drawSheep,
  });
  if (!World.passable(World.cellAt(s.x, s.y))) { const p = World.nearestPassable(home.x, home.y); s.x = p.x; s.y = p.y; }
  G.sheep.push(s);
  return s;
}
function nodeValid(n) { return n.kind === 'tree' ? n.wood > 0 : n.kind === 'sheep' ? !n.dead : n.amount > 0; }
function nodeLeft(n) { return n.kind === 'tree' ? n.wood : n.kind === 'sheep' ? SHEEP_MEAT : n.amount; }
function nodeRoom(n) { return n.users < (n.big ? BIG_USERS : NODE_USERS[n.kind]); } // c'è posto per un altro pedone
function nodesOf(res) { return res === 'wood' ? G.trees : res === 'gold' ? G.golds : res === 'stone' ? G.stones : G.sheep; }

function chopTree(t, amount) {
  t.wood -= amount;
  if (t.wood <= 0) { t.wood = 0; t.regrow = 110; addFx('dust2', t.x, t.y); }
}
function killSheep(s) {
  s.dead = true;
  G.index.delete(s.id);
  addFx('dust1', s.x, s.y);
}

// Luccichio dell'oro: solo estetico, ogni giocatore ha il suo.
function twinkle(dt) {
  for (const g of G.golds) {
    g.shine += dt;
    if (g.shine > 0.6) g.shine = -rand(3, 7);
  }
}

function updateResources(dt) {
  for (const t of G.trees) {
    if (t.wood <= 0 && (t.regrow -= dt) <= 0) { t.wood = TREE_WOOD; if (onScreen(t.x, t.y)) addFx('dust1', t.x, t.y); }
  }
  twinkle(dt);
  // le vene si ricaricano piano: una partita lunga non resta mai senza oro o pietra
  for (const g of G.golds) if (g.amount < g.max && (g.regen += dt) > 25) { g.regen = 0; g.amount = Math.min(g.max, g.amount + 10); }
  for (const s of G.stones) if (s.amount < s.max && (s.regen += dt) > 25) { s.regen = 0; s.amount = Math.min(s.max, s.amount + 10); }
  for (const s of G.sheep) {
    s.animT += dt; s.t -= dt;
    if (s.state === 'move') {
      const dx = s.tx - s.x, dy = s.ty - s.y, d = Math.hypot(dx, dy);
      if (d < 4 || s.t <= 0) { s.state = 'idle'; s.t = rand(1, 3); s.animT = 0; }
      else { World.tryMove(s, dx / d * 38 * dt, dy / d * 38 * dt); s.flip = dx < 0; }
    } else if (s.t <= 0) {
      if (s.state === 'idle' && Math.random() < 0.45) { s.state = 'grass'; s.t = 1.2; s.animT = 0; }
      else {
        s.state = 'move'; s.t = 4; s.animT = 0;
        s.tx = s.home.x + rand(-120, 120); s.ty = s.home.y + rand(-80, 80);
      }
    }
  }
  G.sheep = G.sheep.filter(s => !s.dead);
  if ((G.sheepT -= dt) <= 0) { // i prati selvatici si ripopolano piano
    G.sheepT = 10;
    const m = pick(World.meadows);
    if (G.sheep.filter(s => s.home === m).length < 3) {
      const s = makeSheep(m);
      if (onScreen(s.x, s.y)) addFx('dust1', s.x, s.y);
    }
  }
}

// ---------------------------------------------------------------- punti da conquistare

// c = prima cella in alto a sinistra delle 2x2 che occupa. owner = regno che lo tiene, by = regno che lo sta
// conquistando, t = da quanti secondi.
function makePoint(c, kind) {
  const cx = c % World.W, cy = Math.floor(c / World.W), y = (cy + 2) * TILE - 4;
  return track({
    id: NEXT_ID++, isPoint: true, kind, def: POINT_DEF[kind], c, cells: World.quad(c),
    x: (cx + 1) * TILE, y, sy: y, x0: cx * TILE, y0: cy * TILE, x1: (cx + 2) * TILE, y1: (cy + 2) * TILE, gx: cx + 1, gy: cy + 1,
    owner: null, by: null, t: 0, draw: drawPoint,
  });
}
function capturePoint(p, f) {
  const old = p.owner;
  p.owner = f; p.by = null; p.t = 0;
  addFx('dust2', p.x, p.y, { scale: 1.6 });
  if (f.human) { emit(['play', f.idx, 'build']); G.announce('Hai conquistato: ' + p.def.label, f, 2.6, f); }
  if (old && old.human) { emit(['alert', old.idx, Math.round(p.x), Math.round(p.y)]); G.announce('Hai perso: ' + p.def.label, f, 3, old); }
}
// Un punto passa al regno che per CAPTURE_TIME secondi è il solo ad averci attorno delle truppe (i Pedoni non
// contano). Finché è di qualcuno gli rende la sua risorsa, senza bisogno di Pedoni.
function updatePoints(dt) {
  const tick = (G.pointT -= dt) <= 0;
  if (tick) G.pointT = 0.5;
  for (const p of G.points) {
    const o = p.owner;
    if (o && p.def.res) { o.res[p.def.res] += POINT_INCOME * dt; o.gathered += POINT_INCOME * dt; }
    if (!tick) continue;
    let one = null, many = false;
    for (const u of G.units) {
      if (u.dead || u.inside || u.type === 'pawn' || Math.abs(u.x - p.x) > CAPTURE_RADIUS || Math.hypot(u.x - p.x, u.y - p.y + TILE) > CAPTURE_RADIUS) continue;
      if (one && one !== u.faction) { many = true; break; }
      one = u.faction;
    }
    if (many) continue; // conteso: tutto fermo
    if (!one || one === o) { if ((p.t = Math.max(0, p.t - 0.5)) === 0) p.by = null; continue; }
    if (p.by !== one) { p.by = one; p.t = 0; }
    if ((p.t += 0.5) >= CAPTURE_TIME) capturePoint(p, one);
  }
}

// ---------------------------------------------------------------- unità: statistiche (con le ricerche del regno)

function unitMaxHp(f, type) { return Math.round(UNIT_DEF[type].hp * (type === 'monk' ? f.mods.monkHp : 1)); }
function unitSpeed(f, type) {
  const d = UNIT_DEF[type], m = f.mods;
  return d.speed * (type === 'pawn' ? m.pawnSpeed : d.foot ? m.foot : type === 'knight' ? m.horse : 1);
}
function unitDmg(u) { return ((u.def.dmg || 0) + (u.faction.mods.atk[u.type] || 0)) * (u.def.siege ? u.faction.mods.siege : 1); }
function unitRange(u) { return u.def.range + (u.type === 'archer' ? u.faction.mods.range : u.type === 'monk' ? u.faction.mods.healRange : 0); }
function unitArmor(u) { return u.def.dmg && !u.def.siege ? u.faction.mods.armor : 0; }
// Danno di un colpo di `src` (unità o edificio) contro `t`, bonus compresi.
function damageTo(src, base, t) {
  const d = src.def;
  return base * (t.isBuilding ? d.vsBuilding || 1 : (d.bonus && d.bonus[t.type]) || 1);
}

function spawnUnit(f, type, x, y) {
  const d = UNIT_DEF[type], hp = unitMaxHp(f, type);
  const u = track({
    id: NEXT_ID++, isUnit: true, faction: f, type, def: d, x, y, sy: y, draw: drawUnit,
    hp, maxHp: hp, speed: unitSpeed(f, type),
    state: 'idle', anim: 'idle', animT: Math.random(), flip: false, act: 0, job: -1,
    cd: 0, thinkT: Math.random() * 0.4, target: null, order: { k: 'idle', x, y },
    faith: d.faith || 0, // riserva di cura dei monaci
    foe: null, lead: null, // Monaci: il soldato nemico più vicino e il soldato del proprio regno che seguono
    carry: null, tool: null, workT: 0, pauseT: 0, node: null, skip: null,
    hurtT: 0, lastHurt: -99, moving: false, dustT: 0, combo: 0, hitDone: false, aimX: x, aimY: y, blockT: 0,
    bestD: Infinity, stallT: 0, breachT: -99, dead: false, cross: false,
    inside: null,   // edificio in cui un pedone si è rifugiato: finché è lì non si vede e non può essere colpito
    brave: false,   // ha un ordine diretto del giocatore: non scappa finché attorno non torna la calma (vedi embolden)
    fearT: Math.random() * 0.4, lookT: 0, calmT: 0,
  });
  G.units.push(u);
  return u;
}

// ---------------------------------------------------------------- unità: ordini e movimento

// Un pedone "prenota" il nodo su cui lavora, così gli altri si distribuiscono.
function claim(u, n) {
  if (u.node === n) return;
  if (u.node) u.node.users--;
  u.node = n;
  if (n) n.users++;
}
// Ordini: idle {x,y = punto di guardia, escort = Monaco al seguito dei soldati} · move/amove {x,y, pace = passo
// del gruppo, escort} · attack {t} · heal {t} · explore {goal} · gather {res, first, px, py} · build/repair {b} ·
// drop {b} · shelter {b, back = ordine da riprendere, calm}.
function setOrder(u, o) {
  if (u.inside) leaveShelter(u);
  claim(u, null);
  u.order = o; u.target = null; u.skip = null; u.workT = 0; u.bestD = Infinity; u.stallT = 0; u.breachT = -99;
}
function idle(u) { setOrder(u, { k: 'idle', x: u.x, y: u.y }); }
function gatherOrder(n) { return { k: 'gather', res: NODE_RES[n.kind], first: n, px: n.x, py: n.y }; }

function moveUnit(u, tx, ty, dt) {
  const dx = tx - u.x, dy = ty - u.y, d = Math.hypot(dx, dy);
  if (d < 0.5) return;
  const step = Math.min(d, u.speed * dt);
  World.tryMove(u, dx / d * step, dy / d * step);
  if (Math.abs(dx) > 4) u.flip = dx < 0;
  u.moving = true;
}
// Regno di un'unità per i percorsi (vedi World.passable): le sue palizzate non la fermano.
function sideOf(u) { return u.faction.idx + 1; }
// Campo di distanza verso un edificio o un nodo, per quell'unità. u.cross = la strada che sta seguendo passa
// attraverso i suoi edifici (succede solo quando attorno non c'è altra via: vedi World.getField).
function fieldOf(u, e) {
  const f = World.fieldFor(e, sideOf(u), World.cellAt(u.x, u.y));
  u.cross = World.crossed;
  return f;
}
// Segue un campo di distanza: 'arrived' sulle celle di destinazione, 'blocked' se irraggiungibile.
function navTo(u, field, dt) {
  const c = World.cellAt(u.x, u.y);
  if (field[c] === 0) return 'arrived';
  const n = World.stepFrom(field, c, sideOf(u), u.cross);
  if (n < 0) return 'blocked';
  moveUnit(u, World.cx(n), World.cy(n), dt);
  return 'moving';
}
// Va verso un punto: in linea retta se libero, altrimenti col campo di distanza della cella.
// Lascia in u.pathLeft la strada ancora da fare, in px (lungo il percorso quando deve girare attorno a qualcosa).
function approach(u, tx, ty, dt) {
  const side = sideOf(u), d = Math.hypot(tx - u.x, ty - u.y);
  u.pathLeft = d;
  u.cross = false;
  if (d < 280 && World.clearLine(u.x, u.y, tx, ty, side)) { moveUnit(u, tx, ty, dt); return true; }
  const b = World.cellAt(tx, ty);
  if (b < 0) return false;
  const a = World.cellAt(u.x, u.y), f = World.fieldToCell(b, side, a);
  u.cross = World.crossed;
  if (f[a] === 0) return false;
  const n = World.stepFrom(f, a, side, u.cross);
  if (n < 0) return false;
  u.pathLeft = Math.max(d, f[a] * TILE / 10); // il campo è in decimi di cella
  moveUnit(u, World.cx(n), World.cy(n), dt);
  return true;
}
// Viaggio verso un punto. Ritorna true quando l'unità è arrivata o non può avvicinarsi oltre (meta
// irraggiungibile, oppure ferma da un po' contro altre unità attorno alla meta). L'avanzare si misura lungo
// il percorso: girare attorno a un'altura allontana in linea d'aria, ma non è restare fermi.
function travel(u, x, y, dt, near = 10) {
  const d = Math.hypot(x - u.x, y - u.y);
  if (d <= near) return true;
  const moving = approach(u, x, y, dt);
  if (u.pathLeft < u.bestD - 3) { u.bestD = u.pathLeft; u.stallT = 0; }
  else if ((u.stallT += dt) > (d < 120 ? 1.2 : 4)) return true;
  return !moving;
}

// Posto in formazione: 0 prima linea (mischia e Arieti, che devono arrivare agli edifici), 1 tiratori,
// 2 Catapulte, 3 retrovie (Monaci, Esploratori, Pedoni).
function lineOf(u) { const d = u.def; return !d.dmg ? 3 : d.splash ? 2 : d.range > 200 ? 1 : 0; }

// Assegna un ordine a un gruppo di unità della fazione. I movimenti si dispongono in formazione, rivolta verso
// dove si va: davanti chi combatte in mischia e gli Arieti, dietro i tiratori, poi le Catapulte, in fondo i
// Monaci. In marcia d'attacco il gruppo tiene il passo del più lento, così arriva insieme.
function orderUnits(f, ids, kind, x, y, targetId) {
  const units = [];
  for (const id of ids) {
    const u = G.index.get(id);
    if (u && u.isUnit && !u.dead && u.faction === f) units.push(u);
  }
  if (!units.length) return;
  if (f.human) for (const u of units) embolden(u); // un ordine dato di persona vale più della paura
  const t = targetId ? G.index.get(targetId) : null;
  if (kind === 'stop') { for (const u of units) idle(u); return; }
  if (kind === 'explore') { for (const u of units) if (u.def.scout) setOrder(u, { k: 'explore', goal: null }); return; }
  if (kind === 'move' || kind === 'amove') {
    if (!Number.isFinite(x) || !Number.isFinite(y)) return;
    const p = World.nearestPassable(x, y), n = units.length, amove = kind === 'amove', army = units.some(u => u.def.dmg);
    let mx = 0, my = 0, pace = Infinity;
    for (const u of units) { mx += u.x / n; my += u.y / n; if (u.def.dmg) pace = Math.min(pace, u.speed); }
    const far = Math.hypot(p.x - mx, p.y - my), dx = far > 1 ? (p.x - mx) / far : 0, dy = far > 1 ? (p.y - my) / far : 1;
    const per = Math.max(3, Math.ceil(Math.sqrt(n) * 1.5)), lines = [[], [], [], []];
    for (const u of units) lines[lineOf(u)].push(u);
    let back = 0;
    for (const line of lines) {
      if (!line.length) continue;
      line.sort((a, b) => (a.x * -dy + a.y * dx) - (b.x * -dy + b.y * dx)); // chi è a sinistra resta a sinistra
      line.forEach((u, i) => {
        const row = Math.floor(i / per), inRow = Math.min(per, line.length - row * per), side = (i % per - (inRow - 1) / 2) * LINE_GAP, deep = back + row * LINE_GAP;
        const q = World.nearestPassable(p.x - dy * side - dx * deep, p.y + dx * side - dy * deep);
        // i Monaci partiti con dei soldati li seguono; in marcia d'attacco tutti al passo del più lento che combatte.
        // ax, ay = la meta comune del gruppo, verso cui si cammina finché non tocca alla strada del proprio posto (vedi march)
        setOrder(u, { k: amove && u.def.dmg ? 'amove' : 'move', x: q.x, y: q.y, ax: n > 1 ? p.x : undefined, ay: p.y,
          pace: amove && army ? pace : 0, escort: amove && army && !!u.def.heal });
      });
      back += Math.ceil(line.length / per) * LINE_GAP + 36;
    }
    return;
  }
  if (!t || t.dead) return;
  for (const u of units) {
    const pawn = u.type === 'pawn';
    if (kind === 'attack' && (t.isUnit || t.isBuilding) && t.faction !== f) {
      if (u.def.dmg && (t.isBuilding || !u.def.onlyBuildings)) setOrder(u, { k: 'attack', t });
      else if (u.def.heal) setOrder(u, { k: 'idle', x: u.x, y: u.y, escort: true }); // i Monaci vanno dietro a chi attacca
    } else if (kind === 'gather' && pawn && NODE_RES[t.kind] && nodeValid(t)) {
      setOrder(u, gatherOrder(t));
    } else if (kind === 'build' && pawn && t.isBuilding && t.faction === f) {
      // chi lascia una raccolta per un cantiere la riprende a lavoro finito (resume)
      const o = u.order.k === 'shelter' ? u.order.back : u.order, resume = o.k === 'gather' ? o : o.resume || null;
      setOrder(u, { k: t.done ? 'repair' : 'build', b: t, resume });
    } else if (kind === 'drop' && pawn && t.isBuilding && t.faction === f && t.done) {
      setOrder(u, { k: 'drop', b: t });
    } else if (kind === 'heal' && u.type === 'monk' && t.isUnit && t.faction === f && t !== u) {
      setOrder(u, { k: 'heal', t });
    }
  }
}

// ---------------------------------------------------------------- unità: combattimento

function startAttack(u, tx, ty) {
  u.state = 'attack'; u.hitDone = false; u.aimX = tx; u.aimY = ty;
  u.cd = u.def.cd;
  if (Math.abs(tx - u.x) > 2) u.flip = tx < u.x;
  let a = 'attack';
  if (u.type === 'warrior') { u.combo ^= 1; a = 'attack' + (u.combo + 1); }
  else if (u.type === 'lancer') a = 'attack' + dir5(tx - u.x, ty - u.y);
  u.anim = a; u.animT = 0;
}
function startBlock(u, from) {
  u.state = 'block'; u.blockT = 0.5;
  if (from) u.flip = from.x < u.x;
  setAnim(u, u.type === 'lancer' ? 'block' + (from ? dir5(from.x - u.x, from.y - u.y) : 'Right') : 'block');
}

// true finché l'unità è impegnata in un'azione che non va interrotta.
function tickAction(u, dt) {
  if (u.state === 'attack') {
    const sh = unitSheet(u.faction.color, u.type, u.anim), fr = Math.floor(u.animT * sh.fps);
    if (!u.hitDone && fr >= u.def.hitFrame) { u.hitDone = true; doHit(u); }
    if (fr >= sh.n) { u.state = 'idle'; return false; }
    return true;
  }
  if (u.state === 'block') {
    if ((u.blockT -= dt) <= 0) { u.state = 'idle'; return false; }
    return true;
  }
  return false;
}

// Freccia tesa: owner = unità o edificio che tira; il danno vero si calcola all'impatto, sul bersaglio.
function fireArrow(owner, x, y, tx, ty, dmg) {
  const d = Math.hypot(tx - x, ty - y) || 1, sp = 640;
  G.arrows.push({
    id: NEXT_ID++, x, y, vx: (tx - x) / d * sp, vy: (ty - y) / d * sp, ang: Math.atan2(ty - y, tx - x),
    faction: owner.faction, dmg, owner, life: (d + 70) / sp,
  });
  Sfx.at('shoot', x, y);
}
// Masso di catapulta: vola ad arco fino al punto mirato e lì colpisce tutto ciò che c'è attorno.
function fireRock(owner, tx, ty, dmg) {
  const sx = owner.x + (owner.flip ? -20 : 20), sy = owner.y - 50, d = Math.hypot(tx - sx, ty - sy);
  G.arrows.push({
    id: NEXT_ID++, rock: true, x: sx, y: sy, sx, sy, tx, ty, t: 0, dur: 0.5 + d / 520, ang: 0,
    faction: owner.faction, dmg, owner,
  });
  Sfx.at('shoot', sx, sy);
}

function doHit(u) {
  const t = u.target, alive = t && !t.dead;
  if (u.type === 'archer') {
    let ax = u.aimX, ay = u.aimY;
    if (alive) { ax = t.isBuilding ? (t.x0 + t.x1) / 2 : t.x; ay = t.isBuilding ? (t.y0 + t.y1) / 2 - 20 : t.y - 28; }
    fireArrow(u, u.x + (u.flip ? -16 : 16), u.y - 34, ax, ay, unitDmg(u));
  } else if (u.type === 'catapult') {
    fireRock(u, alive && t.isUnit ? t.x : u.aimX, alive && t.isUnit ? t.y : u.aimY, unitDmg(u));
  } else if (u.type === 'monk') {
    if (alive) {
      // una cura spende dalla riserva quanta vita rende davvero: niente spreco su chi è quasi sano
      const amount = Math.min(u.def.heal * u.faction.mods.heal, t.maxHp - t.hp, u.faith);
      t.hp += amount; u.faith -= amount;
      addFx('heal', t.x, t.y, { follow: t, dy: 4, color: u.faction.color });
      Sfx.at('heal', t.x, t.y);
    }
  } else if (alive && distTo(u, t) <= unitRange(u) * 1.5 + (t.isBuilding ? 44 : 0)) {
    hurt(t, damageTo(u, unitDmg(u), t), u);
    Sfx.at(u.type === 'ram' ? 'boom' : 'hit', u.x, u.y);
  }
}

// by = unità, edificio o proiettile che infligge il danno.
function hurt(t, amount, by) {
  if (t.dead) return;
  const src = by && by.owner ? by.owner : by, tf = t.faction;
  if (src && src.isUnit && !src.dead) {
    tf.threat = src; tf.threatT = G.time;
    // regno di un giocatore colpito in casa (edifici, pedoni, o chiunque attorno al castello): allarme
    const c = tf.castle;
    if (tf.human && (t.isBuilding || t.type === 'pawn' || (c && Math.hypot(t.x - c.x, t.y - c.y) < HOME_RADIUS))) tf.alarm = { x: src.x, y: src.y, t: G.time };
  }
  if (t.isBuilding) {
    amount *= BUILDING_ARMOR;
  } else {
    if (by && by.vx !== undefined) { // freccia: l'ariete la sente appena, gli scudi rinforzati la dimezzano
      if (t.type === 'ram') amount *= 0.2;
      else if (t.def.block || t.type === 'knight') amount *= tf.mods.arrowRes;
    }
    amount = Math.max(1, amount - unitArmor(t));
    if (t.def.block && t.state !== 'attack' && Math.random() < t.def.block) {
      amount *= 0.35;
      startBlock(t, src);
      Sfx.at('block', t.x, t.y);
    }
    t.hurtT = 0.12; t.lastHurt = G.time;
    // un soldato colpito mentre è fermo o in marcia d'attacco reagisce
    const k = t.order.k;
    if (!t.target && t.def.dmg && !t.def.onlyBuildings && (k === 'idle' || k === 'amove') && src && src.isUnit && !src.dead) t.target = src;
  }
  t.hp -= amount;
  if (tf.human && G.time - tf.alertT > ALERT_EVERY) { tf.alertT = G.time; emit(['alert', tf.idx, Math.round(t.x), Math.round(t.y)]); }
  if (t.hp <= 0) { if (t.isUnit) killUnit(t, by); else destroyBuilding(t, by); }
}

function killUnit(u, by) {
  if (u.inside) leaveShelter(u);
  u.dead = true;
  claim(u, null);
  G.index.delete(u.id);
  addFx('boom1', u.x, u.y - 22, { dy: 60 });
  Sfx.at('death', u.x, u.y);
  const bf = by && by.faction;
  if (bf && bf !== u.faction) bf.kills++;
}

// Bersaglio automatico: l'unità nemica più vicina, poi gli edifici (le palizzate solo se sbarrano
// la strada, vedi breach). Fra i nemici a portata i Monaci contano come più vicini di quanto sono (chi cura
// va tolto di mezzo per primo), i Pedoni come più lontani. Chi è di guardia non si allontana troppo dal suo
// posto, a meno che stia accorrendo a un allarme (roam).
function acquireTarget(u, roam = false) {
  const f = u.faction, o = u.order, guard = o.k === 'idle' && !roam, leash = 430;
  let best = null, bd = u.def.aggro;
  if (!u.def.onlyBuildings) {
    for (const e of G.units) {
      if (e.dead || e.inside || e.faction === f) continue;
      const far = Math.hypot(e.x - u.x, e.y - u.y);
      if (far >= u.def.aggro) continue;
      const d = far * (e.type === 'pawn' ? 2.2 : e.def.heal ? HEALER_FOCUS : 1);
      if (d < bd && (!guard || Math.hypot(e.x - o.x, e.y - o.y) < leash)) { bd = d; best = e; }
    }
    if (best) return best;
    // IA: la guarnigione va a stanare chi colpisce il regno da fuori portata
    const th = f.threat, c = f.castle;
    if (!f.human && guard && th && !th.dead && G.time - f.threatT < 5 &&
      Math.hypot(th.x - c.x, th.y - c.y) < 950 && Math.hypot(u.x - c.x, u.y - c.y) < 950) return th;
  }
  bd = u.def.onlyBuildings ? 520 : Math.max(260, unitRange(u)); // gli Arieti cercano il loro edificio da lontano
  for (const b of G.buildings) {
    if (b.dead || b.faction === f || b.kind === 'wall') continue;
    const d = rectDist(u.x, u.y, b);
    if (d < bd && (!guard || rectDist(o.x, o.y, b) < leash)) { bd = d; best = b; }
  }
  return best;
}

// La strada verso la meta è sbarrata: l'edificio nemico raggiungibile (di solito una palizzata) da
// abbattere per passare, scelto fra i più vicini all'unità e alla meta.
function breach(u, gx, gy) {
  const here = World.cellAt(u.x, u.y);
  const near = G.buildings
    .filter(b => !b.dead && b.faction !== u.faction && rectDist(u.x, u.y, b) < 900)
    .map(b => [rectDist(u.x, u.y, b) + rectDist(gx, gy, b), b])
    .sort((p, q) => p[0] - q[0]);
  for (const [, b] of near.slice(0, 4)) if (fieldOf(u, b)[here] < Infinity) return b;
  return null;
}

// Attacca il bersaglio o gli si avvicina. false se non può raggiungerlo.
function fight(u, t, dt) {
  const d = distTo(u, t);
  let inRange = d <= unitRange(u) + (t.isBuilding ? 0 : 6);
  if (!inRange && t.isBuilding && u.def.range < 200) inRange = fieldOf(u, t)[World.cellAt(u.x, u.y)] === 0;
  if (inRange) {
    // i tiratori tengono le distanze: con un nemico da mischia addosso arretrano mentre ricaricano
    if (t.isUnit && d < KITE_RANGE && u.cd > 0.3 && u.def.range > 200 && !u.def.siege && !(t.def.range > 200)) {
      moveUnit(u, u.x * 2 - t.x, u.y * 2 - t.y, dt);
      return true;
    }
    const tx = t.isBuilding ? clamp(u.x, t.x0, t.x1) : t.x, ty = t.isBuilding ? (t.y0 + t.y1) / 2 : t.y;
    if (Math.abs(tx - u.x) > 4) u.flip = tx < u.x;
    if (u.cd <= 0) startAttack(u, tx, ty);
    return true;
  }
  if (t.isBuilding) return navTo(u, fieldOf(u, t), dt) !== 'blocked';
  return approach(u, t.x, t.y, dt);
}

// Viaggio verso il punto di un ordine, al passo del gruppo se l'ordine ne ha uno (vedi orderUnits).
function march(u, o, dt, near) {
  const own = u.speed;
  let done = false;
  if (o.pace) u.speed = Math.min(own, o.pace);
  // In gruppo ognuno ha il suo posto, cioè una strada sua da calcolare: 40 unità mandate insieme erano 30 campi
  // di distanza nello stesso passo (40 ms di fermo). Se ne lasciano partire pochi per passo (World.fresh): chi
  // aspetta il suo turno cammina intanto verso la meta comune del gruppo, che è una strada sola per tutti.
  if (o.ax !== undefined && !o.own) {
    if (World.fresh-- > 0 || travel(u, o.ax, o.ay, dt)) { o.own = true; u.bestD = Infinity; u.stallT = 0; }
  } else done = travel(u, o.x, o.y, dt, near);
  u.speed = own;
  return done;
}

// Torna al punto di guardia dopo un inseguimento; se non ci arriva, il nuovo posto è dov'è ora.
function holdPost(u, dt) {
  const o = u.order;
  if (Math.hypot(o.x - u.x, o.y - u.y) > 48 && travel(u, o.x, o.y, dt, 24)) { o.x = u.x; o.y = u.y; }
}

// Allarme a cui l'unità risponde: il regno di un giocatore è colpito e lei è ferma senza ordini.
// Arieti (solo edifici) e Monaci restano dove sono.
function alarmFor(u) {
  const f = u.faction, a = f.alarm;
  return f.human && a && G.time - a.t < ALARM_TIME && u.order.k === 'idle' && u.def.dmg && !u.def.onlyBuildings ? a : null;
}
// Corre dove il regno è colpito. Chi era fermo lontano da casa, a cose finite resta di guardia al castello.
function answerAlarm(u, a, dt) {
  const o = u.order, c = u.faction.castle;
  if (c && Math.hypot(o.x - c.x, o.y - c.y) > HOME_RADIUS) {
    const p = World.nearestPassable(c.x + rand(-140, 140), c.y1 + rand(70, 150));
    o.x = p.x; o.y = p.y;
  }
  if (Math.hypot(a.x - u.x, a.y - u.y) > 120) approach(u, a.x, a.y, dt);
}

// Assalto che prosegue da solo (regni dei giocatori): finito un bersaglio o arrivata sul posto, l'unità
// va sul nemico più vicino nei dintorni; quando non ce ne sono più si ferma lì di guardia.
function raidOn(u) {
  const f = u.faction;
  let best = null, bd = RAID_RADIUS;
  if (f.human) {
    if (!u.def.onlyBuildings) {
      for (const e of G.units) {
        if (e.dead || e.inside || e.faction === f) continue;
        const d = Math.hypot(e.x - u.x, e.y - u.y);
        if (d < bd) { bd = d; best = e; }
      }
    }
    for (const b of G.buildings) {
      if (b.dead || b.faction === f || b.kind === 'wall') continue;
      const d = rectDist(u.x, u.y, b);
      if (d < bd) { bd = d; best = b; }
    }
  }
  if (best) setOrder(u, { k: 'amove', x: best.x, y: best.isBuilding ? best.y1 + 24 : best.y, auto: true });
  else idle(u);
}

function updateSoldier(u, dt) {
  const o = u.order, breaching = G.time < u.breachT && u.target && !u.target.dead, alarm = alarmFor(u);
  if (o.k === 'attack') {
    if (o.t.dead) { raidOn(u); return; }
    if (!breaching) u.target = o.t;
  } else if (o.k === 'move') {
    u.target = null;
  } else if (!breaching && (u.thinkT -= dt) <= 0) {
    u.thinkT = rand(0.3, 0.6);
    const t = acquireTarget(u, !!alarm);
    if (t !== u.target) { u.target = t; u.bestD = Infinity; u.stallT = 0; }
  }
  const t = u.target;
  if (t && !t.dead) {
    if (fight(u, t, dt)) { o.fought = true; return; }
    // irraggiungibile: se è all'attacco apre un varco, altrimenti lascia perdere
    const gate = o.k === 'idle' ? null : breach(u, t.x, t.y);
    u.target = gate;
    if (gate) u.breachT = G.time + 8;
    else if (o.k === 'attack') idle(u);
    return;
  }
  u.target = null;
  if (alarm) answerAlarm(u, alarm, dt);
  else if (o.k === 'idle') holdPost(u, dt);
  else if (march(u, o, dt)) {
    const gate = o.k === 'amove' && Math.hypot(o.x - u.x, o.y - u.y) > 200 ? breach(u, o.x, o.y) : null;
    if (gate) { u.target = gate; u.breachT = G.time + 8; u.bestD = Infinity; u.stallT = 0; }
    // un passo d'assalto automatico finito senza trovare da combattere chiude la catena: niente giri a vuoto
    else if (o.k === 'amove' && (o.fought || !o.auto)) raidOn(u);
    else idle(u);
  }
}

// Monaci. Curano l'alleato messo peggio nei dintorni (o quello indicato), da lontano quanto arrivano; con un
// soldato nemico troppo vicino si scansano; se sono partiti insieme a dei soldati (escort) li seguono tenendosi
// a distanza, invece di restare dove l'ordine li aveva portati.
function updateMonk(u, dt) {
  const o = u.order, f = u.faction;
  if (o.k === 'move') { if (march(u, o, dt)) setOrder(u, { k: 'idle', x: u.x, y: u.y, escort: o.escort }); return; }
  if (o.k === 'heal' && (o.t.dead || o.t.hp >= o.t.maxHp)) { idle(u); return; }
  if ((u.thinkT -= dt) <= 0) {
    u.thinkT = rand(0.4, 0.7);
    let heal = null, hr = 0.9, foe = null, fd = MONK_SPACE * 1.6, lead = null, ld = 1500;
    for (const e of G.units) {
      if (e.dead || e.inside || e === u) continue;
      const d = Math.hypot(e.x - u.x, e.y - u.y), soldier = e.def.dmg && !e.def.onlyBuildings;
      if (e.faction !== f) { if (soldier && d < fd) { fd = d; foe = e; } continue; }
      if (!e.def.siege && d < 430 && e.hp / e.maxHp < hr) { hr = e.hp / e.maxHp; heal = e; }
      if (soldier && d < ld) { ld = d; lead = e; }
    }
    u.foe = foe; u.lead = lead; u.target = heal;
  }
  if (o.k === 'heal') u.target = o.t;
  const t = u.target && !u.target.dead && u.target.hp < u.target.maxHp ? u.target : null, foe = u.foe && !u.foe.dead ? u.foe : null;
  const reach = t && distTo(u, t) <= unitRange(u);
  if (reach && u.cd <= 0 && u.faith >= 1) { startAttack(u, t.x, t.y); return; }
  if (foe && Math.hypot(foe.x - u.x, foe.y - u.y) < MONK_SPACE) { // via da lui; il posto di guardia si sposta col Monaco
    moveUnit(u, u.x * 2 - foe.x, u.y * 2 - foe.y, dt);
    if (o.k === 'idle') { o.x = u.x; o.y = u.y; }
    return;
  }
  if (t) { if (!reach) approach(u, t.x, t.y, dt); return; } // a riserva vuota aspetta lì
  u.target = null;
  if (o.k !== 'idle') { idle(u); return; }
  const lead = o.escort && u.lead && !u.lead.dead ? u.lead : null;
  if (!lead) holdPost(u, dt);
  else if (Math.hypot(lead.x - u.x, lead.y - u.y) > ESCORT_GAP) { approach(u, lead.x, lead.y, dt); o.x = u.x; o.y = u.y; }
}

// Esploratori. Vanno dove li si manda; con l'ordine di esplorare raggiungono da soli, una dopo l'altra, la zona
// mai vista più vicina a piedi, finché ce ne sono.
function updateScout(u, dt) {
  const o = u.order, f = u.faction;
  if (o.k === 'idle') { holdPost(u, dt); return; }
  if (o.k !== 'explore') { if (travel(u, o.x, o.y, dt)) idle(u); return; }
  const g = o.goal;
  let next = !g || explored(f, g.x, g.y);
  if (!next && travel(u, g.x, g.y, dt, 40)) { next = true; (o.bad || (o.bad = new Set())).add(World.cellAt(g.x, g.y)); } // lì non ci arriva: un'altra
  if (!next) return;
  o.goal = frontier(u, o.bad);
  u.bestD = Infinity; u.stallT = 0;
  if (!o.goal) { idle(u); if (f.human) emit(['toast', f.idx, 'Esplorazione finita: a piedi non resta nulla da scoprire']); }
}
// La cella mai vista più vicina a piedi (se ne guarda una ogni tre: basta e avanza), o null. bad = celle già
// provate senza riuscire ad arrivarci.
function frontier(u, bad) {
  const seen = u.faction.seen, W = World.W, H = World.H;
  if (!seen) return null;
  const here = World.cellAt(u.x, u.y), field = World.fieldToCell(here, sideOf(u), here);
  let best = -1, bd = Infinity;
  for (let y = 2; y < H - 2; y += 3) {
    for (let c = y * W + 2, end = y * W + W - 2; c < end; c += 3) {
      if (!seen[c] && field[c] < bd && !(bad && bad.has(c))) { bd = field[c]; best = c; }
    }
  }
  return best < 0 ? null : { x: World.cx(best), y: World.cy(best) };
}

// ---------------------------------------------------------------- unità: pedoni

// Punto di consegna di un regno più vicino a un punto, per quel tipo di risorsa.
function dropSiteAt(f, x, y, kind) {
  let best = null, bd = Infinity;
  for (const b of G.buildings) {
    if (b.faction !== f || !b.done || b.dead || !b.def.drop || !b.def.drop.includes(kind)) continue;
    const d = rectDist(x, y, b);
    if (d < bd) { bd = d; best = b; }
  }
  return best;
}
function dropSite(u, kind) { return dropSiteAt(u.faction, u.x, u.y, kind); }

// Nodo da lavorare: quello indicato dal giocatore se c'è posto, altrimenti il più vicino dello
// stesso tipo attorno al punto d'origine. I pedoni di un giocatore, se lì non c'è più nulla, allargano
// la ricerca a tutto il territorio attorno al castello e spostano lì il punto d'origine; da soli scelgono
// solo nodi in territorio già esplorato. Per le pecore, che si spostano e rinascono alla fattoria, conta
// soltanto la più vicina a dove il pedone si trova, ovunque fosse stato mandato all'inizio.
function findNode(u, o) {
  const f = u.faction, c = f.castle, wide = f.human && c;
  const room = n => nodeValid(n) && nodeRoom(n) && !(u.skip && u.skip.has(n.id));
  const free = n => room(n) && explored(f, n.x, n.y);
  if (o.first && room(o.first)) return o.first;
  let best = null, bd = Infinity;
  if (!(wide && o.res === 'meat')) {
    for (const n of nodesOf(o.res)) {
      if (!free(n)) continue;
      const d0 = Math.hypot(n.x - o.px, n.y - o.py);
      if (d0 > 900) continue;
      const d = d0 + Math.hypot(n.x - u.x, n.y - u.y) * 0.3;
      if (d < bd) { bd = d; best = n; }
    }
    if (best || !wide) return best;
  }
  for (const n of nodesOf(o.res)) {
    if (!free(n) || Math.hypot(n.x - c.x, n.y - c.y) > WORK_RADIUS) continue;
    const d = Math.hypot(n.x - u.x, n.y - u.y);
    if (d < bd) { bd = d; best = n; }
  }
  if (best) { o.first = null; o.px = best.x; o.py = best.y; }
  return best;
}

function workOn(u, target, dt) {
  u.flip = target.x < u.x;
  if (Math.floor((u.workT + dt) / 0.6) !== Math.floor(u.workT / 0.6) && onScreen(u.x, u.y)) Sfx.at('chop', u.x, u.y);
  u.state = 'work';
  setAnim(u, 'work' + u.tool);
}

function deposit(u) {
  const f = u.faction, c = u.carry;
  f.res[c.kind] += c.amount;
  f.gathered += c.amount;
  if (f.human) { addFloat(f, u.x, u.y - 70, '+' + c.amount, c.kind); Sfx.at('coin', u.x, u.y); }
  u.carry = null;
  u.pauseT = 0.4;
}

function gatherStep(u, o, dt) {
  const f = u.faction;
  if (u.carry) { // prima consegna quello che ha in spalla
    const site = dropSite(u, u.carry.kind);
    if (site && navTo(u, fieldOf(u, site), dt) === 'arrived') deposit(u);
    return;
  }
  let n = u.node;
  if (!n || !nodeValid(n)) {
    n = findNode(u, o);
    claim(u, n);
    if (!n) {
      // nessun nodo libero adesso (pecore finite, vene esaurite): il pedone di un giocatore tiene il
      // suo lavoro e riprova fra poco; quello dell'IA torna libero e sarà lei a riassegnarlo
      if (f.human) { u.skip = null; u.pauseT = 1; } else idle(u);
      return;
    }
  }
  let at;
  if (n.kind === 'sheep') {
    let d = Math.hypot(n.x - u.x, n.y - u.y);
    // strada facendo: una pecora libera comparsa molto più vicina (nata alla fattoria) vale più di quella scelta,
    // a meno che quella l'abbia indicata il giocatore
    if (d > 120 && n !== o.first && (u.lookT -= dt) <= 0) {
      u.lookT = 0.5;
      const m = findNode(u, o), dm = m ? Math.hypot(m.x - u.x, m.y - u.y) : Infinity;
      if (dm < d * 0.6) { claim(u, m); n = m; d = dm; }
    }
    at = d < 36;
    if (!at && !approach(u, n.x, n.y, dt)) at = null;
  } else {
    const r = navTo(u, fieldOf(u, n), dt);
    at = r === 'arrived' ? true : r === 'blocked' ? null : false;
  }
  if (at === null) { (u.skip || (u.skip = new Set())).add(n.id); claim(u, null); return; } // irraggiungibile: un altro
  if (!at) return;
  workOn(u, n, dt);
  u.workT += dt * f.mods.gather[o.res];
  if (u.workT < JOB_TIME[o.res]) return;
  const cap = CARRY + f.mods.carry;
  let amount;
  if (n.kind === 'tree') { amount = Math.min(cap, n.wood); chopTree(n, amount); }
  else if (n.kind === 'sheep') { amount = SHEEP_MEAT; killSheep(n); }
  else { amount = Math.min(cap, n.amount); n.amount -= amount; }
  if (!nodeValid(n)) claim(u, null);
  u.carry = { kind: o.res, amount };
  u.workT = 0;
  u.pauseT = 0.5;
}

// Dopo un cantiere: un altro cantiere vicino, oppure al lavoro accanto al punto di consegna appena finito,
// oppure la raccolta lasciata per venire a costruire.
function afterBuild(u, b) {
  const f = u.faction, wasBuilding = u.order.k === 'build', resume = u.order.resume || null;
  let next = null, bd = 700;
  for (const o of G.buildings) {
    if (o.faction !== f || o.done || o.dead) continue;
    const d = distTo(u, o);
    if (d < bd) { bd = d; next = o; }
  }
  if (next) { setOrder(u, { k: 'build', b: next, resume }); return; }
  if (wasBuilding && b && !b.dead && b.done && (b.kind === 'depot' || b.kind === 'farm')) {
    let node = null, nd = 520;
    for (const n of b.kind === 'farm' ? G.sheep : [...G.trees, ...G.golds, ...G.stones]) {
      if (!nodeValid(n) || !explored(f, n.x, n.y)) continue;
      const d = rectDist(n.x, n.y, b);
      if (d < nd) { nd = d; node = n; }
    }
    if (node) { setOrder(u, gatherOrder(node)); return; }
  }
  if (resume) setOrder(u, { ...resume, first: null }); else idle(u);
}

function buildStep(u, o, dt) {
  const b = o.b;
  if (b.dead || (o.k === 'build' ? b.done : b.hp >= b.maxHp)) { afterBuild(u, b); return; }
  const r = navTo(u, fieldOf(u, b), dt);
  if (r === 'blocked') { idle(u); return; }
  if (r !== 'arrived') return;
  workOn(u, b, dt);
  u.workT += dt;
  if (o.k === 'repair') { b.hp = Math.min(b.maxHp, b.hp + b.maxHp * REPAIR_RATE * dt); return; }
  b.progress += dt;
  b.hp = Math.min(b.maxHp, b.hp + b.maxHp * 0.9 * dt / b.def.work);
  if (b.progress >= b.def.work) completeBuilding(b);
}

// ---- pedoni al riparo: con una truppa nemica in vista corrono nella casa più vicina e ci restano nascosti

// C'è una truppa nemica (non pedoni, monaci o arieti, che ai pedoni non fanno nulla) entro r px dal punto?
function threatNear(f, x, y, r) {
  for (const e of G.units) {
    if (e.dead || e.inside || e.faction === f || !e.def.dmg || e.def.onlyBuildings) continue;
    if (Math.abs(e.x - x) < r && Math.abs(e.y - y) < r && Math.hypot(e.x - x, e.y - y) < r) return true;
  }
  return false;
}
function sightPx(u) { return (u.def.sight + u.faction.mods.sight) * TILE; }
function shelterFull(b) { return b.kind !== 'castle' && b.inside.length >= SHELTER_CAP; }
// La casa più vicina con un posto libero; se non ce n'è, il castello.
function shelterFor(u) {
  const f = u.faction;
  let best = null, bd = Infinity;
  for (const b of G.buildings) {
    if (b.faction !== f || b.dead || !b.done || b.kind !== 'house' || shelterFull(b)) continue;
    const d = rectDist(u.x, u.y, b);
    if (d < bd) { bd = d; best = b; }
  }
  return best || (f.castle && !f.castle.dead ? f.castle : null);
}
function enterShelter(u, b) {
  if (onScreen(u.x, u.y)) addFx('dust1', u.x, u.y + 4, { dy: -6 });
  u.inside = b;
  b.inside.push(u);
  u.x = (b.x0 + b.x1) / 2; u.y = (b.y0 + b.y1) / 2; // al centro dell'edificio: così anche i client sanno in quale
  u.moving = false; u.state = 'idle';
}
function leaveShelter(u) {
  const b = u.inside, i = b.inside.indexOf(u);
  if (i >= 0) b.inside.splice(i, 1);
  u.inside = null;
  const p = World.nearestPassable(b.x + rand(-0.3, 0.3) * b.def.w * TILE, b.y1 + 24);
  u.x = p.x; u.y = p.y;
  if (onScreen(u.x, u.y)) addFx('dust1', u.x, u.y + 4, { dy: -6 });
}
// Passato il pericolo (o senza un riparo raggiungibile) il pedone riprende ciò che stava facendo.
function resumeWork(u) {
  const back = u.order.back;
  if (back.k === 'gather') setOrder(u, { ...back, first: back.first && nodeValid(back.first) ? back.first : null });
  else if (back.k === 'idle') { setOrder(u, back); idle(u); } // fermo dov'è uscito
  else setOrder(u, back);
}
// Un ordine diretto dato con truppe nemiche attorno, e il pedone non scappa più finché per CALM_TIME secondi non
// ne vede. Da coraggioso guarda lontano quanto dal riparo: così chi lo teneva chiuso in casa non lo fa scappare
// di nuovo un passo dopo l'uscita. Un ordine dato in tempo di pace non cambia nulla: se poi arriva qualcuno, scappa.
function embolden(u) {
  const b = u.inside;
  if (u.type !== 'pawn' || !threatNear(u.faction, b ? b.x : u.x, b ? b.y : u.y, sightPx(u) + TILE * 2)) return;
  u.brave = true; u.calmT = 0;
}
function shelterStep(u, o, dt) {
  if (o.calm >= CALM_TIME) { resumeWork(u); return; }
  if (u.inside) return;
  if (o.b.dead || !o.b.done || shelterFull(o.b)) o.b = shelterFor(u);
  const r = o.b ? navTo(u, fieldOf(u, o.b), dt) : 'blocked';
  if (r === 'arrived') enterShelter(u, o.b);
  else if (r === 'blocked') { u.brave = true; u.calmT = 0; resumeWork(u); } // nessun riparo: tanto vale lavorare
}

function updatePawn(u, dt) {
  let o = u.order;
  if ((u.fearT -= dt) <= 0) { // ogni 0,4 s guarda se ci sono truppe nemiche in vista (dal riparo: attorno all'edificio)
    u.fearT = 0.4;
    const b = u.inside, seen = threatNear(u.faction, b ? b.x : u.x, b ? b.y : u.y, sightPx(u) + (b || u.brave ? TILE * 2 : 0));
    if (o.k === 'shelter') o.calm = seen ? 0 : o.calm + 0.4;
    else if (u.brave) { u.calmT = seen ? 0 : u.calmT + 0.4; if (u.calmT >= CALM_TIME) u.brave = false; }
    else if (seen) {
      const to = shelterFor(u);
      if (to) { setOrder(u, { k: 'shelter', b: to, back: o, calm: 0 }); o = u.order; }
    }
  }
  if (o.k === 'shelter') { shelterStep(u, o, dt); return; }
  u.tool = o.k === 'gather' ? JOB_TOOL[o.res] : o.k === 'build' || o.k === 'repair' ? 'Hammer' : null;
  // breve sosta dopo il raccolto (col carico in spalla) e dopo la consegna (con l'attrezzo in mano)
  if (u.pauseT > 0) { u.pauseT -= dt; return; }
  if (o.k === 'gather') gatherStep(u, o, dt);
  else if (o.k === 'build' || o.k === 'repair') buildStep(u, o, dt);
  else if (o.k === 'drop') {
    const b = o.b, r = b.dead ? 'blocked' : navTo(u, fieldOf(u, b), dt);
    if (r === 'arrived' && u.carry && b.def.drop && b.def.drop.includes(u.carry.kind)) deposit(u);
    if (r !== 'moving') idle(u);
  } else if (o.k !== 'idle' && travel(u, o.x, o.y, dt)) idle(u);
}

function unitAct(u) {
  const o = u.order, k = o.k;
  if (u.state === 'attack' || u.target) return u.type === 'monk' ? 9 : 2;
  if (k === 'gather') return o.res === 'wood' ? 3 : o.res === 'gold' ? 4 : o.res === 'stone' ? 11 : 5; // anche col carico in spalla: è il suo lavoro
  if (k === 'build') return 6;
  if (k === 'repair') return 7;
  if (k === 'drop') return 8; // in viaggio solo per consegnare: per la schermata Lavoratori è un pedone libero
  if (k === 'shelter') return u.inside ? 12 : 13;
  if (k === 'explore') return 14;
  if (k === 'idle') return u.type === 'pawn' ? 0 : 10;
  return 1;
}
// Risorsa a cui un pedone è assegnato (indice in RES_KINDS, -1 = nessuna): vale anche mentre è al riparo o in
// fuga, perché a pericolo passato ci torna.
function jobOf(u) {
  const o = u.order.k === 'shelter' ? u.order.back : u.order;
  return o.k === 'gather' ? RES_KINDS.indexOf(o.res) : -1;
}

function updateUnit(u, dt) {
  if (u.inside) { updatePawn(u, dt); u.act = unitAct(u); u.job = jobOf(u); return; } // nascosto in casa: aspetta soltanto
  u.cd -= dt; u.hurtT -= dt; u.animT += dt; u.moving = false; u.cross = false;
  if (u.def.heal) u.faith = Math.min(u.def.faith, u.faith + u.def.faithRegen * u.faction.mods.heal * dt);
  if (u.state === 'work') u.state = 'idle';
  if (!tickAction(u, dt)) {
    const cell = World.cellAt(u.x, u.y), side = sideOf(u);
    // dentro un edificio: uno appena piazzato lì, oppure uno proprio (ci si passa, ma non ci si ferma). Esce.
    if (!World.passable(cell, side) && (!World.crossable(cell, side) || (u.order.k === 'idle' && !u.target))) {
      const p = World.nearestPassable(u.x, u.y);
      moveUnit(u, p.x, p.y, dt);
    } else if (u.type === 'pawn') updatePawn(u, dt);
    else if (u.def.heal) updateMonk(u, dt);
    else if (u.def.scout) updateScout(u, dt);
    else updateSoldier(u, dt);
  }
  if (u.state === 'idle' || u.state === 'move') {
    u.state = u.moving ? 'move' : 'idle';
    let a = u.moving ? 'run' : 'idle';
    if (u.type === 'pawn') a += u.carry ? LOAD_NAME[u.carry.kind] : (u.tool || '');
    setAnim(u, a);
  }
  u.act = unitAct(u);
  u.job = u.type === 'pawn' ? jobOf(u) : -1;
  if (u.moving && (u.dustT -= dt) <= 0) {
    u.dustT = rand(0.7, 1.5);
    if (onScreen(u.x, u.y)) addFx('dust1', u.x, u.y + 4, { dy: -6 });
  }
}

// Le unità sovrapposte si spingono via a vicenda. Per non confrontare tutte le coppie (con 400 unità sono 80 000)
// le unità stanno in una griglia di SEP_CELL px fatta a inizio passo, e ognuna guarda solo le 9 caselle attorno a
// sé; le coppie si visitano nello stesso ordine di prima (i, poi j crescente). Una casella è il doppio della
// distanza di spinta: basta finché nessuno si sposta di 32 px in un passo.
const SEP_CELL = 64;
function separateUnits() {
  const us = G.units, n = us.length, R = 32, grid = new Map(), near = [];
  const cell = (x, y) => Math.floor(x / SEP_CELL) * 4096 + Math.floor(y / SEP_CELL);
  for (let i = 0; i < n; i++) {
    const u = us[i];
    if (u.inside) continue;
    const k = cell(u.x, u.y), list = grid.get(k);
    if (list) list.push(i); else grid.set(k, [i]);
  }
  for (let i = 0; i < n; i++) {
    const a = us[i];
    if (a.inside) continue;
    const k0 = cell(a.x, a.y);
    near.length = 0;
    for (let gx = -1; gx <= 1; gx++) {
      for (let gy = -1; gy <= 1; gy++) {
        const list = grid.get(k0 + gx * 4096 + gy);
        if (list) for (const j of list) if (j > i) near.push(j);
      }
    }
    if (near.length > 1) near.sort((p, q) => p - q);
    for (const j of near) {
      const b = us[j], dx = b.x - a.x, dy = b.y - a.y;
      if (dx > R || dx < -R || dy > R || dy < -R) continue;
      const d = Math.hypot(dx, dy);
      if (d >= R) continue;
      const push = (R - d) * 0.12, ux = d > 0.01 ? dx / d : Math.cos(i), uy = d > 0.01 ? dy / d : Math.sin(i);
      const wa = a.state === 'work' || a.state === 'attack' ? 0.2 : 1, wb = b.state === 'work' || b.state === 'attack' ? 0.2 : 1;
      World.tryMove(a, -ux * push * wa, -uy * push * wa);
      World.tryMove(b, ux * push * wb, uy * push * wb);
    }
  }
}

// ---------------------------------------------------------------- edifici

function buildingCells(kind, cx, cy) {
  const d = BUILD_DEF[kind], cells = [];
  for (let y = 0; y < d.h; y++) for (let x = 0; x < d.w; x++) cells.push(World.idx(cx + x, cy + y));
  return cells;
}

// Posizione di un edificio sulla griglia: cella d'angolo, celle occupate, punto d'appoggio e rettangolo.
function buildingPlace(kind, cx, cy) {
  const d = BUILD_DEF[kind];
  return {
    cx, cy, cells: buildingCells(kind, cx, cy),
    x: (cx + d.w / 2) * TILE, y: (cy + d.h) * TILE - 4, sy: (cy + d.h) * TILE - 4,
    x0: cx * TILE, y0: cy * TILE, x1: (cx + d.w) * TILE, y1: (cy + d.h) * TILE,
  };
}
// Posizione e ingombro di un edificio (comune a chi simula e a chi lo riceve dalla rete).
function buildingShape(kind, cx, cy) {
  return {
    isBuilding: true, kind, def: BUILD_DEF[kind], ...buildingPlace(kind, cx, cy),
    variant: 0, queue: [], rally: null, fireSeed: Math.random() * 100, dead: false, draw: drawBuilding,
  };
}
// Occupa (o libera) le celle di un edificio. Chi è di quel regno può passarci; sulle palizzate anche fermarsi.
function blockBuilding(b, on) {
  World.setBlock(b.cells, on ? 1 : -1);
  World.setOwner(b.cells, on ? b.faction.idx + 1 : 0, b.kind === 'wall');
}
// Sposta un edificio su un'altra cella così com'è: vita, coda e cantiere restano; il punto di raccolta lo segue.
function relocateBuilding(b, cx, cy) {
  const dx = (cx - b.cx) * TILE, dy = (cy - b.cy) * TILE;
  for (let i = 0; i < b.def.w; i++) addFx('dust2', b.x0 + (i + 0.5) * TILE, b.y + 4);
  blockBuilding(b, false);
  Object.assign(b, buildingPlace(b.kind, cx, cy));
  blockBuilding(b, true);
  for (const u of b.inside) { u.x = (b.x0 + b.x1) / 2; u.y = (b.y0 + b.y1) / 2; }
  b.rally = { x: b.rally.x + dx, y: b.rally.y + dy, t: b.rally.t };
  b.pasture = { x: b.pasture.x + dx, y: b.pasture.y + dy };
  for (let i = 0; i < b.def.w; i++) addFx('dust2', b.x0 + (i + 0.5) * TILE, b.y + 4);
  Sfx.at('build', b.x, b.y);
}

// queue = addestramenti e ricerche in corso: [{ unit | tech, t, total }];
// rally = dove vanno le unità appena addestrate ({ x, y, t = nodo o cantiere a cui mandare i pedoni }).
function addBuilding(f, kind, cx, cy, done) {
  const d = BUILD_DEF[kind], maxHp = Math.round(d.hp * f.mods.buildHp);
  const b = track({
    ...buildingShape(kind, cx, cy), id: NEXT_ID++, faction: f, maxHp,
    hp: done ? maxHp : maxHp * 0.1, done, progress: done ? d.work : 0, cd: rand(0, 1), breedT: 0, housedT: -99,
    inside: [], // pedoni al riparo qui dentro (case e castello)
  });
  if (d.sprites) b.variant = f.houses++ % d.sprites.length;
  b.rally = { x: b.x, y: b.y1 + 72, t: null };
  b.pasture = { x: b.x, y: b.y1 + 70 };
  blockBuilding(b, true);
  G.buildings.push(b);
  return b;
}

function completeBuilding(b) {
  const f = b.faction;
  b.done = true;
  for (let i = 0; i < b.def.w; i++) addFx('dust2', b.x0 + (i + 0.5) * TILE, b.y + 4, { scale: b.def.w > 1 ? 1.5 : 1 });
  if (b.kind === 'farm') for (let i = 0; i < 2; i++) makeSheep(b.pasture);
  census(f);
  if (f.human && b.kind !== 'wall') { emit(['play', f.idx, 'build']); G.announce('Costruzione finita: ' + b.def.label, f, 2.2, f); }
}

function destroyBuilding(b, by) {
  if (b.dead) return;
  b.dead = true;
  G.index.delete(b.id);
  blockBuilding(b, false);
  for (const u of b.inside.slice()) leaveShelter(u); // chi era al riparo qui si ritrova fuori e cerca un'altra casa
  const d = b.def, f = b.faction;
  for (const q of b.queue) if (q.tech) delete f.busy[q.tech];
  for (let i = 0; i < d.w + 1; i++) {
    addFx('boom2', b.x + rand(-0.4, 0.4) * d.w * TILE, b.y - rand(10, d.ay * 0.5), { dy: 80, scale: d.w > 1 ? rand(1, 1.5) : 0.7 });
  }
  Sfx.at('boom', b.x, b.y);
  const bf = by && by.faction;
  if (bf && bf !== f) {
    bf.razed++;
    if (b.kind !== 'wall') { // bottino per chi l'ha abbattuto: parte di ciò che era costato, o cibo dalla fattoria
      const p = World.nearestPassable(b.x, b.y1 + 20);
      const loot = b.kind === 'farm' ? ['meat', 'meat', 'wood'] : b.kind === 'tower' ? ['stone', 'stone', 'wood'] : ['wood', 'wood', 'gold'];
      for (const kind of loot) dropPickup(kind, p.x + rand(-30, 30), p.y, 15, 60);
    }
  }
  if (b.kind === 'castle') eliminateFaction(f, bf);
}

// Uscita delle unità addestrate: sotto l'edificio, poi verso il punto di raccolta.
function deploy(b, type) {
  const f = b.faction, p = World.nearestPassable(b.x + rand(-0.3, 0.3) * b.def.w * TILE, b.y1 + 24);
  const u = spawnUnit(f, type, p.x, p.y), r = b.rally, pawn = type === 'pawn';
  if (onScreen(u.x, u.y)) addFx('dust2', u.x, u.y + 2);
  if (pawn && r.t && NODE_RES[r.t.kind] && nodeValid(r.t)) setOrder(u, gatherOrder(r.t));
  else if (pawn && r.t && r.t.isBuilding && !r.t.dead && !r.t.done) setOrder(u, { k: 'build', b: r.t });
  else {
    const q = World.nearestPassable(r.x + rand(-34, 34), r.y + rand(-22, 22));
    setOrder(u, { k: 'move', x: q.x, y: q.y });
  }
  if (pawn) f.pawnN++; else f.troopN++;
  return u;
}

// Con n edifici dello stesso tipo ognuno addestra più in fretta (oltre ad avere ciascuno la sua coda).
function trainSpeed(n) { return 1 + TRAIN_BONUS * clamp(n - 1, 0, 5); }
// C'è posto per un'altra unità di quel tipo? I Pedoni stanno nelle case, le truppe negli accampamenti.
function roomFor(f, type) { return type === 'pawn' ? f.pawnN < f.pawnCap : f.troopN < f.troopCap; }

function updateQueue(b, dt) {
  const q = b.queue[0], f = b.faction;
  if (!q) return;
  if (q.t < q.total) { q.t = Math.min(q.total, q.t + dt * (q.unit ? trainSpeed(f.has[b.kind] || 1) : 1)); return; }
  if (q.tech) {
    b.queue.shift();
    delete f.busy[q.tech];
    applyTech(f, q.tech);
  } else if (roomFor(f, q.unit)) {
    b.queue.shift();
    deploy(b, q.unit);
  } else if (f.human && G.time - b.housedT > 15) { // pronta, ma non c'è posto: aspetta una casa o un accampamento
    b.housedT = G.time;
    const full = q.unit === 'pawn' ? f.pawnCap >= PAWN_MAX : f.troopCap >= TROOP_MAX;
    emit(['toast', f.idx, q.unit === 'pawn' ? (full ? 'Pedoni al massimo' : 'Servono più case') : full ? 'Truppe al massimo' : 'Servono più accampamenti']);
  }
}

function updateBuilding(b, dt) {
  if (!b.done) return;
  const f = b.faction;
  updateQueue(b, dt);
  if (b.kind === 'farm' && (b.breedT += dt * f.mods.breed) >= FARM_BREED) {
    b.breedT = 0;
    if (G.sheep.filter(s => s.home === b.pasture).length < FARM_SHEEP) makeSheep(b.pasture);
  }
  if (b.kind !== 'tower' && b.kind !== 'castle') return;
  if ((b.cd -= dt) > 0) return;
  const range = (b.kind === 'tower' ? 420 : 380) + f.mods.range, oy = b.y - b.def.ay * 0.62;
  let best = null, bd = range;
  for (const o of G.units) {
    if (o.dead || o.inside || o.faction === f) continue;
    const d = Math.hypot(o.x - b.x, o.y - b.y);
    if (d < bd) { bd = d; best = o; }
  }
  if (!best) { b.cd = 0.3; return; }
  fireArrow(b, b.x, oy, best.x, best.y - 28, (b.kind === 'tower' ? 15 : 14) + f.mods.atk.archer);
  b.cd = b.kind === 'tower' ? 1.1 : 1.2;
}

// ---------------------------------------------------------------- proiettili, bottino, effetti

function rockImpact(a) {
  const R = a.owner.def.splash;
  for (const u of G.units) {
    if (!u.dead && !u.inside && u.faction !== a.faction && Math.hypot(u.x - a.tx, u.y - a.ty) < R) hurt(u, damageTo(a.owner, a.dmg, u), a);
  }
  for (const b of G.buildings) {
    if (!b.dead && b.faction !== a.faction && rectDist(a.tx, a.ty, b) < R * 0.5) hurt(b, damageTo(a.owner, a.dmg, b), a);
  }
  addFx('boom1', a.tx, a.ty - 10, { dy: 30 });
  addFx('dust2', a.tx, a.ty + 6, { scale: 1.6 });
  Sfx.at('boom', a.tx, a.ty);
}

function updateArrows(dt) {
  for (const a of G.arrows) {
    if (a.rock) {
      const k = Math.min(1, (a.t += dt) / a.dur);
      a.x = a.sx + (a.tx - a.sx) * k;
      a.y = a.sy + (a.ty - a.sy) * k - Math.sin(k * Math.PI) * 150;
      a.ang += dt * 9;
      if (k >= 1) { a.dead = true; rockImpact(a); }
      continue;
    }
    a.x += a.vx * dt; a.y += a.vy * dt;
    if ((a.life -= dt) <= 0) { a.dead = true; continue; }
    for (const u of G.units) {
      if (u.dead || u.inside || u.faction === a.faction) continue;
      if (Math.abs(u.x - a.x) < 18 && Math.abs(u.y - 28 - a.y) < 26) {
        hurt(u, damageTo(a.owner, a.dmg, u), a);
        Sfx.at('hit', a.x, a.y);
        a.dead = true;
        break;
      }
    }
    if (a.dead) continue;
    for (const b of G.buildings) {
      if (b.dead || b.faction === a.faction) continue;
      if (a.x > b.x0 && a.x < b.x1 && a.y > b.y0 - 70 && a.y < b.y1) { hurt(b, damageTo(a.owner, a.dmg, b), a); a.dead = true; break; }
    }
  }
  G.arrows = G.arrows.filter(a => !a.dead);
}

function updatePickups(dt) {
  const scan = (G.pickT -= dt) <= 0;
  if (scan) G.pickT = 0.2;
  for (const p of G.pickups) {
    p.t += dt; p.life -= dt;
    if (p.t < 0.4) {
      const k = p.t / 0.4;
      p.x = p.sx + (p.tx - p.sx) * k; p.y = p.sy0 + (p.ty - p.sy0) * k; p.hop = Math.sin(k * Math.PI) * 30;
      continue;
    }
    p.hop = 0;
    if (!scan) continue;
    for (const u of G.units) {
      if (u.dead || u.inside || Math.abs(u.x - p.x) > 32 || Math.abs(u.y - p.y) > 28) continue;
      const f = u.faction;
      p.life = 0;
      f.res[p.kind] += p.amount;
      if (p.chest) emit(['prop', 'chest_open', Math.round(p.x), Math.round(p.y)]);
      if (f.human) {
        addFloat(f, u.x, u.y - 80, '+' + p.amount, p.kind);
        emit(['play', f.idx, 'coin']);
        if (p.chest) emit(['toast', f.idx, `Forziere trovato: +${p.amount} ${RES_LABEL[p.kind].toLowerCase()}`]);
      }
      break;
    }
  }
  G.pickups = G.pickups.filter(p => p.life > 0);
}

function updateFx(dt) {
  for (const f of G.fx) f.t += dt;
  G.fx = G.fx.filter(f => f.t * f.sh.fps < f.sh.n);
  for (const f of G.floats) { f.t += dt; f.y -= 28 * dt; }
  G.floats = G.floats.filter(f => f.t < 1.2);
}

// ---------------------------------------------------------------- disegno

function drawUnit(ctx, u) {
  const sh = unitSheet(u.faction.color, u.type, u.anim);
  let fr = Math.floor(u.animT * sh.fps);
  if (u.state === 'attack') fr = Math.min(fr, sh.n - 1);
  if (Ctl.sel.has(u.id) && !SHADOW.on) { // anello di selezione: bianco per i propri, rosso per gli altri
    const big = u.def.siege || u.type === 'knight';
    ctx.strokeStyle = u.faction === G.player ? '#ffffff' : '#ff5a4a';
    ctx.lineWidth = 2.5;
    ctx.beginPath(); ctx.ellipse(u.x, u.y + 1, big ? 38 : 24, big ? 13 : 10, 0, 0, Math.PI * 2); ctx.stroke();
  }
  if (u.hurtT > 0) ctx.globalAlpha = 0.5;
  drawSheet(ctx, sh, fr, u.x, u.y, u.flip);
  ctx.globalAlpha = 1;
}

// Palizzata: accanto ad altre (o da sola) è lo steccato visto di fronte; sopra o sotto un'altra diventa una
// fila di pali vista di lato, fatta ripetendo un solo palo dell'immagine dall'alto verso il basso.
const STAKE = { sx: 31, sw: 14, n: 5 }; // colonne di un palo nell'immagine, pali per coprire una cella in altezza
function drawWall(ctx, b) {
  const im = buildingImg(b.faction.color, 'wall'), d = b.def, W = World.W, c = b.cells[0];
  const wall = k => k >= 0 && k < World.wallOf.length && World.wallOf[k] !== 0;
  const side = wall(c - 1) || wall(c + 1), up = wall(c - W), down = wall(c + W), front = side || !(up || down);
  if (!b.done) ctx.globalAlpha = 0.35 + 0.65 * clamp(b.progress / d.work, 0, 1);
  if (!front || up) { // i pali in colonna: tutti se la cella è solo verticale, altrimenti quelli dietro lo steccato
    const n = front ? STAKE.n - 1 : STAKE.n, x = Math.round(b.x - STAKE.sw / 2);
    for (let i = 0; i < n; i++) ctx.drawImage(im, STAKE.sx, 0, STAKE.sw, im.height, x, Math.round(b.y0 + (i + 1) * TILE / STAKE.n - 72), STAKE.sw, im.height);
  }
  if (front) ctx.drawImage(im, Math.round(b.x - d.ax), Math.round(b.y - d.ay));
  ctx.globalAlpha = 1;
}

function drawBuilding(ctx, b) {
  if (b.kind === 'wall') { drawWall(ctx, b); return; }
  const im = buildingImg(b.faction.color, b.kind, b.variant), d = b.def;
  const dx = Math.round(b.x - d.ax), dy = Math.round(b.y - d.ay);
  if (!b.done) {
    ctx.globalAlpha = 0.3;
    ctx.drawImage(im, dx, dy);
    ctx.globalAlpha = 1;
    const h = Math.round(im.height * clamp(b.progress / d.work, 0, 1));
    if (h > 0) ctx.drawImage(im, 0, im.height - h, im.width, h, dx, dy + im.height - h, im.width, h);
    return;
  }
  ctx.drawImage(im, dx, dy);
  const r = b.hp / b.maxHp;
  if (r >= 0.7 || SHADOW.on) return;
  const sh = r < 0.25 ? FX.fire3() : r < 0.45 ? FX.fire2() : FX.fire1(), n = r < 0.25 ? 4 : r < 0.45 ? 3 : 2;
  for (let i = 0; i < n; i++) {
    const h1 = Math.abs(Math.sin(b.fireSeed + i * 12.9898) * 43758.5453) % 1, h2 = Math.abs(Math.sin(b.fireSeed + i * 78.233) * 12543.123) % 1;
    drawSheet(ctx, sh, Math.floor(G.time * sh.fps + i * 3), b.x + (h1 - 0.5) * d.w * TILE * 0.7, b.y - 24 - h2 * d.ay * 0.45, false, 1.4);
  }
}

function drawTree(ctx, t) {
  if (t.wood > 0) drawSheet(ctx, treeSheet(t.v), Math.floor(G.time * 8 + t.phase), t.x, t.y);
  else ctx.drawImage(stumpImg(t.v), Math.round(t.x - 96), Math.round(t.y - TREE_DEF[t.v].stumpAy));
}

function drawGold(ctx, g) {
  const stage = clamp(Math.ceil(g.amount / 40), 1, 6);
  if (g.amount <= 0) ctx.globalAlpha = 0.45;
  if (g.big) drawBigNode(ctx, g);
  else if (g.shine > 0 && g.amount > 0) drawSheet(ctx, goldShine(stage), Math.floor(g.shine * 10), g.x, g.y);
  else ctx.drawImage(goldImg(stage), Math.round(g.x - 64), Math.round(g.y - 64));
  ctx.globalAlpha = 1;
}

function drawStone(ctx, s) {
  if (s.amount <= 0) ctx.globalAlpha = 0.45;
  if (s.big) drawBigNode(ctx, s);
  else {
    const im = stoneImg(s.amount > 190 ? 3 : s.amount > 80 ? 2 : 1);
    ctx.drawImage(im, Math.round(s.x - im.width / 2), Math.round(s.y + 22 - im.height));
  }
  ctx.globalAlpha = 1;
}
// Grande giacimento: appoggiato sul bordo basso delle sue 2x2 celle.
function drawBigNode(ctx, n) {
  const im = bigNodeImg(n.kind);
  ctx.drawImage(im, Math.round(n.x - im.width / 2), Math.round(World.cy(n.c) + TILE * 1.5 - 6 - im.height));
}

function drawPoint(ctx, p) {
  ctx.drawImage(pointImg(p.kind), Math.round(p.x - p.def.ax), Math.round(p.y - p.def.ay));
}

function drawProp(ctx, p) {
  const im = img(PROP(p.name));
  ctx.drawImage(im, Math.round(p.x - im.width / 2), Math.round(p.y - im.height));
}

const SHEEP_SHEET = {
  idle: () => sheet(RES + 'Meat/Sheep/Sheep_Idle.png', 128, 64, 82, 8),
  move: () => sheet(RES + 'Meat/Sheep/Sheep_Move.png', 128, 64, 82, 8),
  grass: () => sheet(RES + 'Meat/Sheep/Sheep_Grass.png', 128, 64, 82, 10),
};
function drawSheep(ctx, s) {
  const sh = SHEEP_SHEET[s.state]();
  drawSheet(ctx, sh, Math.floor(s.animT * sh.fps), s.x, s.y, s.flip);
}

// Forzieri e mucchietti di bottino (questi piccoli: ingranditi perché si notino sull'erba).
function drawPickup(ctx, p) {
  if (p.chest) { drawProp(ctx, { name: 'chest', x: p.x, y: p.y + 8 }); return; }
  if (p.life < 5 && Math.floor(p.life * 6) % 2) return;
  const y = p.y - p.hop - Math.abs(Math.sin(p.t * 3)) * 4, k = 1.5;
  ctx.save();
  ctx.translate(Math.round(p.x), Math.round(y));
  ctx.scale(k, k);
  if (p.kind === 'gold') {
    const ph = p.t % 2.2;
    if (ph < 0.6) drawSheet(ctx, sheet(RES + 'Gold/Gold Resource/Gold_Resource_Highlight.png', 128, 64, 70, 10), Math.floor(ph * 10), 0, 0);
    else ctx.drawImage(img(RES + 'Gold/Gold Resource/Gold_Resource.png'), -64, -70);
  } else if (p.kind === 'stone') {
    ctx.drawImage(img('Extra/Resources/Stone_Resource.png'), -13, -20);
  } else {
    const im = img(p.kind === 'wood' ? RES + 'Wood/Wood Resource/Wood Resource.png' : RES + 'Meat/Meat Resource/Meat Resource.png');
    ctx.drawImage(im, -32, -42);
  }
  ctx.restore();
}

function drawFx(ctx, f) {
  const x = f.follow ? f.follow.x : f.x, y = f.follow ? f.follow.y : f.y;
  drawSheet(ctx, f.sh, Math.floor(f.t * f.sh.fps), x, y, false, f.scale);
}

function drawArrows(ctx) {
  for (const a of G.arrows) {
    if (!Fog.sees(a.x, a.y)) continue;
    ctx.save();
    ctx.translate(Math.round(a.x), Math.round(a.y));
    ctx.rotate(a.ang);
    if (a.rock) { ctx.scale(1.3, 1.3); ctx.drawImage(img('Extra/Resources/Stone_Resource.png'), -13, -11); }
    else ctx.drawImage(img(`Units/${a.faction.color} Units/Archer/Arrow.png`), -32, -32);
    ctx.restore();
  }
}

const BUSH_SHEET = v => sheet(`${TERR}Decorations/Bushes/Bushe${v + 1}.png`, 128, 64, 80, 8);
function drawBush(ctx, d) { drawSheet(ctx, BUSH_SHEET(d.v), Math.floor(G.time * 8 + d.phase), d.x, d.y); }
function drawRock(ctx, d) { ctx.drawImage(img(`${TERR}Decorations/Rocks/Rock${d.v + 1}.png`), Math.round(d.x - 32), Math.round(d.y - 50)); }

// Barre della vita, avanzamento dei cantieri, targhe dei castelli e bandiere di raccolta, sopra tutto il resto.
function drawOverlays(ctx, visible) {
  const hidden = G.units.filter(u => u.inside && u.faction === G.player); // pedoni miei al riparo: stanno al centro dell'edificio
  for (const e of visible) {
    const picked = Ctl.sel.has(e.id);
    if (e.isUnit) {
      if (e.hp < e.maxHp || picked) smallBar(ctx, e.x - 24, e.y - e.def.barY, 48, e.hp / e.maxHp, 0.5, Assets.barFill[e.faction.color]);
    } else if (e.isBuilding) {
      const top = e.y - e.def.ay, wide = e.def.w > 1;
      const housed = hidden.reduce((k, u) => k + (u.x > e.x0 && u.x < e.x1 && u.y > e.y0 && u.y < e.y1 ? 1 : 0), 0);
      if (housed) { // quanti pedoni sono al riparo qui (sul castello: accanto al nastro col nome)
        const bx = e.kind === 'castle' ? e.x + 98 : e.x - 34, by = e.kind === 'castle' ? top - 14 : top - 22;
        drawAvatar(ctx, e.faction.color, 'pawn', bx, by, 34);
        label(ctx, '×' + housed, bx + 38, by + 18, 17, '#fff7c9', 'left');
      }
      if (!e.done) { if (wide) bigBar(ctx, e.x - 45, e.y - 60, 90, e.progress / e.def.work, 0.4); }
      else if (e.hp < e.maxHp || picked) smallBar(ctx, e.x - (wide ? 44 : 22), top + 18, wide ? 88 : 44, e.hp / e.maxHp, wide ? 0.7 : 0.5, Assets.barFill[e.faction.color]);
      if (e.kind === 'castle') smallRibbon(ctx, e.faction.colorIdx, 0, e.x, top + 4, 190, 0.8, e.faction.name, 15);
      if (picked) {
        drawBrackets(ctx, e.x0 - 10, e.y0 - 14, e.x1 - e.x0 + 20, e.y1 - e.y0 + 28, wide ? 1 : 0.7);
        if (e.faction === G.player && e.done && e.def.trains) { // bandiera sul punto di raccolta
          ctx.strokeStyle = 'rgba(255,255,255,0.6)'; ctx.lineWidth = 2; ctx.setLineDash([6, 6]);
          ctx.beginPath(); ctx.moveTo(e.x, e.y1); ctx.lineTo(e.rally.x, e.rally.y); ctx.stroke();
          ctx.setLineDash([]);
          drawPennant(ctx, e.faction.color, e.rally.x, e.rally.y - 50, 0.45);
        }
      }
    } else if (e.isPoint) { // stendardo di chi lo tiene, e a che punto è chi lo sta conquistando
      const top = e.y - e.def.ay;
      if (e.owner) drawPennant(ctx, e.owner.color, e.x, top - 34, 0.5);
      if (e.by && e.t > 0) smallBar(ctx, e.x - 44, top + 22, 88, e.t / CAPTURE_TIME, 0.7, Assets.barFill[e.by.color]);
      if (picked) drawBrackets(ctx, e.x0 - 10, e.y0 - 14, e.x1 - e.x0 + 20, e.y1 - e.y0 + 28, 1);
    } else if (picked) { // albero, giacimento o pecora sotto esame
      if (e.big) drawBrackets(ctx, e.x - 74, e.y - 80, 148, 148, 1);
      else drawBrackets(ctx, e.x - 30, e.y - 54, 60, 64, 0.7);
    }
  }
  for (const f of G.floats) {
    ctx.globalAlpha = clamp(1.6 - f.t * 1.4, 0, 1);
    drawIcon(ctx, f.icon, f.x - 18, f.y, 30);
    label(ctx, f.str, f.x, f.y, 18, '#fff7c9');
    ctx.globalAlpha = 1;
  }
}
