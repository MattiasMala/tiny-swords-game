'use strict';
// Interfaccia a modalità immediata: menu, lobby, HUD da gioco di strategia, pannelli modali, cursore.

const INK = '#4a3a33', RED = '#c0302b';
const HELP_LINES = [
  ['Clic · trascina', 'seleziona unità ed edifici (doppio clic: tutte le uguali in vista)'],
  ['Clic destro', 'muovi, attacca, raccogli, costruisci, ripara, punto di raccolta'],
  ['Frecce o WASD · bordo dello schermo', 'sposta la visuale · con Maiusc più svelta, con Ctrl veloce'],
  ['Rotellina · +  e  −', 'ingrandisci e rimpicciolisci la visuale'],
  ['Frecce o WASD + Invio o Spazio', 'nelle schermate: scegli e premi · posando un edificio: muovi e posa'],
  ['Maiusc + 1-9 · 1-9', 'crea · richiama un gruppo di unità'],
  ['.  ·  H  ·  Spazio', 'pedone inattivo · castello · centra la selezione'],
  ['U · L · I · K · C', 'schermate: unità · lavoratori · ricerche · mercato · costruisci'],
  ['V · Tab · Backspace', 'sposta un edificio · tutte le truppe · richiamale alla base'],
  ['X · E · F · G', 'con una selezione: attacco in marcia · esplora · ferma · punto di raccolta'],
  ['Esc · P · Canc', 'annulla o deseleziona · pausa · demolisci'],
  ['M · F1 · F2 · F3 · F4', 'audio · aiuto · luci · risoluzione · modalità test (risorse, tempo, nebbia)'],
];
const HELP_NOTES = [
  'I Pedoni raccolgono legna, oro, pietra e cibo: clic destro su alberi, giacimenti e pecore, o schermata Lavoratori.',
  'Da soli lavorano solo nel territorio esplorato. Se vedono truppe nemiche si rifugiano nelle case finché non passano.',
  'Le Case danno posto ai Pedoni, gli Accampamenti alle truppe. Più fabbriche dello stesso tipo addestrano più in fretta.',
  'Punti da conquistare: tienici accanto delle truppe per qualche secondo. Rendono risorse; attorno agli Avamposti si costruisce.',
  'Torri e Palizzate solo vicino al castello o a un tuo Avamposto. Le tue unità passano fra i tuoi edifici, i nemici no.',
  'In marcia d\'attacco l\'esercito va in formazione: mischia e Arieti davanti, tiratori dietro, Monaci in fondo.',
  'Vince l\'ultimo regno rimasto: abbatti i castelli nemici e difendi il tuo.',
];

const UI = {
  hot: false, _hot: false, locked: false, cursor: 'arrow', tip: null, help: false, mapDrag: false,
  panel: null,        // schermata di gestione aperta: 'units' | 'workers' | 'tree' | 'build' | 'market' | 'send' (vedi panels.js)
  fresh: false,       // appena aperta: il clic che l'ha aperta non deve richiuderla
  marks: new Set(),   // unità spuntate nella schermata Unità
  slide: null,        // risorsa il cui cursore è trascinato nella schermata Lavoratori
  sent: null,         // ultimo valore inviato da un cursore
  send: null,         // schermata Invia: { ids = unità da mandare, x, y = punto scelto con le frecce (px del mondo), kbd, go }
  // tastiera: frecce o WASD spostano un evidenziatore fra i pulsanti della schermata aperta, Invio o Spazio premono quello evidenziato
  kbd: false,         // l'evidenziatore lo guida la tastiera (finché il mouse non si muove)
  focus: null,        // punto dentro il pulsante evidenziato
  focusRect: null,    // il suo rettangolo, per disegnarlo
  rects: [], last: [],// pulsanti disegnati in questo fotogramma e nel precedente
  nav: null, enter: false,

  // Apre una schermata di gestione, o la chiude se è già quella aperta.
  open(name) {
    this.panel = this.panel === name ? null : name;
    this.fresh = true; this.slide = null; this.focus = null;
    this.marks.clear();
    G.placing = null; Ctl.mode = null;
  },

  begin() {
    this._hot = false; this.cursor = 'arrow'; this.tip = null; this.locked = false;
    this.last = this.rects; this.rects = []; this.focusRect = null;
    if (this.nav) { this.step(this.nav[0], this.nav[1]); this.nav = null; }
  },
  end() { this.hot = this._hot; this.enter = false; },
  over(x, y, w, h) {
    const m = Input.mouse;
    return !this.locked && m.x >= x && m.x < x + w && m.y >= y && m.y < y + h;
  },
  area(x, y, w, h) { if (this.over(x, y, w, h)) this._hot = true; },
  hit(x, y, w, h) {
    let hover;
    if (this.kbd) {
      const f = this.focus;
      hover = !this.locked && f !== null && f.x >= x && f.x < x + w && f.y >= y && f.y < y + h;
      if (hover) this.focusRect = [x, y, w, h];
    } else hover = this.over(x, y, w, h);
    if (!this.locked) this.rects.push([x, y, w, h]);
    if (hover) { this._hot = true; this.cursor = 'hand'; }
    return { hover, down: hover && !this.kbd && Input.lmb, clicked: hover && (this.kbd ? this.enter : Input.pressed) };
  },

  // Freccia premuta con una schermata aperta.
  navigate(dx, dy) {
    const t = this.panel === 'send' ? this.send : null;
    if (t) { // sulla mappa grande le frecce muovono il punto d'arrivo
      t.kbd = true;
      t.x = clamp(t.x + dx * TILE * 3, 0, World.W * TILE); t.y = clamp(t.y + dy * TILE * 3, 0, World.H * TILE);
    } else { this.kbd = true; this.nav = [dx, dy]; }
  },
  press() {
    if (this.panel === 'send' && this.send && this.send.kbd) this.send.go = true;
    else if (this.kbd && this.focus) this.enter = true;
  },
  // Sposta l'evidenziatore sul pulsante più vicino in quella direzione (fra quelli dell'ultimo fotogramma, e non
  // troppo di lato: altrimenti da un bordo si salterebbe a un pulsante lontano); la prima volta su quello più
  // vicino al centro dello schermo.
  step(dx, dy) {
    const all = this.last, f = this.focus, mid = r => [r[0] + r[2] / 2, r[1] + r[3] / 2];
    if (!all.length) return;
    const cur = f && all.find(r => f.x >= r[0] && f.x < r[0] + r[2] && f.y >= r[1] && f.y < r[1] + r[3]);
    let best = null, bd = Infinity;
    for (const r of all) {
      const [x, y] = mid(r);
      let d;
      if (!cur) d = Math.hypot(x - G.W / 2, y - G.H / 2);
      else {
        const [x0, y0] = mid(cur), along = (x - x0) * dx + (y - y0) * dy, across = Math.abs((x - x0) * dy) + Math.abs((y - y0) * dx);
        if (r === cur || along < 4 || across > along * 2) continue;
        d = along + across * 1.5; // meglio chi è in linea
      }
      if (d < bd) { bd = d; best = r; }
    }
    if (best) { const [x, y] = mid(best); this.focus = { x, y }; }
  },
  drawFocus(ctx) {
    const r = this.focusRect;
    if (this.kbd && r) drawBrackets(ctx, r[0] - 5, r[1] - 5, r[2] + 10, r[3] + 10, clamp(Math.min(r[2], r[3]) / 70, 0.45, 1));
  },
};

