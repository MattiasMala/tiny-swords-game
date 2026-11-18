'use strict';
// Fazioni: economia, addestramento, ricerche, mercato, comandi dei giocatori e IA dei regni nemici.

const PAWN_MAX = 30, TROOP_MAX = 50, QUEUE_MAX = 5; // tetti di Pedoni (case) e di truppe (accampamenti e fabbriche)
const RAID_SPACING = 120; // secondi in cui un regno appena attaccato dall'IA è un bersaglio poco probabile per le altre
const WORK_RADIUS = 2600; // px dal castello entro cui i pedoni vengono mandati al lavoro dalle schermate di gestione
// Dove si può costruire o spostare un edificio, in caselle: le opere di difesa (torri, palizzate) solo vicino al
// castello, tutto il resto in un raggio molto più ampio; attorno a un avamposto conquistato, di tutto.
const DEFENSE_RADIUS = 12, BUILD_RADIUS = 40, OUTPOST_RADIUS = 9;
const START_RES = { wood: 200, gold: 100, meat: 200, stone: 100 };
// Livelli dell'IA: rendita passiva (risorse/s), secondi fra le decisioni, pedoni voluti per era,
// tetto dell'esercito, attesa della prima ondata e fra le ondate successive (secondi);
// fattorie e fabbriche per tipo di soldato al massimo, trade = compra cibo al mercato quando l'oro avanza;
// counter = quanto insegue le contromisure (0 = esercito base); garrison = quota dell'esercito che resta a casa;
// odds = forza dell'ondata rispetto alla difesa del bersaglio; retreat = quota rimasta sotto cui l'ondata ripiega;
// reinforce = manda i rinforzi all'assalto; rally = chi è a casa va incontro agli invasori; recall = richiama
// l'ondata se la base è in pericolo; torri, monaci e macchine d'assedio al massimo; capture = manda squadre a
// occupare i punti da conquistare attorno a casa.
const AI_LEVEL = {
  easy: { label: 'Facile', trickle: 0, think: 2.4, pawns: [7, 10, 12], army: 9, first: [420, 540], gap: [160, 220],
    farms: 2, factories: 1, trade: false, counter: 0.3, garrison: 0.1, odds: 0.8, retreat: 0.2, reinforce: false, rally: false, recall: false, towers: 0, monks: 1, siege: 1, capture: false },
  normal: { label: 'Normale', trickle: 0, think: 1.5, pawns: [9, 13, 16], army: 18, first: [360, 450], gap: [160, 200],
    farms: 3, factories: 1, trade: false, counter: 1, garrison: 0.15, odds: 0.8, retreat: 0.3, reinforce: true, rally: false, recall: false, towers: 0, monks: 1, siege: 1, capture: true },
  hard: { label: 'Difficile', trickle: 0.6, think: 1, pawns: [12, 18, 22], army: 36, first: [240, 300], gap: [90, 120],
    farms: 4, factories: 2, trade: true, counter: 1.8, garrison: 0.3, odds: 1.1, retreat: 0.45, reinforce: true, rally: true, recall: true, towers: 3, monks: 3, siege: 2, capture: true },
};
const AI_LEVELS = Object.keys(AI_LEVEL);
// Piano dell'IA, in ordine: ['build', edificio] oppure ['tech', ricerca]. Case e ricostruzioni le decide da sola;
// fattorie e torri oltre il tetto del livello si saltano, altre fattorie le aggiunge quando servono pedoni sul cibo.
const AI_PLAN = [
  ['build', 'farm'], ['build', 'barracks'], ['build', 'depot'], ['tech', 'age2'], ['build', 'farm'],
  ['build', 'archery'], ['build', 'smith'], ['build', 'farm'], ['build', 'stable'], ['tech', 'blades'],
  ['build', 'tower'], ['tech', 'axes'], ['build', 'market'], ['build', 'farm'], ['tech', 'age3'],
  ['build', 'monastery'], ['tech', 'armor'], ['build', 'siege'], ['tech', 'picks'], ['tech', 'fletching'],
  ['build', 'tower'], ['tech', 'carts'], ['tech', 'horseshoes'], ['tech', 'herbs'], ['tech', 'breeding'],
  ['tech', 'boots'], ['tech', 'masonry'], ['tech', 'engineering'], ['tech', 'scriptures'],
];

// human = guidata da un giocatore (locale o in rete) invece che dall'IA; base = cella del castello.
function makeFaction(color, base, human, level = 'normal') {
  const L = AI_LEVEL[level];
  return {
    color, colorIdx: COLORS.indexOf(color), name: 'Regno ' + COLOR_NAME[color], base, human, alive: true, idx: 0, conn: 0,
    res: { ...START_RES }, age: 1, tech: {}, busy: {}, castle: null,
    // modificatori dati dalle ricerche (vedi applyTech)
    mods: {
      atk: { warrior: 0, lancer: 0, archer: 0, knight: 0 }, range: 0, armor: 0, arrowRes: 1, siege: 1,
      gather: { wood: 1, gold: 1, meat: 1, stone: 1 }, carry: 0, pawnSpeed: 1, foot: 1, horse: 1,
      heal: 1, healRange: 0, monkHp: 1, breed: 1, buildHp: 1, sight: 0,
    },
    trades: { wood: 0, meat: 0, stone: 0 }, // scambi recenti al mercato: peggiorano il prezzo
    pawnN: 0, troopN: 0, pawnCap: 0, troopCap: 0, has: {}, sites: {}, pawns: [], soldiers: [], houses: 0,
    rate: { wood: 0, gold: 0, meat: 0, stone: 0 }, // rendita attesa al minuto (vedi estimateRates)
    kills: 0, razed: 0, gathered: 0, threat: null, threatT: -99, alertT: -99, tickT: Math.random(),
    raidedT: -999, // quando è partita l'ultima ondata dell'IA contro questo regno
    alarm: null,   // { x, y, t }: dove e quando il regno è stato colpito l'ultima volta (vedi alarmFor)
    seen: null, seeT: 0, // celle già esplorate da un regno guidato da un giocatore (vedi explore)
    // stato dell'IA: passo del piano, esercito voluto (mix), ondata in corso (wave), conti alla rovescia
    ai: { level: L, step: 0, thinkT: rand(1, 3), waveT: rand(...L.first), waveNeed: 4, target: null, shuffleT: 20,
      mix: null, wave: null, dueT: null, attackedT: -999, lackT: 0, captureT: 60 },
  };
}

function setupFaction(f) {
  f.castle = addBuilding(f, 'castle', f.base.x - 2, f.base.y - 1, true);
  for (let i = 0; i < 4; i++) idle(deploy(f.castle, 'pawn'));
  idle(deploy(f.castle, 'warrior'));
  census(f);
}

function canAfford(f, cost) {
  for (const k in cost) if (f.res[k] < cost[k]) return false;
  return true;
}
function pay(f, cost) { for (const k in cost) f.res[k] -= cost[k]; }
function refund(f, cost) { for (const k in cost) f.res[k] += cost[k]; }

// ---------------------------------------------------------------- ricerche

