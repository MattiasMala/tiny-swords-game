'use strict';
// Stato di gioco, input, ciclo principale e rendering del mondo.

const cv = document.getElementById('game');
const ctx = cv.getContext('2d');
// Passi di zoom, in pixel fisici per pixel di sprite: multipli di 0,25, così 64px * zoom resta intero
// e i tile combaciano senza fessure.
const ZOOMS = Array.from({ length: 17 }, (_, i) => 0.5 + i * 0.25);

const Input = {
  keys: new Set(), hits: [], mouse: { x: 0, y: 0, inside: false },
  navs: [],     // frecce (o WASD) premute dall'ultimo fotogramma, ripetizioni comprese (vedi handleNav)
  moved: false, // il mouse si è spostato dall'ultimo fotogramma
  lmb: false, rmb: false, pressed: false, released: false, rpressed: false, wheel: 0,
  downX: 0, downY: 0, rdownX: 0, rdownY: 0, // dove è stato premuto l'ultima volta il tasto sinistro / destro
};

const G = {
  state: 'loading', error: null,
  W: 0, H: 0, zoom: 1, us: 1, viewW: 0, viewH: 0, cam: { x: 0, y: 0 },
  dpr: 1, // pixel fisici dello schermo per pixel CSS (1.6 con lo schermo al 160%)
  time: 0, clock: 0, demo: true, paused: false, placing: null, result: null, endT: 0, endTime: 0, demoT: 0,
  frozen: false,   // modalità test: il tempo è fermo, ma si guarda in giro e si danno ordini (vedi drawTestScreen)
  spectate: false, // sconfitto in una partita in rete, resto a guardare
  factions: [], units: [], buildings: [], trees: [], golds: [], stones: [], sheep: [], arrows: [], fx: [], waterFx: [],
  points: [],                // punti da conquistare
  pickups: [], floats: [], banners: [], clouds: [], decor: [],
  index: new Map(),          // id -> unità, edificio o risorsa
  player: null, setup: null, // player = fazione del giocatore locale
  toastT: 0, toastText: '', sheepT: 5, pickT: 0, pointT: 0, splashT: 2, mouseWX: 0, mouseWY: 0,

  // faction = colore del nastro (null: quello di chi legge); to = fazione destinataria (null: tutti).
  announce(text, faction, dur = 3, to = null) {
    emit(['say', to ? to.idx : -1, text, faction ? faction.idx : -1, dur]);
  },
  toast(text) { this.toastText = text; this.toastT = 2; },

  reset() {
    UI.panel = null;
    NEXT_ID = 1; // host e client numerano alberi e giacimenti allo stesso modo: gli ordini li indicano per id
    this.index = new Map();
    this.time = 0; this.paused = false; this.frozen = false; this.placing = null; this.result = null; this.endT = 0; this.demoT = 0;
    this.spectate = false; this.toastT = 0; this.sheepT = 5; this.pickT = 0; this.pointT = 0; this.player = null;
    this.factions = []; this.units = []; this.buildings = []; this.sheep = []; this.arrows = []; this.fx = [];
    this.waterFx = []; this.pickups = []; this.floats = []; this.banners = [];
  },

  // Alberi, giacimenti, punti da conquistare, decorazioni e nuvole del mondo corrente.
  scenery() {
    const bigs = kind => World.bigs.filter(b => b.kind === kind);
    this.trees = World.trees.map(t => makeTree(t.c, t.v));
    this.golds = [...World.golds.map(g => makeGold(g.c, g.amount)), ...bigs('gold').map(g => makeGold(g.c, g.amount, true))];
    this.stones = [...World.stones.map(s => makeStone(s.c, s.amount)), ...bigs('stone').map(s => makeStone(s.c, s.amount, true))];
    this.points = World.points.map(p => makePoint(p.c, p.kind));
    this.decor = [
      ...World.bushes.map(d => ({ ...d, sy: d.y, draw: drawBush })),
      ...World.rocks.map(d => ({ ...d, sy: d.y, draw: drawRock })),
      ...World.props.map(p => makeProp(p.name, p.x, p.y)),
    ];
    const mw = World.W * TILE, mh = World.H * TILE;
    this.clouds = Array.from({ length: Math.round(8 * World.W * World.H / MAP_AREA) }, (_, i) => ({
      im: img(`${TERR}Decorations/Clouds/Clouds_0${i % 8 + 1}.png`), x: rand(-300, mw), y: rand(-100, mh - 150), v: rand(7, 16),
    }));
  },

  // setup = { players: [{ color, local, conn }], level }: i giocatori umani (conn = id di rete) e
  // la difficoltà dell'IA. Senza giocatori è la battaglia dimostrativa dietro al menu.
  start(setup) {
    const humans = setup.players;
    this.setup = setup;
    this.demo = !humans.length;
    this.state = this.demo ? 'menu' : 'play';
    this.reset();
    World.generate((Math.random() * 1e9) | 0);
    this.scenery();
    for (const m of World.meadows) for (let i = 0; i < 3; i++) makeSheep(m);
    for (const t of World.treasures) dropPickup(t.kind, World.cx(t.c), World.cy(t.c) + 6, TREASURE, Infinity, true);

    // le basi sono equivalenti: i regni vi sono distribuiti a sorte
    const order = COLORS.slice().sort(() => Math.random() - 0.5);
    this.factions = order.map((c, i) => {
      const p = humans.find(p => p.color === c), f = makeFaction(c, World.bases[i], !!p, setup.level || 'normal');
      f.idx = i;
      if (p) { f.conn = p.conn || 0; if (p.local) this.player = f; }
      return f;
    });
    for (const f of this.factions) setupFaction(f);

    Ctl.reset();
    Fog.reset(!this.demo);
    const focus = (this.player || this.factions[0]).castle;
    Ctl.lookAt(focus.x, focus.y);
    Fog.reveal(focus.x, focus.y, START_SIGHT); // i dintorni della base sono già esplorati
    if (!this.demo) this.announce('Distruggi i castelli nemici!', null, 3.5);
  },

  toMenu() { UI.help = false; Net.leave(); this.start({ players: [] }); },

  // Chiamata quando cade un regno (e, sui client, a ogni aggiornamento dall'host).
  checkEnd() {
    const alive = this.factions.filter(f => f.alive);
    if (this.demo) { if (alive.length <= 1 && this.demoT <= 0) this.demoT = 8; return; }
    if (this.spectate && alive.length <= 1) { this.spectate = false; this.state = 'over'; }
    if (this.result) return;
    if (!this.player.alive) this.result = 'lose';
    else if (alive.length === 1) this.result = 'win';
    if (this.result) {
      this.endT = 2.5; this.endTime = this.time; this.placing = null;
      Ctl.select([]);
      Fog.on = false; // a partita finita si vede tutta la mappa
      Sfx.play(this.result);
    }
  },
};