// Spezza un testo in righe che stanno in `width`.
function wrapText(ctx, str, width, size) {
  const lines = [];
  let line = '';
  for (const word of str.split(' ')) {
    const next = line ? line + ' ' + word : word;
    if (line && textWidth(ctx, next, size) > width) { lines.push(line); line = word; } else line = next;
  }
  if (line) lines.push(line);
  return lines;
}

// ---------------------------------------------------------------- pulsanti

function bigButton(ctx, x, y, w, h, s, text, red, icon) {
  const r = UI.hit(x, y, w, h);
  drawPanel(ctx, red ? (r.down ? PANEL.bigRedDown : PANEL.bigRed) : (r.down ? PANEL.bigBlueDown : PANEL.bigBlue), x, y, w, h, s);
  const cy = y + h / 2 + (r.down ? 3 : -4) * s;
  const tw = textWidth(ctx, text, 24 * s), iw = icon ? 40 * s : 0;
  if (icon) drawIcon(ctx, ICON(icon), x + w / 2 - tw / 2 - 4 * s, cy, 40 * s);
  label(ctx, text, x + w / 2 + iw / 2, cy, 24 * s, '#fff', 'center');
  if (r.clicked) Sfx.play('click');
  return r.clicked;
}

// Pulsante 128x128 (quadrato o tondo, blu o rosso). size = lato del frame disegnato.
function smallButton(ctx, cx, cy, size, shape, red) {
  const v = size * 0.35, r = UI.hit(cx - v, cy - v, v * 2, v * 2);
  const im = img(`${UIP}Buttons/Small${red ? 'Red' : 'Blue'}${shape}Button_${r.down ? 'Pressed' : 'Regular'}.png`);
  ctx.drawImage(im, Math.round(cx - size / 2), Math.round(cy - size / 2), Math.round(size), Math.round(size));
  r.dy = (r.down ? 0.04 : -0.03) * size;
  return r;
}

function tinyButton(ctx, cx, cy, size, shape, red, icon) {
  const r = UI.hit(cx - size / 2, cy - size / 2, size, size);
  const im = img(`${UIP}Buttons/Tiny${shape}${red ? 'Red' : 'Blue'}Button.png`), k = r.down ? 0.92 : 1;
  ctx.drawImage(im, Math.round(cx - size * k / 2), Math.round(cy - size * k / 2), Math.round(size * k), Math.round(size * k));
  if (icon) drawIcon(ctx, ICON(icon), cx, cy - size * 0.03, size * 0.74 * k);
  if (r.clicked) Sfx.play('click');
  return r;
}

// ---------------------------------------------------------------- HUD: barra in alto

// light = su fondo scuro (testo chiaro col contorno) invece che su pergamena. Con maxW il costo è centrato su x
// e, se le voci non ci stanno alla distanza solita, si avvicinano (e solo se serve si rimpiccioliscono un poco):
// tre risorse in una casella stretta.
function drawCost(ctx, f, cost, x, y, s, light = false, maxW = 0) {
  const kinds = RES_KINDS.filter(k => cost[k]), step = maxW ? Math.min(74 * s, maxW / kinds.length) : 74 * s, z = Math.min(s, step / 58);
  if (maxW) x -= step * kinds.length / 2;
  for (const k of kinds) {
    const ok = f.res[k] >= cost[k];
    drawIcon(ctx, RES_ICON[k], x + 13 * z, y, 28 * z);
    if (light) label(ctx, String(cost[k]), x + 28 * z, y, 15 * z, ok ? '#fff' : '#ff8a7a', 'left');
    else label(ctx, String(cost[k]), x + 28 * z, y, 15 * z, ok ? INK : RED, 'left', null);
    x += step;
  }
}