// Effetto di una ricerca sui modificatori di un regno (usata anche dai client in rete, per le statistiche mostrate).
function techMods(m, id) {
  switch (id) {
    case 'carts': m.carry = 5; m.pawnSpeed = 1.15; break;
    case 'axes': m.gather.wood = 1.3; break;
    case 'picks': m.gather.gold = 1.3; m.gather.stone = 1.3; break;
    case 'breeding': m.breed = 1.5; break;
    case 'boots': m.foot = 1.12; break;
    case 'horseshoes': m.horse = 1.15; break;
    case 'blades': m.atk.warrior += 4; m.atk.lancer += 4; m.atk.knight += 4; break;
    case 'shields': m.arrowRes = 0.5; break;
    case 'fletching': m.atk.archer += 3; m.range = 45; break;
    case 'armor': m.armor = 3; break;
    case 'maps': m.sight = 2; break;
    case 'herbs': m.heal = 1.5; break;
    case 'scriptures': m.healRange = 70; m.monkHp = 2; break;
    case 'engineering': m.siege = 1.25; break;
    case 'masonry': m.buildHp = 1.3; break;
  }
}

function applyTech(f, id) {
  const m = f.mods;
  f.tech[id] = true;
  if (id === 'age2' || id === 'age3') {
    f.age = id === 'age2' ? 2 : 3;
    G.announce(`${f.name}: ${AGE_NAME[f.age]}`, f, 4);
    if (f.human) emit(['play', f.idx, 'age']);
    return;
  }
  techMods(m, id);
  // chi è già in campo riceve subito i miglioramenti
  for (const u of G.units) {
    if (u.faction !== f) continue;
    const hp = unitMaxHp(f, u.type);
    u.speed = unitSpeed(f, u.type);
    if (hp !== u.maxHp) { u.hp *= hp / u.maxHp; u.maxHp = hp; }
  }
  for (const b of G.buildings) {
    const hp = Math.round(b.def.hp * m.buildHp);
    if (b.faction === f && hp !== b.maxHp) { b.hp *= hp / b.maxHp; b.maxHp = hp; }
  }
  if (f.human) { emit(['play', f.idx, 'research']); G.announce('Ricerca completata: ' + TECH_DEF[id].label, f, 2.6, f); }
}

// Motivo per cui non si può (ancora) addestrare / ricercare / costruire, oppure null.
function ageLock(f, def) { return f.age < def.age ? 'Richiede: ' + AGE_NAME[def.age] : null; }
// Una ricerca chiede la sua era e, se ne ha una, la ricerca che la precede nell'albero.
function techLock(f, def) {
  return ageLock(f, def) || (def.req && !f.tech[def.req] ? 'Richiede: ' + TECH_DEF[def.req].label : null);
}
function trainBlock(f, b, type) {
  const d = UNIT_DEF[type];
  if (!d || d.from !== b.kind || !b.done) return 'Non disponibile';
  return ageLock(f, d) || (b.queue.length >= QUEUE_MAX ? 'Coda piena' : canAfford(f, d.cost) ? null : 'Risorse insufficienti');
}
function techBlock(f, b, id) {
  const d = TECH_DEF[id];
  if (!d || d.at !== b.kind || !b.done) return 'Non disponibile';
  if (f.tech[id]) return 'Già ricercata';
  if (f.busy[id]) return 'Ricerca in corso';
  return techLock(f, d) || (b.queue.length >= QUEUE_MAX ? 'Coda piena' : canAfford(f, d.cost) ? null : 'Risorse insufficienti');
}

// ---------------------------------------------------------------- costruzione

// own = celle dell'edificio stesso, quando lo si sposta: non contano come occupate.
function canPlace(kind, cx, cy, own = null) {
  const d = BUILD_DEF[kind];
  let lvl = -1;
  for (let y = cy; y < cy + d.h; y++) {
    for (let x = cx; x < cx + d.w; x++) {
      if (!World.inside(x, y)) return false;
      const c = World.idx(x, y), mine = own !== null && own.includes(c) && World.block[c] === 1;
      if ((!mine && !World.passable(c)) || World.stair[c] || World.noBuild[c]) return false;
      if (lvl < 0) lvl = World.level[c]; else if (lvl !== World.level[c]) return false;
    }
  }
  return true;
}
// Motivo per cui lì un edificio di quel tipo non può stare (troppo lontano da castello e avamposti), oppure null.
function zoneBlock(f, kind, cx, cy) {
  const d = BUILD_DEF[kind], x = cx + d.w / 2, y = cy + d.h / 2, c = f.castle, r = d.defense ? DEFENSE_RADIUS : BUILD_RADIUS;
  if (c && Math.hypot(x - (c.cx + c.def.w / 2), y - (c.cy + c.def.h / 2)) <= r) return null;
  for (const p of G.points) if (p.owner === f && p.def.zone && Math.hypot(x - p.gx, y - p.gy) <= OUTPOST_RADIUS) return null;
  return `${d.defense ? 'Torri e palizzate' : 'Troppo lontano'}: entro ${r} caselle dal castello o ${OUTPOST_RADIUS} da un tuo avamposto`;
}
// Motivo per cui un edificio non può essere spostato lì, oppure null.
function moveBlock(f, b, cx, cy) {
  if (b.kind === 'castle') return 'Il castello non si sposta';
  return zoneBlock(f, b.kind, cx, cy) || (canPlace(b.kind, cx, cy, b.cells) ? null : 'Qui non può stare');
}
function placeBlock(f, kind, cx, cy) {
  const d = BUILD_DEF[kind];
  return ageLock(f, d) || (!canAfford(f, d.cost) ? 'Risorse insufficienti' : zoneBlock(f, kind, cx, cy) || (canPlace(kind, cx, cy) ? null : 'Qui non si può costruire'));
}

// Posto libero per un edificio attorno a una cella, con un corridoio di una cella tutto intorno.
function findSpot(kind, x0, y0, rmin = 3, rmax = 18) {
  const d = BUILD_DEF[kind];
  const roomy = (cx, cy) => {
    if (!canPlace(kind, cx, cy)) return false;
    for (let y = cy - 1; y <= cy + d.h; y++) {
      for (let x = cx - 1; x <= cx + d.w; x++) {
        if (!World.inside(x, y) || !World.passable(World.idx(x, y))) return false;
      }
    }
    return true;
  };
  for (let r = rmin; r <= rmax; r++) {
    const ring = [];
    for (let i = -r; i <= r; i++) ring.push([i, -r], [i, r], [-r, i], [r, i]);
    ring.sort(() => Math.random() - 0.5);
    for (const [dx, dy] of ring) {
      const cx = Math.round(x0 + dx - d.w / 2), cy = Math.round(y0 + dy - d.h / 2);
      if (roomy(cx, cy)) return [cx, cy];
    }
  }
  return null;
}

// Unità, edifici posseduti, cantieri aperti e posti per Pedoni e truppe: ricalcolati ogni secondo e quando cambia qualcosa.
function census(f) {
  const pawns = [], soldiers = [], has = {}, sites = {};
  let pawnCap = 0, troopCap = 0;
  for (const u of G.units) {
    if (u.faction !== f || u.dead) continue;
    (u.type === 'pawn' ? pawns : soldiers).push(u);
  }
  for (const b of G.buildings) {
    if (b.faction !== f || b.dead) continue;
    if (!b.done) { sites[b.kind] = (sites[b.kind] || 0) + 1; continue; }
    has[b.kind] = (has[b.kind] || 0) + 1;
    pawnCap += b.def.pawns || 0; troopCap += b.def.troops || 0;
  }
  f.pawns = pawns; f.soldiers = soldiers; f.has = has; f.sites = sites;
  f.pawnN = pawns.length; f.troopN = soldiers.length;
  f.pawnCap = Math.min(PAWN_MAX, pawnCap); f.troopCap = Math.min(TROOP_MAX, troopCap);
}