// ---------------------------------------------------------------- input

// Il canvas ha un pixel per ogni pixel fisico dello schermo. Con la scala dello schermo sopra il 100% un canvas
// grande quanto la finestra in pixel CSS verrebbe ingrandito dal browser, e le scritte uscirebbero sfocate.
// Da qui in poi G.W, G.H e il mouse sono in pixel fisici; l'interfaccia si adatta tramite G.us e lo zoom.
function zoomSteps() {
  const steps = ZOOMS.filter(z => z >= 0.5 * G.dpr && z <= 1.5 * G.dpr);
  return steps.length ? steps : [ZOOMS[0]];
}
// Risoluzione del disegno rispetto ai pixel fisici dello schermo. Piena di norma. Su schermi molto densi quasi
// tutto il costo di un fotogramma è riempire pixel: se il gioco resta lento anche a luci spente (vedi Light.watch)
// la risoluzione scende da sola, e il browser ingrandisce la tela; F3 la cambia a mano.
const Quality = {
  steps: [1, 0.75, 0.5], i: 0, auto: true, slow: 0,
  get scale() { return this.steps[this.i]; },
  set(i, note) { this.i = i; this.slow = 0; resize(); G.toast(note); },
  cycle() { this.auto = false; this.set((this.i + 1) % this.steps.length, ['Risoluzione piena', 'Risoluzione al 75%', 'Risoluzione al 50%'][(this.i + 1) % this.steps.length]); },
  // dt = secondi veri fra due fotogrammi. Tre secondi sotto i 33 fotogrammi al secondo, con le luci già spente
  // (o scelte a mano), e scende di un passo.
  watch(dt) {
    if (!this.auto || this.i === this.steps.length - 1 || (Light.auto && Light.level > 0) || dt > 0.25) return;
    this.slow = dt > 0.03 ? this.slow + dt : Math.max(0, this.slow - dt * 2);
    if (this.slow >= 3) this.set(this.i + 1, 'Il gioco rallentava: risoluzione ridotta (F3 per cambiarla)');
  },
};