function drawTopBar(ctx, s) {
  const f = G.player, W = G.W, w = 800 * s, x = W / 2 - w / 2, y = 12 * s, h = 58 * s, col = w / 6;
  UI.area(x, y, w, h);
  drawPanel(ctx, PANEL.paper, x, y, w, h, 0.6 * s);
  const tip = (i, title, desc) => { if (UI.over(x + col * i, y, col, h)) UI.tip = { x: x + col * (i + 0.5), y: y + h + 34 * s, below: true, title, desc }; };
  // risorse: quante ce n'è e, sotto, quante ne entrano al minuto col lavoro di adesso
  RES_KINDS.forEach((k, i) => {
    const r = f.rate[k];
    drawIcon(ctx, RES_ICON[k], x + col * i + 30 * s, y + h / 2, 40 * s);
    label(ctx, String(Math.floor(f.res[k])), x + col * i + 54 * s, y + 21 * s, 19 * s, INK, 'left', null);
    label(ctx, `+${r}/min`, x + col * i + 54 * s, y + 43 * s, 12.5 * s, r ? '#2f7a3a' : '#9a8f80', 'left', null);
    tip(i, RES_LABEL[k], `Rendita attesa: +${r} al minuto. Conta chi ci lavora adesso, quanto porta a ogni viaggio e quanto dura il giro fra la risorsa e il punto di consegna, più i punti conquistati.`);
  });
  // posti: Pedoni nelle case, truppe negli accampamenti
  const np = f.pawns.length, nt = f.soldiers.length;
  drawAvatar(ctx, f.color, 'pawn', x + col * 4 + 8 * s, y + 8 * s, 42 * s);
  label(ctx, `${np}/${f.pawnCap}`, x + col * 4 + 54 * s, y + h / 2 + s, 19 * s, np >= f.pawnCap ? RED : INK, 'left', null);
  tip(4, 'Pedoni', `${np} su ${f.pawnCap} posti. Ogni Casa ne aggiunge ${BUILD_DEF.house.pawns}, fino a ${PAWN_MAX}.`);
  drawIcon(ctx, XICON('tent'), x + col * 5 + 30 * s, y + h / 2, 40 * s);
  label(ctx, `${nt}/${f.troopCap}`, x + col * 5 + 54 * s, y + h / 2 + s, 19 * s, nt >= f.troopCap ? RED : INK, 'left', null);
  tip(5, 'Truppe', `${nt} su ${f.troopCap} posti. Ogni Accampamento ne aggiunge ${BUILD_DEF.camp.troops}, ogni edificio che addestra truppe ${BUILD_DEF.barracks.troops}, fino a ${TROOP_MAX}.`);

  const t = Math.floor(G.time), clock = `${String(Math.floor(t / 60)).padStart(2, '0')}:${String(t % 60).padStart(2, '0')}`;
  swordBanner(ctx, f.colorIdx, W / 2, y + h + 34 * s, 400 * s, 0.5 * s, '', 0);
  label(ctx, AGE_NAME[f.age], W / 2 - 34 * s, y + h + 27 * s, 15 * s, INK, 'center', null);
  drawIcon(ctx, XICON('hourglass'), W / 2 + 78 * s, y + h + 27 * s, 24 * s);
  label(ctx, clock, W / 2 + 94 * s, y + h + 27 * s, 15 * s, INK, 'left', null);

  // in alto a sinistra: il regno, i pedoni fermi, l'ultimo attacco subito
  smallRibbon(ctx, f.colorIdx, 1, 150 * s, 36 * s, 250 * s, 0.9 * s, f.name, 17 * s);
  let idle = 0;
  for (const u of f.pawns) if (!u.dead && u.act === 0) idle++;
  if (idle) {
    const r = smallButton(ctx, 48 * s, 102 * s, 96 * s, 'Round', false);
    drawAvatar(ctx, f.color, 'pawn', 28 * s, 80 * s + r.dy, 40 * s);
    label(ctx, String(idle), 70 * s, 122 * s, 16 * s, '#ffe27a');
    if (r.hover) UI.tip = { x: 170 * s, y: 250 * s, title: 'Pedoni inattivi', key: '.', desc: 'Seleziona il prossimo pedone senza un compito.' };
    if (r.clicked) { Ctl.nextIdle(); Sfx.play('click'); }
  }
  if (Ctl.alertAt && G.time - Ctl.alertT < 30) {
    const r = smallButton(ctx, (idle ? 120 : 48) * s, 102 * s, 96 * s, 'Round', true);
    drawIcon(ctx, XICON('bell'), (idle ? 120 : 48) * s, 100 * s + r.dy, 44 * s);
    if (r.hover) UI.tip = { x: 170 * s, y: 250 * s, title: 'Sotto attacco', desc: 'Porta la visuale dove il regno è stato colpito.' };
    if (r.clicked) { Ctl.lookAt(Ctl.alertAt.x, Ctl.alertAt.y); Sfx.play('click'); }
  }

  const bx = W - 40 * s, size = 44 * s;
  if (tinyButton(ctx, bx, 38 * s, size, 'Round', false, 10).clicked) G.paused = true;
  if (tinyButton(ctx, bx - 52 * s, 38 * s, size, 'Round', !Sfx.enabled, 12).clicked) Sfx.enabled = !Sfx.enabled;
  if (tinyButton(ctx, bx - 104 * s, 38 * s, size, 'Round', false, 11).clicked) UI.help = true;
}

// ---------------------------------------------------------------- HUD: pannello comandi

// tip.below: il riquadro sta sotto il punto indicato invece che sopra (per ciò che è in cima allo schermo).
function drawTooltip(ctx, tip, s) {
  const f = G.player, w = 330 * s, lines = wrapText(ctx, tip.desc, w - 28 * s, 13.5 * s);
  const h = (34 + (tip.cost ? 30 : 0) + lines.length * 18 + (tip.lock ? 24 : 0) + 8) * s;
  const x = clamp(tip.x - w / 2, 12 * s, G.W - w - 12 * s), y = tip.below ? tip.y : Math.max(8 * s, tip.y - h - 28 * s);
  drawPanel(ctx, PANEL.paper, x, y, w, h, 0.6 * s);
  smallRibbon(ctx, f.colorIdx, 1, x + w / 2, y - 2 * s, Math.min(w, textWidth(ctx, tip.title, 15 * s) + 130 * s), 0.78 * s, tip.key ? `${tip.title}  [${tip.key}]` : tip.title, 15 * s);
  let ty = y + 36 * s;
  if (tip.cost) { drawCost(ctx, f, tip.cost, x + 14 * s, ty, s); ty += 30 * s; }
  for (const line of lines) { label(ctx, line, x + 14 * s, ty, 13.5 * s, INK, 'left', null); ty += 18 * s; }
  if (tip.lock) label(ctx, tip.lock, x + 14 * s, ty + 4 * s, 14 * s, RED, 'left', null);
}

// Sopra il pannello della selezione: cosa farà il prossimo clic, oppure l'ultimo avviso.
function drawHints(ctx, s) {
  const y = G.H - 188 * s, p = G.placing, m = Ctl.mode;
  const text = p ? (p.move ? 'Clic, Invio o Spazio: sposta qui  ·  Frecce o WASD: muovi la sagoma  ·  Destro / Esc: annulla'
    : p.kind === 'wall' ? 'Clic, Invio o Spazio: piazza  ·  Maiusc + trascina: una fila  ·  Frecce o WASD: muovi la sagoma  ·  Destro / Esc: annulla' : 'Clic, Invio o Spazio: piazza (Maiusc: più d\'uno)  ·  Frecce o WASD: muovi la sagoma  ·  Destro / Esc: annulla')
    : m === 'amove' ? 'Clic sul punto da attaccare  ·  Esc: annulla' : m === 'rally' ? 'Clic sul punto di raccolta  ·  Esc: annulla'
    : m === 'pickmove' ? 'Clic sull\'edificio da spostare, oppure Frecce (o WASD) e Invio  ·  Esc: annulla' : null;
  if (text) label(ctx, text, G.W / 2, y, 16 * s, '#fff', 'center');
  else if (G.toastT > 0) {
    ctx.globalAlpha = clamp(G.toastT * 2, 0, 1);
    label(ctx, G.toastText, G.W / 2, y, 19 * s, '#ffd9cf', 'center');
    ctx.globalAlpha = 1;
  }
}

// Azioni di contesto di ciò che è selezionato, in alto a destra nel pannello della selezione.
function drawActions(ctx, actions, x, y, w, s) {
  actions.forEach((a, i) => {
    const cx = x + w - 38 * s - (actions.length - 1 - i) * 50 * s, cy = y + 40 * s;
    const r = tinyButton(ctx, cx, cy, 44 * s, 'Square', false);
    drawSpec(ctx, a.icon, G.player.color, cx, cy - s, 24 * s);
    label(ctx, a.key, cx - 19 * s, cy - 18 * s, 10 * s, '#fff', 'left');
    if (a.on) drawBrackets(ctx, cx - 24 * s, cy - 24 * s, 48 * s, 48 * s, 0.5 * s);
    if (r.hover) UI.tip = { x: cx, y: y + 8 * s, title: a.title, key: a.key, desc: a.desc };
    if (r.clicked) Ctl.act(a);
  });
}

// ---------------------------------------------------------------- HUD: minimappa

