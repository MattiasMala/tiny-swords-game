'use strict';
// Mappa: generazione dell'isola, tile, percorribilità e campi di distanza per il movimento.

// A ogni partita l'isola cambia: forma, golfi e laghi, posto dei cinque castelli, numero e posto degli altopiani,
// dei punti da conquistare e dei grandi giacimenti.
const BASE_COUNT = 5;
const BASE_GAP = 64;                // celle minime fra due castelli
const MAP_AREA = 144 * 96;          // celle della mappa di una volta: risorse, alture e oggetti sono contati in proporzione
const BASE_CLEAR = { x: 9, y: 7 }; // semi-lati della zona tenuta sgombra attorno a ogni base
// Scorte minime attorno a ogni base perché la partenza sia equa: [lista, raggio in celle, quante].
// Entro 14 celle si è dentro la zona già esplorata a inizio partita (START_SIGHT), dove i pedoni lavorano da soli.
const BASE_STOCK = [['trees', 20, 12], ['golds', 22, 4], ['stones', 22, 2], ['trees', 14, 6], ['golds', 14, 2], ['stones', 14, 1]];
// stair: 1 = scala sinistra bassa, 2 = sinistra alta, 3 = destra bassa, 4 = destra alta
const DIRS8 = [[1, 0, 1], [-1, 0, 1], [0, 1, 1], [0, -1, 1], [1, 1, 1.414], [-1, 1, 1.414], [1, -1, 1.414], [-1, -1, 1.414]];
const FIELD_CACHE = 128;  // campi di distanza tenuti (221 KB l'uno): con 320 se ne ricalcolava il 3-5% in meno, per 42 MB in più
const FIELD_REFRESH = 2; // campi superati ma ancora buoni (vedi getField) ricalcolati a ogni passo di simulazione
const FIELD_NEW = 2;     // unità di un gruppo che a ogni passo lasciano la strada comune per quella del proprio posto (vedi march)
const CROSS_COST = 30;   // decimi di cella in più per passare su una cella di un proprio edificio (vedi getField)
// Conquistabili sparsi sulla mappa, nell'ordine in cui vengono posati (vedi POINT_DEF), e ciò che cresce accanto a ognuno.
const POINT_MIX = ['mine', 'sawmill', 'outpost', 'quarry', 'ranch', 'outpost'];
const POINT_STOCK = { mine: 'gold', quarry: 'stone', sawmill: 'forest', ranch: 'meadow' };

