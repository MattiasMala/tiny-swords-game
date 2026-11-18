'use strict';
// Multigiocatore in rete locale. Il browser dell'host fa girare la simulazione e trasmette istantanee
// dello stato; i client inviano i comandi dei giocatori e disegnano ciò che ricevono. Selezione,
// telecamera e nebbia di guerra sono locali a ogni giocatore. server.py inoltra soltanto.
//
// Messaggi client -> host:  join, pick (lobby) · c (comando: vedi execCommand)
// Messaggi host -> client:  lobby, no, init (mondo e fazioni) · s (istantanea)

const SNAP_DT = 0.05;         // secondi fra le istantanee dell'host
const SLOW_EVERY = 5;         // edifici e giacimenti viaggiano ogni N istantanee (subito se cambia la mappa)
const MAX_BUFFERED = 1 << 18; // byte in attesa d'invio oltre cui l'host salta un'istantanea
const NO_SERVER = 'Multigiocatore: avvia "python3 server.py" e apri l\'indirizzo che stampa.';

const Net = {
  role: null,       // 'host' | 'client', dalla risposta del server in poi
  playing: false,   // partita in rete in corso
  ws: null, id: 0, urls: [], pingT: 0,
  lobby: [],        // [{ id, color }]; il primo è l'host
  level: 'normal',  // difficoltà dell'IA, scelta dall'host
  // host
  events: [],       // eventi di presentazione in attesa della prossima istantanea
  snapT: 0, snapN: 0, mapV: -1, clock: null,
  // client
  maps: {},         // id -> oggetto locale, per tipo di entità
  ready: false,     // prima istantanea ricevuta

  get hosting() { return this.role === 'host' && this.playing; },
  get remote() { return this.role === 'client' && this.playing; },

  // ------------------------------------------------------------ connessione

  connect() {
    if (this.ws) return;
    Menu.note = '';
    if (location.protocol === 'file:') { Menu.note = NO_SERVER; return; }
    const ws = this.ws = new WebSocket((location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + '/ws');
    ws.onmessage = e => { if (this.ws === ws) this.receive(e.data); };
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.quit(this.role === 'client' ? 'Connessione con l\'host persa.' : this.role ? 'Connessione col server persa.' : NO_SERVER);
    };
    this.pingT = setInterval(() => { if (ws.readyState === 1) ws.send(''); }, 5000); // segnale di vita per il server
  },

  // Chiude la sessione di rete; la schermata torna al menu principale.
  leave() {
    clearInterval(this.pingT);
    if (this.clock) this.clock.terminate();
    const ws = this.ws;
    Object.assign(this, { ws: null, role: null, playing: false, ready: false, clock: null, lobby: [], events: [], maps: {} });
    if (ws) ws.close();
    Menu.screen = 'main';
  },

  // Sessione finita non per scelta del giocatore: torna al menu con un avviso.
  quit(note) {
    if (this.playing) G.toMenu(); else this.leave();
    Menu.note = note;
  },

  // L'host indirizza i messaggi: a un client (il suo id) o a tutti.
  send(m, to = '*') {
    if (!this.ws || this.ws.readyState !== 1) return;
    const text = JSON.stringify(m);
    this.ws.send(this.role === 'host' ? to + '|' + text : text);
  },

  receive(data) {
    if (this.role === 'host') {
      const cut = data.indexOf('|'), from = +data.slice(0, cut), m = JSON.parse(data.slice(cut + 1));
      if (from) this.fromClient(from, m);
      else if (m.t === 'leave') this.playerLeft(m.id);
      return;
    }
    const m = JSON.parse(data);
    if (m.t === 'hello') this.hello(m);
    else if (m.t === 'lobby') { this.lobby = m.players; this.level = m.level; }
    else if (m.t === 'no') this.quit(m.why);
    else if (m.t === 'hostleft') this.quit('L\'host ha chiuso la partita.');
    else if (m.t === 'init') this.loadMatch(m);
    else if (m.t === 's' && this.playing) this.applySnapshot(m);
  },

  // Il server assegna il ruolo: il primo che entra è l'host.
  hello(m) {
    this.id = m.id;
    this.urls = m.urls;
    this.role = m.host ? 'host' : 'client';
    if (m.host) { this.lobby = [{ id: m.id, color: Menu.color }]; this.level = Menu.level; }
    else this.send({ t: 'join', color: Menu.color });
    Menu.screen = 'lobby';
  },

  // ------------------------------------------------------------ lobby

  shareLobby() { this.send({ t: 'lobby', players: this.lobby, level: this.level }); },

  // Scelta del regno del giocatore locale.
  choose(color) {
    if (this.role === 'host') this.pick(this.id, color);
    else this.send({ t: 'pick', color });
  },
  // Host: applica la scelta di un giocatore, se il regno è libero.
  pick(id, color) {
    const p = this.lobby.find(p => p.id === id);
    if (!p || this.playing || !COLORS.includes(color) || this.lobby.some(o => o.color === color)) return;
    p.color = color;
    this.shareLobby();
  },
  setLevel(level) {
    if (this.role !== 'host' || !AI_LEVEL[level]) return;
    this.level = level;
    this.shareLobby();
  },

  // ------------------------------------------------------------ host

  fromClient(id, m) {
    if (m.t === 'join') {
      if (this.lobby.some(p => p.id === id)) return;
      const why = this.playing ? 'Partita già in corso.' : this.lobby.length >= COLORS.length ? 'Partita al completo.' : null;
      if (why) { this.send({ t: 'no', why }, id); return; }
      const taken = this.lobby.map(p => p.color);
      this.lobby.push({ id, color: COLORS.includes(m.color) && !taken.includes(m.color) ? m.color : COLORS.find(c => !taken.includes(c)) });
      this.shareLobby();
    } else if (m.t === 'pick') {
      this.pick(id, m.color);
    } else if (m.t === 'c' && this.playing && typeof m.c === 'string' && Array.isArray(m.a)) {
      const f = G.factions.find(f => f.conn === id);
      if (!f || !f.alive) return;
      let why = null;
      try { why = execCommand(f, m.c, ...m.a.slice(0, 5)); } catch (err) { console.warn('comando non valido da', id, m, err); }
      if (why) emit(['toast', f.idx, why]);
    }
  },

  playerLeft(id) {
    const i = this.lobby.findIndex(p => p.id === id);
    if (i < 0) return;
    this.lobby.splice(i, 1);
    if (!this.playing) { this.shareLobby(); return; }
    const f = G.factions.find(f => f.conn === id);
    if (!f || !f.alive) return;
    // il regno resta in gioco, guidato dall'IA: i suoi soldati tornano di guardia
    Object.assign(f, { human: false, conn: 0 });
    for (const u of f.soldiers) idle(u);
    G.announce(`${f.name}: il giocatore è uscito, ora lo guida l'IA`, f, 4);
  },

  startMatch() {
    Object.assign(this, { playing: true, events: [], snapT: 0, snapN: 0, mapV: -1 });
    G.start({ players: this.lobby.map(p => ({ color: p.color, conn: p.id, local: p.id === this.id })), level: this.level });
    this.send({ t: 'init', world: World.serialize(), level: this.level, factions: G.factions.map(f => [f.color, f.human ? 1 : 0, f.conn]) });
    // i timer di un worker non vengono rallentati quando la scheda è in secondo piano
    this.clock = new Worker(URL.createObjectURL(new Blob(['setInterval(() => postMessage(0), 50)'])));
    this.clock.onmessage = stepUnseen;
  },

  hostTick(dt) {
    if ((this.snapT += dt) < SNAP_DT) return;
    this.snapT = 0;
    if (this.ws.bufferedAmount > MAX_BUFFERED) return; // rete in affanno: gli eventi aspettano la prossima
    const slow = this.snapN++ % SLOW_EVERY === 0 || this.mapV !== World.version;
    this.mapV = World.version;
    this.send(this.snapshot(slow));
  },

  // Stato del mondo in forma compatta: una riga (array) per entità. Vedi applySnapshot per la lettura.
  snapshot(slow) {
    const r = Math.round, r2 = v => Math.round(v * 100) / 100, live = o => !o.dead;
    const bits = set => TECH_IDS.reduce((b, id, i) => (set[id] ? b | 1 << i : b), 0);
    const s = {
      t: 's', time: r2(G.time),
      u: G.units.filter(live).map(u => [
        u.id, u.faction.idx, UNIT_TYPES.indexOf(u.type), r(u.x), r(u.y), Math.ceil(u.hp), u.maxHp, u.anim, r2(u.animT),
        (u.flip ? 1 : 0) | (u.hurtT > 0 ? 2 : 0) | (u.inside ? 4 : 0) | (u.job + 1) << 3, u.act, Math.floor(u.faith || 0),
      ]),
      sh: G.sheep.filter(live).map(o => [o.id, r(o.x), r(o.y), o.state, o.flip ? 1 : 0]),
      a: G.arrows.map(o => [o.id, r(o.x), r(o.y), r2(o.ang), o.faction.idx, o.rock ? 1 : 0]),
      p: G.pickups.map(o => [o.id, r(o.x), r(o.y - o.hop), o.kind, o.life < 5 ? 1 : 0, o.chest ? 1 : 0]),
      f: G.factions.map(f => [
        f.alive ? 1 : 0, f.human ? 1 : 0, Math.floor(f.res.wood), Math.floor(f.res.gold), Math.floor(f.res.meat), Math.floor(f.res.stone),
        f.pawnCap, f.troopCap, f.age, bits(f.tech), bits(f.busy), f.kills, f.razed, r2(f.trades.wood), r2(f.trades.meat), r2(f.trades.stone),
        f.rate.wood, f.rate.gold, f.rate.meat, f.rate.stone,
      ]),
      e: this.events,
    };
    this.events = [];
    if (slow) {
      s.b = G.buildings.filter(live).map(b => [
        b.id, b.faction.idx, BUILD_KINDS.indexOf(b.kind), b.cx, b.cy, b.variant, r(b.hp), b.maxHp, b.done ? 1 : 0, r2(b.progress),
        b.queue.map(q => [q.unit ? UNIT_TYPES.indexOf(q.unit) : 100 + TECH_IDS.indexOf(q.tech), r2(q.t), q.total]),
        r(b.rally.x), r(b.rally.y), b.rally.t && !b.rally.t.dead ? b.rally.t.id : 0,
      ]);
      s.g = G.golds.map(g => g.amount);
      s.q = G.stones.map(q => q.amount);
      s.cp = G.points.map(p => [p.owner ? p.owner.idx : -1, p.by ? p.by.idx : -1, p.t]); // punti: di chi sono, chi li sta prendendo
      s.st = [];
      G.trees.forEach((t, i) => { if (t.wood <= 0) s.st.push(i); });
    }
    return s;
  },

  // ------------------------------------------------------------ client

  loadMatch(m) {
    Object.assign(this, { playing: true, ready: false, maps: {}, level: m.level });
    G.reset();
    World.load(m.world);
    G.scenery();
    G.demo = false;
    G.setup = null;
    G.factions = m.factions.map(([color, human, conn], i) => Object.assign(makeFaction(color, World.bases[i], !!human, m.level), { idx: i, conn }));
    G.player = G.factions.find(f => f.conn === this.id) || null;
    if (!G.player) { this.quit('Partita già in corso.'); return; } // entrato in lobby troppo tardi per essere incluso
    Ctl.reset();
    Fog.reset(true);
  },

  // Allinea gli oggetti locali di un tipo alle righe dell'istantanea (riga[0] = id):
  // crea i nuovi, aggiorna gli esistenti, scarta quelli che l'host non ha più.
  mirror(name, rows, create, update) {
    const old = this.maps[name] || new Map(), map = this.maps[name] = new Map();
    G[name] = rows.map(row => {
      let o = old.get(row[0]);
      if (!o) { o = create(row); G.index.set(row[0], o); }
      update(o, row);
      map.set(row[0], o);
      return o;
    });
    for (const [id, o] of old) if (!map.has(id)) { o.dead = true; G.index.delete(id); }
  },

  applySnapshot(s) {
    if (Math.abs(s.time - G.time) > 0.25) G.time = s.time;

    s.f.forEach((row, i) => {
      const [alive, human, wood, gold, meat, stone, pawnCap, troopCap, age, tech, busy, kills, razed, tWood, tMeat, tStone, rWood, rGold, rMeat, rStone] = row;
      const f = G.factions[i];
      Object.assign(f, {
        alive: !!alive, human: !!human, res: { wood, gold, meat, stone }, pawnCap, troopCap, age, kills, razed,
        trades: { wood: tWood, meat: tMeat, stone: tStone }, rate: { wood: rWood, gold: rGold, meat: rMeat, stone: rStone }, pawns: [], soldiers: [],
      });
      TECH_IDS.forEach((id, j) => {
        if (tech >> j & 1 && !f.tech[id]) { f.tech[id] = true; techMods(f.mods, id); } // le statistiche mostrate seguono le ricerche
        if (busy >> j & 1) f.busy[id] = true; else delete f.busy[id];
      });
    });

    this.mirror('units', s.u, ([id, fi, ti, x, y]) => {
      const type = UNIT_TYPES[ti];
      return {
        id, isUnit: true, faction: G.factions[fi], type, def: UNIT_DEF[type], x, y, sy: y, draw: drawUnit,
        anim: 'idle', hostAnim: '', animT: 0, state: 'idle', flip: false, hurtT: 0, hp: 1, maxHp: 1, act: 0, dead: false,
      };
    }, (o, [, , , x, y, hp, maxHp, anim, animT, flags, act, faith]) => {
      glideTo(o, x, y);
      o.hp = hp; o.maxHp = maxHp; o.act = act; o.faith = faith; // faith = riserva di cura dei monaci
      if (o.hostAnim !== anim) { // animazione cambiata sull'host: riparte dal suo stesso punto
        o.hostAnim = o.anim = anim; o.animT = animT;
        o.state = anim.startsWith('attack') ? 'attack' : 'idle';
      }
      o.flip = !!(flags & 1); o.hurtT = flags & 2 ? 1 : 0;
      o.inside = !!(flags & 4); // al riparo in un edificio: non si disegna e non si seleziona sulla mappa
      o.job = (flags >> 3 & 7) - 1; // risorsa a cui il pedone è assegnato (vedi jobOf)
      (o.type === 'pawn' ? o.faction.pawns : o.faction.soldiers).push(o);
    });
    this.mirror('sheep', s.sh, ([id, x, y]) => ({ id, kind: 'sheep', x, y, sy: y, state: '', animT: 0, flip: false, users: 0, dead: false, draw: drawSheep }),
      (o, [, x, y, state, flip]) => {
        glideTo(o, x, y);
        if (o.state !== state) { o.state = state; o.animT = 0; }
        o.flip = !!flip;
      });
    this.mirror('arrows', s.a, ([id, x, y, , , rock]) => ({ id, x, y, ang: 0, faction: null, rock: !!rock }),
      (o, [, x, y, ang, fi]) => { glideTo(o, x, y); o.ang = ang; o.faction = G.factions[fi]; });
    this.mirror('pickups', s.p, ([id, x, y, kind, , chest]) => ({ id, kind, chest: !!chest, x, y, sy: y, hop: 0, t: 0, life: 99, draw: drawPickup }),
      (o, [, x, y, , fading]) => { glideTo(o, x, y); if (fading && o.life > 5) o.life = 5; });
    if (s.b) this.applyMap(s);

    for (const ev of s.e) showEvent(ev);
    if (!this.ready) {
      this.ready = true;
      G.state = 'play';
      Ctl.lookAt(G.player.castle.x, G.player.castle.y);
      Fog.reveal(G.player.castle.x, G.player.castle.y, START_SIGHT);
    }
    G.checkEnd();
  },

  // Edifici, giacimenti e ceppi; da questi si ricava anche quali celle sono occupate.
  applyMap(s) {
    this.mirror('buildings', s.b, ([id, fi, ki, cx, cy, variant]) => ({ ...buildingShape(BUILD_KINDS[ki], cx, cy), id, faction: G.factions[fi], variant }),
      (o, [, , , cx, cy, , hp, maxHp, done, progress, queue, rx, ry, rt]) => {
        if (o.cx !== cx || o.cy !== cy) Object.assign(o, buildingPlace(o.kind, cx, cy)); // spostato dal suo giocatore
        Object.assign(o, { hp, maxHp, done: !!done, progress, rally: { x: rx, y: ry, t: rt ? G.index.get(rt) || null : null } });
        o.queue = queue.map(([code, t, total]) => (code >= 100 ? { tech: TECH_IDS[code - 100], t, total } : { unit: UNIT_TYPES[code], t, total }));
      });
    for (const b of G.buildings) if (b.kind === 'castle') b.faction.castle = b;
    s.g.forEach((amount, i) => { G.golds[i].amount = amount; });
    s.q.forEach((amount, i) => { G.stones[i].amount = amount; });
    s.cp.forEach(([owner, by, t], i) => { Object.assign(G.points[i], { owner: G.factions[owner] || null, by: G.factions[by] || null, t }); });
    const stumps = new Set(s.st);
    G.trees.forEach((t, i) => { t.wood = stumps.has(i) ? 0 : TREE_WOOD; });
    World.resetBlock();
    for (const b of G.buildings) blockBuilding(b, true);
  },

  clientUpdate(dt) {
    if (!this.ready) return;
    G.time += dt;
    const k = dt / (SNAP_DT * 1.25); // un po' più lento dell'intervallo: assorbe le istantanee in ritardo
    for (const list of [G.units, G.sheep, G.arrows, G.pickups]) {
      for (const o of list) {
        o.glide = Math.min(1, o.glide + k);
        o.x = o.fromX + (o.toX - o.fromX) * o.glide;
        o.y = o.fromY + (o.toY - o.fromY) * o.glide;
      }
    }
    for (const u of G.units) u.animT += dt;
    for (const o of G.sheep) o.animT += dt;
    for (const p of G.pickups) { p.t += dt; p.life -= dt; }
    for (const b of G.buildings) { const q = b.queue[0]; if (q && b.done) q.t = Math.min(q.total, q.t + dt); } // la barra avanza anche fra due istantanee
    twinkle(dt);
    updateFx(dt);
    updateView(dt);
  },
};

// Nuova posizione arrivata dall'host: l'oggetto ci scivola da dove si trova ora.
function glideTo(o, x, y) {
  o.fromX = o.x; o.fromY = o.y; o.toX = x; o.toY = y; o.glide = 0;
}