// La mappa in piccolo dentro un rettangolo: terreno, edifici, punti da conquistare, unità, nebbia, visuale e
// allarmi. Serve alla minimappa e alla mappa grande della schermata Invia. Ritorna i pixel per pixel del mondo.
function drawMap(ctx, x, y, mw, mh, s) {
  const f = G.player, k = mw / (World.W * TILE);
  ctx.drawImage(World.minimap, Math.round(x), Math.round(y), Math.round(mw), Math.round(mh));
  for (const b of G.buildings) {
    if (b.dead || (b.faction !== f && !Fog.knows(b.x, b.y - 20))) continue;
    const r = (b.kind === 'castle' ? 9 : b.def.w > 1 ? 5 : 3) * s;
    ctx.fillStyle = '#1b1f33';
    ctx.fillRect(x + b.x * k - r / 2 - 1, y + b.y1 * k - r - 1, r + 2, r + 2);
    ctx.fillStyle = COLOR_CSS[b.faction.color];
    ctx.fillRect(x + b.x * k - r / 2, y + b.y1 * k - r, r, r);
  }
  for (const p of G.points) { // un rombo: chiaro se è libero, del colore di chi lo tiene
    if (!Fog.knows(p.x, p.y - TILE)) continue;
    const px = x + p.x * k, py = y + (p.y - TILE) * k;
    for (const [r, color] of [[6.5 * s, '#1b1f33'], [4.5 * s, p.owner ? COLOR_CSS[p.owner.color] : '#f4ecd2']]) {
      ctx.fillStyle = color;
      ctx.beginPath(); ctx.moveTo(px, py - r); ctx.lineTo(px + r, py); ctx.lineTo(px, py + r); ctx.lineTo(px - r, py); ctx.fill();
    }
  }
  for (const u of G.units) {
    if (u.dead || u.inside || (u.faction !== f && !Fog.sees(u.x, u.y))) continue;
    ctx.fillStyle = Ctl.sel.has(u.id) ? '#ffffff' : COLOR_CSS[u.faction.color];
    ctx.fillRect(x + u.x * k - s, y + u.y * k - s, 2.5 * s, 2.5 * s);
  }
  ctx.imageSmoothingEnabled = true;
  ctx.drawImage(Fog.cv, Math.round(x), Math.round(y), Math.round(mw), Math.round(mh));
  ctx.imageSmoothingEnabled = false;
  ctx.strokeStyle = 'rgba(255,255,255,0.9)'; ctx.lineWidth = 1.5 * s;
  ctx.strokeRect(x + clamp(G.cam.x * k, 0, mw), y + clamp(G.cam.y * k, 0, mh), Math.min(G.viewW * k, mw), Math.min(G.viewH * k, mh));
  for (const p of Ctl.pings) { // dove il regno viene colpito
    ctx.strokeStyle = '#ff4a3d'; ctx.lineWidth = 2.5 * s; ctx.globalAlpha = clamp(2 - p.t / 2, 0, 1);
    ctx.beginPath(); ctx.arc(x + p.x * k, y + p.y * k, (5 + (p.t * 14) % 16) * s, 0, Math.PI * 2); ctx.stroke();
    ctx.globalAlpha = 1;
  }
  return k;
}

function drawMinimap(ctx, s) {
  const mw = 270 * s, mh = 180 * s, x = G.W - mw - 24 * s, y = G.H - mh - 26 * s, m = Input.mouse;
  drawPanel(ctx, PANEL.paper, x - 10 * s, y - 10 * s, mw + 20 * s, mh + 20 * s, 0.5 * s);
  const k = drawMap(ctx, x, y, mw, mh, s), wx = (m.x - x) / k, wy = (m.y - y) / k;
  // clic sinistro (anche trascinando): sposta la visuale; clic destro: ordine alle unità selezionate
  if (UI.over(x - 10 * s, y - 10 * s, mw + 20 * s, mh + 20 * s)) {
    UI._hot = true;
    if (Input.pressed) UI.mapDrag = true;
    if (Input.rpressed && !G.spectate) Ctl.order(clamp(wx, 0, World.W * TILE), clamp(wy, 0, World.H * TILE));
  }
  if (!Input.lmb) UI.mapDrag = false;
  if (UI.mapDrag) Ctl.lookAt(wx, wy);
  return x - 10 * s;
}

// ---------------------------------------------------------------- HUD: pannello della selezione

function hpText(e) { return `${Math.max(0, Math.ceil(e.hp))} / ${e.maxHp}`; }

function drawUnitInfo(ctx, u, x, y, w, s) {
  const f = u.faction, mine = f === G.player;
  drawPanel(ctx, PANEL.bannerSlots, x + 14 * s, y + 16 * s, 96 * s, 96 * s, 0.7 * s);
  drawPortrait(ctx, f.color, u.type, x + 20 * s, y + 20 * s, 84 * s);
  smallRibbon(ctx, f.colorIdx, 1, x + 240 * s, y + 30 * s, 240 * s, 0.85 * s, mine ? u.def.label : `${u.def.label} · ${f.name}`, 15 * s);
  bigBar(ctx, x + 128 * s, y + 52 * s, Math.min(260 * s, w - 150 * s), u.hp / u.maxHp, 0.6 * s);
  label(ctx, hpText(u), x + 128 * s + Math.min(260 * s, w - 150 * s) / 2, y + 68 * s, 13 * s, '#fff', 'center');
  // statistiche: icona e valore (spada attacco, scudo armatura, frecce gittata, erbe cura, stivale velocità)
  const stats = [];
  if (u.def.dmg) stats.push([ICON(5), Math.round(unitDmg(u))], [ICON(6), unitArmor(u)]);
  if (u.def.dmg && u.def.range > 100) stats.push([XICON('arrows'), Math.round(unitRange(u) / TILE)]);
  stats.push([XICON('boot'), Math.round(unitSpeed(f, u.type) / 10)]);
  // per i propri monaci, dopo la cura per colpo, quanta riserva di cura resta (in fondo alla riga: è un testo lungo)
  if (u.def.heal) stats.push([XICON('herbs'), Math.round(u.def.heal * f.mods.heal) + (mine && u.def.faith ? `  ·  riserva ${Math.floor(u.faith || 0)}/${u.def.faith}` : '')]);
  const step = Math.min(72 * s, (w - 150 * s) / stats.length); // in una finestra stretta si stringono col pannello
  stats.forEach(([icon, value], i) => {
    drawIcon(ctx, icon, x + 140 * s + i * step, y + 96 * s, 28 * s);
    label(ctx, String(value), x + 156 * s + i * step, y + 97 * s, 15 * s, INK, 'left', null);
  });
  if (mine) label(ctx, ACT_LABEL[u.act], x + 128 * s, y + 124 * s, 13.5 * s, '#8a3b2e', 'left', null);
}