// w, h = misura esatta in pixel fisici, quando il browser la fornisce; altrimenti finestra * scala, arrotondata.
function resize(w, h) {
  const d = window.devicePixelRatio || 1, q = Quality.scale;
  G.fullW = w || Math.round(window.innerWidth * d); G.fullH = h || Math.round(window.innerHeight * d);
  G.W = cv.width = Math.round(G.fullW * q);
  G.H = cv.height = Math.round(G.fullH * q);
  cv.style.imageRendering = q < 1 ? 'auto' : 'pixelated'; // ingrandita dal browser: meglio sfumata che a pixel disuguali
  G.dpr = G.W / window.innerWidth;
  const fit = Math.min(G.W / 1500, G.H / 850), steps = zoomSteps();
  G.zoom = G.zoomChosen ? clamp(G.zoom, steps[0], steps[steps.length - 1]) : steps.filter(z => z <= fit).pop() || steps[0];
  G.us = clamp(fit, 0.72 * G.dpr, 1.5 * G.dpr);
  G.viewW = G.W / G.zoom; G.viewH = G.H / G.zoom;
}
// Zoom di uno scatto tenendo fermo il punto del mondo sotto il mouse.
function setZoom(dir, sx, sy) {
  const steps = zoomSteps(), z = steps[clamp(steps.indexOf(G.zoom) + dir, 0, steps.length - 1)];
  if (z === G.zoom) return;
  const wx = G.cam.x + sx / G.zoom, wy = G.cam.y + sy / G.zoom;
  G.zoom = z; G.zoomChosen = true;
  G.viewW = G.W / z; G.viewH = G.H / z;
  G.cam.x = wx - sx / z; G.cam.y = wy - sy / z;
  clampCamera();
}
window.addEventListener('resize', () => resize());
// La misura esatta in pixel fisici, dove il browser la fornisce: l'arrotondamento di larghezza * scala può
// sbagliare di un pixel, e basta quello perché il browser riscali tutto il canvas. Vale solo come correzione
// di quell'arrotondamento: con la scala emulata dagli strumenti per sviluppatori di Chromium questa misura
// ignora la scala (1488 invece di 2381 a 1.6) e va scartata.
try {
  new ResizeObserver(entries => {
    const box = entries[0].devicePixelContentBoxSize, d = window.devicePixelRatio || 1;
    if (!box) return;
    const w = box[0].inlineSize, h = box[0].blockSize;
    if (Math.abs(w - window.innerWidth * d) < 1.5 && Math.abs(h - window.innerHeight * d) < 1.5 && (w !== G.fullW || h !== G.fullH)) resize(w, h);
  }).observe(cv, { box: 'device-pixel-content-box' });
} catch (err) { /* browser senza questa misura: resta l'arrotondamento */ }
function mouseAt(e) {
  const x = e.clientX * G.dpr, y = e.clientY * G.dpr;
  if (Math.abs(x - Input.mouse.x) + Math.abs(y - Input.mouse.y) > 2) { Input.moved = true; UI.kbd = false; } // il mouse riprende la mano alla tastiera
  Input.mouse.x = x; Input.mouse.y = y; Input.mouse.inside = true;
}