// Rendita attesa di ogni risorsa, al minuto. Per ogni pedone al lavoro: quanto porta a ogni viaggio, diviso il
// tempo del giro (lavoro al nodo, andata e ritorno fino al punto di consegna più vicino lungo la strada vera,
// cioè il campo di distanza che il pedone segue, più le soste). Mentre porta il carico vale il giro appena
// fatto; chi non ha né nodo né carico sta cercando, e non conta. Il cibo non può superare le pecore che
// fattorie e pascoli gli mettono davanti. In più ciò che rendono i punti conquistati.
// (I numeri del cammino vengono dalla misura dei giri veri: si arriva e si parte dal bordo delle celle, quindi
// circa 0,7 celle in meno per tratta; le pecore si spostano, quindi circa 1,3 celle in più.)
function estimateRates(f) {
  const rate = { wood: 0, gold: 0, meat: 0, stone: 0 }, side = f.idx + 1, W = World.W, c = f.castle;
  for (const u of f.pawns) {
    const o = u.order, n = u.node && nodeValid(u.node) ? u.node : null;
    if (o.k !== 'gather') continue;
    if (n) {
      const site = dropSiteAt(f, n.x, n.y, o.res), here = World.cellAt(n.x, n.y);
      let d = Infinity;
      if (site) {
        const field = World.fieldFor(site, side, World.cellAt(u.x, u.y));
        d = field[here]; // alberi e giacimenti occupano la loro cella: vale la più vicina di quelle attorno
        if (!(d < Infinity) && n.cells) for (const k of n.cells) for (const [dx, dy] of DIRS8) if (field[k + dx + dy * W] < d) d = field[k + dx + dy * W];
      }
      u.trip = d < Infinity ? Math.max(0, d / 10 - 0.7) + (n.kind === 'sheep' ? 1.3 : 0) : -1; // celle per tratta
      u.haul = n.kind === 'sheep' ? SHEEP_MEAT : Math.min(CARRY + f.mods.carry, nodeLeft(n));
    } else if (!u.carry) continue;
    if (!(u.trip >= 0)) continue;
    rate[o.res] += u.haul / (JOB_TIME[o.res] / f.mods.gather[o.res] + 2 * u.trip * TILE / u.speed + 0.9) * 60;
  }
  let flock = 0;
  if (c) for (const s of G.sheep) if (!s.dead && Math.hypot(s.x - c.x, s.y - c.y) < WORK_RADIUS && explored(f, s.x, s.y)) flock++;
  rate.meat = Math.min(rate.meat, ((f.has.farm || 0) * 60 / FARM_BREED * f.mods.breed + flock) * SHEEP_MEAT);
  for (const p of G.points) if (p.owner === f && p.def.res) rate[p.def.res] += POINT_INCOME * 60;
  for (const k of RES_KINDS) rate[k] = Math.round(rate[k]);
  return rate;
}

// ---------------------------------------------------------------- mercato

// Oro per 100 unità vendute / oro richiesto per comprarne 100: ogni scambio peggiora il prezzo, che poi si riprende.
const SELL_PRICE = 50, SELL_DROP = 6, BUY_PRICE = 130, BUY_RISE = 10;
function sellPrice(f, res) { return Math.max(20, SELL_PRICE - f.trades[res] * SELL_DROP); }
function buyPrice(f, res) { return Math.min(320, BUY_PRICE + f.trades[res] * BUY_RISE); }

// ---------------------------------------------------------------- comandi dei giocatori

function ownBuilding(f, id) {
  const b = G.index.get(id);
  return b && b.isBuilding && b.faction === f && !b.dead ? b : null;
}
function ownPawns(f) { return G.units.filter(u => u.faction === f && !u.dead && u.type === 'pawn'); }

// Territorio esplorato da un regno guidato da un giocatore: tutto ciò che unità ed edifici suoi hanno visto
// finora, più i dintorni della base noti dall'inizio (le stesse celle che la nebbia di guerra gli scopre).
// I suoi pedoni, da soli, lavorano solo lì.
function explore(f) {
  const W = World.W, H = World.H, first = !f.seen, seen = f.seen || (f.seen = new Uint8Array(W * H));
  const mark = (x, y, r) => {
    const cx = Math.floor(x / TILE), cy = Math.floor(y / TILE);
    for (let j = Math.max(0, cy - r); j <= Math.min(H - 1, cy + r); j++) {
      for (let i = Math.max(0, cx - r); i <= Math.min(W - 1, cx + r); i++) {
        if ((i - cx) ** 2 + (j - cy) ** 2 <= r * r + r) seen[j * W + i] = 1;
      }
    }
  };
  if (first && f.castle) mark(f.castle.x, f.castle.y, START_SIGHT);
  // chi non ha cambiato cella (né raggio di vista) dall'ultima volta ha già segnato tutto
  const look = (e, x, y, r) => {
    const key = (Math.floor(y / TILE) * W + Math.floor(x / TILE)) * 32 + r;
    if (e.seenKey !== key) { e.seenKey = key; mark(x, y, r); }
  };
  for (const u of G.units) if (u.faction === f && !u.dead) look(u, u.x, u.y, u.def.sight + f.mods.sight);
  for (const b of G.buildings) if (b.faction === f && !b.dead) look(b, b.x, b.y - 30, (b.done ? b.def.sight : 2) + f.mods.sight);
  for (const p of G.points) if (p.owner === f) look(p, p.x, p.y - TILE, POINT_SIGHT + f.mods.sight);
}
function explored(f, x, y) {
  if (!f.human || !f.seen) return true; // l'IA non ha nebbia
  const c = World.cellAt(x, y);
  return c >= 0 && f.seen[c] === 1;
}

// Nodo libero di una risorsa più vicino a un pedone, fra quelli esplorati attorno al regno: { node, d } oppure null.
function nodeNear(f, res, p) {
  const c = f.castle;
  let best = null, bd = Infinity;
  for (const n of nodesOf(res)) {
    if (!nodeValid(n) || !nodeRoom(n) || Math.hypot(n.x - c.x, n.y - c.y) > WORK_RADIUS || !explored(f, n.x, n.y)) continue;
    const d = Math.hypot(n.x - p.x, n.y - p.y);
    if (d < bd) { bd = d; best = n; }
  }
  return best && { node: best, d: bd };
}

// Porta a n i pedoni assegnati a una risorsa (contano anche quelli al riparo, che ci torneranno). In più: parte
// il pedone libero più vicino a un nodo di quella risorsa. In meno: i pedoni tolti restano liberi (chi ha un
// carico va prima a consegnarlo). Libero = senza ordini oppure in viaggio solo per consegnare: così "abbasso la
// legna, alzo l'oro" funziona subito anche con il pedone ancora carico (il carico lo consegna comunque prima di cominciare).
function setWorkers(f, res, n) {
  const pawns = ownPawns(f), job = RES_KINDS.indexOf(res), on = pawns.filter(u => jobOf(u) === job);
  const free = u => u.order.k === 'idle' || u.order.k === 'drop';
  on.sort((a, b) => (a.carry ? 1 : 0) - (b.carry ? 1 : 0));
  for (const u of on.splice(0, Math.max(0, on.length - n))) {
    const site = u.carry && dropSite(u, u.carry.kind);
    if (u.order.k === 'shelter') u.order.back = { k: 'idle', x: u.x, y: u.y }; // è al riparo: quando esce resta libero
    else if (site) setOrder(u, { k: 'drop', b: site }); else idle(u);
  }
  for (let have = on.length; have < n; have++) {
    let best = null;
    for (const u of pawns) {
      if (!free(u)) continue;
      const near = nodeNear(f, res, u);
      if (near && (!best || near.d < best.d)) best = { u, ...near };
    }
    if (!best) return pawns.some(free) ? `${RES_LABEL[res]}: nessun posto libero nel territorio esplorato` : 'Nessun pedone libero';
    setOrder(best.u, gatherOrder(best.node));
  }
  return null;
}

