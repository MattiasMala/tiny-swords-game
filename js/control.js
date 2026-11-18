'use strict';
// Controlli da gioco di strategia: selezione, ordini col tasto destro, azioni di contesto, telecamera
// e nebbia di guerra. Tutto questo è locale: verso la simulazione (o l'host) partono solo comandi.

const CAM_SPEED = 1100;     // px al secondo con frecce, WASD o bordo dello schermo
const CAM_MEDIUM = 2;       // volte tanto con Maiusc premuto
const CAM_FAST = 4;         // volte tanto con Ctrl premuto
const EDGE = 8;             // px CSS dal bordo della finestra entro cui la visuale scorre
const DRAG_MIN = 8;         // px CSS di spostamento oltre i quali un clic diventa un riquadro di selezione
const START_SIGHT = 15;     // celle attorno al castello già esplorate a inizio partita
const MARK_COLOR = { move: '#9be37a', attack: '#ff6a5a', gather: '#ffe27a', rally: '#ffffff' };
// 1 se uno dei due tasti è premuto (freccia o lettera, tasto sinistro o destro), altrimenti 0.
function held(keys, a, b) { return keys.has(a) || keys.has(b) ? 1 : 0; }

// ---------------------------------------------------------------- nebbia di guerra

const Fog = {
  on: false,
  vis: null,     // celle viste adesso da unità ed edifici del giocatore
  seen: null,    // celle già esplorate
  shade: null,   // opacità attuale della nebbia per cella (insegue il valore giusto, così non scatta)
  cv: null, g: null, image: null, t: 0,

  reset(on) {
    const N = World.W * World.H;
    this.on = on;
    this.vis = new Uint8Array(N);
    this.seen = new Uint8Array(N);
    this.shade = new Float32Array(N).fill(on ? 1 : 0);
    this.cv = document.createElement('canvas');
    this.cv.width = World.W; this.cv.height = World.H;
    this.g = this.cv.getContext('2d');
    // il colore non cambia mai: si scrive qui una volta, poi a ogni fotogramma solo l'opacità
    const px = (this.image = this.g.createImageData(World.W, World.H)).data;
    for (let c = 0; c < N; c++) { px[c * 4] = 10; px[c * 4 + 1] = 14; px[c * 4 + 2] = 26; px[c * 4 + 3] = this.shade[c] * 255; }
    this.g.putImageData(this.image, 0, 0);
    this.t = 0;
  },
  sees(x, y) { const c = World.cellAt(x, y); return !this.on || (c >= 0 && this.vis[c] === 1); },
  knows(x, y) { const c = World.cellAt(x, y); return !this.on || (c >= 0 && this.seen[c] === 1); },

  reveal(x, y, r) {
    const W = World.W, cx = Math.floor(x / TILE), cy = Math.floor(y / TILE);
    for (let j = Math.max(0, cy - r); j <= Math.min(World.H - 1, cy + r); j++) {
      for (let i = Math.max(0, cx - r); i <= Math.min(W - 1, cx + r); i++) {
        if ((i - cx) ** 2 + (j - cy) ** 2 <= r * r + r) { this.vis[j * W + i] = 1; this.seen[j * W + i] = 1; }
      }
    }
  },

  update(dt) {
    const f = G.player;
    if (this.on && f && (this.t -= dt) <= 0) {
      this.t = 0.2;
      this.vis.fill(0);
      for (const u of G.units) if (u.faction === f && !u.dead) this.reveal(u.x, u.y, u.def.sight + f.mods.sight);
      for (const b of G.buildings) if (b.faction === f && !b.dead) this.reveal(b.x, b.y - 30, (b.done ? b.def.sight : 2) + f.mods.sight);
      for (const p of G.points) if (p.owner === f) this.reveal(p.x, p.y - TILE, POINT_SIGHT + f.mods.sight);
    }
    // ogni cella insegue l'opacità giusta; arrivata a meno di un millesimo ci si ferma, e da lì in poi (quasi tutte
    // le celle, quasi sempre) non c'è niente da riscrivere: né la cella né, se nessuna è cambiata, la tela
    const px = this.image.data, shade = this.shade, vis = this.vis, seen = this.seen, on = this.on, k = Math.min(1, dt * 5);
    const known = Math.fround(0.42); // come la conserva shade (Float32Array): così il confronto qui sotto è esatto
    let changed = false;
    for (let c = 0; c < shade.length; c++) {
      const want = !on || vis[c] ? 0 : seen[c] ? known : 1, s0 = shade[c];
      if (s0 === want) continue;
      let s = s0 + (want - s0) * k;
      if (Math.abs(want - s) < 0.001) s = want;
      shade[c] = s;
      px[c * 4 + 3] = shade[c] * 255;
      changed = true;
    }
    if (changed) this.g.putImageData(this.image, 0, 0);
  },
};