// Al browser si tolgono i tasti che userebbe per sé: frecce e spazio (scorrono la pagina), Tab, i tasti funzione
// del gioco; e WASD e +/−, che con Ctrl premuto (visuale veloce) sarebbero «seleziona tutto», «salva», «segnalibro»
// e lo zoom della pagina. Ctrl+W invece chiude la scheda e una pagina non può impedirlo: vedi beforeunload.
// + e − si riconoscono dal carattere, non dalla posizione del tasto: sulla tastiera italiana stanno altrove.
window.addEventListener('keydown', e => {
  Sfx.unlock();
  const zoom = e.key === '+' || (e.key === '=' && e.code === 'Equal') ? 'ZoomIn' : e.key === '-' ? 'ZoomOut' : null;
  if (zoom || NAV[e.code] || ['Space', 'Tab', 'F1', 'F2', 'F3', 'F4', 'Backspace', 'Enter'].includes(e.code)) e.preventDefault();
  if (!e.repeat) { Input.hits.push(e.code); if (zoom) Input.hits.push(zoom); }
  if (NAV[e.code]) Input.navs.push(e.code);
  Input.keys.add(e.code);
});
// A partita in corso il browser chiede conferma prima di chiudere o ricaricare la pagina: Ctrl+W (visuale veloce
// verso l'alto) altrimenti butterebbe via la partita.
window.addEventListener('beforeunload', e => {
  if (G.state === 'play' && !G.result) { e.preventDefault(); e.returnValue = ''; }
});
window.addEventListener('keyup', e => Input.keys.delete(e.code));
window.addEventListener('blur', () => { Input.keys.clear(); Input.lmb = Input.rmb = false; });
cv.addEventListener('mousemove', mouseAt);
cv.addEventListener('mouseleave', () => { Input.mouse.inside = false; });
cv.addEventListener('mousedown', e => {
  Sfx.unlock();
  mouseAt(e);
  if (e.button === 0) { Input.lmb = true; Input.pressed = true; Input.downX = Input.mouse.x; Input.downY = Input.mouse.y; }
  if (e.button === 2) { Input.rmb = true; Input.rpressed = true; Input.rdownX = Input.mouse.x; Input.rdownY = Input.mouse.y; }
});
window.addEventListener('mouseup', e => {
  if (e.button === 0) { Input.lmb = false; Input.released = true; }
  if (e.button === 2) Input.rmb = false;
});
cv.addEventListener('wheel', e => { e.preventDefault(); Input.wheel += Math.sign(e.deltaY); }, { passive: false });
cv.addEventListener('contextmenu', e => e.preventDefault());

// Frecce o WASD, e Invio o Spazio. Con una schermata aperta (menu, pausa, gestione) spostano l'evidenziatore fra i
// pulsanti e li premono; mentre si posa o si sposta un edificio muovono la sagoma di una casella alla volta e la
// posano; altrimenti muovono la visuale (vedi Ctl.update).
const NAV = {
  ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1],
  KeyA: [-1, 0], KeyD: [1, 0], KeyW: [0, -1], KeyS: [0, 1],
};
function handleNav() {
  const modal = G.state === 'menu' || G.state === 'over' || G.paused || UI.help || !!UI.panel;
  const placing = !modal && G.state === 'play' && !G.spectate && !G.result && (G.placing || Ctl.mode === 'pickmove');
  const enter = Input.hits.includes('Enter') || Input.hits.includes('NumpadEnter') || Input.hits.includes('Space');
  if (!modal) UI.kbd = false;
  for (const code of Input.navs) {
    if (modal) UI.navigate(NAV[code][0], NAV[code][1]);
    else if (placing) Ctl.keyMove(NAV[code][0], NAV[code][1]);
  }
  Input.navs.length = 0;
  if (enter && modal) UI.press();
  else if (enter && placing) Ctl.keyConfirm();
}

function handleKeys() {
  const shift = Input.keys.has('ShiftLeft') || Input.keys.has('ShiftRight');
  for (const code of Input.hits) {
    if (code === 'KeyM') Sfx.enabled = !Sfx.enabled;
    if (code === 'F1') UI.help = !UI.help;
    if (code === 'F2') Light.cycle();
    if (code === 'F3') Quality.cycle();
    if (G.state !== 'play' || G.result) { if (code === 'Escape') UI.help = false; continue; }
    if (code === 'Escape') { // chiude, nell'ordine: aiuto, schermata, edificio da piazzare, ordine in attesa, selezione; poi pausa
      if (UI.help) UI.help = false;
      else if (UI.panel) UI.panel = UI.panel === 'send' ? 'units' : null; // dalla mappa d'invio si torna a Unità
      else if (G.placing) G.placing = null;
      else if (Ctl.mode) Ctl.mode = null;
      else if (Ctl.sel.size && !G.paused) Ctl.select([]);
      else G.paused = !G.paused;
    }
    if (code === 'KeyP') G.paused = !G.paused;
    if (G.paused || UI.help) continue;
    if (code === 'F4') { // schermata della modalità test: solo da soli, in rete falserebbe la partita degli altri
      if (Net.playing) { G.toast('La modalità test c\'è solo nelle partite da soli'); Sfx.play('deny'); } else UI.open('test');
      continue;
    }
    const shortcut = SHORTCUTS.find(c => c.code === code);
    if (shortcut && (shortcut.panel || !UI.panel)) { runShortcut(shortcut); continue; }
    if (UI.panel) continue; // con una schermata aperta i tasti dei comandi non agiscono sulla partita
    if (code === 'ZoomIn' || code === 'ZoomOut') setZoom(code === 'ZoomIn' ? 1 : -1, G.W / 2, G.H / 2); // attorno al centro dello schermo
    else if (code === 'Period') Ctl.nextIdle();
    else if (code === 'KeyH') Ctl.home();
    else if (code === 'Space') { if (!G.placing && Ctl.mode !== 'pickmove') Ctl.centerOnSelection(); } // lì Spazio posa, come Invio
    else if (/^Digit[1-9]$/.test(code)) Ctl.group(+code[5], shift);
    else {
      const action = Ctl.actions().find(a => a.code === code);
      if (action) Ctl.act(action);
    }
  }
}