// Esegue il comando di un giocatore sulla sua fazione. Ritorna il motivo del rifiuto, o null.
function execCommand(f, cmd, a, b, c, d, e) {
  if (!f.alive) return null;
  if (cmd === 'order') { // a = id delle unità, b = tipo, c,d = punto, e = id del bersaglio
    if (Array.isArray(a) && a.length <= 200) orderUnits(f, a, b, c, d, e);
    return null;
  }
  if (cmd === 'place') { // a = edificio, b,c = cella, d = id dei pedoni che lo costruiscono
    if (!BUILD_MENU.includes(a) || !Number.isInteger(b) || !Number.isInteger(c)) return null;
    const why = placeBlock(f, a, b, c);
    if (why) return why;
    const def = BUILD_DEF[a], site = addBuilding(f, a, b, c, false);
    pay(f, def.cost);
    f.sites[a] = (f.sites[a] || 0) + 1;
    for (let i = 0; i < def.w; i++) addFx('dust2', (b + i + 0.5) * TILE, (c + def.h) * TILE);
    if (Array.isArray(d)) orderUnits(f, d.slice(0, 200), 'build', 0, 0, site.id);
    return null;
  }
  if (cmd === 'jobs') { // a = risorsa, b = quanti pedoni devono lavorarci (gli slider della schermata Lavoratori)
    return RES_KINDS.includes(a) && Number.isInteger(b) && b >= 0 ? setWorkers(f, a, b) : null;
  }
  if (cmd === 'job') { // a = id di pedoni, b = risorsa a cui mandarli (schermata Unità)
    if (!Array.isArray(a) || a.length > 200 || !RES_KINDS.includes(b)) return null;
    let sent = 0, asked = 0;
    for (const u of ownPawns(f)) {
      if (!a.includes(u.id)) continue;
      const near = nodeNear(f, b, u);
      asked++;
      if (near) { embolden(u); setOrder(u, gatherOrder(near.node)); sent++; }
    }
    return asked && !sent ? `${RES_LABEL[b]}: nessun posto libero nel territorio esplorato` : null;
  }
  if (cmd === 'recall') { // a = id dei soldati da riportare davanti al castello
    const c = f.castle;
    if (!Array.isArray(a) || a.length > 200 || !c) return null;
    const ids = G.units.filter(u => u.faction === f && !u.dead && u.type !== 'pawn' && a.includes(u.id)).map(u => u.id);
    orderUnits(f, ids, 'move', c.x, c.y1 + 130);
    return null;
  }
  const bld = ownBuilding(f, a);
  if (!bld) return null;
  if (cmd === 'train') {
    const why = trainBlock(f, bld, b);
    if (why) return why;
    pay(f, UNIT_DEF[b].cost);
    bld.queue.push({ unit: b, t: 0, total: UNIT_DEF[b].time });
  } else if (cmd === 'research') {
    const why = techBlock(f, bld, b);
    if (why) return why;
    pay(f, TECH_DEF[b].cost);
    f.busy[b] = true;
    bld.queue.push({ tech: b, t: 0, total: TECH_DEF[b].time });
  } else if (cmd === 'cancel') { // b = posizione nella coda, c = unità o ricerca attesa lì: rimborso completo
    const q = bld.queue[b];
    if (!q || (q.unit || q.tech) !== c) return null; // in rete la coda può essere avanzata nel frattempo: meglio niente che l'elemento sbagliato
    bld.queue.splice(b, 1);
    refund(f, q.tech ? TECH_DEF[q.tech].cost : UNIT_DEF[q.unit].cost);
    if (q.tech) delete f.busy[q.tech];
  } else if (cmd === 'rally') { // b,c = punto, d = id di un nodo o cantiere su cui mandare i pedoni
    if (!Number.isFinite(b) || !Number.isFinite(c)) return null;
    const t = d ? G.index.get(d) : null;
    bld.rally = { x: b, y: c, t: t && !t.dead && (NODE_RES[t.kind] || (t.isBuilding && t.faction === f)) ? t : null };
  } else if (cmd === 'move') { // b,c = nuova cella
    if (!Number.isInteger(b) || !Number.isInteger(c) || (b === bld.cx && c === bld.cy)) return null;
    const why = moveBlock(f, bld, b, c);
    if (why) return why;
    relocateBuilding(bld, b, c);
  } else if (cmd === 'demolish') { // un cantiere rende tutto il costo, un edificio finito nulla
    if (bld.kind === 'castle') return 'Il castello non si demolisce';
    if (!bld.done) refund(f, bld.def.cost);
    destroyBuilding(bld, null);
    census(f);
  } else if (cmd === 'trade') { // b = 'sell' | 'buy', c = risorsa
    if (bld.kind !== 'market' || !bld.done || !(c in f.trades)) return null;
    if (b === 'sell') {
      if (f.res[c] < 100) return 'Risorse insufficienti';
      f.res[c] -= 100; f.res.gold += sellPrice(f, c);
    } else if (b === 'buy') {
      const price = buyPrice(f, c);
      if (f.res.gold < price) return 'Oro insufficiente';
      f.res.gold -= price; f.res[c] += 100;
    } else return null;
    f.trades[c]++;
    if (f.human) emit(['play', f.idx, 'coin']);
  }
  return null;
}

// Comando del giocatore locale: eseguito subito oppure, se gioco come client, spedito all'host.
function command(cmd, ...args) {
  if (Net.remote) { Net.send({ t: 'c', c: cmd, a: args }); return null; }
  return execCommand(G.player, cmd, ...args);
}

// ---------------------------------------------------------------- IA

// Esercito base quando l'IA non vede ancora nemici: pesi per tipo (i tipi non disponibili si saltano).
const AI_BASE = { warrior: 3, lancer: 2, archer: 2, knight: 2 };
const AI_HOME = 1400; // px dal castello: un nemico più vicino minaccia la base, un soldato più vicino è "a casa"

// Quota di pedoni per risorsa: segue ciò che l'IA sta per spendere (un pedone raccoglie più o meno lo stesso
// su ogni risorsa): il prossimo passo del piano, i pedoni ancora da addestrare, l'esercito voluto (vedi aiWant),
// più un fondo per case ed edifici; meno su ciò che abbonda già in magazzino.
function aiJobMix(f) {
  const ai = f.ai, L = ai.level, need = { wood: 0.5, gold: 0.15, meat: 0.35, stone: f.age > 1 ? 0.12 : 0 };
  const add = (cost, w) => {
    let s = 0;
    for (const k in cost) s += cost[k];
    for (const k in cost) need[k] += w * cost[k] / s;
  };
  const step = AI_PLAN[ai.step];
  if (step) add((step[0] === 'build' ? BUILD_DEF : TECH_DEF)[step[1]].cost, 1);
  if (f.age === 1 && !(step && step[1] === 'age2')) add(TECH_DEF.age2.cost, 0.6); // l'Età Feudale si prepara da subito
  if (f.pawns.length < L.pawns[f.age - 1]) add(UNIT_DEF.pawn.cost, 0.5);
  if (ai.mix) for (const t in ai.mix) add(UNIT_DEF[t].cost, ai.mix[t] * (f.age > 1 ? 1.5 : 0.8));
  let all = 0;
  for (const k in need) {
    if (f.res[k] > 800) need[k] *= 0.25;
    else if (f.res[k] > 400) need[k] *= 0.5;
    all += need[k];
  }
  for (const k in need) need[k] /= all;
  return need;
}