function drawGroupInfo(ctx, units, x, y, w, s) {
  const size = 46 * s, cols = Math.max(1, Math.floor((w - 24 * s) / size)), shown = units.slice(0, cols * 3 - 1);
  const shift = Input.keys.has('ShiftLeft') || Input.keys.has('ShiftRight');
  shown.forEach((u, i) => {
    const cx = x + 14 * s + (i % cols + 0.5) * size, cy = y + 14 * s + (Math.floor(i / cols) + 0.5) * size - 3 * s;
    const r = tinyButton(ctx, cx, cy, size * 0.96, 'Square', u.hp < u.maxHp * 0.35);
    drawPortrait(ctx, u.faction.color, u.type, cx - size * 0.34, cy - size * 0.36, size * 0.68);
    ctx.fillStyle = '#1b1f33'; ctx.fillRect(cx - size * 0.32, cy + size * 0.3, size * 0.64, 4 * s);
    ctx.fillStyle = u.hp > u.maxHp * 0.35 ? '#7be06a' : '#ff5a4a'; ctx.fillRect(cx - size * 0.32, cy + size * 0.3, size * 0.64 * u.hp / u.maxHp, 4 * s);
    if (r.clicked) { // clic: solo questa unità; con Maiusc la toglie dal gruppo
      if (shift) { Ctl.sel.delete(u.id); Ctl.refresh(); } else Ctl.select([u.id]);
    }
  });
  if (units.length > shown.length) {
    const i = shown.length;
    label(ctx, '+' + (units.length - shown.length), x + 14 * s + (i % cols + 0.5) * size, y + 14 * s + (Math.floor(i / cols) + 0.5) * size, 17 * s, INK, 'center', null);
  }
}

function drawBuildingInfo(ctx, b, x, y, w, s) {
  const f = b.faction, mine = f === G.player, d = b.def;
  drawPanel(ctx, PANEL.bannerSlots, x + 14 * s, y + 16 * s, 96 * s, 96 * s, 0.7 * s);
  drawBuildingThumb(ctx, f.color, b.kind, x + 62 * s, y + 64 * s, 82 * s);
  smallRibbon(ctx, f.colorIdx, 1, x + 240 * s, y + 30 * s, 240 * s, 0.85 * s, mine ? d.label : `${d.label} · ${f.name}`, 15 * s);
  const bw = Math.min(260 * s, w - 150 * s);
  bigBar(ctx, x + 128 * s, y + 52 * s, bw, b.hp / b.maxHp, 0.6 * s);
  label(ctx, hpText(b), x + 128 * s + bw / 2, y + 68 * s, 13 * s, '#fff', 'center');
  if (!b.done) { label(ctx, `In costruzione: ${Math.floor(100 * b.progress / d.work)}%`, x + 128 * s, y + 100 * s, 14.5 * s, '#8a3b2e', 'left', null); return; }
  if (!mine) return;
  if (!b.queue.length) {
    if (d.trade) { // listino del mercato: oro per 100 venduti / comprati, per ogni risorsa
      const step = Math.min(96 * s, (w - 190 * s) / 3), size = Math.min(13.5 * s, step * 0.15);
      drawIcon(ctx, XICON('scales'), x + 142 * s, y + 104 * s, 34 * s);
      ['wood', 'meat', 'stone'].forEach((k, i) => {
        drawIcon(ctx, RES_ICON[k], x + 180 * s + i * step, y + 104 * s, 28 * s);
        label(ctx, `${sellPrice(f, k)}/${buyPrice(f, k)}`, x + 194 * s + i * step, y + 105 * s, size, INK, 'left', null);
      });
      if (w > 400 * s) label(ctx, 'oro per vendere / comprare 100', x + 128 * s, y + 128 * s, 12 * s, '#8a3b2e', 'left', null);
      return;
    }
    const hint = d.trains ? 'Clic destro sulla mappa: punto di raccolta' : d.desc, same = G.buildings.filter(o => o.faction === f && o.kind === b.kind && o.done && !o.dead).length;
    const lines = wrapText(ctx, hint, w - 150 * s, 13.5 * s).slice(0, 2);
    // quante fabbriche di questo tipo, e di quanto accelerano l'addestramento
    if (d.trains && b.kind !== 'castle') lines[1] = same > 1 ? `${d.label} ×${same}: addestramento +${Math.round((trainSpeed(same) - 1) * 100)}%` : 'Un\'altra uguale: +' + Math.round(TRAIN_BONUS * 100) + '% di velocità';
    for (const [i, line] of lines.entries()) label(ctx, line, x + 128 * s, y + 98 * s + i * 18 * s, 13.5 * s, i && d.trains ? '#8a3b2e' : INK, 'left', null);
    return;
  }
  // coda: il primo è in lavorazione, un clic annulla e rimborsa
  const q = b.queue[0], waiting = q.unit && q.t >= q.total;
  let cancel = -1;
  b.queue.forEach((item, i) => {
    const cx = x + 152 * s + i * 54 * s, cy = y + 116 * s;
    const r = smallButton(ctx, cx, cy, 74 * s, 'Round', false), name = item.unit ? UNIT_DEF[item.unit].label : TECH_DEF[item.tech].label;
    drawSpec(ctx, item.unit ? ['unit', item.unit] : TECH_DEF[item.tech].icon, f.color, cx, cy - 2 * s + r.dy, 28 * s);
    if (r.hover) UI.tip = { x: cx, y: y + 30 * s, title: name, desc: 'Clic per annullare: le risorse tornano indietro.' };
    if (r.clicked) cancel = i;
  });
  smallBar(ctx, x + 128 * s + bw + 20 * s, y + 104 * s, Math.max(40 * s, w - bw - 170 * s), q.t / q.total, 0.8 * s, Assets.barFill[f.color]);
  if (waiting) label(ctx, q.unit === 'pawn' ? 'In attesa di case' : 'In attesa di accampamenti', x + 128 * s + bw + 20 * s, y + 132 * s, 13 * s, RED, 'left', null);
  if (cancel >= 0) { // a disegno finito: la coda cambia subito
    const item = b.queue[cancel];
    command('cancel', b.id, cancel, item.unit || item.tech);
    Sfx.play('click');
  }
}

function drawNodeInfo(ctx, n, x, y, s) {
  const res = NODE_RES[n.kind], name = n.big ? (n.kind === 'gold' ? 'Grande vena d\'oro' : 'Grande cava di pietra') : { tree: 'Albero', gold: 'Vena d\'oro', stone: 'Cava di pietra', sheep: 'Pecora' }[n.kind];
  drawIcon(ctx, RES_ICON[res], x + 62 * s, y + 64 * s, 84 * s);
  label(ctx, name, x + 128 * s, y + 48 * s, 20 * s, INK, 'left', null);
  label(ctx, `${RES_LABEL[res]}: ${nodeLeft(n)}`, x + 128 * s, y + 78 * s, 16 * s, '#8a3b2e', 'left', null);
  label(ctx, n.big ? `Ci lavorano fino a ${BIG_USERS} Pedoni insieme (clic destro per mandarli).` : 'Clic destro con dei Pedoni selezionati per raccogliere.', x + 128 * s, y + 106 * s, 13.5 * s, INK, 'left', null);
}