// ---------------------------------------------------------------- update

function clampCamera() {
  const mw = World.W * TILE, mh = World.H * TILE, bottom = 150 * G.us / G.zoom; // spazio sotto la mappa per il pannello
  G.cam.x = mw > G.viewW ? clamp(G.cam.x, 0, mw - G.viewW) : (mw - G.viewW) / 2;
  G.cam.y = mh > G.viewH ? clamp(G.cam.y, 0, mh - G.viewH + bottom) : (mh - G.viewH) / 2;
}

function updateMouse() {
  G.mouseWX = G.cam.x + Input.mouse.x / G.zoom;
  G.mouseWY = G.cam.y + Input.mouse.y / G.zoom;
}

// Simulazione: partita in singolo, battaglia del menu, host di una partita in rete.
function updateWorld(dt) {
  G.time += dt;
  World.refresh = FIELD_REFRESH;
  World.fresh = FIELD_NEW;
  for (const f of G.factions) updateFaction(f, dt);
  for (const u of G.units) if (!u.dead) updateUnit(u, dt);
  separateUnits();
  G.units = G.units.filter(u => !u.dead);
  for (const b of G.buildings) if (!b.dead) updateBuilding(b, dt);
  G.buildings = G.buildings.filter(b => !b.dead);
  updateResources(dt);
  updatePoints(dt);
  updateArrows(dt);
  updatePickups(dt);
  updateFx(dt);
  updateView(dt);
  if (G.demo && G.demoT > 0 && (G.demoT -= dt) <= 0) G.start({ players: [] });
}

// Telecamera, comandi del giocatore, nebbia, ambiente e avvisi: comune a chi simula e a chi riceve lo stato dall'host.
function updateView(dt) {
  const mw = World.W * TILE, mh = World.H * TILE;
  if (G.demo) {
    G.cam.x = (mw - G.viewW) / 2 + Math.cos(G.clock * 0.04) * Math.max(0, (mw - G.viewW) / 2 - 150);
    G.cam.y = (mh - G.viewH) / 2 + Math.sin(G.clock * 0.055) * Math.max(0, (mh - G.viewH) / 2 - 150);
    updateMouse();
  } else Ctl.update(dt);
  Fog.update(dt);

  for (const c of G.clouds) { c.x += c.v * dt; if (c.x > mw + 100) { c.x = -600; c.y = rand(-100, mh - 150); } }
  for (const f of G.waterFx) f.t += dt;
  G.waterFx = G.waterFx.filter(f => f.t * f.sh.fps < f.sh.n);
  if ((G.splashT -= dt) <= 0) {
    G.splashT = rand(0.4, 1.2);
    const c = pick(World.openWater);
    if (c !== undefined) G.waterFx.push({ sh: FX.splash(), x: World.cx(c), y: World.cy(c), t: 0 });
  }

  if (G.banners.length && (G.banners[0].t += dt) >= G.banners[0].dur) G.banners.shift();
  G.toastT -= dt;
  if (G.result && G.state === 'play' && !G.spectate && (G.endT -= dt) <= 0) G.state = 'over';
}