// Nodo libero più comodo per il regno: vicino al castello e al pedone.
function aiNode(f, res, p) {
  const c = f.castle;
  let best = null, bd = 2200;
  for (const n of res === 'wood' ? G.trees : res === 'meat' ? G.sheep : res === 'gold' ? G.golds : G.stones) {
    if (!nodeValid(n) || !nodeRoom(n)) continue;
    const d = Math.hypot(n.x - c.x, n.y - c.y) + Math.hypot(n.x - p.x, n.y - p.y) * 0.3;
    if (d < bd) { bd = d; best = n; }
  }
  return best;
}

function aiAssignPawns(f, dt) {
  const ai = f.ai, jobs = { wood: 0, gold: 0, meat: 0, stone: 0 }, idlers = [], n = f.pawns.length;
  for (const p of f.pawns) {
    if (p.order.k === 'gather') jobs[p.order.res]++;
    else if (p.order.k === 'idle') idlers.push(p);
  }
  const mix = aiJobMix(f), lack = k => mix[k] * n - jobs[k];
  const neediest = () => RES_KINDS.reduce((a, b) => (lack(b) > lack(a) ? b : a));
  if ((ai.shuffleT -= dt) <= 0 && !idlers.length) { // ogni tanto sposta un pedone dalla risorsa in eccesso a quella che manca
    ai.shuffleT = 6;
    const over = RES_KINDS.reduce((a, b) => (lack(b) < lack(a) ? b : a));
    if (lack(over) < -1 && lack(neediest()) > 1) {
      const p = f.pawns.find(p => p.order.k === 'gather' && p.order.res === over && !p.carry);
      if (p) { jobs[over]--; idlers.push(p); }
    }
  }
  for (const p of idlers) {
    const order = RES_KINDS.slice().sort((a, b) => lack(b) - lack(a));
    for (const res of order) {
      const node = aiNode(f, res, p);
      if (!node) continue;
      setOrder(p, gatherOrder(node));
      jobs[res]++;
      break;
    }
  }
}

// Apre un cantiere e ci manda il pedone più vicino. true se ci è riuscita.
// toward = punto verso cui spingere una torre (di solito da dove arriva il nemico).
function aiBuild(f, kind, toward = null) {
  const def = BUILD_DEF[kind], c = f.castle;
  if (f.age < def.age || !canAfford(f, def.cost) || !f.pawns.length) return false;
  let x = f.base.x, y = f.base.y + 1, rmin = 4;
  if (kind === 'depot') { // accanto al bosco o alla miniera più vicini che non hanno già un punto di consegna
    let best = null, bd = Infinity;
    for (const n of [...G.trees, ...G.golds, ...G.stones]) {
      if (!nodeValid(n)) continue;
      const d = Math.hypot(n.x - c.x, n.y - c.y);
      if (d < 420 || d > bd || d > (BUILD_RADIUS - 4) * TILE) continue;
      if (G.buildings.some(b => b.faction === f && b.def.drop && rectDist(n.x, n.y, b) < 380)) continue;
      bd = d; best = n;
    }
    if (best) { x = Math.floor(best.x / TILE); y = Math.floor(best.y / TILE); rmin = 2; }
  } else if (kind === 'tower') { // verso il nemico (o il punto indicato), a qualche casella dal castello
    const foe = toward || (nearestEnemyFaction(f) || {}).castle;
    if (foe) {
      const a = Math.atan2(foe.y - c.y, foe.x - c.x);
      x += Math.round(Math.cos(a) * 7); y += Math.round(Math.sin(a) * 6); rmin = 1;
    }
  }
  const spot = findSpot(kind, x, y, rmin);
  if (!spot) return false;
  const near = f.pawns.slice().sort((p, q) => Math.hypot(p.x - spot[0] * TILE, p.y - spot[1] * TILE) - Math.hypot(q.x - spot[0] * TILE, q.y - spot[1] * TILE));
  return execCommand(f, 'place', kind, spot[0], spot[1], near.slice(0, def.work > 15 ? 2 : 1).map(p => p.id)) === null;
}

function nearestEnemyFaction(f) {
  let best = null, bd = Infinity;
  for (const o of G.factions) {
    if (o === f || !o.alive) continue;
    const d = Math.hypot(o.castle.x - f.castle.x, o.castle.y - f.castle.y);
    if (d < bd) { bd = d; best = o; }
  }
  return best;
}

// ---- conti dell'IA sugli eserciti: valore, forza a parità di spesa, cosa schierano gli altri

function aiCost(type) { let s = 0; for (const k in UNIT_DEF[type].cost) s += UNIT_DEF[type].cost[k]; return s; }
function aiWorth(u) { return aiCost(u.type) * u.hp / u.maxHp; }
function aiFighter(type) { const d = UNIT_DEF[type]; return !!d.dmg && !d.onlyBuildings; }

// Forza a parità di spesa di un'unità `t` del regno f contro una `e` del regno o: colpo (bonus e corazze
// comprese) al secondo × resistenza (con lo scudo) / costo², con un piccolo vantaggio a chi colpisce da lontano.
function aiPower(f, t, o, e) {
  const d = UNIT_DEF[t], de = UNIT_DEF[e];
  const hit = Math.max(1, ((d.dmg || 0) + (f.mods.atk[t] || 0)) * ((d.bonus && d.bonus[e]) || 1) - (de.dmg && !de.siege ? o.mods.armor : 0));
  const reach = 1 + Math.max(0, d.range - 60) / 1500;
  const tough = unitMaxHp(f, t) / (1 - (d.block || 0) * 0.65);
  return hit / d.cd * reach * tough / aiCost(t) ** 2;
}
// Vantaggio (logaritmo del rapporto di forza) di `t` contro `e`: > 0 se a parità di spesa vince t.
function aiEdge(f, t, o, e) {
  if (!aiFighter(e)) return Math.log((UNIT_DEF[t].bonus && UNIT_DEF[t].bonus[e]) || 1); // monaci, arieti: conta solo il bonus
  return Math.log(aiPower(f, t, o, e) / aiPower(o, e, f, t));
}

// Ciò che l'IA sa degli altri: per ogni regno il valore dei soldati per tipo, pesato da quanto contano
// (vicini al castello dell'IA = minaccia, regno bersaglio dell'ondata, regno vicino); la minaccia in casa
// (valore e punto più vicino al castello); i propri soldati a casa e la loro forza.
function aiIntel(f) {
  const c = f.castle, ai = f.ai, foes = new Map();
  const intel = { foes, total: 0, danger: 0, dangerAt: null, home: [], homeValue: 0 };
  let dd = Infinity;
  for (const o of G.factions) {
    if (o === f || !o.alive || !o.castle) continue;
    const near = 1 / (1 + ((o.castle.x - c.x) ** 2 + (o.castle.y - c.y) ** 2) / 4e6);
    foes.set(o, { o, near: near + (o === ai.target ? 1 : 0), mix: {}, defense: 0 });
  }
  for (const u of G.units) {
    if (u.dead || u.type === 'pawn') continue;
    const d = Math.hypot(u.x - c.x, u.y - c.y);
    if (u.faction === f) {
      if (d < AI_HOME) { intel.home.push(u); if (u.def.dmg) intel.homeValue += aiWorth(u); }
      continue;
    }
    const e = foes.get(u.faction);
    if (!e) continue;
    const v = aiWorth(u), w = e.near + (d < AI_HOME + 800 ? 2 : 0);
    e.mix[u.type] = (e.mix[u.type] || 0) + v * w;
    if (Math.hypot(u.x - u.faction.castle.x, u.y - u.faction.castle.y) < AI_HOME) e.defense += v;
    intel.total += v * w;
    if (d < AI_HOME && u.def.dmg) {
      intel.danger += v;
      if (d < dd) { dd = d; intel.dangerAt = u; }
    }
  }
  for (const e of foes.values()) e.defense += 150 * ((e.o.has.tower || 0) + 1); // torri e castello tirano frecce
  return intel;
}