// Punto da conquistare: cos'è, di chi è, cosa rende, a che punto è chi lo sta prendendo.
function drawPointInfo(ctx, p, x, y, w, s) {
  const d = p.def, o = p.owner, [sx, sy, sw, sh] = d.crop, k = Math.min(84 * s / sw, 84 * s / sh);
  drawPanel(ctx, PANEL.bannerSlots, x + 14 * s, y + 16 * s, 96 * s, 96 * s, 0.7 * s);
  ctx.drawImage(pointImg(p.kind), sx, sy, sw, sh, Math.round(x + 62 * s - sw * k / 2), Math.round(y + 64 * s - sh * k / 2), Math.round(sw * k), Math.round(sh * k));
  smallRibbon(ctx, (o || G.player).colorIdx, o ? 1 : 0, x + 240 * s, y + 30 * s, 240 * s, 0.85 * s, d.label, 15 * s);
  drawIcon(ctx, XICON('banner'), x + 140 * s, y + 66 * s, 30 * s);
  label(ctx, o ? (o === G.player ? 'È tuo' : `Lo tiene il ${o.name}`) : 'Libero', x + 160 * s, y + 66 * s, 15 * s, o === G.player ? '#2f7a3a' : '#8a3b2e', 'left', null);
  if (p.by && p.t > 0) label(ctx, `${p.by === G.player ? 'Lo stai conquistando' : 'Lo sta conquistando il ' + p.by.name}: ${Math.round(100 * p.t / CAPTURE_TIME)}%`, x + 290 * s, y + 66 * s, 13.5 * s, RED, 'left', null);
  const text = d.desc + (d.res ? ` +${POINT_INCOME * 60} al minuto.` : ` Entro ${OUTPOST_RADIUS} caselle.`) + ` Si conquista tenendoci accanto delle truppe per ${CAPTURE_TIME} secondi, senza nemici attorno.`;
  wrapText(ctx, text, w - 150 * s, 13 * s).slice(0, 3).forEach((line, i) => label(ctx, line, x + 128 * s, y + 92 * s + i * 17 * s, 13 * s, INK, 'left', null));
}

function drawSelection(ctx, s, right) {
  const h = 148 * s, w = Math.min(660 * s, right - 40 * s), x = Math.max(12 * s, Math.min((G.W - w) / 2, right - w - 14 * s)), y = G.H - h - 16 * s, list = Ctl.list;
  if (w < 260 * s) return;
  UI.area(x, y, w, h);
  drawPanel(ctx, PANEL.paper, x, y, w, h, 0.6 * s);
  const units = list.filter(e => e.isUnit), actions = Ctl.actions();
  if (units.length > 1) drawGroupInfo(ctx, units, x, y, w - actions.length * 50 * s, s);
  else if (units.length === 1) drawUnitInfo(ctx, units[0], x, y, w, s);
  else if (list.length && list[0].isBuilding) drawBuildingInfo(ctx, list[0], x, y, w, s);
  else if (list.length && list[0].isPoint) drawPointInfo(ctx, list[0], x, y, w, s);
  else if (list.length) drawNodeInfo(ctx, list[0], x, y, s);
  else {
    const f = G.player, mine = G.points.filter(p => p.owner === f).length;
    label(ctx, 'Nessuna selezione', x + 20 * s, y + 30 * s, 19 * s, INK, 'left', null);
    label(ctx, 'Clic o trascina per selezionare · clic destro per dare ordini · F1 per i comandi', x + 20 * s, y + 62 * s, 13.5 * s, INK, 'left', null);
    label(ctx, `Pedoni ${f.pawns.length}   Truppe ${f.soldiers.length}   Punti conquistati ${mine}/${G.points.length}   Nemici abbattuti ${f.kills}   Edifici distrutti ${f.razed}`, x + 20 * s, y + 92 * s, 14 * s, '#8a3b2e', 'left', null);
    label(ctx, `Regni rimasti: ${G.factions.filter(o => o.alive).map(o => COLOR_NAME[o.color]).join(', ')}`, x + 20 * s, y + 118 * s, 13.5 * s, INK, 'left', null);
  }
  drawActions(ctx, actions, x, y, w, s);
}

function drawAnnouncement(ctx, s) {
  const b = G.banners[0];
  if (!b) return;
  ctx.globalAlpha = clamp(Math.min(b.t * 4, (b.dur - b.t) * 2), 0, 1);
  bigRibbon(ctx, b.faction.colorIdx, G.W / 2, 190 * s, textWidth(ctx, b.text, 21 * s) + 190 * s, 0.7 * s, b.text, 21 * s);
  ctx.globalAlpha = 1;
}

function drawHUD(ctx) {
  const s = G.us;
  drawTopBar(ctx, s);
  drawSelection(ctx, s, drawMinimap(ctx, s));
  drawShortcuts(ctx, s);
  drawHints(ctx, s);
  if (G.state !== 'over') drawAnnouncement(ctx, s);
  // modalità test: ciò che è diverso da una partita normale resta scritto a schermo
  if (G.frozen || (!Fog.on && !G.result)) {
    label(ctx, `Modalità test: ${G.frozen ? 'tempo fermo' : ''}${G.frozen && !Fog.on ? ' · ' : ''}${Fog.on ? '' : 'nebbia tolta'}  (F4)`, G.W / 2, 138 * s, 15 * s, '#ffe27a', 'center');
  }
  if (UI.tip) drawTooltip(ctx, UI.tip, s);
  if (G.state === 'play' && !G.paused && !UI.help && !UI._hot) UI.cursor = Ctl.cursor();
}

// Sconfitto in una partita in rete: resta la visuale libera sulla battaglia.
function drawSpectator(ctx) {
  const s = G.us, bx = G.W - 40 * s, size = 44 * s;
  if (tinyButton(ctx, bx, 38 * s, size, 'Round', false, 10).clicked) G.paused = true;
  if (tinyButton(ctx, bx - 52 * s, 38 * s, size, 'Round', !Sfx.enabled, 12).clicked) Sfx.enabled = !Sfx.enabled;
  drawMinimap(ctx, s);
  drawAnnouncement(ctx, s);
  smallRibbon(ctx, G.player.colorIdx, 0, G.W / 2, 36 * s, 520 * s, 0.9 * s, 'Spettatore · frecce o minimappa per guardare', 15 * s);
}

// ---------------------------------------------------------------- pannelli modali

function dim(ctx, a) {
  ctx.fillStyle = `rgba(16,20,36,${a})`;
  ctx.fillRect(0, 0, G.W, G.H);
  UI._hot = true;
}