function update(dt) {
  G.clock += dt;
  handleNav();
  handleKeys();
  Input.hits.length = 0;
  // in rete la partita non si ferma: né per la pausa né quando il giocatore locale ha finito
  if (Net.remote) Net.clientUpdate(dt);
  else if (G.state === 'menu' || Net.hosting || (G.state === 'play' && !G.paused && !UI.help)) {
    if (G.frozen) updateView(dt); else updateWorld(dt); // a tempo fermo restano visuale, comandi e nebbia
  }
  else if (G.state !== 'loading') Ctl.refresh();
  if (Net.hosting) Net.hostTick(dt);
}

// ---------------------------------------------------------------- render

// Colore dell'acqua: "Water Background color.png" del pacchetto è tutta di questo colore (lo stesso della minimappa
// e della schermata di caricamento). Una tinta unita costa molto meno del motivo ripetuto e ingrandito con lo zoom.
const WATER_COLOR = '#47aba9';
const visible = [];

function renderWorld() {
  const z = G.zoom, cam = G.cam, vw = G.viewW, vh = G.viewH, t = G.time, me = G.player;
  ctx.setTransform(z, 0, 0, z, -Math.round(cam.x * z), -Math.round(cam.y * z));

  ctx.fillStyle = WATER_COLOR;
  ctx.fillRect(Math.floor(cam.x) - 1, Math.floor(cam.y) - 1, vw + 3, vh + 3);

  const inView = (x, y, m) => x > cam.x - m && y > cam.y - m && x < cam.x + vw + m && y < cam.y + vh + m;

  const foam = sheet(TERR + 'Tileset/Water Foam.png', 192, 96, 96, 10), ff = Math.floor(t * foam.fps);
  for (const c of World.foam) {
    const x = World.cx(c), y = World.cy(c);
    if (inView(x, y, 100)) drawSheet(ctx, foam, ff, x, y);
  }
  for (const r of World.waterRocks) {
    if (!inView(r.x, r.y, 60)) continue;
    const sh = sheet(`${TERR}Decorations/Rocks in the Water/Water Rocks_0${r.v + 1}.png`, 64, 32, 36, 10);
    drawSheet(ctx, sh, Math.floor(t * sh.fps + r.phase), r.x, r.y);
  }
  for (const f of G.waterFx) if (inView(f.x, f.y, 100)) drawSheet(ctx, f.sh, Math.floor(f.t * f.sh.fps), f.x, f.y);
  const duck = sheet(TERR + 'Decorations/Rubber Duck/Rubber duck.png', 32, 16, 18, 4);
  for (const d of World.ducks) {
    const r = d.pond ? 18 : 46, a = t * 0.3 + d.phase;
    const x = d.x + Math.sin(a) * r, y = d.y + Math.cos(a * 0.8) * r * 0.4 + Math.sin(t * 2 + d.phase) * 1.5;
    if (inView(x, y, 40)) drawSheet(ctx, duck, Math.floor(t * duck.fps + d.phase), x, y, Math.cos(a) < 0);
  }

  World.drawGround(ctx, cam, vw, vh);
  Light.tick();

  // entità ordinate per profondità; dei nemici si vede solo ciò che è in vista (gli edifici restano noti)
  visible.length = 0;
  for (const d of G.decor) if (inView(d.x, d.y, 100)) visible.push(d);
  for (const e of G.trees) if (inView(e.x, e.y, 260)) visible.push(e);
  for (const e of G.golds) if (inView(e.x, e.y, 90)) visible.push(e);
  for (const e of G.stones) if (inView(e.x, e.y, 110)) visible.push(e);
  for (const e of G.points) if (inView(e.x, e.y, 260)) visible.push(e);
  for (const e of G.sheep) if (inView(e.x, e.y, 90) && Fog.sees(e.x, e.y)) { e.sy = e.y; visible.push(e); }
  for (const e of G.buildings) if (inView(e.x, e.y, 340) && (e.faction === me || Fog.knows(e.x, e.y - 20))) visible.push(e);
  for (const e of G.units) if (!e.inside && inView(e.x, e.y, 200) && (e.faction === me || Fog.sees(e.x, e.y))) { e.sy = e.y; visible.push(e); }
  for (const e of G.pickups) if (inView(e.x, e.y, 80) && Fog.knows(e.x, e.y)) { e.sy = e.y; visible.push(e); }
  for (const e of G.fx) if (inView(e.x, e.y, 200) && Fog.sees(e.x, e.y)) visible.push(e);
  visible.sort((a, b) => a.sy - b.sy);
  Light.shadows(ctx, visible);
  for (const e of visible) e.draw(ctx, e);
  drawArrows(ctx);

  ctx.globalAlpha = 0.6;
  for (const c of G.clouds) if (inView(c.x + 288, c.y + 128, 320)) ctx.drawImage(c.im, Math.round(c.x), Math.round(c.y));
  ctx.globalAlpha = 1;
  Light.apply(ctx, visible);

  // nebbia: un pixel per cella, ingrandito con interpolazione perché i bordi sfumino
  ctx.imageSmoothingEnabled = true;
  ctx.drawImage(Fog.cv, 0, 0, World.W * TILE, World.H * TILE);
  ctx.imageSmoothingEnabled = false;

  // la passata finale tocca solo il mondo: barre della vita, nastri, sagome e segnali restano nitidi sopra
  const world = ctx.getTransform();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  Light.post(ctx);
  ctx.setTransform(world);
  drawOverlays(ctx, visible);
  if (!G.demo) Ctl.drawWorld(ctx);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
}