const World = {
  W: 288, H: 192,
  version: 0,
  opened: 0,  // versione dell'ultima cella liberata: i campi più vecchi vanno rifatti subito
  refresh: 0, // ricalcoli ancora concessi in questo passo (vedi getField)
  fields: new Map(),
  moves: new Map(), // mosse permesse per regno e variante, finché la mappa non cambia (vedi movesFor)
  walls: [0, 0, 0, 0, 0, 0],
  tFlat: null, tTop: null, tShadow: null, // tile precalcolati per cella (vedi buildTiles)
  minimap: null,

  idx(x, y) { return y * this.W + x; },
  inside(x, y) { return x >= 0 && y >= 0 && x < this.W && y < this.H; },
  cellAt(px, py) {
    const x = Math.floor(px / TILE), y = Math.floor(py / TILE);
    return this.inside(x, y) ? y * this.W + x : -1;
  },
  cx(c) { return (c % this.W) * TILE + TILE / 2; },
  cy(c) { return Math.floor(c / this.W) * TILE + TILE / 2; },
  isLand(x, y) { return this.inside(x, y) && this.land[y * this.W + x] === 1; },
  // side = regno di chi si muove (indice + 1; 0 = nessuno). Su una cella ci si può fermare se è libera o se la
  // occupa una palizzata del proprio regno (passable); ci si può passare anche se la occupa un proprio edificio
  // (crossable): le truppe non restano chiuse fra le proprie case, quelle nemiche sì.
  passable(c, side = 0) {
    return c >= 0 && this.land[c] === 1 && this.cliff[c] === 0 && (this.block[c] === 0 || (side !== 0 && this.wallOf[c] === side));
  },
  crossable(c, side = 0) {
    return c >= 0 && this.land[c] === 1 && this.cliff[c] === 0 && (this.block[c] === 0 || (side !== 0 && this.ownOf[c] === side));
  },
  quad(c) { return [c, c + 1, c + this.W, c + this.W + 1]; }, // le quattro celle di ciò che ne occupa 2x2, dalla prima in alto a sinistra

  // Si cambia livello solo fra la metà bassa e quella alta della stessa scala.
  linkOK(a, b) {
    if (this.level[a] === this.level[b]) return true;
    return this.stair[a] !== 0 && this.stair[b] !== 0 && Math.abs(a - b) === this.W;
  },
  // Un passo da un punto a un altro. Sulle celle dei propri edifici si passa solo con cross (chi si muove sta
  // seguendo una strada che li attraversa, vedi getField) o se ci si è già dentro: di norma gli si gira attorno.
  stepOK(x0, y0, x1, y1, side = 0, cross = false) {
    const a = this.cellAt(x0, y0), b = this.cellAt(x1, y1);
    if (a === b) return true;
    if (!(cross || !this.passable(a, side) ? this.crossable(b, side) : this.passable(b, side))) return false;
    if (!this.crossable(a, side)) return true; // chi è finito in una cella bloccata può sempre uscirne
    return this.linkOK(a, b);
  },
  tryMove(e, dx, dy) {
    const r = 9, side = e.faction ? e.faction.idx + 1 : 0, cross = e.cross === true;
    if (dx && this.stepOK(e.x, e.y, e.x + dx + Math.sign(dx) * r, e.y, side, cross)) e.x += dx;
    if (dy && this.stepOK(e.x, e.y, e.x, e.y + dy + Math.sign(dy) * r, side, cross)) e.y += dy;
  },
  // Linea retta libera, senza tagliare attraverso gli edifici (nemmeno i propri).
  clearLine(x0, y0, x1, y1, side = 0) {
    const d = Math.hypot(x1 - x0, y1 - y0), n = Math.ceil(d / 24);
    let px = x0, py = y0;
    for (let i = 1; i <= n; i++) {
      const x = x0 + (x1 - x0) * i / n, y = y0 + (y1 - y0) * i / n;
      if (!this.stepOK(px, py, x, py, side, true) || !this.stepOK(x, py, x, y, side, true)) return false;
      px = x; py = y;
    }
    return true;
  },

  setBlock(cells, delta) {
    for (const c of cells) this.block[c] += delta;
    this.version++;
    if (delta < 0) this.opened = this.version;
  },
  // Segna di chi è l'edificio che occupa delle celle già bloccate con setBlock (side = regno, 0 = lo toglie);
  // wall = è una palizzata, su cui chi è di quel regno può anche fermarsi.
  setOwner(cells, side, wall) {
    for (const c of cells) {
      if (this.wallOf[c]) this.walls[this.wallOf[c]]--;
      this.ownOf[c] = side;
      this.wallOf[c] = wall ? side : 0;
      if (wall && side) this.walls[side]++;
    }
    this.version++;
    if (!side) this.opened = this.version;
  },

  // Centro della cella percorribile più vicina a un punto (il punto stesso se lo è già).
  nearestPassable(px, py) {
    const x0 = clamp(Math.floor(px / TILE), 0, this.W - 1), y0 = clamp(Math.floor(py / TILE), 0, this.H - 1);
    if (this.passable(this.idx(x0, y0))) return { x: px, y: py };
    for (let r = 1; r <= 10; r++) {
      let best = -1, bd = Infinity;
      for (let y = y0 - r; y <= y0 + r; y++) {
        for (let x = x0 - r; x <= x0 + r; x++) {
          if (Math.max(Math.abs(x - x0), Math.abs(y - y0)) !== r || !this.inside(x, y)) continue;
          const c = this.idx(x, y);
          if (!this.passable(c)) continue;
          const d = Math.hypot(this.cx(c) - px, this.cy(c) - py);
          if (d < bd) { bd = d; best = c; }
        }
      }
      if (best >= 0) return { x: this.cx(best), y: this.cy(best) };
    }
    return { x: px, y: py };
  },

  // ------------------------------------------------------------ campi di distanza

  neighborOK(c, x, y, dx, dy, side = 0, cross = false) {
    const nx = x + dx, ny = y + dy;
    if (!this.inside(nx, ny)) return -1;
    const n = ny * this.W + nx;
    if (!(cross ? this.crossable(n, side) : this.passable(n, side))) return -1;
    if (dx && dy) {
      const o1 = c + dx, o2 = c + dy * this.W;
      if (!(cross ? this.crossable(o1, side) && this.crossable(o2, side) : this.passable(o1, side) && this.passable(o2, side))) return -1;
      if (!this.linkOK(c, o1) || !this.linkOK(c, o2) || !this.linkOK(o1, n) || !this.linkOK(o2, n)) return -1;
    } else if (!this.linkOK(c, n)) return -1;
    return n;
  },

  // Mosse permesse da ogni cella per chi è del regno `side` (con cross anche attraverso i propri edifici), un bit
  // per direzione: ovest, est, nord, sud, poi le diagonali (stesso ordine di `step` in computeField). Una cella si raggiunge se è aperta
  // (terra, non parete, libera o del proprio regno) e sullo stesso livello o in cima/fondo alla stessa scala; in
  // diagonale solo se sono libere le due celle ai lati, sullo stesso livello lungo entrambi i giri.
  moveMask(side = 0, cross = false) {
    const W = this.W, H = this.H, N = W * H, { land, cliff, block, wallOf, level, stair } = this, own = cross ? this.ownOf : wallOf;
    const open = new Uint8Array(N), m = new Uint8Array(N);
    for (let c = 0; c < N; c++) open[c] = land[c] === 1 && cliff[c] === 0 && (block[c] === 0 || (side !== 0 && own[c] === side)) ? 1 : 0;
    const link = (a, b) => level[a] === level[b] || (stair[a] !== 0 && stair[b] !== 0 && (a - b === W || b - a === W));
    for (let c = 0; c < N; c++) {
      const x = c % W, y = (c - x) / W;
      const w = x > 0 && open[c - 1] === 1 && link(c, c - 1), e = x < W - 1 && open[c + 1] === 1 && link(c, c + 1);
      const n = y > 0 && open[c - W] === 1 && link(c, c - W), s = y < H - 1 && open[c + W] === 1 && link(c, c + W);
      let bits = (w ? 1 : 0) | (e ? 2 : 0) | (n ? 4 : 0) | (s ? 8 : 0);
      for (let k = 0; k < 4; k++) {
        const dx = k & 1 ? 1 : -1, dy = k & 2 ? W : -W;
        if (!(dx < 0 ? w : e) || !(dy < 0 ? n : s)) continue;
        const t = c + dx + dy;
        if (open[t] === 1 && link(c + dx, t) && link(c + dy, t)) bits |= 16 << k;
      }
      m[c] = bits;
    }
    return m;
  },
  // Le stesse, tenute da parte: calcolarle costa quanto un campo, e i campi chiesti durante la partita le
  // riusano finché la mappa non cambia (ogni cambio passa da setBlock, setOwner, resetBlock o load, che
  // aumentano version; la generazione invece le rifà a ogni campo, vedi computeField).
  movesFor(side, cross) {
    const key = side * 2 + (cross ? 1 : 0), e = this.moves.get(key);
    if (e && e.v === this.version && e.block === this.block) return e.m;
    const m = this.moveMask(side, cross);
    this.moves.set(key, { v: this.version, block: this.block, m });
    return m;
  },

  // Distanza a piedi di ogni cella dalle celle `seeds`, in decimi di cella (10 un passo dritto, 14 uno in
  // diagonale; Infinity dove non si arriva). Le palizzate del regno `side` non sbarrano la strada; con cross
  // nemmeno gli altri suoi edifici, ma ogni loro cella costa CROSS_COST in più. I costi sono interi piccoli,
  // quindi non serve una coda con priorità: le celle da visitare stanno in 64 secchi, uno per distanza
  // modulo 64, svuotati in ordine. moves = mosse permesse (moveMask): con quelle già pronte il campo costa
  // circa la metà che controllando i vicini cella per cella. reuse = un campo che non serve più: se c'è, il
  // nuovo ne prende la memoria invece di chiederne altra.
  computeField(seeds, side = 0, cross = false, moves = this.moveMask(side, cross), reuse = null) {
    const W = this.W, H = this.H, { block, wallOf } = this, extra = cross ? CROSS_COST : 0;
    const dist = (reuse !== null && reuse.length === W * H ? reuse : new Float32Array(W * H)).fill(Infinity);
    const buckets = this.buckets || (this.buckets = Array.from({ length: 64 }, () => []));
    const step = [-1, 1, -W, W, -1 - W, 1 - W, -1 + W, 1 + W]; // nell'ordine dei bit di moveMask
    let pending = 0, t = 0, nd = 0;
    for (const s of seeds) if (dist[s] !== 0) { dist[s] = 0; buckets[0].push(s); pending++; }
    for (let d = 0; pending > 0; d++) {
      const q = buckets[d & 63];
      while (q.length) {
        const c = q.pop();
        pending--;
        if (dist[c] !== d) continue; // raggiunta nel frattempo per una strada più corta
        const bits = moves[c];
        if (bits === 0) continue;
        for (let k = 0; k < 8; k++) {
          if ((bits & 1 << k) === 0) continue;
          t = c + step[k];
          // (una cella aperta e occupata che non è una palizzata è un proprio edificio: costa di più)
          nd = d + (k < 4 ? 10 : 14) + (extra !== 0 && block[t] !== 0 && wallOf[t] !== side ? extra : 0);
          if (nd < dist[t]) { dist[t] = nd; buckets[nd & 63].push(t); pending++; }
        }
      }
    }
    return dist;
  },

  // I campi di chi ha palizzate proprie sono diversi da quelli degli altri: si tengono separati.
  // Quando la mappa cambia i campi vanno rifatti, ma non tutti subito. Se da allora si è solo costruito (nessuna
  // cella liberata) il campo vecchio indica ancora strade valide, salvo le celle appena occupate che i controlli
  // passo per passo evitano comunque. Se si è anche liberato qualcosa (un edificio abbattuto) resta buono per chi
  // dal suo posto una strada ce l'ha già: al più ne esiste ora una più corta. Di questi se ne rifanno pochi per
  // passo, così una fila di palizzate posata o un edificio caduto in mezzo a un assedio non fermano il gioco
  // (erano 30 campi nello stesso passo, 30 ms). Chi invece non aveva strada lo riceve rifatto subito.
  // from = cella di chi chiede la strada: se da lì il campo normale non arriva (è chiuso fra i propri edifici, o
  // lo è la meta), vale quello che li attraversa, e `crossed` lo dice a chi si muove. Così i campi restano gli
  // stessi per tutti, salvo in quei casi.
  getField(key, seedFn, side = 0, from = -1) {
    const walls = this.walls[side] ? side : 0, d = this.cached(key + ':' + walls, seedFn, walls, false, from);
    this.crossed = from >= 0 && side !== 0 && d[from] === Infinity;
    return this.crossed ? this.cached(key + ':x' + side, seedFn, side, true, from) : d;
  },
  cached(key, seedFn, side, cross, from) {
    const e = this.fields.get(key);
    if (e) {
      if (e.v === this.version) return e.d;
      if (e.v >= this.opened || (from >= 0 && e.d[from] < Infinity)) {
        if (this.refresh <= 0) return e.d;
        this.refresh--;
      }
    }
    // il campo che esce dalla cache (quello vecchio di questa chiave, o il più antico se è piena) cede la sua
    // memoria a quello nuovo: nessuno lo tiene da parte, chi chiede un campo lo usa subito
    let old = e ? e.d : null;
    this.fields.delete(key);
    if (old === null && this.fields.size >= FIELD_CACHE) {
      const first = this.fields.keys().next().value;
      old = this.fields.get(first).d;
      this.fields.delete(first);
    }
    const d = this.computeField(seedFn(side), side, cross, this.movesFor(side, cross), old);
    this.fields.set(key, { v: this.version, d });
    return d;
  },
  // Celle percorribili attorno a un insieme di celle occupate, sullo stesso livello.
  ring(cells, side = 0) {
    const W = this.W, own = new Set(cells), out = new Set(), lvl = this.level[cells[0]];
    for (const c of cells) {
      const x = c % W, y = (c / W) | 0;
      for (const [dx, dy] of DIRS8) {
        if (!this.inside(x + dx, y + dy)) continue;
        const n = c + dx + dy * W;
        if (!own.has(n) && this.passable(n, side) && this.level[n] === lvl) out.add(n);
      }
    }
    return [...out];
  },
  fieldToCell(c, side = 0, from = -1) { return this.getField('c' + c, s => (this.passable(c, s) ? [c] : this.ring([c], s)), side, from); },
  fieldFor(e, side = 0, from = -1) { return this.getField('e' + e.id, s => this.ring(e.cells, s), side, from); },

  // Prossima cella verso la destinazione del campo, -1 se irraggiungibile. cross = il campo attraversa i propri edifici.
  stepFrom(field, c, side = 0, cross = false) {
    const W = this.W, x = c % W, y = (c / W) | 0;
    let best = -1, bd = field[c];
    if (!(cross ? this.crossable(c, side) : this.passable(c, side))) { // da una cella dove non si sta: fuori dalla parte migliore
      bd = Infinity;
      for (const [dx, dy] of DIRS8) {
        if (!this.inside(x + dx, y + dy)) continue;
        const n = c + dx + dy * W;
        if (this.crossable(n, side) && field[n] < bd) { bd = field[n]; best = n; }
      }
      return best;
    }
    for (const [dx, dy] of DIRS8) {
      const n = this.neighborOK(c, x, y, dx, dy, side, cross);
      if (n >= 0 && field[n] < bd) { bd = field[n]; best = n; }
    }
    return best;
  },

  // ------------------------------------------------------------ generazione

  generate(seed) {
    // i primi tentativi osano (golfi, laghi, coste molto mosse); se nessuno riesce si passa a isole semplici
    let ok = false;
    for (let attempt = 0; attempt < 400 && !ok; attempt++) ok = this.build(seed + attempt * 7919, attempt >= 30) && this.sound();
    if (!ok) throw new Error('Generazione della mappa fallita (seme ' + seed + ')'); // mai visto su 2000 semi: meglio fermarsi che giocare su una mappa rotta
    this.buildTiles();
    this.renderMinimap();
  },

  // La mappa appena costruita è giocabile? Ogni base, ogni altura e ogni terrazza raggiungibili a piedi dalla
  // prima base, e attorno a ogni base legna, oro, pietra e un prato con le pecore.
  sound() {
    const W = this.W, goals = this.bases.map(b => this.idx(b.x, b.y + 2));
    for (const p of this.plateaus) { goals.push(this.idx(p.x, p.y)); if (p.top) goals.push(this.idx(p.top.x, p.top.y)); }
    const f = this.computeField([goals[0]]);
    if (!goals.every(c => f[c] < Infinity)) return false;
    const open = c => f[c - 1] < Infinity || f[c + 1] < Infinity || f[c - W] < Infinity || f[c + W] < Infinity; // ci si arriva accanto
    for (const b of this.bases) {
      for (const [list, r, need] of BASE_STOCK) {
        let n = 0;
        for (const o of this[list]) if (Math.hypot(o.c % W - b.x, Math.floor(o.c / W) - b.y) < r && open(o.c)) n++;
        if (n < need) return false;
      }
      if (!this.meadows.some(m => Math.hypot(m.x / TILE - b.x, m.y / TILE - b.y) < 14)) return false;
    }
    return true;
  },

  // Costruisce una mappa dal seme; false se non c'è posto per basi e altopiani (se ne prova un'altra).
  build(seed, calm = false) {
    const rng = mulberry32(seed);
    const W = this.W, H = this.H, N = W * H, I = (x, y) => y * W + x, scale = N / MAP_AREA;
    const land = this.land = new Uint8Array(N);
    const level = this.level = new Uint8Array(N);
    const cliff = this.cliff = new Uint8Array(N);
    const stair = this.stair = new Uint8Array(N);
    const plateau = this.plateau = new Uint8Array(N); // 1..4 = tileset dell'altura a cui la cella appartiene
    this.block = new Uint8Array(N);
    this.wallOf = new Uint8Array(N); // regno (indice + 1) della palizzata che occupa la cella, 0 se non ce n'è
    this.ownOf = new Uint8Array(N);  // regno dell'edificio (palizzate comprese) che occupa la cella
    const noBuild = this.noBuild = new Uint8Array(N);
    const reserved = new Uint8Array(N);
    this.fields.clear();
    this.version++;
    const bases = this.bases = [], plateaus = this.plateaus = [];
    this.ponds = []; this.foam = []; this.trees = []; this.golds = []; this.stones = []; this.meadows = [];
    this.props = []; this.treasures = []; this.bushes = []; this.rocks = []; this.waterRocks = []; this.ducks = []; this.openWater = [];
    this.points = []; this.bigs = []; // conquistabili { c, kind } e grandi giacimenti { c, kind, amount }: occupano 2x2 celle da c

    // rumore a valori per frastagliare la costa
    const lattice = new Float32Array(1024).map(() => rng());
    const noise = (x, y) => {
      const xi = Math.floor(x), yi = Math.floor(y), fx = x - xi, fy = y - yi;
      const v = (i, j) => lattice[((j & 31) << 5) | (i & 31)];
      const u = fx * fx * (3 - 2 * fx), w = fy * fy * (3 - 2 * fy);
      const a = v(xi, yi), b = v(xi + 1, yi), c = v(xi, yi + 1), d = v(xi + 1, yi + 1);
      return a + (b - a) * u + (c - a) * w + (a - b - c + d) * u * w;
    };
    // forma dell'isola: un ovale più o meno squadrato, con il bordo spinto in dentro e in fuori da poche onde
    // larghe e dal rumore; poi golfi che mordono la costa e laghi all'interno
    const squareness = 2.2 + rng() * 1.4; // 2 = ellisse, oltre 3 = quasi un rettangolo
    const waves = [2, 3, 4, 5].map(k => ({ k, a: (calm ? 0.03 : 0.14) * rng() / Math.sqrt(k / 2), ph: rng() * Math.PI * 2 }));
    const rough = 0.4 + rng() * 0.45, grain = 0.09 + rng() * 0.07;
    const waters = [];
    if (!calm) {
      for (let i = Math.floor(rng() * 4 * Math.sqrt(scale)); i > 0; i--) { // golfi: centro sul bordo della mappa
        const a = rng() * Math.PI * 2;
        waters.push({ x: W / 2 + Math.cos(a) * (W / 2 - 3), y: H / 2 + Math.sin(a) * (H / 2 - 3), rx: 13 + rng() * 14, ry: 10 + rng() * 11 });
      }
      for (let i = Math.floor(rng() * 3 * Math.sqrt(scale)); i > 0; i--) { // laghi
        waters.push({ x: W * (0.2 + rng() * 0.6), y: H * (0.2 + rng() * 0.6), rx: 6 + rng() * 9, ry: 5 + rng() * 7 });
      }
    }
    const inMargin = (x, y) => x >= 2 && y >= 2 && x < W - 2 && y < H - 3;
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        if (!inMargin(x, y)) continue;
        const nx = (x + 0.5 - W / 2) / (W / 2 - 2.5), ny = (y + 0.5 - H / 2) / (H / 2 - 2.5), a = Math.atan2(ny, nx);
        let reach = 1;
        for (const w of waves) reach += w.a * Math.cos(w.k * a + w.ph);
        const d = (Math.pow(Math.abs(nx), squareness) + Math.pow(Math.abs(ny), squareness)) / Math.pow(reach, squareness);
        if (d + (noise(x * grain, y * grain) - 0.5) * rough >= 0.8) continue;
        const wobble = (noise(x * 0.21 + 7, y * 0.21 + 3) - 0.5) * 0.7;
        if (!waters.some(q => ((x - q.x) / q.rx) ** 2 + ((y - q.y) / q.ry) ** 2 + wobble < 1)) land[I(x, y)] = 1;
      }
    }
    // smussa: via gli spuntoni, riempi le insenature di una cella
    for (let pass = 0; pass < 2; pass++) {
      for (let y = 1; y < H - 1; y++) {
        for (let x = 1; x < W - 1; x++) {
          const n = land[I(x - 1, y)] + land[I(x + 1, y)] + land[I(x, y - 1)] + land[I(x, y + 1)];
          if (land[I(x, y)] && n < 2) land[I(x, y)] = 0;
          else if (!land[I(x, y)] && n >= 3 && inMargin(x, y)) land[I(x, y)] = 1;
        }
      }
    }
    const fill = (xa, ya, xb, yb, fn) => {
      for (let y = ya; y <= yb; y++) for (let x = xa; x <= xb; x++) if (inMargin(x, y)) fn(I(x, y));
    };
    // quanta parte di un rettangolo è terra (0..1)
    const landShare = (xa, ya, xb, yb) => {
      let n = 0;
      for (let y = ya; y <= yb; y++) for (let x = xa; x <= xb; x++) if (this.inside(x, y) && land[I(x, y)]) n++;
      return n / ((xb - xa + 1) * (yb - ya + 1));
    };

    // basi: cinque punti a caso sulla terraferma. Dopo il primo, ogni volta il più lontano dalle basi già
    // scelte fra una manciata di candidati estratti a sorte: sparse, ma mai nello stesso disegno.
    const spots = [];
    for (let y = BASE_CLEAR.y + 7; y <= H - BASE_CLEAR.y - 9; y += 2) {
      for (let x = BASE_CLEAR.x + 7; x <= W - BASE_CLEAR.x - 8; x += 2) {
        if (landShare(x - BASE_CLEAR.x - 2, y - BASE_CLEAR.y - 2, x + BASE_CLEAR.x + 2, y + BASE_CLEAR.y + 2) > 0.92) spots.push({ x, y });
      }
    }
    if (!spots.length) return false;
    const apart = s => Math.min(...bases.map(b => Math.hypot(b.x - s.x, b.y - s.y)));
    bases.push({ ...spots[Math.floor(rng() * spots.length)] });
    while (bases.length < BASE_COUNT) {
      let best = null, bd = -1;
      for (let t = 0; t < 400 && (t < 14 || bd < BASE_GAP); t++) {
        const s = spots[Math.floor(rng() * spots.length)], d = apart(s);
        if (d > bd) { bd = d; best = s; }
      }
      if (bd < BASE_GAP) return false;
      bases.push({ ...best });
    }
    // terra garantita e zona sgombra per castello e primi edifici
    for (const b of bases) {
      fill(b.x - BASE_CLEAR.x - 3, b.y - BASE_CLEAR.y - 3, b.x + BASE_CLEAR.x + 3, b.y + BASE_CLEAR.y + 3, c => { land[c] = 1; });
      fill(b.x - BASE_CLEAR.x, b.y - BASE_CLEAR.y, b.x + BASE_CLEAR.x, b.y + BASE_CLEAR.y, c => { reserved[c] = 1; });
    }

    // laghetti
    for (let t = 0; t < 900 * scale && this.ponds.length < 9 * scale; t++) {
      const px = 6 + rng() * (W - 12), py = 6 + rng() * (H - 12), rx = 1.6 + rng() * 1.6, ry = 1.2 + rng() * 1.0;
      let ok = true;
      for (let y = Math.floor(py - ry - 3); y <= py + ry + 3 && ok; y++) {
        for (let x = Math.floor(px - rx - 3); x <= px + rx + 3 && ok; x++) {
          if (!this.inside(x, y) || !land[I(x, y)] || reserved[I(x, y)]) ok = false;
        }
      }
      if (!ok || this.ponds.some(q => Math.hypot(q.x - px, q.y - py) < 14)) continue;
      for (let y = Math.floor(py - ry); y <= py + ry; y++) {
        for (let x = Math.floor(px - rx); x <= px + rx; x++) {
          if (((x + 0.5 - px) / rx) ** 2 + ((y + 0.5 - py) / ry) ** 2 <= 1) land[I(x, y)] = 0;
        }
      }
      this.ponds.push({ x: px, y: py });
    }

    // tieni solo la terra collegata alla prima base
    const seen = new Uint8Array(N), stack = [I(bases[0].x, bases[0].y)];
    seen[stack[0]] = 1;
    while (stack.length) {
      const c = stack.pop(), x = c % W, y = (c / W) | 0;
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const n = I(x + dx, y + dy);
        if (this.inside(x + dx, y + dy) && land[n] && !seen[n]) { seen[n] = 1; stack.push(n); }
      }
    }
    for (let c = 0; c < N; c++) if (!seen[c]) land[c] = 0;
    const landNear = (x, y, r) => {
      for (let j = -r; j <= r; j++) for (let i = -r; i <= r; i++) if (this.isLand(x + i, y + j)) return true;
      return false;
    };
    const solid = land.reduce((s, v) => s + v, 0); // celle di terraferma

    // ---- rilievi: alture dal contorno irregolare (più rettangoli sovrapposti), con la parete di roccia sotto
    // ogni bordo sud, anche a picco sull'acqua, e qualche scala. Sulle più grandi, una terrazza di secondo livello.
    const keep = new Uint8Array(N); // dove non può sorgere un'altra altura: attorno alle basi e alle alture già fatte
    for (const b of bases) fill(b.x - BASE_CLEAR.x - 4, b.y - BASE_CLEAR.y - 4, b.x + BASE_CLEAR.x + 4, b.y + BASE_CLEAR.y + 5, c => { keep[c] = 1; });
    // Unione di n rettangoli agganciati l'uno all'altro attorno a (cx, cy); null se lì non c'è posto.
    // bottom: il primo rettangolo ha (cx, cy) sulla sua ultima riga (per le alture a picco sul mare).
    const blob = (cx, cy, n, [wMin, wMax, hMin, hMax], ok, bottom = false) => {
      const cells = new Set();
      let ax = cx, ay = cy;
      for (let k = 0; k < n; k++) {
        const w = wMin + Math.floor(rng() * (wMax - wMin + 1)), h = hMin + Math.floor(rng() * (hMax - hMin + 1));
        const x0 = ax - Math.floor(rng() * w), y0 = bottom && k === 0 ? ay - h + 1 : ay - Math.floor(rng() * h); // contiene il punto d'aggancio
        let fits = true;
        for (let y = y0; y < y0 + h && fits; y++) for (let x = x0; x < x0 + w && fits; x++) fits = this.inside(x, y) && ok(I(x, y));
        if (!fits) { if (k === 0) return null; continue; }
        for (let y = y0; y < y0 + h; y++) for (let x = x0; x < x0 + w; x++) cells.add(I(x, y));
        const all = [...cells], next = all[Math.floor(rng() * all.length)];
        ax = next % W; ay = (next / W) | 0;
      }
      return cells;
    };
    // Dove può stare una scala per salire su M: a un'estremità di un tratto di parete, su terreno libero del
    // livello di sotto. hi = metà alta (accanto all'ultima cella dell'altura), lo = metà bassa (accanto alla
    // parete), ap = la cella da cui ci si arriva.
    const stairSpots = (M, ground) => {
      const out = [];
      for (const c of M) {
        if (M.has(c + W)) continue;
        for (const dir of [-1, 1]) {
          const hi = c + dir, lo = hi + W, ap = lo + W;
          if (M.has(hi) || M.has(lo) || M.has(ap) || M.has(hi - W) || M.has(hi + dir) || M.has(lo + dir)) continue;
          if (ground(hi) && ground(lo) && ground(ap)) out.push({ hi, lo, ap, dir });
        }
      }
      return out;
    };
    // Alza M al livello L: parete sotto ogni bordo sud, e fino a `count` scale lontane fra loro.
    const raise = (M, L, spots, count) => {
      for (const c of M) level[c] = L;
      for (const c of M) if (!M.has(c + W)) cliff[c + W] = 1;
      const put = [], gap = (a, b) => Math.abs(a.hi % W - b.hi % W) + Math.abs(Math.floor(a.hi / W) - Math.floor(b.hi / W));
      while (put.length < count && spots.length) {
        let pick = Math.floor(rng() * spots.length);
        if (put.length) {
          let far = -1;
          spots.forEach((s, i) => { const d = Math.min(...put.map(q => gap(q, s))); if (d > far) { far = d; pick = i; } });
          if (far < 5) break;
        }
        const s = spots.splice(pick, 1)[0];
        stair[s.lo] = s.dir < 0 ? 1 : 3; stair[s.hi] = s.dir < 0 ? 2 : 4; level[s.hi] = L;
        fill(s.hi % W - 1, Math.floor(s.hi / W) - 1, s.hi % W + 1, Math.floor(s.hi / W) + 2, k => { noBuild[k] = 1; }); // accesso alla scala
        put.push(s);
      }
      return put;
    };
    const hills = [], wantHills = Math.round(solid / 360 * (0.8 + rng() * 0.5));
    const hillOK = c => { const x = c % W, y = (c / W) | 0; return x >= 3 && x <= W - 4 && y >= 3 && y <= H - 6 && land[c] === 1 && !keep[c]; };
    const cape = []; // celle di terra con il mare subito a sud: un'altura posata lì finisce a picco sull'acqua
    for (let c = W; c < N - W; c++) if (land[c] === 1 && !land[c + W]) cape.push(c);
    for (let t = 0; t < 3000 * scale && hills.length < wantHills; t++) {
      const edge = cape.length && rng() < 0.35 ? cape[Math.floor(rng() * cape.length)] : -1;
      const cx = edge < 0 ? 4 + Math.floor(rng() * (W - 8)) : edge % W, cy = edge < 0 ? 4 + Math.floor(rng() * (H - 10)) : (edge / W) | 0;
      const M = blob(cx, cy, 2 + Math.floor(rng() * 4), [6, 14, 4, 9], hillOK, edge >= 0);
      if (!M || M.size < 24) continue;
      const spots = stairSpots(M, c => land[c] === 1 && level[c] === 0 && !cliff[c] && !stair[c] && !reserved[c]);
      if (!spots.length) continue;
      const hill = { cells: M, x: cx, y: cy, stairs: raise(M, 1, spots, M.size > 90 ? 3 : 2), top: null };
      if (M.size >= 50 && rng() < 0.8) { // terrazza: ben dentro l'altura, con lo spazio per la sua parete e le sue scale
        const inner = c => {
          for (let dy = -2; dy <= 3; dy++) for (let dx = -2; dx <= 2; dx++) { const n = c + dx + dy * W; if (!M.has(n) || noBuild[n]) return false; }
          return true;
        };
        const all = [...M];
        for (let k = 0; k < 30 && !hill.top; k++) {
          const p = all[Math.floor(rng() * all.length)], M2 = blob(p % W, (p / W) | 0, 1 + Math.floor(rng() * 2), [3, 8, 2, 5], inner);
          if (!M2 || M2.size < 8) continue;
          const sp = stairSpots(M2, c => M.has(c) && level[c] === 1 && !cliff[c] && !stair[c]);
          if (sp.length) hill.top = { cells: M2, x: p % W, y: (p / W) | 0, stairs: raise(M2, 2, sp, M2.size > 24 ? 2 : 1) };
        }
      }
      // il punto attorno a cui mettere le risorse: una sua cella libera, la più vicina al punto di partenza
      let bd = Infinity;
      for (const c of M) {
        if (cliff[c] || noBuild[c] || level[c] !== 1) continue;
        const d = Math.hypot(c % W - cx, Math.floor(c / W) - cy);
        if (d < bd) { bd = d; hill.x = c % W; hill.y = (c / W) | 0; }
      }
      for (const c of M) fill(c % W - 3, Math.floor(c / W) - 3, c % W + 3, Math.floor(c / W) + 4, k => { keep[k] = 1; });
      hills.push(hill);
    }
    if (hills.length < 4) return false;
    // la più vicina al baricentro delle basi, contesa da tutti, è la più ricca d'oro e va per prima
    const mid = { x: bases.reduce((s, b) => s + b.x, 0) / BASE_COUNT, y: bases.reduce((s, b) => s + b.y, 0) / BASE_COUNT };
    hills.sort((p, q) => Math.hypot(p.x - mid.x, p.y - mid.y) - Math.hypot(q.x - mid.x, q.y - mid.y));
    // per controllare che ci si arrivi vale la cella in cima alla prima scala, che resta sempre sgombra
    const landing = s => ({ x: (s.hi - s.dir) % W, y: Math.floor((s.hi - s.dir) / W) });
    hills.forEach((h, i) => {
      h.id = i % 4 + 1;
      for (const c of h.cells) plateau[c] = h.id;
      for (const s of h.stairs) plateau[s.hi] = plateau[s.lo] = h.id;
      plateaus.push({ id: h.id, ...landing(h.stairs[0]), n: h.cells.size, top: h.top && landing(h.top.stairs[0]) });
    });

    // sacche di terra chiuse fra pareti e acqua, dove non si arriva a piedi: restano paesaggio, senza risorse
    const walk = this.computeField([I(bases[0].x, bases[0].y + 2)]);
    for (let c = 0; c < N; c++) if (land[c] === 1 && !cliff[c] && walk[c] === Infinity) reserved[c] = noBuild[c] = 1;

    // ---- isolotti: ciuffi d'erba in mare poco lontano dalla costa. Non ci si arriva (land = 2): sono paesaggio.
    // I più grandi sono scogli rialzati, con la loro parete di roccia.
    const isles = [], wantIsles = 6 + Math.floor(rng() * 6);
    for (let t = 0; t < 900 && isles.length < wantIsles; t++) {
      const x = 6 + Math.floor(rng() * (W - 12)), y = 6 + Math.floor(rng() * (H - 13)), rx = 1.3 + rng() * 2, ry = 1.1 + rng() * 1.4;
      const r = Math.ceil(Math.max(rx, ry));
      if (land[I(x, y)] || landNear(x, y, r + 2) || !landNear(x, y, r + 9) || isles.some(q => Math.hypot(q.x - x, q.y - y) < 10)) continue;
      const cells = [];
      for (let dy = -r; dy <= r; dy++) for (let dx = -r; dx <= r; dx++) if ((dx / rx) ** 2 + (dy / ry) ** 2 <= 1) cells.push(I(x + dx, y + dy));
      for (const c of cells) land[c] = 2;
      if (ry > 1.9) { // in ogni colonna l'ultima cella in basso fa da piede alla parete, le altre stanno in alto
        const tile = 1 + Math.floor(rng() * 4);
        for (const c of cells) { if (land[c + W] === 2) { level[c] = 1; plateau[c] = tile; } }
        for (const c of cells) if (!level[c] && level[c - W]) cliff[c] = 1;
      }
      isles.push({ x, y });
      (rng() < 0.6 ? this.bushes : this.rocks).push({ x: x * TILE + 16 + rng() * 32, y: (y - (ry > 1.9 ? 1 : 0)) * TILE + 30 + rng() * 26, v: Math.floor(rng() * 4), phase: rng() * 8 });
    }

    // costa (schiuma): attorno alla terra, agli isolotti e ai piedi delle pareti a picco sull'acqua
    const ground = (x, y) => this.inside(x, y) && land[I(x, y)] > 0;
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const c = I(x, y);
        if (!land[c]) { if (cliff[c]) this.foam.push(c); continue; }
        let coast = false;
        for (const [dx, dy] of DIRS8) if (!ground(x + dx, y + dy)) coast = true;
        if (coast) this.foam.push(c);
      }
    }

    // ---- risorse
    const used = new Uint8Array(N);
    const freeLow = c => land[c] === 1 && !level[c] && !cliff[c] && !stair[c] && !reserved[c] && !noBuild[c] && !used[c];
    const freeHigh = c => land[c] === 1 && level[c] && !cliff[c] && !stair[c] && !noBuild[c] && !used[c];
    const scatter = (cx, cy, n, r, ok) => {
      const out = [];
      for (let t = 0; t < n * 14 && out.length < n; t++) {
        const x = Math.round(cx + (rng() * 2 - 1) * r), y = Math.round(cy + (rng() * 2 - 1) * r);
        if (!this.inside(x, y) || !ok(I(x, y))) continue;
        used[I(x, y)] = 1;
        out.push(I(x, y));
      }
      return out;
    };
    const rich = solid / (N * 0.71); // 0,71 = la terra dell'isola di una volta
    // strict: se non c'è posto per un gruppo intero non lascia niente (e si può riprovare altrove)
    const place = (kind, px, py, ok = freeLow, strict = false) => {
      const group = (cells, min) => {
        if (cells.length >= min || !strict) return cells;
        for (const c of cells) used[c] = 0;
        return [];
      };
      if (kind === 'forest' || kind === 'grove') { // grove = bosco fitto, grande il doppio
        const grove = kind === 'grove', cells = group(scatter(px, py, grove ? 20 + Math.floor(rng() * 8) : 8 + Math.floor(rng() * 4), grove ? 4.4 : 2.8, ok), 5);
        for (const c of cells) this.trees.push({ c, v: Math.floor(rng() * 4) });
        return cells.length >= 5;
      }
      if (kind === 'gold' || kind === 'stone') {
        const gold = kind === 'gold', cells = group(scatter(px, py, gold ? 3 : 2, 1.6, ok), 2);
        for (const c of cells) (gold ? this.golds : this.stones).push({ c, amount: gold ? 240 : 300 });
        return cells.length >= 2;
      }
      const cells = scatter(px, py, 1, 1.5, ok);
      if (!cells.length) return false;
      used[cells[0]] = 0;
      this.meadows.push({ x: this.cx(cells[0]), y: this.cy(cells[0]) });
      return true;
    };
    // Attorno a ogni base, appena fuori dalla zona sgombra e dentro quella già esplorata alla partenza. Per tutte
    // si comincia dalla stessa direzione rispetto al centro della mappa (partenza equa); se da quella parte c'è
    // acqua o non c'è posto, si gira un po' alla volta. pad = celle oltre il bordo della zona sgombra, far = più in là.
    const near = (b, kind, angle, pad, far = 0) => {
      const a0 = Math.atan2(H / 2 - b.y, W / 2 - b.x) + angle;
      for (const turn of [0, 0.4, -0.4, 0.8, -0.8, 1.3, -1.3, 1.9, -1.9, 2.5, -2.5, 3.14]) {
        const ca = Math.cos(a0 + turn), sa = Math.sin(a0 + turn);
        const edge = Math.min(BASE_CLEAR.x / Math.max(1e-6, Math.abs(ca)), BASE_CLEAR.y / Math.max(1e-6, Math.abs(sa)));
        const d0 = edge + pad + far;
        for (let d = d0; d <= Math.max(d0, 12.5 + far); d++) if (place(kind, b.x + ca * d, b.y + sa * d, freeLow, true)) return;
      }
    };
    for (const b of bases) {
      near(b, 'meadow', 0.2, 2);
      near(b, 'forest', 1.9, 2.5);
      near(b, 'forest', -2.3, 2.5);
      near(b, 'gold', -1.05, 1.5);
      near(b, 'gold', 0.85, 1.5, 5);
      near(b, 'stone', -0.25, 1.5);
    }
    // sulle alture: oro (il triplo sulla prima, la più contesa) e pietra sulle più centrali, alberi su tutte
    hills.forEach((h, i) => {
      const ok = c => h.cells.has(c) && freeHigh(c);
      if (i === 0) for (let k = -1; k <= 1; k++) place('gold', h.x + k * 3, h.y, ok);
      else if (i < 4 * scale) place('gold', h.x, h.y, ok);
      if (i < 4 * scale) place('stone', h.x + (i === 0 ? 0 : 3), h.y + (i === 0 ? 2.5 : 1), ok);
      for (const c of scatter(h.x, h.y, 3 + Math.floor(rng() * 4), Math.sqrt(h.cells.size) / 2, ok)) this.trees.push({ c, v: Math.floor(rng() * 4) });
    });
    const farFromBases = (x, y) => bases.every(b => Math.hypot(b.x - x, b.y - y) > 14);

    // ---- punti da conquistare e grandi giacimenti: lontano dalle basi e sparsi. Fra una manciata di posti liberi
    // estratti a sorte vince ogni volta il più lontano dalle basi e da quelli già messi.
    const roomy = c => { // 2x2 celle libere da c, con una cella libera tutto attorno
      const x = c % W, y = (c / W) | 0;
      if (x < 3 || y < 3 || x > W - 5 || y > H - 6) return false;
      for (let dy = -1; dy <= 2; dy++) for (let dx = -1; dx <= 2; dx++) if (!freeLow(c + dx + dy * W)) return false;
      return true;
    };
    const marks = bases.map(b => ({ x: b.x, y: b.y }));
    const spread = (fromBase, gap) => {
      let best = -1, bd = -1;
      for (let t = 0; t < 80; t++) {
        const x = 4 + Math.floor(rng() * (W - 8)), y = 4 + Math.floor(rng() * (H - 9)), c = I(x, y);
        if (!roomy(c) || bases.some(b => Math.hypot(b.x - x, b.y - y) < fromBase)) continue;
        const d = Math.min(...marks.map(m => Math.hypot(m.x - x, m.y - y)));
        if (d > bd) { bd = d; best = c; }
      }
      if (best < 0 || bd < gap) return -1;
      marks.push({ x: best % W, y: (best / W) | 0 });
      for (const c of this.quad(best)) used[c] = 1;
      return best;
    };
    for (let i = 0; i < Math.round(9 * scale * rich); i++) {
      const c = spread(20, 11);
      if (c < 0) continue;
      const kind = POINT_MIX[i % POINT_MIX.length], x = c % W + 1, y = ((c / W) | 0) + 1;
      this.points.push({ c, kind });
      for (let dy = -1; dy <= 2; dy++) for (let dx = -1; dx <= 2; dx++) noBuild[c + dx + dy * W] = 1; // ci si deve sempre poter arrivare
      if (POINT_STOCK[kind]) for (const [dx, dy] of [[4.5, 0], [-4.5, 1]]) place(POINT_STOCK[kind], x + dx, y + dy); // accanto, ciò che il punto produce
    }
    for (let i = 0; i < Math.round(5 * scale * rich); i++) {
      const c = spread(18, 9);
      if (c >= 0) this.bigs.push({ c, kind: i % 2 ? 'stone' : 'gold', amount: i % 2 ? 1800 : 1500 });
    }
    for (const [kind, count] of [['grove', 3], ['forest', 26], ['gold', 9], ['stone', 10], ['meadow', 10]]) { // in proporzione alla terra
      for (let i = 0, t = 0; i < Math.round(count * rich * scale) && t < 500 * scale; t++) {
        const x = 4 + rng() * (W - 8), y = 4 + rng() * (H - 8);
        if (farFromBases(x, y) && place(kind, x, y)) i++;
      }
    }

    // ---- oggetti di scena (assets/Extra/Props) e forzieri per chi esplora
    const prop = (name, c, solid = false) => {
      used[c] = 1;
      this.props.push({ name, c, solid, x: this.cx(c) + (solid ? 0 : Math.round((rng() - 0.5) * 28)), y: this.cy(c) + 20 });
    };
    const chest = c => { used[c] = 1; this.treasures.push({ c, kind: RES_KINDS[this.treasures.length % 4] }); };
    const spot = (test = freeLow) => { // una cella libera a caso lontano dalle basi, o -1
      for (let t = 0; t < 300; t++) {
        const x = 3 + Math.floor(rng() * (W - 6)), y = 3 + Math.floor(rng() * (H - 6));
        if (farFromBases(x, y) && test(I(x, y))) return I(x, y);
      }
      return -1;
    };
    // accampamenti abbandonati: tenda, fuoco, qualche cassa e un forziere
    const room3 = c => [-W - 1, -W, -W + 1, -1, 0, 1, W - 1, W, W + 1].every(d => freeLow(c + d));
    for (let i = 0; i < 9 * scale; i++) {
      const c = spot(room3);
      if (c < 0) continue;
      prop('tent', c - W, true); prop('campfire', c + 1, true); chest(c);
      prop(['barrel', 'crate', 'logs'][i % 3], c - W + 1, true); prop(['hay', 'cart', 'crate'][i % 3], c - 1, true);
    }
    for (let i = 0; i < 16 * scale; i++) { const c = spot(); if (c >= 0) chest(c); }
    // punti di riferimento solidi, poi fiori, funghi, erba, sassi e ossa su cui si cammina
    const landmarks = ['well', 'statue', 'column', 'ruin_arch', 'deadtree', 'boulder', 'deadtree', 'scarecrow', 'grave', 'menhir', 'signpost', 'ruin_wall',
      'fence', 'stone_circle', 'cart', 'hay', 'logs', 'barrel', 'shrine', 'column', 'obelisk', 'grave', 'wagon_wreck', 'beehive', 'signpost', 'fence', 'windmill', 'boulder'];
    for (let i = 0; i < 54 * scale; i++) { const c = spot(); if (c >= 0) prop(landmarks[i % landmarks.length], c, true); }
    const details = [['grass', 110], ['tall_grass', 70], ['flowers', 60], ['flowers_red', 34], ['flowers_blue', 34], ['flowers_white', 34], ['clover', 44], ['ferns', 44],
      ['pebbles', 44], ['sticks', 30], ['mushrooms', 34], ['stump', 18], ['fallen_log', 14], ['bones', 14], ['puddle', 12], ['pumpkins', 8]];
    for (const [name, count] of details) {
      for (let i = 0; i < count * scale; i++) {
        const x = 3 + Math.floor(rng() * (W - 6)), y = 3 + Math.floor(rng() * (H - 6));
        if (freeLow(I(x, y))) prop(name, I(x, y));
      }
    }
    this.resetBlock();
    // ciò che è rimasto dove non si arriva a piedi (un passaggio chiuso da un oggetto di scena, una sacca fra le
    // pareti) si toglie. Le risorse qui non contano come ostacolo: una foresta si attraversa tagliandola.
    for (const o of [...this.trees, ...this.golds, ...this.stones]) this.block[o.c] = 0;
    const way = this.computeField([I(bases[0].x, bases[0].y + 2)]), lost = c => way[c] === Infinity;
    const shut = o => ![o.c - 1, o.c + 2, o.c - W, o.c + 2 * W, o.c + W - 1, o.c + W + 2, o.c - W + 1, o.c + 2 * W + 1].some(c => !lost(c)); // 2x2: nessuna cella accanto raggiungibile
    this.trees = this.trees.filter(o => !lost(o.c));
    this.golds = this.golds.filter(o => !lost(o.c));
    this.stones = this.stones.filter(o => !lost(o.c));
    this.treasures = this.treasures.filter(o => !lost(o.c));
    this.meadows = this.meadows.filter(m => !lost(this.cellAt(m.x, m.y)));
    this.points = this.points.filter(o => !shut(o));
    this.bigs = this.bigs.filter(o => !shut(o));
    this.resetBlock();

    // ---- decorazioni
    for (let t = 0; t < 12000 * scale && this.bushes.length + this.rocks.length < 330 * scale; t++) {
      const x = Math.floor(rng() * W), y = Math.floor(rng() * H), c = I(x, y);
      if (!freeLow(c)) continue;
      used[c] = 1;
      const d = { x: x * TILE + 16 + rng() * 32, y: y * TILE + 30 + rng() * 26, v: Math.floor(rng() * 4), phase: rng() * 8 };
      (this.bushes.length < 200 ? this.bushes : this.rocks).push(d);
    }
    // (scogli e papere in acqua)
    for (let t = 0; t < 14000 * scale && this.waterRocks.length < 100 * scale; t++) {
      const x = 1 + Math.floor(rng() * (W - 2)), y = 1 + Math.floor(rng() * (H - 2)), c = I(x, y);
      if (land[c] || used[c] || landNear(x, y, 1) || !landNear(x, y, 3)) continue;
      used[c] = 1;
      this.waterRocks.push({ x: this.cx(c), y: this.cy(c), v: Math.floor(rng() * 4), phase: rng() * 16 });
    }
    for (let t = 0; t < 14000 * scale && this.ducks.length < 9 * scale; t++) {
      const x = 1 + Math.floor(rng() * (W - 2)), y = 1 + Math.floor(rng() * (H - 2)), c = I(x, y);
      if (land[c] || used[c] || landNear(x, y, 1) || !landNear(x, y, 4)) continue;
      if (this.ducks.some(d => Math.hypot(d.x - this.cx(c), d.y - this.cy(c)) < 1100)) continue;
      this.ducks.push({ x: this.cx(c), y: this.cy(c), phase: rng() * 6 });
    }
    for (const p of this.ponds) {
      this.ducks.push({ x: p.x * TILE, y: p.y * TILE, phase: rng() * 6, pond: true });
      this.props.push({ name: 'lilypads', c: -1, solid: false, x: Math.round(p.x * TILE + (rng() - 0.5) * 60), y: Math.round(p.y * TILE + 30) });
    }
    // canne e barche tirate in secco lungo la riva
    const shore = [];
    for (let y = 2; y < H - 2; y++) for (let x = 2; x < W - 2; x++) if (!land[I(x, y)] && !cliff[I(x, y)] && !used[I(x, y)] && this.isLand(x, y - 1)) shore.push(I(x, y));
    for (let i = 0; i < 70 * scale && shore.length; i++) {
      const c = shore.splice(Math.floor(rng() * shore.length), 1)[0];
      const name = i % 9 === 0 ? 'boat' : i % 13 === 5 ? 'dock' : i % 17 === 8 ? 'shipwreck' : i % 4 === 0 ? 'lilypads' : 'reeds', big = name === 'boat' || name === 'dock' || name === 'shipwreck';
      this.props.push({ name, c: -1, solid: false, x: this.cx(c) + Math.round((rng() - 0.5) * 30), y: this.cy(c) + (big ? 14 : 6) });
    }

    // celle d'acqua aperta per gli spruzzi
    for (let y = 1; y < H - 1; y++) {
      for (let x = 1; x < W - 1; x++) if (!landNear(x, y, 1) && landNear(x, y, 5)) this.openWater.push(I(x, y));
    }
    return true;
  },

  // ------------------------------------------------------------ tile

  // Indice nel 3x3 + strisce singole del tileset: [0 bordo iniziale, 1 centro, 2 bordo finale, 3 isolato].
  edge(before, after) { return before && after ? 1 : !before && after ? 0 : before && !after ? 2 : 3; },

  // Precalcola per ogni cella quale tile disegnare: il terreno piano, poi uno strato per ogni livello rialzato
  // (superficie dell'altura, scale, pareti) con l'ombra ai piedi delle sue pareti.
  buildTiles() {
    const W = this.W, H = this.H, N = W * H;
    const flat = this.tFlat = new Int8Array(N).fill(-1);                    // col | row << 2
    const tops = this.tTop = [1, 2].map(() => new Int16Array(N).fill(-1));  // per livello: tileset << 8 | col << 4 | row
    const shadows = this.tShadow = [1, 2].map(() => new Uint8Array(N));     // per livello: 1 ombra, +2 finisce a sinistra, +4 a destra
    const ground = (x, y) => this.inside(x, y) && this.land[this.idx(x, y)] > 0;
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        if (ground(x, y)) flat[this.idx(x, y)] = this.edge(ground(x - 1, y), ground(x + 1, y)) | this.edge(ground(x, y - 1), ground(x, y + 1)) << 2;
      }
    }
    for (let L = 1; L <= 2; L++) {
      const top = tops[L - 1], shadow = shadows[L - 1];
      const up = (x, y) => this.inside(x, y) && this.level[this.idx(x, y)] >= L;
      const wall = c => this.cliff[c] === 1 && this.level[c - W] === L && this.level[c] < L;        // parete di questo livello
      const steps = c => this.stair[c] !== 0 && this.level[c] === (this.stair[c] % 2 ? L - 1 : L);  // scala che sale a questo livello
      for (let y = 1; y < H - 1; y++) {
        for (let x = 1; x < W - 1; x++) {
          const c = this.idx(x, y), id = this.plateau[c], st = this.stair[c];
          if (wall(c)) { // sull'acqua la parete ha il piede bagnato (riga 5) e niente ombra
            top[c] = this.plateau[c - W] << 8 | (5 + this.edge(wall(c - 1), wall(c + 1))) << 4 | (this.land[c] ? 4 : 5);
            if (this.land[c]) shadow[c] = 1;
          } else if (steps(c)) {
            top[c] = id << 8 | (st <= 2 ? 0 : 3) << 4 | (st % 2 === 0 ? 4 : 5);
            if (st % 2) shadow[c] = 1;
          } else if (this.level[c] >= L) {
            top[c] = id << 8 | (5 + this.edge(up(x - 1, y), up(x + 1, y))) << 4 | this.edge(up(x, y - 1), up(x, y + 1));
          }
        }
      }
      for (let c = 1; c < N - 1; c++) if (shadow[c]) shadow[c] |= (shadow[c - 1] ? 0 : 2) | (shadow[c + 1] ? 0 : 4);
    }
  },

  // Disegna i tile visibili. 64px * zoom è intero, quindi i tile combaciano senza fessure.
  drawGround(ctx, cam, vw, vh) {
    const W = this.W;
    const xa = Math.max(0, Math.floor(cam.x / TILE) - 1), xb = Math.min(W - 1, Math.floor((cam.x + vw) / TILE) + 1);
    const ya = Math.max(0, Math.floor(cam.y / TILE) - 3), yb = Math.min(this.H - 1, Math.floor((cam.y + vh) / TILE));
    if (!this.tileImgs) this.tileImgs = [1, 2, 3, 4, 5].map(i => img(`${TERR}Tileset/Tilemap_color${i}.png`));
    const tiles = this.tileImgs, sh = img(TERR + 'Tileset/Shadow.png');
    for (let y = ya; y <= yb; y++) {
      for (let x = xa, c = y * W + xa; x <= xb; x++, c++) {
        const t = this.tFlat[c];
        if (t >= 0) ctx.drawImage(tiles[0], (t & 3) * TILE, (t >> 2) * TILE, TILE, TILE, x * TILE, y * TILE, TILE, TILE);
      }
    }
    // per ogni livello: prima l'ombra ai piedi delle pareti (una colonna di 64px per cella, così le ombre
    // vicine non si sommano), poi superficie, scale e pareti
    for (let L = 0; L < 2; L++) {
      const shadow = this.tShadow[L], top = this.tTop[L];
      for (let y = ya; y <= yb; y++) {
        for (let x = xa, c = y * W + xa; x <= xb; x++, c++) {
          const s = shadow[c];
          if (!s) continue;
          const dy = y * TILE - 42;
          ctx.drawImage(sh, 64, 0, 64, 192, x * TILE, dy, 64, 192);
          if (s & 2) ctx.drawImage(sh, 0, 0, 64, 192, x * TILE - 64, dy, 64, 192);
          if (s & 4) ctx.drawImage(sh, 128, 0, 64, 192, x * TILE + 64, dy, 64, 192);
        }
      }
      for (let y = ya; y <= yb; y++) {
        for (let x = xa, c = y * W + xa; x <= xb; x++, c++) {
          const t = top[c];
          if (t >= 0) ctx.drawImage(tiles[t >> 8], ((t >> 4) & 15) * TILE, (t & 15) * TILE, TILE, TILE, x * TILE, y * TILE, TILE, TILE);
        }
      }
    }
  },

  renderMinimap() {
    const k = 2, cv = this.minimap = document.createElement('canvas');
    cv.width = this.W * k; cv.height = this.H * k;
    const g = cv.getContext('2d');
    // il mare della minimappa è il tile d'acqua del pacchetto (a schermo, dove si ridisegna a ogni fotogramma, basta la sua tinta: vedi WATER_COLOR)
    g.fillStyle = g.createPattern(img(TERR + 'Tileset/Water Background color.png'), 'repeat') || '#47aba9';
    g.fillRect(0, 0, cv.width, cv.height);
    for (let y = 0; y < this.H; y++) {
      for (let x = 0; x < this.W; x++) {
        const c = this.idx(x, y);
        if (!this.land[c] && !this.cliff[c]) continue;
        g.fillStyle = this.cliff[c] ? '#5c8f92' : this.stair[c] ? '#c9b87a' : this.level[c] > 1 ? '#d9e58a' : this.level[c] ? '#b9cf62' : '#8fb24c';
        g.fillRect(x * k, y * k, k, k);
      }
    }
    g.fillStyle = '#4f7f3a';
    for (const t of this.trees) g.fillRect((t.c % this.W) * k, Math.floor(t.c / this.W) * k, k, k);
    g.fillStyle = '#f2d24b';
    for (const o of this.golds) g.fillRect((o.c % this.W) * k, Math.floor(o.c / this.W) * k, k, k);
    g.fillStyle = '#9aa6ad';
    for (const o of this.stones) g.fillRect((o.c % this.W) * k, Math.floor(o.c / this.W) * k, k, k);
    for (const o of this.bigs) { // i grandi giacimenti si notano: 2x2 celle col bordo scuro
      const x = (o.c % this.W) * k, y = Math.floor(o.c / this.W) * k;
      g.fillStyle = '#1b1f33'; g.fillRect(x - 1, y - 1, k * 2 + 2, k * 2 + 2);
      g.fillStyle = o.kind === 'gold' ? '#f2d24b' : '#c3ccd1'; g.fillRect(x, y, k * 2, k * 2);
    }
  },

  // Celle occupate da ciò che non si sposta mai: alberi, giacimenti, punti da conquistare e oggetti di scena
  // solidi. Gli edifici si aggiungono sopra (setBlock).
  resetBlock() {
    this.block.fill(0);
    this.wallOf.fill(0);
    this.ownOf.fill(0);
    this.walls = [0, 0, 0, 0, 0, 0]; // palizzate in piedi per regno (indice + 1)
    for (const o of [...this.trees, ...this.golds, ...this.stones, ...this.props]) if (o.solid !== false) this.block[o.c] = 1;
    for (const o of [...this.points, ...this.bigs]) for (const c of this.quad(o.c)) this.block[c] = 1;
    this.version++;
  },

  // ------------------------------------------------------------ rete

  // Il mondo generato dall'host, da spedire ai client (che non possono rigenerarlo identico).
  serialize() {
    const N = this.W * this.H, cells = new Array(N);
    for (let c = 0; c < N; c++) {
      cells[c] = this.land[c] | this.level[c] << 2 | this.cliff[c] << 4 | this.noBuild[c] << 5 | this.stair[c] << 6 | this.plateau[c] << 9;
    }
    const { bases, plateaus, ponds, trees, golds, stones, props, meadows, bushes, rocks, waterRocks, ducks, foam, openWater, points, bigs } = this;
    return { cells, bases, plateaus, ponds, trees, golds, stones, props, meadows, bushes, rocks, waterRocks, ducks, foam, openWater, points, bigs };
  },

  load(data) {
    const N = this.W * this.H, { cells, ...lists } = data;
    for (const k of ['land', 'level', 'cliff', 'noBuild', 'stair', 'plateau', 'block', 'wallOf', 'ownOf']) this[k] = new Uint8Array(N);
    for (let c = 0; c < N; c++) {
      const v = cells[c];
      this.land[c] = v & 3; this.level[c] = v >> 2 & 3; this.cliff[c] = v >> 4 & 1; this.noBuild[c] = v >> 5 & 1;
      this.stair[c] = v >> 6 & 7; this.plateau[c] = v >> 9;
    }
    Object.assign(this, lists);
    this.treasures = [];
    this.resetBlock();
    this.fields.clear();
    this.version++;
    this.buildTiles();
    this.renderMinimap();
  },
};