function drawHelp(ctx) {
  const lines = HELP_LINES.length, notes = HELP_NOTES.length, h0 = 132 + lines * 32 + notes * 28; // la scala fa stare tutte le righe in altezza
  const s = Math.min(G.W / 980, G.H / (h0 + 20), 1.3 * G.dpr), w = 880 * s, h = h0 * s, x = (G.W - w) / 2, y = (G.H - h) / 2;
  dim(ctx, 0.45);
  drawPanel(ctx, PANEL.banner, x, y, w, h, 0.8 * s);
  label(ctx, 'Comandi', G.W / 2, y + 32 * s, 28 * s, INK, 'center', null);
  HELP_LINES.forEach(([k, d], i) => {
    const ly = y + 80 * s + i * 32 * s;
    label(ctx, k, x + 36 * s, ly, 15 * s, '#8a3b2e', 'left', null);
    label(ctx, d, x + 330 * s, ly, 15 * s, INK, 'left', null);
  });
  HELP_NOTES.forEach((t, i) => label(ctx, '• ' + t, x + 36 * s, y + (100 + lines * 32 + i * 28) * s, 14 * s, INK, 'left', null));
  const close = tinyButton(ctx, x + w - 34 * s, y + 32 * s, 40 * s, 'Round', true, 9).clicked;
  if (close || (Input.pressed && !UI.over(x, y, w, h))) UI.help = false;
}

function drawPause(ctx) {
  const s = Math.min(G.W / 900, G.H / 640, 1.3 * G.dpr), w = 380 * s, h = 270 * s, x = (G.W - w) / 2, y = (G.H - h) / 2;
  const net = Net.playing; // in rete la partita non si può fermare: questo è solo un menu
  dim(ctx, 0.5);
  drawPanel(ctx, PANEL.special, x, y, w, h, 0.9 * s);
  bigRibbon(ctx, G.player.colorIdx, G.W / 2, y + 16 * s, 300 * s, 0.75 * s, net ? 'MENU' : 'PAUSA', 26 * s);
  if (bigButton(ctx, x + 60 * s, y + 80 * s, w - 120 * s, 62 * s, 0.75 * s, 'Riprendi', false, 7)) G.paused = false;
  if (bigButton(ctx, x + 60 * s, y + 166 * s, w - 120 * s, 62 * s, 0.75 * s, net ? 'Esci' : 'Menu', true, 8)) G.toMenu();
  if (net) {
    const note = Net.role === 'host' ? 'La partita continua. Se esci finisce per tutti.' : 'La partita continua mentre il menu è aperto.';
    label(ctx, note, G.W / 2, y + 248 * s, 13 * s, '#d9cfb0', 'center');
  }
}

function drawGameOver(ctx) {
  const s = Math.min(G.W / 900, G.H / 640, 1.3 * G.dpr), w = 500 * s, h = 330 * s, x = (G.W - w) / 2, y = (G.H - h) / 2;
  const f = G.player, win = G.result === 'win', net = Net.playing;
  const alive = G.factions.filter(o => o.alive), t = Math.floor(G.endTime);
  dim(ctx, 0.55);
  drawPanel(ctx, PANEL.special, x, y, w, h, 0.9 * s);
  bigRibbon(ctx, f.colorIdx, G.W / 2, y + 14 * s, 380 * s, 0.85 * s, win ? 'VITTORIA!' : 'SCONFITTA', 30 * s);
  const line = win ? 'I cinque regni sono tuoi.' : alive.length === 1 ? `Vince il ${alive[0].name}.` : `Il ${f.name} è caduto.`;
  label(ctx, line, G.W / 2, y + 84 * s, 19 * s, '#f1e6c8', 'center');
  label(ctx, `Tempo ${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}    Nemici abbattuti ${f.kills}    Edifici distrutti ${f.razed}    ${AGE_NAME[f.age]}`,
    G.W / 2, y + 122 * s, 14 * s, '#d9cfb0', 'center');
  const bx = x + 90 * s, bw = w - 180 * s, bh = 62 * s;
  if (!net) {
    if (bigButton(ctx, bx, y + 156 * s, bw, bh, 0.75 * s, 'Rigioca', false, 7)) G.start(G.setup);
    if (bigButton(ctx, bx, y + 236 * s, bw, bh, 0.75 * s, 'Menu', true, 8)) G.toMenu();
    return;
  }
  const running = alive.length > 1; // gli altri stanno ancora giocando
  if (running && bigButton(ctx, bx, y + 156 * s, bw, bh, 0.75 * s, 'Guarda', false, 7)) { G.spectate = true; G.state = 'play'; }
  if (bigButton(ctx, bx, y + 236 * s, bw, bh, 0.75 * s, 'Esci', true, 8)) G.toMenu();
  if (running && Net.role === 'host') label(ctx, 'Sei l\'host: se esci la partita finisce per tutti.', G.W / 2, y + 314 * s, 13 * s, '#d9cfb0', 'center');
}

// ---------------------------------------------------------------- menu

// screen: 'main' oppure 'lobby' (partita in rete in preparazione); note: avviso mostrato sotto il menu.
const Menu = { color: 'Blue', level: 'normal', screen: 'main', note: '' };
const LEVEL_UNIT = { easy: 'pawn', normal: 'warrior', hard: 'knight' }; // la figura che rappresenta ogni difficoltà

// Riga dei cinque regni; info(c) = [etichetta, tipo del ritratto, attenuato]. Ritorna il regno cliccato, o null.
function kingdomRow(ctx, y, s, selected, info) {
  let clicked = null;
  COLORS.forEach((c, i) => {
    const cx = G.W / 2 + (i - 2) * 124 * s, [text, type, dimmed] = info(c);
    const r = UI.hit(cx - 52 * s, y - 4 * s, 104 * s, 140 * s);
    drawPanel(ctx, PANEL.storeSlots, cx - 46 * s, y, 92 * s, 108 * s, 0.6 * s);
    drawPennant(ctx, c, cx, y - 2 * s, 0.62 * s);
    if (dimmed) ctx.globalAlpha = 0.4;
    drawAvatar(ctx, c, type, cx - 32 * s, y + 42 * s, 64 * s);
    ctx.globalAlpha = 1;
    label(ctx, text, cx, y + 124 * s, 16 * s, INK, 'center', null);
    if (selected === c) drawBrackets(ctx, cx - 56 * s, y - 10 * s, 112 * s, 126 * s, 0.9 * s);
    if (r.clicked) { clicked = c; Sfx.play('click'); }
  });
  return clicked;
}

// Riga delle difficoltà dell'IA, con una figura del colore dato. Ritorna il livello cliccato, o null.
function levelRow(ctx, y, s, color, selected, enabled = true) {
  let clicked = null;
  AI_LEVELS.forEach((level, i) => {
    const cx = G.W / 2 + (i - 1) * 190 * s, type = LEVEL_UNIT[level];
    const r = enabled ? UI.hit(cx - 80 * s, y - 4 * s, 160 * s, 136 * s) : { clicked: false };
    drawPanel(ctx, PANEL.bannerSlots, cx - 72 * s, y, 144 * s, 100 * s, 0.7 * s);
    const sh = unitSheet(color, type, 'idle');
    drawSheet(ctx, sh, Math.floor(G.clock * sh.fps), cx, y + 86 * s, false, s * 0.9);
    label(ctx, AI_LEVEL[level].label, cx, y + 118 * s, 16 * s, INK, 'center', null);
    if (selected === level) drawBrackets(ctx, cx - 82 * s, y - 10 * s, 164 * s, 120 * s, 0.9 * s);
    if (r.clicked) { clicked = level; Sfx.play('click'); }
  });
  return clicked;
}