function renderLoading() {
  ctx.fillStyle = '#47aba9';
  ctx.fillRect(0, 0, G.W, G.H);
  const d = G.dpr;
  if (G.error) {
    label(ctx, G.error, G.W / 2, G.H / 2, 18 * d, '#fff', 'center');
    return;
  }
  const w = Math.min(420 * d, G.W * 0.6), x = (G.W - w) / 2, y = G.H / 2;
  label(ctx, 'Tiny Swords', G.W / 2, y - 50 * d, 34 * d, '#fff', 'center');
  ctx.fillStyle = '#1b1f33'; ctx.fillRect(x - 3 * d, y - 3 * d, w + 6 * d, 22 * d);
  ctx.fillStyle = '#e3c84e'; ctx.fillRect(x, y, w * Assets.loaded / Assets.total, 16 * d);
  label(ctx, `Caricamento sprite ${Assets.loaded}/${Assets.total}`, G.W / 2, y + 40 * d, 14 * d, '#fff', 'center');
}

function render() {
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.imageSmoothingEnabled = false;
  if (G.state === 'loading') { renderLoading(); return; }
  UI.begin();
  renderWorld();
  if (G.state === 'menu') {
    drawMenu(ctx);
  } else {
    UI.locked = G.paused || UI.help || G.state === 'over' || !!UI.panel;
    if (G.spectate) drawSpectator(ctx); else drawHUD(ctx);
    UI.locked = false;
    Ctl.drawScreen(ctx);
    if (G.state === 'over') drawGameOver(ctx);
    else if (G.paused) drawPause(ctx);
    else if (UI.panel && !G.spectate) drawManage(ctx); // la partita continua sotto
  }
  if (UI.help) { UI.locked = false; drawHelp(ctx); }
  UI.drawFocus(ctx);
  UI.end();
  drawCursor(ctx);
}

// ---------------------------------------------------------------- avvio

let lastFrame = 0, lastPaint = 0;
function step(now) {
  const dt = Math.min(0.05, (now - lastFrame) / 1000 || 0);
  lastFrame = now;
  update(dt);
}
function frame(now) {
  requestAnimationFrame(frame); // prima di tutto: un errore in un fotogramma non deve fermare il gioco
  if (lastPaint) { Light.watch((now - lastPaint) / 1000); Quality.watch((now - lastPaint) / 1000); }
  lastPaint = now;
  try {
    step(now);
    render();
  } finally {
    Input.pressed = Input.released = Input.rpressed = Input.moved = false;
    Input.wheel = 0;
  }
}
// Per il timer dell'host in rete: fa avanzare la partita quando il browser ha smesso di chiedere
// fotogrammi (scheda in secondo piano, finestra coperta), altrimenti i client resterebbero fermi.
function stepUnseen() {
  const now = performance.now();
  if (now - lastPaint < 150) return;
  step(now);
  Input.pressed = Input.released = Input.rpressed = false;
}

resize();
requestAnimationFrame(frame);
Assets.load().then(() => G.start({ players: [] })).catch(err => { G.error = err.message; });