// Esercito voluto: quota per tipo di soldato disponibile. Ogni tipo vale quanto vince, a parità di spesa,
// contro ciò che schierano i nemici che contano (aiEdge, con i bonus di UNIT_DEF); level.counter dice quanto
// l'IA insegue le contromisure. Senza notizie sui nemici, l'avversario immaginato è l'esercito base.
function aiWant(f, intel) {
  const L = f.ai.level, types = UNIT_TYPES.filter(t => aiFighter(t) && !UNIT_DEF[t].siege && f.has[UNIT_DEF[t].from] && f.age >= UNIT_DEF[t].age);
  if (!types.length) return null;
  const score = {};
  for (const t of types) {
    let s = 0;
    if (intel.total > 300) {
      for (const e of intel.foes.values()) for (const k in e.mix) s += e.mix[k] / intel.total * aiEdge(f, t, e.o, k);
    } else {
      let all = 0;
      for (const k in AI_BASE) all += AI_BASE[k];
      for (const k in AI_BASE) s += AI_BASE[k] / all * aiEdge(f, t, f, k);
    }
    score[t] = (AI_BASE[t] || 1) * Math.exp(L.counter * s);
  }
  let sum = 0;
  for (const t of types) sum += score[t];
  const want = {};
  for (const t of types) want[t] = 0.85 * score[t] / sum + 0.15 / types.length; // un po' di tutto, sempre
  return want;
}

// Prossima unità da addestrare (soldati, monaci al seguito, assedio), o null.
// cheap = solo fra ciò che si può pagare subito (quando la prima scelta aspetta da troppo le sue risorse).
function aiNextUnit(f, intel, cheap = false) {
  const L = f.ai.level, have = {};
  for (const s of f.soldiers) if (!s.dead) have[s.type] = (have[s.type] || 0) + 1;
  for (const b of G.buildings) if (b.faction === f) for (const q of b.queue) if (q.unit) have[q.unit] = (have[q.unit] || 0) + 1;
  const ok = t => f.has[UNIT_DEF[t].from] && f.age >= UNIT_DEF[t].age;
  let fighters = 0;
  for (const t in have) if (aiFighter(t)) fighters += have[t];
  const list = [];
  // monaci: uno ogni cinque soldati, fino al tetto del livello
  for (const t of UNIT_TYPES) {
    if (UNIT_DEF[t].heal && ok(t) && (have[t] || 0) < Math.min(L.monks, Math.floor(fighters / 5))) list.push(t);
  }
  // assedio: un ariete per i castelli; poi una catapulta, se fra i nemici non abbondano i suoi cacciatori
  const siege = UNIT_TYPES.filter(t => UNIT_DEF[t].siege && ok(t));
  let sieges = 0;
  for (const t of siege) sieges += have[t] || 0;
  if (siege.length && fighters >= 6 && sieges < Math.min(L.siege, Math.floor(fighters / 6))) {
    const hunted = t => {
      let v = 0;
      for (const e of intel.foes.values()) for (const k in e.mix) if (UNIT_DEF[k].bonus && UNIT_DEF[k].bonus[t] > 1) v += e.mix[k];
      return intel.total ? v / intel.total : 0;
    };
    const opts = siege.filter(t => !(have[t] > 0) && (UNIT_DEF[t].onlyBuildings || hunted(t) < 0.3));
    opts.sort((a, b) => (UNIT_DEF[b].onlyBuildings ? 1 : 0) - (UNIT_DEF[a].onlyBuildings ? 1 : 0));
    list.push(...opts);
  }
  // soldati: prima il tipo più indietro rispetto alla quota voluta
  const want = f.ai.mix || {}, lag = t => want[t] * (fighters + 1) - (have[t] || 0);
  list.push(...Object.keys(want).sort((a, b) => lag(b) - lag(a)));
  return (cheap ? list.find(t => canAfford(f, UNIT_DEF[t].cost)) : list[0]) || null;
}

// Bersaglio di un'ondata: più probabile un regno vicino e poco difeso rispetto a chi attacca, molto meno
// uno appena preso di mira da un altro (altrimenti due vicini colpiscono spesso lo stesso regno negli stessi secondi).
function chooseTarget(f, intel = null, power = 0) {
  const opts = G.factions.filter(o => o !== f && o.alive);
  let total = 0;
  const w = opts.map(o => {
    let v = 1 / (1 + ((o.castle.x - f.castle.x) ** 2 + (o.castle.y - f.castle.y) ** 2) / 4e6);
    if (intel) { const e = intel.foes.get(o); if (e) v *= 0.3 + power / (power + e.defense); }
    if (G.time - o.raidedT < RAID_SPACING) v *= 0.1;
    total += v;
    return v;
  });
  let r = Math.random() * total;
  for (let i = 0; i < opts.length; i++) if ((r -= w[i]) <= 0) return opts[i];
  return opts[0] || null;
}

function freeBuilding(f, kind) { // edificio finito di quel tipo con la coda più corta
  let best = null;
  for (const b of G.buildings) {
    if (b.faction === f && b.kind === kind && b.done && !b.dead && (!best || b.queue.length < best.queue.length)) best = b;
  }
  return best;
}
function queued(f, test) {
  let n = 0;
  for (const b of G.buildings) if (b.faction === f) for (const q of b.queue) if (q.unit && test(q.unit)) n++;
  return n;
}

// ---- difesa: guarnigione contro chi entra in casa, torri, richiamo dell'ondata

function aiDefend(f, intel, stepCost) {
  const ai = f.ai, L = ai.level, c = f.castle;
  if (!intel.danger) return;
  ai.attackedT = G.time;
  const at = intel.dangerAt;
  // chi è a casa senza nemici a tiro va incontro agli invasori (con level.rally; altrimenti resta di guardia al suo posto)
  const free = L.rally ? intel.home.filter(s => s.def.dmg && !s.def.onlyBuildings && !s.target && s.order.k === 'idle' && !(ai.wave && ai.wave.ids.has(s.id))) : [];
  if (free.length) orderUnits(f, free.map(s => s.id), 'amove', at.x, at.y);
  // l'ondata torna se la minaccia supera ciò che la base può reggere, a meno che il castello nemico stia per cadere
  const w = ai.wave;
  if (L.recall && w && intel.danger > 0.8 * intel.homeValue + 250) {
    const tc = w.target.castle;
    if (!w.target.alive || !tc || tc.dead || tc.hp > 0.3 * tc.maxHp) {
      orderUnits(f, [...w.ids], 'amove', at.x, at.y);
      ai.wave = null;
      ai.waveT = Math.max(ai.waveT, 30);
    }
  }
  // una torre verso gli invasori, se l'era lo permette, non ce ne sono abbastanza e la pietra non serve al piano
  if (!f.sites.tower && (f.has.tower || 0) < L.towers && f.res.stone - BUILD_DEF.tower.cost.stone >= ((stepCost && stepCost.stone) || 0)) aiBuild(f, 'tower', at);
}

// ---- ondate: un gruppo compatto che marcia a tappe aspettando i più lenti, poi l'assalto al castello;
// se viene decimato torna a casa ad aspettare i rinforzi.