// ---------------------------------------------------------------- selezione e ordini

const Ctl = {
  sel: new Set(),     // id delle entità selezionate
  list: [],           // le stesse come oggetti, aggiornate a ogni fotogramma
  groups: {},         // gruppi di controllo (Ctrl + numero)
  drag: null,         // { x, y }: punto dello schermo dove è cominciato il trascinamento
  mode: null,         // 'amove' | 'rally': il prossimo clic sinistro dà quell'ordine · 'pickmove': sceglie l'edificio da spostare
  pickB: null,        // 'pickmove' da tastiera: l'edificio su cui sono arrivate le frecce
  enter: false,       // Invio premuto mentre si posa un edificio (vedi place)
  markers: [],        // segni sul terreno degli ordini appena dati
  pings: [],          // lampi sulla minimappa dove il regno viene attaccato
  alertAt: null, alertT: -99,
  lastPick: { id: 0, t: -9 }, lastGroup: { n: 0, t: -9 }, idleI: 0,

  reset() {
    this.sel.clear();
    Object.assign(this, { list: [], groups: {}, drag: null, mode: null, pickB: null, enter: false, markers: [], pings: [], alertAt: null, alertT: -99, idleI: 0 });
  },

  select(ids, add = false) {
    if (!add) this.sel.clear();
    for (const id of ids) this.sel.add(id);
    this.mode = null;
    this.refresh();
  },
  refresh() {
    this.list = [];
    for (const id of this.sel) {
      const e = G.index.get(id);
      if (e && !e.dead && (!NODE_RES[e.kind] || nodeValid(e))) this.list.push(e); else this.sel.delete(id);
    }
  },
  mine() { return this.list.filter(e => e.isUnit && e.faction === G.player); },
  lookAt(x, y) { G.cam.x = x - G.viewW / 2; G.cam.y = y - G.viewH / 2; clampCamera(); },
  mark(kind, x, y) { this.markers.push({ kind, x, y, t: 0 }); },

  // Il regno è stato colpito: lampo sulla minimappa, avviso e corno (l'host li manda al più ogni tot secondi).
  alert(x, y) {
    this.pings.push({ x, y, t: 0 });
    this.alertAt = { x, y }; this.alertT = G.time;
    if (!onScreen(x, y, 0)) {
      if (G.banners.length < 4) G.banners.push({ text: 'Il tuo regno è sotto attacco!', faction: G.player, t: 0, dur: 2.6 });
      Sfx.play('horn');
    }
  },

  // ---- cosa c'è sotto il mouse: prima le unità, poi gli edifici e i punti da conquistare, poi le risorse
  pick(wx, wy) {
    let best = null;
    for (const u of G.units) {
      if (u.dead || u.inside || (u.faction !== G.player && !Fog.sees(u.x, u.y))) continue;
      const wide = u.def.siege || u.type === 'knight' ? 40 : 24;
      if (Math.abs(wx - u.x) < wide && wy > u.y - u.def.barY + 8 && wy < u.y + 10 && (!best || u.y > best.y)) best = u;
    }
    if (best) return best;
    for (const b of G.buildings) {
      if (b.dead || !Fog.knows(b.x, b.y - 20)) continue;
      const top = b.def.w > 1 ? b.y - b.def.ay * 0.85 : b.y0 - 6;
      if (wx > b.x0 - 4 && wx < b.x1 + 4 && wy > top && wy < b.y1 && (!best || b.y > best.y)) best = b;
    }
    if (best) return best;
    for (const p of G.points) if (wx > p.x0 - 4 && wx < p.x1 + 4 && wy > p.y - p.def.ay * 0.85 && wy < p.y1 && Fog.knows(p.x, p.y - TILE)) return p;
    for (const s of G.sheep) if (!s.dead && Math.abs(wx - s.x) < 26 && wy > s.y - 44 && wy < s.y + 8 && Fog.sees(s.x, s.y)) return s;
    for (const list of [G.golds, G.stones]) {
      for (const n of list) { const r = n.big ? 68 : 36; if (Math.abs(wx - n.x) < r && Math.abs(wy - n.y) < r && Fog.knows(n.x, n.y)) return n; }
    }
    for (const t of G.trees) {
      if (t.wood > 0 && Math.abs(wx - t.x) < 30 && wy > t.y - 100 && wy < t.y + 8 && Fog.knows(t.x, t.y) && (!best || t.y > best.y)) best = t;
    }
    return best;
  },

  boxSelect(x0, y0, x1, y1, add) {
    const z = G.zoom, ids = [];
    for (const u of G.units) {
      if (u.dead || u.inside || u.faction !== G.player) continue;
      const sx = (u.x - G.cam.x) * z, sy = (u.y - 26 - G.cam.y) * z;
      if (sx >= Math.min(x0, x1) && sx <= Math.max(x0, x1) && sy >= Math.min(y0, y1) && sy <= Math.max(y0, y1)) ids.push(u.id);
    }
    // con soldati e pedoni insieme nel riquadro restano i soldati, come ci si aspetta in battaglia
    const army = ids.filter(id => G.index.get(id).type !== 'pawn');
    if (ids.length || !add) this.select(army.length && army.length < ids.length ? army : ids, add);
  },

  clickSelect(wx, wy, add) {
    const e = this.pick(wx, wy), now = G.clock;
    if (!e) { if (!add) this.select([]); return; }
    const own = e.isUnit && e.faction === G.player;
    if (own && this.lastPick.id === e.id && now - this.lastPick.t < 0.35) { // doppio clic: tutte le unità uguali in vista
      this.select(G.units.filter(u => !u.dead && !u.inside && u.faction === G.player && u.type === e.type && onScreen(u.x, u.y, 0)).map(u => u.id), add);
    } else if (add && own && this.list.every(o => o.isUnit && o.faction === G.player)) {
      if (this.sel.has(e.id)) this.sel.delete(e.id); else this.sel.add(e.id);
      this.refresh();
    } else this.select([e.id]);
    this.lastPick = { id: e.id, t: now };
    Sfx.play('select');
  },

  // ---- ordine col tasto destro: dipende da cosa si è cliccato e da chi è selezionato
  order(wx, wy, forceAttackMove = false) {
    const own = this.mine();
    if (!own.length) return;
    const t = this.pick(wx, wy);
    const pawns = own.filter(u => u.type === 'pawn'), army = own.filter(u => u.def.dmg), monks = own.filter(u => u.type === 'monk');
    const except = group => own.filter(u => !group.includes(u));
    const send = (units, kind, target) => { if (units.length) command('order', units.map(u => u.id), kind, Math.round(wx), Math.round(wy), target ? target.id : 0); };
    let mark = 'move';
    if (t && (t.isUnit || t.isBuilding) && t.faction !== G.player) {
      const band = army.concat(monks); // i Monaci vanno dietro a chi attacca, non sul bersaglio
      send(band, 'attack', t); send(except(band), 'move');
      mark = army.length ? 'attack' : 'move';
    } else if (forceAttackMove) {
      send(own, 'amove'); mark = 'attack';
    } else if (t && NODE_RES[t.kind]) {
      send(pawns, 'gather', t); send(except(pawns), 'move');
      mark = pawns.length ? 'gather' : 'move';
    } else if (t && t.isBuilding) {
      const work = !t.done || t.hp < t.maxHp;
      send(pawns, work ? 'build' : t.def.drop ? 'drop' : 'move', t); send(except(pawns), 'move');
      mark = pawns.length && work ? 'gather' : 'move';
    } else if (t && t.isUnit && t.hp < t.maxHp && monks.length) {
      send(monks, 'heal', t); send(except(monks), 'move');
    } else send(own, 'move');
    this.mark(mark, wx, wy);
    Sfx.play('ack');
  },

  setRally(b, wx, wy) {
    const t = this.pick(wx, wy), ok = t && (NODE_RES[t.kind] || (t.isBuilding && t.faction === G.player && !t.done));
    command('rally', b.id, Math.round(wx), Math.round(wy), ok ? t.id : 0);
    b.rally = { x: wx, y: wy, t: ok ? t : null }; // subito visibile anche sui client
    this.mark('rally', wx, wy);
    Sfx.play('ack');
  },

  // ---- piazzamento: G.placing = { kind, cx, cy, ok, move = edificio che si sta spostando, last = ultima cella
  // posata, kbd = la sagoma la muovono le frecce invece del mouse }
  startPlacing(kind) {
    const d = BUILD_DEF[kind];
    // scelto da tastiera: la sagoma parte dal centro della visuale e si muove con le frecce
    G.placing = { kind, cx: Math.round((G.cam.x + G.viewW / 2) / TILE - d.w / 2), cy: Math.round((G.cam.y + G.viewH / 2) / TILE - d.h / 2), ok: false, move: null, last: null, kbd: UI.kbd };
    this.mode = null;
  },
  // Sposta un edificio: quello selezionato, altrimenti il prossimo su cui si clicca (o si arriva con le frecce).
  moveMode() {
    const b = this.list.length === 1 && this.list[0].isBuilding && this.list[0].faction === G.player ? this.list[0] : null;
    if (b) return this.startMoving(b);
    G.placing = null; this.mode = 'pickmove'; this.pickB = null;
    return null;
  },
  startMoving(b, kbd = false) {
    if (b.kind === 'castle') return 'Il castello non si sposta';
    G.placing = { kind: b.kind, cx: b.cx, cy: b.cy, ok: false, move: b, last: null, kbd };
    this.mode = null;
    return null;
  },
  // Frecce: mentre si posa, la sagoma si sposta di una casella e la visuale la segue; mentre si sceglie cosa
  // spostare, si passa all'edificio più vicino in quella direzione.
  keyMove(dx, dy) {
    const p = G.placing;
    if (p) {
      const d = BUILD_DEF[p.kind];
      p.kbd = true;
      p.cx = clamp(p.cx + dx, 0, World.W - d.w); p.cy = clamp(p.cy + dy, 0, World.H - d.h);
      const x = (p.cx + d.w / 2) * TILE, y = (p.cy + d.h / 2) * TILE, mx = G.viewW * 0.3, my = G.viewH * 0.3;
      if (x < G.cam.x + mx || x > G.cam.x + G.viewW - mx || y < G.cam.y + my || y > G.cam.y + G.viewH - my) this.lookAt(x, y);
      return;
    }
    const cur = this.pickB && !this.pickB.dead ? this.pickB : null, x0 = cur ? cur.x : G.cam.x + G.viewW / 2, y0 = cur ? cur.y : G.cam.y + G.viewH / 2;
    let best = null, bd = Infinity;
    for (const b of G.buildings) {
      if (b.faction !== G.player || b.dead || b.kind === 'castle' || b === cur) continue;
      const along = (b.x - x0) * dx + (b.y - y0) * dy, across = Math.abs((b.x - x0) * dy) + Math.abs((b.y - y0) * dx);
      if (cur && along < 8) continue;
      const d = cur ? along + across * 2 : Math.hypot(b.x - x0, b.y - y0);
      if (d < bd) { bd = d; best = b; }
    }
    if (best) { this.pickB = best; this.lookAt(best.x, best.y - 60); }
  },
  // Invio: posa la sagoma dov'è, oppure prende l'edificio scelto con le frecce.
  keyConfirm() {
    if (G.placing) { this.enter = true; return; }
    const b = this.pickB && !this.pickB.dead ? this.pickB : null, why = b ? this.startMoving(b, true) : 'Scegli un edificio con le frecce';
    if (why) G.toast(why);
    Sfx.play(why ? 'deny' : 'click');
  },
  // Pedoni per un cantiere: quelli selezionati; se non ce ne sono, i liberi più vicini, altrimenti chi sta
  // raccogliendo lì attorno (a lavoro finito torna alla sua raccolta). Per le palizzate basta chi sta già
  // costruendo lì vicino: finito un cantiere passa da solo al successivo.
  builders(kind, cx, cy) {
    const sel = this.mine().filter(u => u.type === 'pawn');
    if (sel.length) return sel.map(u => u.id);
    const d = BUILD_DEF[kind], x = (cx + d.w / 2) * TILE, y = (cy + d.h / 2) * TILE;
    const near = list => list.sort((a, b) => Math.hypot(a.x - x, a.y - y) - Math.hypot(b.x - x, b.y - y));
    const pawns = G.units.filter(u => u.faction === G.player && !u.dead && u.type === 'pawn');
    if (kind === 'wall' && pawns.some(u => u.act === 6 && Math.hypot(u.x - x, u.y - y) < 600)) return [];
    const free = near(pawns.filter(u => u.act === 0 || u.act === 8)), busy = near(pawns.filter(u => RES_KINDS.some(k => RES_ACT[k] === u.act)));
    return free.concat(busy).slice(0, d.work > 15 ? 2 : 1).map(u => u.id);
  },

  // ---- azioni di contesto per ciò che è selezionato, mostrate nel pannello della selezione
  // { code (tasto), key, icon, title, desc, on (in attesa del clic), run }
  actions() {
    const f = G.player, own = this.mine(), out = [];
    if (own.length) {
      if (own.some(u => u.def.dmg)) {
        out.push({ code: 'KeyX', key: 'X', icon: ['ui', 5], title: 'Attacco in marcia', on: this.mode === 'amove', run: () => { this.mode = 'amove'; },
          desc: 'Poi clic sul terreno: le unità vanno lì in formazione, al passo del più lento, attaccando chi incontrano; poi proseguono da sole sui nemici vicini.' });
      }
      const scouts = own.filter(u => u.def.scout);
      if (scouts.length) {
        out.push({ code: 'KeyE', key: 'E', icon: ['x', 'eye'], title: 'Esplora', run: () => command('order', scouts.map(u => u.id), 'explore', 0, 0, 0),
          desc: 'L\'Esploratore va da solo, una dopo l\'altra, nelle zone mai viste più vicine, finché ce ne sono.' });
      }
      out.push({ code: 'KeyF', key: 'F', icon: ['x', 'stop'], title: 'Ferma', desc: 'Annulla l\'ordine in corso.', run: () => command('order', own.map(u => u.id), 'stop', 0, 0, 0) });
      return out;
    }
    const b = this.list.length === 1 && this.list[0].isBuilding && this.list[0].faction === f ? this.list[0] : null;
    if (!b) return out;
    if (b.done && b.def.trains) {
      out.push({ code: 'KeyG', key: 'G', icon: ['x', 'flag'], title: 'Punto di raccolta', on: this.mode === 'rally', run: () => { this.mode = 'rally'; },
        desc: 'Poi clic sul terreno o su una risorsa (anche col tasto destro, direttamente).' });
    }
    if (b.kind !== 'castle') {
      out.push({ key: 'V', icon: ['x', 'barrow'], title: 'Sposta', desc: 'Scegli la nuova casella col mouse o con le frecce (Invio per posare), dentro il raggio in cui si può costruire.', run: () => this.startMoving(b) });
      out.push({ code: 'Delete', key: 'Canc', icon: ['ui', 9], title: b.done ? 'Demolisci' : 'Annulla cantiere',
        desc: b.done ? 'Abbatte l\'edificio, senza rimborso.' : 'Rimuove il cantiere e restituisce le risorse.', run: () => { command('demolish', b.id); this.select([]); } });
    }
    return out;
  },
  act(a) {
    const why = a.run() || null;
    if (why) G.toast(why);
    Sfx.play(why ? 'deny' : 'click');
  },

  // ---- scorciatoie
  nextIdle() {
    const idle = G.units.filter(u => u.faction === G.player && !u.dead && u.type === 'pawn' && u.act === 0);
    if (!idle.length) return;
    const u = idle[this.idleI++ % idle.length];
    this.select([u.id]);
    this.lookAt(u.x, u.y);
  },
  home() {
    const c = G.player.castle;
    if (c && !c.dead) { this.select([c.id]); this.lookAt(c.x, c.y - 80); }
  },
  group(n, assign) {
    if (assign) { this.groups[n] = this.mine().map(u => u.id); return; }
    const ids = (this.groups[n] || []).filter(id => G.index.has(id));
    if (!ids.length) return;
    this.select(ids);
    if (this.lastGroup.n === n && G.clock - this.lastGroup.t < 0.4) this.centerOnSelection();
    this.lastGroup = { n, t: G.clock };
  },
  centerOnSelection() {
    if (!this.list.length) return;
    const e = this.list[0];
    this.lookAt(e.x, e.y - (e.isBuilding ? 80 : 0));
  },
  army() { return G.units.filter(u => u.faction === G.player && !u.dead && u.type !== 'pawn'); },
  selectArmy() {
    const ids = this.army().map(u => u.id);
    if (!ids.length) { G.toast('Non hai truppe'); Sfx.play('deny'); return; }
    this.select(ids);
    Sfx.play('select');
  },
  // Riporta le truppe (tutte, se non indicate) davanti al castello: in marcia non si fermano a combattere.
  recall(units = this.army()) {
    const c = G.player.castle;
    if (!units.length || !c) { G.toast('Non hai truppe'); Sfx.play('deny'); return; }
    command('recall', units.map(u => u.id));
    this.mark('move', c.x, c.y1 + 130);
    Sfx.play('ack');
  },

  // ---- a ogni fotogramma: telecamera e mouse sulla mappa
  update(dt) {
    const m = Input.mouse, k = Input.keys, free = !UI.hot && !UI.locked;
    this.refresh();
    for (const s of this.markers) s.t += dt;
    this.markers = this.markers.filter(s => s.t < 0.6);
    for (const p of this.pings) p.t += dt;
    this.pings = this.pings.filter(p => p.t < 4);

    // telecamera: frecce o WASD (non mentre muovono la sagoma di un edificio), bordo della finestra; Maiusc e Ctrl
    // la fanno andare più svelta; rotellina (o + e −, vedi handleKeys) per lo zoom
    const keys = !UI.panel && !G.placing && this.mode !== 'pickmove';
    let mx = keys ? held(k, 'ArrowRight', 'KeyD') - held(k, 'ArrowLeft', 'KeyA') : 0, my = keys ? held(k, 'ArrowDown', 'KeyS') - held(k, 'ArrowUp', 'KeyW') : 0;
    if (m.inside && !this.drag && document.hasFocus()) {
      const edge = EDGE * G.dpr;
      if (m.x < edge) mx = -1; else if (m.x > G.W - edge) mx = 1;
      if (m.y < edge) my = -1; else if (m.y > G.H - edge) my = 1;
    }
    const speed = CAM_SPEED * (held(k, 'ControlLeft', 'ControlRight') ? CAM_FAST : held(k, 'ShiftLeft', 'ShiftRight') ? CAM_MEDIUM : 1);
    G.cam.x += mx * speed * dt; G.cam.y += my * speed * dt;
    if (Input.wheel && !UI.hot) setZoom(Input.wheel < 0 ? 1 : -1, m.x, m.y);
    clampCamera();
    updateMouse();
    if (G.spectate || !G.player || !G.player.alive || G.state !== 'play' || G.paused || UI.help) { this.drag = null; return; }

    // gli ordini vanno al punto dove il tasto è stato premuto: prima del fotogramma il mouse può essersi già mosso
    const shift = k.has('ShiftLeft') || k.has('ShiftRight'), z = G.zoom;
    const lx = G.cam.x + Input.downX / z, ly = G.cam.y + Input.downY / z;
    if (G.placing) { this.place(shift, free, lx, ly); return; }
    if (Input.rpressed && free) {
      const rx = G.cam.x + Input.rdownX / z, ry = G.cam.y + Input.rdownY / z;
      const b = this.list.length === 1 && this.list[0].isBuilding && this.list[0].faction === G.player && this.list[0].def.trains ? this.list[0] : null;
      if (this.mode) this.mode = null;
      else if (b && b.done) this.setRally(b, rx, ry);
      else this.order(rx, ry);
    }
    if (Input.pressed && free) {
      if (this.mode === 'amove') { this.order(lx, ly, true); this.mode = null; }
      else if (this.mode === 'rally') { if (this.list[0] && this.list[0].isBuilding) this.setRally(this.list[0], lx, ly); this.mode = null; }
      else if (this.mode === 'pickmove') {
        const t = this.pick(lx, ly), why = t && t.isBuilding && t.faction === G.player ? this.startMoving(t) : 'Clicca un tuo edificio';
        if (why) G.toast(why);
        Sfx.play(why ? 'deny' : 'click');
      } else this.drag = { x: Input.downX, y: Input.downY };
    }
    if (this.drag && Input.released) {
      const d = this.drag;
      this.drag = null;
      if (Math.hypot(m.x - d.x, m.y - d.y) > DRAG_MIN * G.dpr) this.boxSelect(d.x, d.y, m.x, m.y, shift);
      else this.clickSelect(lx, ly, shift);
    }
  },

  // Posizionamento di un edificio (nuovo, o uno esistente che si sposta): la sagoma segue il mouse, il clic
  // lo posa dove è stato premuto; oppure la muovono le frecce e la posa Invio. Le palizzate, con Maiusc e il
  // tasto tenuto premuto, si posano in fila trascinando.
  place(shift, free, lx, ly) {
    const p = G.placing, d = BUILD_DEF[p.kind], f = G.player, click = Input.pressed && free, key = this.enter, press = click || key;
    this.enter = false;
    if (p.move && p.move.dead) { G.placing = null; return; }
    if (Input.moved || click) p.kbd = false; // il mouse riprende la sagoma
    if (!p.kbd) {
      p.cx = Math.round((click ? lx : G.mouseWX) / TILE - d.w / 2);
      p.cy = Math.round((click ? ly : G.mouseWY) / TILE - d.h / 2);
    }
    const seen = (cx, cy) => Fog.knows((cx + d.w / 2) * TILE, (cy + d.h / 2) * TILE);
    const why = (cx, cy) => (p.move ? moveBlock(f, p.move, cx, cy)
      : !canAfford(f, d.cost) ? 'Risorse insufficienti' : zoneBlock(f, p.kind, cx, cy) || (canPlace(p.kind, cx, cy) && seen(cx, cy) ? null : 'Qui non si può costruire'));
    const no = why(p.cx, p.cy);
    p.ok = no === null;
    if (Input.rpressed) { G.placing = null; return; }
    if (press) {
      const refused = no || (p.move ? command('move', p.move.id, p.cx, p.cy) : command('place', p.kind, p.cx, p.cy, this.builders(p.kind, p.cx, p.cy)));
      if (refused) { Sfx.play('deny'); G.toast(refused); return; }
      Sfx.play('click');
      p.last = [p.cx, p.cy];
      if (p.move || !shift) G.placing = null; // con Maiusc si continua a piazzare
      return;
    }
    // (il rilascio conta ancora come tasto premuto: l'ultimo tratto può arrivare nello stesso fotogramma)
    if (p.kind === 'wall' && !p.move && shift && free && p.last && (Input.lmb || Input.released) && (p.last[0] !== p.cx || p.last[1] !== p.cy)) {
      const [x0, y0] = p.last, n = Math.max(Math.abs(p.cx - x0), Math.abs(p.cy - y0));
      const mine = this.mine().some(u => u.type === 'pawn'); // la fila la costruisce chi ha preso la prima cella
      for (let i = 1; i <= n; i++) { // tutte le celle fra l'ultima posata e quella sotto il mouse
        const cx = Math.round(x0 + (p.cx - x0) * i / n), cy = Math.round(y0 + (p.cy - y0) * i / n);
        if (why(cx, cy) === null) command('place', 'wall', cx, cy, mine ? this.builders('wall', cx, cy) : []);
      }
      p.last = [p.cx, p.cy];
    }
    if (!Input.lmb) p.last = null;
  },

  // Cursore adatto a ciò che il clic destro farebbe qui.
  cursor() {
    if (G.placing) return G.placing.ok ? 'arrow' : 'no';
    if (this.mode === 'amove') return 'attack';
    if (this.mode === 'pickmove') {
      const b = UI.hot ? null : this.pick(G.mouseWX, G.mouseWY);
      return b && b.isBuilding && b.faction === G.player && b.kind !== 'castle' ? 'hand' : 'arrow';
    }
    const own = this.mine();
    if (!own.length || UI.hot) return 'arrow';
    const t = this.pick(G.mouseWX, G.mouseWY), pawn = own.some(u => u.type === 'pawn');
    if (!t) return 'arrow';
    if ((t.isUnit || t.isBuilding) && t.faction !== G.player) return own.some(u => u.def.dmg) ? 'attack' : 'arrow';
    if (pawn && t.kind === 'tree') return 'axe';
    if (pawn && (t.kind === 'gold' || t.kind === 'stone')) return 'pick';
    if (pawn && t.kind === 'sheep') return 'knife';
    if (pawn && t.isBuilding && (!t.done || t.hp < t.maxHp)) return 'hammer';
    return 'arrow';
  },

  // Riquadro di selezione, segni degli ordini e sagoma dell'edificio da piazzare (in coordinate mondo).
  drawWorld(ctx) {
    for (const s of this.markers) {
      const k = s.t / 0.6;
      ctx.strokeStyle = MARK_COLOR[s.kind]; ctx.lineWidth = 3; ctx.globalAlpha = 1 - k;
      ctx.beginPath(); ctx.ellipse(s.x, s.y, 26 * (1 - k * 0.6), 11 * (1 - k * 0.6), 0, 0, Math.PI * 2); ctx.stroke();
      ctx.globalAlpha = 1;
    }
    const p = G.placing, me = G.player, pb = this.mode === 'pickmove' && this.pickB && !this.pickB.dead ? this.pickB : null;
    if (pb) drawBrackets(ctx, pb.x0 - 10, pb.y0 - 14, pb.x1 - pb.x0 + 20, pb.y1 - pb.y0 + 28, pb.def.w > 1 ? 1 : 0.7);
    if (p) {
      const d = BUILD_DEF[p.kind], im = buildingImg(me.color, p.kind, p.move ? p.move.variant : 0);
      const x0 = p.cx * TILE, y0 = p.cy * TILE, w = d.w * TILE, h = d.h * TILE, c = me.castle;
      // fin dove si può posare: attorno al castello (vicino per torri e palizzate, molto più in là per il resto) e ai propri avamposti
      ctx.strokeStyle = 'rgba(255,255,255,0.75)'; ctx.lineWidth = 3; ctx.setLineDash([14, 10]);
      const ring = (x, y, r) => { ctx.beginPath(); ctx.arc(x * TILE, y * TILE, r * TILE, 0, Math.PI * 2); ctx.stroke(); };
      if (c) ring(c.cx + c.def.w / 2, c.cy + c.def.h / 2, d.defense ? DEFENSE_RADIUS : BUILD_RADIUS);
      for (const o of G.points) if (o.owner === me && o.def.zone) ring(o.gx, o.gy, OUTPOST_RADIUS);
      ctx.setLineDash([]);
      ctx.fillStyle = p.ok ? 'rgba(120,255,140,0.28)' : 'rgba(255,70,60,0.38)';
      ctx.fillRect(x0, y0, w, h);
      ctx.globalAlpha = 0.65;
      ctx.drawImage(im, Math.round(x0 + w / 2 - d.ax), Math.round(y0 + h - 4 - d.ay));
      ctx.globalAlpha = 1;
      drawBrackets(ctx, x0 - 8, y0 - 8, w + 16, h + 16, d.w > 1 ? 1 : 0.6);
    }
  },
  drawScreen(ctx) {
    const d = this.drag, m = Input.mouse;
    if (!d || Math.hypot(m.x - d.x, m.y - d.y) <= DRAG_MIN * G.dpr) return;
    ctx.fillStyle = 'rgba(255,255,255,0.12)'; ctx.strokeStyle = '#ffffff'; ctx.lineWidth = Math.round(2 * G.dpr);
    ctx.fillRect(d.x, d.y, m.x - d.x, m.y - d.y);
    ctx.strokeRect(Math.round(d.x) + 0.5, Math.round(d.y) + 0.5, Math.round(m.x - d.x), Math.round(m.y - d.y));
  },
};