function drawMenu(ctx) {
  const W = G.W, H = G.H, s = Math.min(W / 980, H / 760, 1.35 * G.dpr);
  const pw = 720 * s, ph = 600 * s, px = (W - pw) / 2, py = (H - ph) / 2 + 28 * s;
  UI.locked = UI.help;
  drawStoreBanner(ctx, px, py, pw, ph, s);
  if (Menu.screen === 'lobby') drawLobby(ctx, px, py, s); else drawMainMenu(ctx, px, py, s);
  if (tinyButton(ctx, px + pw - 56 * s, py + 84 * s, 44 * s, 'Round', false, 11).clicked) UI.help = true;
  UI._hot = true;
}

function drawMainMenu(ctx, px, py, s) {
  const W = G.W;
  storeRibbon(ctx, Menu.color, W / 2, py + 4 * s, 560 * s, s, 'TINY SWORDS', 42 * s);
  label(ctx, 'La Guerra dei Cinque Regni', W / 2, py + 84 * s, 22 * s, INK, 'center', null);
  label(ctx, 'Scegli il tuo regno', W / 2, py + 124 * s, 17 * s, '#8a3b2e', 'center', null);
  Menu.color = kingdomRow(ctx, py + 146 * s, s, Menu.color, c => [COLOR_NAME[c], 'warrior', false]) || Menu.color;
  label(ctx, 'Difficoltà dei regni avversari', W / 2, py + 312 * s, 17 * s, '#8a3b2e', 'center', null);
  Menu.level = levelRow(ctx, py + 332 * s, s, Menu.color, Menu.level) || Menu.level;

  if (bigButton(ctx, W / 2 - 270 * s, py + 480 * s, 200 * s, 64 * s, 0.8 * s, 'GIOCA', false, 7)) {
    G.start({ players: [{ color: Menu.color, local: true }], level: Menu.level });
  }
  if (bigButton(ctx, W / 2 - 50 * s, py + 480 * s, 320 * s, 64 * s, 0.8 * s, 'MULTIGIOCATORE', false, 5)) Net.connect();
  if (tinyButton(ctx, px + 56 * s, py + 84 * s, 44 * s, 'Round', !Sfx.enabled, 12).clicked) Sfx.enabled = !Sfx.enabled;
  if (Menu.note) label(ctx, Menu.note, W / 2, py + 626 * s, 16 * s, '#fff', 'center');
}

// Preparazione di una partita in rete: ogni giocatore sceglie il regno, l'host la difficoltà dell'IA e l'avvio.
function drawLobby(ctx, px, py, s) {
  const W = G.W, me = Net.lobby.find(p => p.id === Net.id), host = Net.role === 'host';
  storeRibbon(ctx, me ? me.color : Menu.color, W / 2, py + 4 * s, 560 * s, s, 'MULTIGIOCATORE', 38 * s);
  if (tinyButton(ctx, px + 56 * s, py + 84 * s, 44 * s, 'Round', true, 9).clicked) { Net.leave(); return; }
  if (!me) { label(ctx, 'Ingresso nella partita…', W / 2, py + 290 * s, 21 * s, INK, 'center', null); return; }

  label(ctx, `Giocatori: ${Net.lobby.length} su ${COLORS.length} · i regni liberi li guida l'IA`, W / 2, py + 84 * s, 18 * s, INK, 'center', null);
  label(ctx, 'Scegli il tuo regno', W / 2, py + 124 * s, 17 * s, '#8a3b2e', 'center', null);
  const color = kingdomRow(ctx, py + 146 * s, s, me.color, c => {
    const p = Net.lobby.find(p => p.color === c);
    return [p ? (p === me ? 'Tu' : p === Net.lobby[0] ? 'Host' : 'Giocatore') : 'IA', p ? 'warrior' : 'pawn', !p];
  });
  if (color) Net.choose(color);
  label(ctx, host ? 'Difficoltà dell\'IA' : 'Difficoltà dell\'IA (la sceglie l\'host)', W / 2, py + 312 * s, 17 * s, '#8a3b2e', 'center', null);
  const level = levelRow(ctx, py + 332 * s, s, me.color, Net.level, host);
  if (level) Net.setLevel(level);

  if (Net.playing) {
    label(ctx, 'Avvio della partita…', W / 2, py + 512 * s, 19 * s, '#8a3b2e', 'center', null);
  } else if (host) {
    if (bigButton(ctx, W / 2 - 130 * s, py + 480 * s, 260 * s, 64 * s, 0.8 * s, 'INIZIA', false, 7)) Net.startMatch();
    const where = Net.urls[0] || `http://<IP di questo PC>:${location.port}`; // gli altri indirizzi sono nel terminale del server
    label(ctx, 'Gli altri giocatori aprono  ' + where, W / 2, py + 626 * s, 17 * s, '#fff', 'center');
  } else {
    label(ctx, 'In attesa che l\'host avvii la partita…', W / 2, py + 512 * s, 19 * s, '#8a3b2e', 'center', null);
  }
}

// ---------------------------------------------------------------- cursore

// Sul mondo il cursore anticipa cosa farà il clic destro: spada sui nemici, attrezzo sulle risorse.
const CURSOR_ICON = { attack: () => ICON(5), axe: () => TOOL(2), pick: () => TOOL(4), knife: () => TOOL(3), hammer: () => TOOL(1) };
function drawCursor(ctx) {
  const m = Input.mouse;
  if (!m.inside) return;
  const s = Math.max(G.dpr, G.us), d = Math.round(64 * s), c = UI.cursor;
  if (c === 'hand') ctx.drawImage(img(UIP + 'Cursors/Cursor_02.png'), Math.round(m.x - 27 * s), Math.round(m.y - 19 * s), d, d);
  else if (c === 'no') ctx.drawImage(img(UIP + 'Cursors/Cursor_03.png'), Math.round(m.x - 32 * s), Math.round(m.y - 31 * s), d, d);
  else if (c === 'attack') drawIcon(ctx, CURSOR_ICON.attack(), m.x + 10 * s, m.y + 10 * s, 46 * s);
  else if (CURSOR_ICON[c]) drawIcon(ctx, CURSOR_ICON[c](), m.x + 6 * s, m.y + 6 * s, 96 * s);
  else ctx.drawImage(img(UIP + 'Cursors/Cursor_01.png'), Math.round(m.x - 23 * s), Math.round(m.y - 18 * s), d, d);
}