// Soldati a casa liberi di partire: tutti quelli fermi tranne la guarnigione, una quota dell'esercito (di più se la
// base è stata colpita da poco). Restano prima gli arcieri, partono prima i più forti.
function aiSpare(f, intel) {
  const ai = f.ai, L = ai.level;
  const ready = intel.home.filter(s => s.order.k === 'idle' && !s.target && !(ai.wave && ai.wave.ids.has(s.id)));
  const fighters = ready.filter(s => s.def.dmg), monks = ready.filter(s => s.def.heal);
  let total = 0, busy = 0;
  for (const s of f.soldiers) if (!s.dead && aiFighter(s.type)) total++;
  for (const s of intel.home) if (s.def.dmg && !fighters.includes(s) && !(ai.wave && ai.wave.ids.has(s.id))) busy++;
  const keep = Math.max(2, Math.round(total * (L.garrison + (G.time - (ai.attackedT || -999) < 90 ? 0.15 : 0))));
  fighters.sort((a, b) => (UNIT_DEF[a.type].range > 200 ? 1 : 0) - (UNIT_DEF[b.type].range > 200 ? 1 : 0) || aiWorth(b) - aiWorth(a));
  return { squad: fighters.slice(0, Math.max(0, fighters.length - Math.max(0, keep - busy))), monks, total };
}

function aiLaunch(f, intel) {
  const ai = f.ai, L = ai.level;
  const { squad, monks, total } = aiSpare(f, intel);
  if (squad.length < ai.waveNeed) return false;
  let power = 0;
  for (const s of squad) power += aiWorth(s);
  const target = chooseTarget(f, intel, power);
  if (!target) return false;
  const def = intel.foes.get(target).defense;
  // all'attacco solo con una forza adeguata alla difesa del bersaglio; con l'esercito al completo, o dopo
  // un'attesa lunga, ci si accontenta di meno (altrimenti regni di forza simile non si attaccano mai)
  const full = total + queued(f, t => t !== 'pawn') >= L.army, late = G.time - ai.dueT;
  if (power < L.odds * def * Math.max(0.5, (full ? 0.6 : 1) - late / 300)) return false;
  squad.push(...monks.slice(0, Math.ceil(monks.length * 0.7)));
  ai.wave = { ids: new Set(squad.map(s => s.id)), target, start: power, phase: 'march', legT: -99, t0: G.time };
  ai.dueT = null;
  ai.target = target;
  target.raidedT = G.time;
  ai.waveT = rand(...L.gap);
  ai.waveNeed = Math.min(Math.round(L.army * (1 - L.garrison)) - 1, ai.waveNeed + 2);
  if (target.human) { G.announce(f.name + ' ti attacca!', f, 3.5, target); emit(['play', target.idx, 'horn']); }
  return true;
}

function aiMarch(f, intel) {
  const ai = f.ai, L = ai.level, c = f.castle, w = ai.wave;
  const units = [];
  for (const id of w.ids) { const u = G.index.get(id); if (u && !u.dead && u.faction === f) units.push(u); else w.ids.delete(id); }
  const fighters = units.filter(u => u.def.dmg);
  const home = () => { orderUnits(f, units.map(u => u.id), 'amove', c.x, c.y1 + 130); ai.wave = null; };
  if (!fighters.length) { if (units.length) home(); else ai.wave = null; return; }
  let value = 0, cx = 0, cy = 0;
  for (const u of fighters) { value += aiWorth(u); cx += u.x; cy += u.y; }
  cx /= fighters.length; cy /= fighters.length;
  if (!w.target.alive) { // bersaglio caduto: avanti sul più vicino se l'ondata è ancora in forze, altrimenti a casa
    const next = value >= 0.5 * w.start ? nearestEnemyFaction(f) : null;
    if (!next || Math.hypot(next.castle.x - cx, next.castle.y - cy) > 2600) { home(); return; }
    w.target = ai.target = next; w.phase = 'march'; w.legT = -99;
  }
  const tc = w.target.castle, dT = Math.hypot(tc.x - cx, tc.y - cy);
  // decimata: ritirata (senza fermarsi a combattere), a meno che il castello nemico sia quasi abbattuto
  if (value < L.retreat * w.start && tc.hp > 0.3 * tc.maxHp) {
    orderUnits(f, units.map(u => u.id), 'move', c.x, c.y1 + 130);
    ai.wave = null;
    ai.waveT = Math.max(ai.waveT, 40);
    return;
  }
  if (w.phase === 'march') {
    if (dT < 900) { w.phase = 'assault'; orderUnits(f, units.map(u => u.id), 'amove', tc.x, tc.y1 + 30); return; }
    // tappa successiva quando quasi tutti sono arrivati alla precedente (o è passato troppo tempo)
    const waiting = fighters.filter(u => u.order.k === 'idle' && !u.target).length;
    if (waiting >= 0.75 * fighters.length || G.time - w.legT > 35) {
      const step = Math.min(700, dT - 500);
      const p = World.nearestPassable(cx + (tc.x - cx) / dT * step, cy + (tc.y - cy) / dT * step);
      orderUnits(f, units.map(u => u.id), 'amove', p.x, p.y);
      w.legT = G.time;
    }
    return;
  }
  // assalto: chi è rimasto fermo senza nemici torna sul castello nemico
  const idle = units.filter(u => u.order.k === 'idle' && !u.target && Math.hypot(u.x - tc.x, u.y - tc.y) > 200);
  if (idle.length) orderUnits(f, idle.map(u => u.id), 'amove', tc.x, tc.y1 + 30);
  // rinforzi (con level.reinforce): i soldati liberi a casa, in gruppo di almeno tre, raggiungono l'assalto
  if (L.reinforce && !intel.danger) {
    const { squad } = aiSpare(f, intel);
    if (squad.length >= 3) {
      for (const s of squad) { w.ids.add(s.id); w.start += aiWorth(s); }
      orderUnits(f, squad.map(s => s.id), 'amove', tc.x, tc.y1 + 30);
    }
  }
}

// Punti da conquistare: ogni tanto, a casa tranquilla e senza ondate in corso, una squadra di tre soldati liberi
// va a occupare il punto non suo più vicino al castello (più lontano man mano che il regno avanza d'era).
function aiCapture(f, intel) {
  const ai = f.ai, c = f.castle;
  if (!ai.level.capture || ai.wave || intel.danger || G.time < ai.captureT) return;
  ai.captureT = G.time + 25;
  const { squad } = aiSpare(f, intel);
  if (squad.length < 3) return;
  let best = null, bd = 2200 + 600 * f.age;
  for (const p of G.points) {
    const d = Math.hypot(p.x - c.x, p.y - c.y);
    if (p.owner !== f && d < bd) { bd = d; best = p; }
  }
  if (best) orderUnits(f, squad.slice(0, 3).map(s => s.id), 'amove', best.x, best.y1 + 40);
}

function aiThink(f, dt) {
  const ai = f.ai, L = ai.level, c = f.castle;
  aiAssignPawns(f, dt);
  const open = Object.values(f.sites).reduce((a, b) => a + b, 0);
  // fattorie e torri del piano oltre il numero voluto dal livello si saltano
  const skip = AI_PLAN[ai.step] && { farm: L.farms, tower: L.towers }[AI_PLAN[ai.step][1]];
  if (skip !== undefined && (f.has[AI_PLAN[ai.step][1]] || 0) + (f.sites[AI_PLAN[ai.step][1]] || 0) >= skip) ai.step++;
  const step = AI_PLAN[ai.step], stepCost = step ? (step[0] === 'build' ? BUILD_DEF : TECH_DEF)[step[1]].cost : null;

  // case e accampamenti quando i posti stringono (e ne servono ancora), poi la ricostruzione di ciò che è andato perduto
  const comingPawns = queued(f, t => t === 'pawn'), comingTroops = queued(f, t => t !== 'pawn');
  if (!f.sites.house && f.pawnCap < PAWN_MAX && f.pawnN + comingPawns >= f.pawnCap - 2 && L.pawns[f.age - 1] > f.pawnCap - 2) aiBuild(f, 'house');
  else if (!f.sites.camp && f.troopCap < TROOP_MAX && f.troopN + comingTroops >= f.troopCap - 2 && L.army > f.troopCap - 2) aiBuild(f, 'camp');
  else if (open < 2) {
    const lost = ['farm', 'barracks', 'depot'].find(k => !f.has[k] && !f.sites[k] && AI_PLAN.findIndex(s => s[1] === k) < ai.step);
    // una fattoria ogni due o tre pedoni voluti sul cibo (una rende quanto 2,5 pedoni ne raccolgono), più le pecore dei prati
    const farms = (f.has.farm || 0) + (f.sites.farm || 0);
    if (lost) aiBuild(f, lost);
    else if (farms && farms < L.farms && !f.sites.farm && farms * 2.5 + 2 < aiJobMix(f).meat * f.pawns.length) aiBuild(f, 'farm');
    else if (step && step[0] === 'build') { if (aiBuild(f, step[1])) ai.step++; }
  }
  if (step && step[0] === 'tech') {
    // prima le ricerche che la precedono nell'albero, se mancano
    let id = step[1];
    while (TECH_DEF[id].req && !f.tech[TECH_DEF[id].req]) id = TECH_DEF[id].req;
    const t = TECH_DEF[id], b = freeBuilding(f, t.at);
    if (f.tech[step[1]] || f.busy[step[1]]) ai.step++;
    else if (f.busy[id]) { /* la precedente è in corso */ }
    else if (!b) { if (!f.sites[t.at] && open < 2) aiBuild(f, t.at); }
    else if (b.queue.length < 2 && execCommand(f, 'research', b.id, id) === null && id === step[1]) ai.step++;
  }

  const intel = aiIntel(f);
  ai.mix = aiWant(f, intel);
  aiDefend(f, intel, stepCost);
  // risorse che avanzano: altre fabbriche (fino al tetto del livello) per il soldato più voluto
  if (open < 2 && ai.mix && RES_KINDS.every(k => k === 'stone' || f.res[k] > 500)) {
    const top = Object.keys(ai.mix).sort((a, b) => ai.mix[b] - ai.mix[a])[0], kind = UNIT_DEF[top].from;
    if ((f.has[kind] || 0) + (f.sites[kind] || 0) < L.factories) aiBuild(f, kind);
  }

  // addestramento: prima i pedoni, poi l'esercito (senza togliere risorse al prossimo passo del piano,
  // salvo con il nemico in casa); ognuno finché c'è posto per lui
  // (se mancano i posti dei pedoni, in attesa della casa si addestrano le truppe: i due tetti sono separati)
  if (f.pawnN + comingPawns < Math.min(L.pawns[f.age - 1], f.pawnCap) && !(intel.danger > intel.homeValue && f.soldiers.length < 4)) {
    if (c.queue.length < 2) execCommand(f, 'train', c.id, 'pawn');
  } else if (f.troopN + comingTroops < Math.min(L.army, f.troopCap)) {
    // la scelta migliore; se da un po' non si riesce a pagarla, la migliore fra quelle che si possono pagare
    let t = aiNextUnit(f, intel);
    if (t && canAfford(f, UNIT_DEF[t].cost)) ai.lackT = G.time;
    else if (G.time - (ai.lackT || 0) > 12) t = aiNextUnit(f, intel, true);
    const d = t && UNIT_DEF[t], b = d && freeBuilding(f, d.from);
    // nella prima era l'esercito non tocca ciò che serve all'Età Feudale
    const hold = f.age === 1 && !f.busy.age2 ? TECH_DEF.age2.cost : stepCost;
    const saving = d && hold && !intel.danger && f.soldiers.length >= 3 && RES_KINDS.some(k => (d.cost[k] || 0) > 0 && f.res[k] - d.cost[k] < (hold[k] || 0));
    if (b && !saving && b.queue.length < 2) execCommand(f, 'train', b.id, t);
  }
  // al mercato: vende ciò che abbonda quando l'oro scarseggia; con level.trade compra anche il cibo
  // quando manca e l'oro avanza
  const market = f.has.market && freeBuilding(f, 'market');
  if (market) {
    const extra = ['wood', 'meat', 'stone'].find(k => f.res[k] > (f.res.gold < 80 ? 450 : 700) && sellPrice(f, k) >= 40);
    if (extra && f.res.gold < (L.trade ? 250 : 80)) execCommand(f, 'trade', market.id, 'sell', extra);
    else if (L.trade && f.res.meat < 150 && f.res.gold > 450 && buyPrice(f, 'meat') <= 170) execCommand(f, 'trade', market.id, 'buy', 'meat');
  }

  // ondate: una alla volta, quando il conto alla rovescia è finito, la base è tranquilla e la forza basta
  ai.waveT -= dt;
  if (ai.wave) aiMarch(f, intel);
  else if (ai.waveT <= 0 && !intel.danger) { if (ai.dueT == null) ai.dueT = G.time; aiLaunch(f, intel); }
  aiCapture(f, intel);
  // chi è fermo lontano da casa senza far parte dell'ondata (inseguimenti, resti di un'ondata, chi ha appena preso
  // un punto) torna al castello; non chi sta ancora occupando un punto
  const stray = f.soldiers.filter(s => !s.dead && s.order.k === 'idle' && !s.target && !(ai.wave && ai.wave.ids.has(s.id)) &&
    Math.hypot(s.x - c.x, s.y - c.y) > 900 && !G.points.some(p => p.owner !== f && Math.hypot(p.x - s.x, p.y - s.y) < CAPTURE_RADIUS));
  if (stray.length) orderUnits(f, stray.map(s => s.id), 'amove', c.x, c.y1 + 130);
}

function updateFaction(f, dt) {
  if (!f.alive) return;
  if (!f.human) for (const k in f.res) f.res[k] += f.ai.level.trickle * dt;
  else if ((f.seeT -= dt) <= 0) { f.seeT = 0.25; explore(f); }
  if ((f.tickT -= dt) <= 0) {
    f.tickT = 1;
    census(f);
    if (f.human) f.rate = estimateRates(f);
    for (const k in f.trades) f.trades[k] = Math.max(0, f.trades[k] - 0.04); // i prezzi tornano normali in qualche minuto
  }
  if (!f.human && (f.ai.thinkT -= dt) <= 0) { f.ai.thinkT = f.ai.level.think; aiThink(f, f.ai.level.think); }
}

function eliminateFaction(f, by) {
  if (!f.alive) return;
  f.alive = false;
  for (const u of G.units) if (u.faction === f && !u.dead) killUnit(u, null);
  for (const b of G.buildings) if (b.faction === f && !b.dead) destroyBuilding(b, null);
  for (const p of G.points) { if (p.owner === f) p.owner = null; if (p.by === f) { p.by = null; p.t = 0; } } // i suoi punti tornano liberi
  G.announce(f.name + ' è caduto!', f, 4);
  if (by && by.human) emit(['play', by.idx, 'win']);
  G.checkEnd();
}
