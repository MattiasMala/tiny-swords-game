'use strict';
// Schermate di gestione, aperte sopra la partita che continua: Unità (elenco, addestramento, cambio di
// lavoro, invio sulla mappa), Lavoratori (un cursore per risorsa), Ricerche (albero a livelli: un livello per
// era), Costruisci, Mercato, e la modalità test. Più le due file di pulsanti fissi sopra la minimappa che le aprono e
// comandano le truppe. Si usano anche da tastiera: frecce o WASD passano da un pulsante all'altro, Invio o Spazio lo
// premono (vedi UI.step).

// Due file da quattro, nell'ordine in cui appaiono.
const SHORTCUTS = [
  { code: 'KeyU', key: 'U', icon: ['unit', 'warrior'], title: 'Unità', panel: 'units', desc: 'Tutte le tue unità in un elenco: le addestri, le spunti e cambi il loro lavoro.' },
  { code: 'KeyL', key: 'L', icon: ['x', 'pickaxe'], title: 'Lavoratori', panel: 'workers', desc: 'Quanti pedoni su ogni risorsa, con un cursore per ciascuna.' },
  { code: 'KeyI', key: 'I', icon: ['x', 'book'], title: 'Ricerche', panel: 'tree', desc: 'L\'albero delle ricerche: un livello per ogni era.' },
  { code: 'KeyK', key: 'K', icon: ['x', 'scales'], title: 'Mercato', panel: 'market', desc: 'Vendi e compra legna, cibo e pietra in cambio d\'oro.' },
  { code: 'KeyC', key: 'C', icon: ['ui', 1], title: 'Costruisci', panel: 'build', desc: 'Scegli un edificio e poi dove metterlo: ci va il pedone libero più vicino, o quelli selezionati.' },
  { code: 'KeyV', key: 'V', icon: ['x', 'barrow'], title: 'Sposta edificio', desc: 'Sposta l\'edificio selezionato, o il prossimo che scegli (clic, oppure Frecce e Invio); poi la nuova casella, allo stesso modo.', run: () => Ctl.moveMode() },
  { code: 'Tab', key: 'Tab', icon: ['x', 'shield'], title: 'Tutte le truppe', desc: 'Seleziona tutti i soldati, ovunque siano.', run: () => Ctl.selectArmy() },
  { code: 'Backspace', key: 'Bksp', icon: ['build', 'castle'], title: 'Richiama alla base', red: true, desc: 'Tutte le truppe tornano davanti al castello senza fermarsi a combattere.', run: () => Ctl.recall() },
];
function runShortcut(c) {
  if (c.panel) { UI.open(c.panel); return; }
  const why = c.run();
  if (why) { G.toast(why); Sfx.play('deny'); }
}

// Icona di ciò che un'unità sta facendo, per indice di attività (vedi ACT_LABEL).
const ACT_ICON = [XICON('hourglass'), XICON('boot'), ICON(5), RES_ICON.wood, RES_ICON.gold, RES_ICON.meat, ICON(1), ICON(1),
  XICON('barrow'), XICON('herbs'), XICON('shield'), RES_ICON.stone, XICON('shield'), XICON('boot'), XICON('spyglass')];
const RES_ACT = { wood: 3, gold: 4, meat: 5, stone: 11 }; // attività di un pedone al lavoro su ogni risorsa
const TREE_ROWS = ['castle', 'depot', 'farm', 'barracks', 'stable', 'smith', 'market', 'monastery', 'siege'];

function drawShortcuts(ctx, s) {
  const f = G.player, step = 72.5 * s, x0 = G.W - 304 * s, y0 = G.H - 364 * s;
  UI.area(x0, y0, step * 4, 146 * s);
  SHORTCUTS.forEach((c, i) => {
    const cx = x0 + (i % 4 + 0.5) * step, cy = y0 + (Math.floor(i / 4) + 0.5) * 72 * s, r = smallButton(ctx, cx, cy, 98 * s, 'Square', !!c.red); // rosso: la ritirata
    drawSpec(ctx, c.icon, f.color, cx, cy - 2 * s + r.dy, 38 * s);
    label(ctx, c.key, cx - 27 * s, cy - 24 * s, 11.5 * s, '#fff', 'left');
    if ((c.panel && UI.panel === c.panel) || (c.code === 'KeyV' && (Ctl.mode === 'pickmove' || (G.placing && G.placing.move)))) drawBrackets(ctx, cx - 36 * s, cy - 37 * s, 72 * s, 72 * s, 0.7 * s);
    if (r.hover) UI.tip = { x: cx, y: y0 + Math.floor(i / 4) * 72 * s + 6 * s, title: c.title, key: c.key === 'Bksp' ? 'Backspace' : c.key, desc: c.desc };
    if (r.clicked) { Sfx.play('click'); runShortcut(c); }
  });
}

// Cornice comune: ritorna il rettangolo e la scala. style: 'paper' (pergamena), 'dark' (albero delle
// ricerche), 'wood' (tavolo di legno: Costruisci).
function manageFrame(ctx, title, w0, h0, style = 'paper') {
  const s = Math.min(G.W / (w0 + 40), G.H / (h0 + 70), 1.3 * G.dpr), w = w0 * s, h = h0 * s, x = (G.W - w) / 2, y = (G.H - h) / 2 - 6 * s;
  dim(ctx, 0.35);
  if (style === 'paper') {
    drawPanel(ctx, PANEL.banner, x, y, w, h, 0.8 * s);
    label(ctx, title, G.W / 2, y + 32 * s, 27 * s, INK, 'center', null);
  } else {
    drawPanel(ctx, style === 'wood' ? PANEL.wood : PANEL.special, x, y, w, h, 0.9 * s);
    bigRibbon(ctx, G.player.colorIdx, G.W / 2, y + 14 * s, 330 * s, 0.75 * s, title, 25 * s);
  }
  const close = tinyButton(ctx, x + w - 36 * s, y + 34 * s, 40 * s, 'Round', true, 9).clicked;
  // il clic che l'ha appena aperta (su un pulsante dell'HUD) non conta come clic fuori
  if (close || (!UI.fresh && Input.pressed && !UI.over(x, y, w, h))) UI.panel = null;
  UI.fresh = false;
  if (G.toastT > 0) label(ctx, G.toastText, G.W / 2, y + h + 20 * s, 18 * s, '#ffd9cf', 'center');
  return { x, y, w, h, s };
}

function mineUnits() { return G.units.filter(u => u.faction === G.player && !u.dead); }
function hpStrip(ctx, u, x, y, w, t) {
  ctx.fillStyle = '#1b1f33'; ctx.fillRect(x, y, w, t);
  ctx.fillStyle = u.hp > u.maxHp * 0.35 ? '#7be06a' : '#ff5a4a'; ctx.fillRect(x, y, w * clamp(u.hp / u.maxHp, 0, 1), t);
}
// Pulsante quadrato con didascalia; spento (enabled = false) resta visibile ma un clic non fa nulla.
function actionButton(ctx, cx, cy, s, icon, text, enabled, run) {
  if (!enabled) ctx.globalAlpha = 0.4;
  const r = smallButton(ctx, cx, cy, 78 * s, 'Square', false);
  drawSpec(ctx, icon, G.player.color, cx, cy - 2 * s + r.dy, 30 * s);
  label(ctx, text, cx, cy + 36 * s, 11.5 * s, INK, 'center', null);
  ctx.globalAlpha = 1;
  if (r.clicked) { Sfx.play(enabled ? 'click' : 'deny'); if (enabled) run(); }
}
function say(why) { if (why) { G.toast(why); Sfx.play('deny'); } }

// ---------------------------------------------------------------- Unità

function drawUnitsScreen(ctx) {
  const f = G.player, { x, y, w, h, s } = manageFrame(ctx, 'Unità', 1000, 630), all = mineUnits(), marks = UI.marks;
  for (const id of marks) if (!G.index.has(id)) marks.delete(id);
  const pawns = all.filter(u => u.type === 'pawn').length;
  label(ctx, `Pedoni ${pawns}/${f.pawnCap}  ·  Truppe ${all.length - pawns}/${f.troopCap}   —   + addestra  ·  − / + spunta una unità di quel tipo  ·  clic su un'unità o sul nome per spuntare`,
    G.W / 2, y + 62 * s, 13 * s, '#8a3b2e', 'center', null);

  // una fascia per tipo (anche quelli che non hai ancora: da qui si addestrano); le caselle vanno a capo e,
  // se le righe sono troppe, si stringono
  const byType = UNIT_TYPES.map(type => [type, all.filter(u => u.type === type)]);
  const left = x + 44 * s, tilesX = left + 306 * s, tilesW = x + w - 52 * s - tilesX, top = y + 84 * s, bodyH = h - 84 * s - 158 * s;
  let tile = 44 * s, per = 1;
  for (; ; tile -= 2 * s) {
    per = Math.max(1, Math.floor(tilesW / tile));
    const rows = byType.reduce((n, e) => n + Math.max(1, Math.ceil(e[1].length / per)), 0);
    if (rows * (tile + 4 * s) <= bodyH || tile <= 24 * s) break;
  }
  const rowH = tile + 4 * s;
  let ry = top;
  for (const [type, units] of byType) {
    const d = UNIT_DEF[type], picked = units.filter(u => marks.has(u.id)), allMarked = units.length > 0 && picked.length === units.length, head = UI.hit(left, ry, 168 * s, rowH);
    if (!units.length) ctx.globalAlpha = 0.55;
    drawPortrait(ctx, f.color, type, left, ry + 2 * s, tile * 0.9);
    label(ctx, `${d.label}  ${units.length}`, left + tile + 4 * s, ry + rowH / 2, 14.5 * s, head.hover && units.length ? '#8a3b2e' : INK, 'left', null);
    ctx.globalAlpha = 1;
    if (head.clicked && units.length) { for (const u of units) { if (allMarked) marks.delete(u.id); else marks.add(u.id); } Sfx.play('click'); }
    // addestramento: nell'edificio di quel tipo con la coda più corta
    const b = freeBuilding(f, d.from), lock = !b ? 'Richiede: ' + BUILD_DEF[d.from].label : trainBlock(f, b, type);
    const queued = G.buildings.reduce((n, o) => n + (o.faction === f ? o.queue.filter(q => q.unit === type).length : 0), 0);
    const tx = left + 186 * s, tcy = ry + rowH / 2, tr = tinyButton(ctx, tx, tcy, Math.min(34 * s, tile * 0.8), 'Round', !!lock);
    label(ctx, '+', tx, tcy - 2 * s, 19 * s, '#fff', 'center');
    if (queued) label(ctx, String(queued), tx + 12 * s, tcy + 11 * s, 12.5 * s, '#ffe27a', 'center');
    if (tr.hover) UI.tip = { x: tx + 130 * s, y: ry, title: 'Addestra ' + d.label, cost: d.cost, desc: d.desc + (queued ? ` In coda: ${queued}.` : ''), lock };
    if (tr.clicked) say(lock || command('train', b.id, type));
    // quante spuntarne di questo tipo: una in meno, una in più (prima le più sane)
    if (units.length) {
      const bs = Math.min(26 * s, tile * 0.62), minus = tinyButton(ctx, left + 226 * s, tcy, bs, 'Square', true), plus = tinyButton(ctx, left + 286 * s, tcy, bs, 'Square', false);
      label(ctx, '−', left + 226 * s, tcy - 2 * s, 16 * s, '#fff', 'center');
      label(ctx, '+', left + 286 * s, tcy - 2 * s, 16 * s, '#fff', 'center');
      label(ctx, String(picked.length), left + 256 * s, tcy, 15 * s, picked.length ? '#8a3b2e' : INK, 'center', null);
      if (minus.hover || plus.hover) UI.tip = { x: left + 256 * s, y: ry, title: d.label, desc: `Spuntate ${picked.length} su ${units.length}: − ne toglie una, + ne aggiunge una. Poi, in basso, cosa farne.` };
      if (minus.clicked && picked.length) marks.delete(picked[picked.length - 1].id);
      if (plus.clicked) { const next = units.filter(u => !marks.has(u.id)).sort((a, b) => b.hp / b.maxHp - a.hp / a.maxHp)[0]; if (next) marks.add(next.id); }
    }
    units.forEach((u, i) => {
      const cx = tilesX + (i % per + 0.5) * tile, cy = ry + (Math.floor(i / per) + 0.5) * rowH;
      const r = tinyButton(ctx, cx, cy, tile * 0.94, 'Square', u.hp < u.maxHp * 0.35);
      drawPortrait(ctx, f.color, type, cx - tile * 0.34, cy - tile * 0.38, tile * 0.68);
      hpStrip(ctx, u, cx - tile * 0.32, cy + tile * 0.3, tile * 0.64, Math.max(2, 4 * s));
      drawIcon(ctx, ACT_ICON[u.act], cx + tile * 0.27, cy - tile * 0.25, tile * 0.44);
      if (marks.has(u.id)) drawBrackets(ctx, cx - tile * 0.5, cy - tile * 0.5, tile, tile, tile / 80);
      if (r.hover) UI.tip = { x: cx, y: cy - tile * 0.2, title: UNIT_DEF[type].label, desc: `${ACT_LABEL[u.act]}  ·  vita ${Math.ceil(u.hp)}/${u.maxHp}` };
      if (r.clicked) { if (marks.has(u.id)) marks.delete(u.id); else marks.add(u.id); }
    });
    ry += Math.max(1, Math.ceil(units.length / per)) * rowH;
  }

  // in basso: cosa fare delle unità spuntate
  const marked = all.filter(u => marks.has(u.id)), mp = marked.filter(u => u.type === 'pawn'), ms = marked.filter(u => u.type !== 'pawn');
  const by = y + h - 104 * s, ids = list => list.map(u => u.id);
  if (bigButton(ctx, left, by - 34 * s, 118 * s, 36 * s, 0.5 * s, 'Tutte', false)) for (const u of all) marks.add(u.id);
  if (bigButton(ctx, left, by + 8 * s, 118 * s, 36 * s, 0.5 * s, 'Nessuna', true)) marks.clear();
  label(ctx, `Pedoni spuntati: ${mp.length}`, left + 290 * s, by - 46 * s, 13 * s, '#8a3b2e', 'center', null);
  RES_KINDS.forEach((k, i) => actionButton(ctx, left + 185 * s + i * 70 * s, by, s, ['res', k], RES_LABEL[k], mp.length > 0,
    () => say(command('job', ids(mp), k))));
  label(ctx, `Truppe spuntate: ${ms.length}`, left + 580 * s, by - 46 * s, 13 * s, '#8a3b2e', 'center', null);
  actionButton(ctx, left + 480 * s, by, s, ['x', 'stop'], 'Ferma', marked.length > 0, () => command('order', ids(marked), 'stop', 0, 0, 0));
  actionButton(ctx, left + 550 * s, by, s, ['build', 'castle'], 'Richiama', ms.length > 0, () => Ctl.recall(ms));
  // la spada: si apre la mappa in grande e un clic le manda lì
  actionButton(ctx, left + 620 * s, by, s, ['ui', 5], 'Invia', ms.length > 0, () => {
    const c = f.castle;
    UI.send = { ids: ids(ms), x: c ? c.x : 0, y: c ? c.y : 0, kbd: UI.kbd, go: false };
    UI.panel = 'send'; UI.fresh = true; UI.focus = null;
  });
  if (!marked.length) ctx.globalAlpha = 0.4;
  if (bigButton(ctx, x + w - 286 * s, by - 28 * s, 236 * s, 56 * s, 0.6 * s, 'Seleziona sulla mappa', false) && marked.length) {
    Ctl.select(ids(marked));
    Ctl.centerOnSelection();
    UI.panel = null;
  }
  ctx.globalAlpha = 1;
}

// ---------------------------------------------------------------- Invia

// La mappa in grande: un clic (oppure le frecce e Invio) manda lì, in marcia d'attacco e in formazione, le truppe
// spuntate nella schermata Unità.
function drawSendScreen(ctx) {
  const t = UI.send, { x, y, w, s } = manageFrame(ctx, 'Dove le mando?', 1000, 720);
  const units = t ? t.ids.map(id => G.index.get(id)).filter(u => u && !u.dead) : [];
  if (!units.length) { if (UI.panel === 'send') UI.panel = 'units'; return; }
  if (UI.panel !== 'send') return; // chiusa con la X o cliccando fuori
  const mw = 900 * s, mh = mw * World.H / World.W, mx = x + (w - mw) / 2, my = y + 88 * s, m = Input.mouse;
  drawIcon(ctx, XICON('map'), x + 78 * s, y + 44 * s, 52 * s);
  label(ctx, `${units.length} unità: clic sul punto d'arrivo, oppure Frecce e Invio. Ci vanno da sole, attaccando chi incontrano.  Esc: torna a Unità`,
    G.W / 2, y + 64 * s, 13.5 * s, '#8a3b2e', 'center', null);
  const k = drawMap(ctx, mx, my, mw, mh, s * 1.5), inside = UI.over(mx, my, mw, mh);
  ctx.strokeStyle = '#ffffff'; ctx.lineWidth = Math.max(1.5, 2 * s);
  for (const u of units) { ctx.beginPath(); ctx.arc(mx + u.x * k, my + u.y * k, 5 * s, 0, Math.PI * 2); ctx.stroke(); } // chi parte
  if (inside && Input.moved) t.kbd = false;
  if (inside && !t.kbd) { t.x = (m.x - mx) / k; t.y = (m.y - my) / k; }
  if (inside || t.kbd) { // il punto d'arrivo
    const px = mx + t.x * k, py = my + t.y * k;
    ctx.strokeStyle = '#ff6a5a'; ctx.lineWidth = Math.max(2, 2.5 * s);
    ctx.beginPath(); ctx.arc(px, py, 13 * s, 0, Math.PI * 2); ctx.stroke();
    drawIcon(ctx, ICON(5), px, py, 34 * s);
    UI.cursor = 'attack';
  }
  const go = t.go || (inside && Input.pressed);
  t.go = false;
  if (!go) return;
  command('order', units.map(u => u.id), 'amove', Math.round(t.x), Math.round(t.y), 0);
  Ctl.mark('attack', t.x, t.y);
  Sfx.play('ack');
  G.toast(`${units.length} unità in marcia`);
  UI.panel = null; UI.send = null; UI.marks.clear();
}

// ---------------------------------------------------------------- Lavoratori

// I cursori sono contatori: mostrano quanti pedoni lavorano ora a ogni risorsa (comunque ci siano arrivati)
// e spostarli ne manda o ne toglie. Salgono fin dove ci sono pedoni liberi.
function drawWorkersScreen(ctx) {
  const f = G.player, { x, y, w, h, s } = manageFrame(ctx, 'Lavoratori', 700, 500);
  const pawns = mineUnits().filter(u => u.type === 'pawn'), N = Math.max(1, pawns.length);
  // liberi: fermi, oppure in viaggio solo per consegnare un carico (attività 8); vedi setWorkers. Chi è al riparo
  // o in fuga resta contato sulla risorsa a cui tornerà (job): altrimenti a ogni allarme i cursori andrebbero a zero.
  const free = pawns.filter(u => u.act === 0 || u.act === 8).length, building = pawns.filter(u => u.act === 6 || u.act === 7).length;
  const hidden = pawns.filter(u => u.act === 12 || u.act === 13).length, moving = pawns.filter(u => u.act === 1).length;
  label(ctx, `Pedoni ${pawns.length}/${f.pawnCap}  ·  Liberi ${free}  ·  Al cantiere ${building}${hidden ? `  ·  Al riparo ${hidden}` : ''}${moving ? `  ·  In viaggio ${moving}` : ''}`,
    G.W / 2, y + 66 * s, 15 * s, hidden ? RED : '#8a3b2e', 'center', null);
  if (!Input.lmb) UI.slide = null;
  // lo stesso valore non parte due volte di fila: in rete il conteggio si aggiorna con un attimo di ritardo
  const send = (k, v) => {
    if (UI.sent && UI.sent.k === k && UI.sent.v === v && G.clock - UI.sent.t < 0.5) return;
    UI.sent = { k, v, t: G.clock };
    say(command('jobs', k, v));
  };
  const tx = x + 250 * s, tw = 300 * s;
  RES_KINDS.forEach((k, i) => {
    const ry = y + 122 * s + i * 68 * s, n = pawns.filter(u => u.job === i).length, max = n + free, rate = f.rate[k];
    drawIcon(ctx, RES_ICON[k], x + 78 * s, ry, 46 * s);
    label(ctx, RES_LABEL[k], x + 110 * s, ry - (rate ? 8 * s : 0), 17 * s, INK, 'left', null);
    if (rate) label(ctx, `+${rate}/min`, x + 110 * s, ry + 13 * s, 12.5 * s, '#2f7a3a', 'left', null);
    if (tinyButton(ctx, tx - 38 * s, ry, 34 * s, 'Round', true).clicked && n > 0) send(k, n - 1);
    label(ctx, '−', tx - 38 * s, ry - 2 * s, 20 * s, '#fff', 'center');
    if (tinyButton(ctx, tx + tw + 38 * s, ry, 34 * s, 'Round', false).clicked) { if (free > 0) send(k, n + 1); else say('Nessun pedone libero'); }
    label(ctx, '+', tx + tw + 38 * s, ry - 2 * s, 20 * s, '#fff', 'center');
    bigBar(ctx, tx, ry - 14 * s, tw, n / N, 0.55 * s);
    ctx.fillStyle = 'rgba(74,58,51,0.75)'; // fin dove può salire adesso
    ctx.fillRect(Math.round(tx + tw * max / N - s), ry - 22 * s, Math.max(2, 2 * s), 44 * s);
    if (UI.over(tx - 12 * s, ry - 22 * s, tw + 24 * s, 44 * s)) { // il cursore si trascina col mouse; da tastiera ci sono − e +
      UI._hot = true; UI.cursor = 'hand';
      if (Input.pressed) UI.slide = k;
    }
    const want = UI.slide === k ? clamp(Math.round((Input.mouse.x - tx) / tw * N), 0, max) : n;
    if (want !== n) send(k, want);
    const knob = 34 * s, kx = tx + tw * want / N;
    ctx.drawImage(img(UIP + 'Buttons/TinyRoundBlueButton.png'), Math.round(kx - knob / 2), Math.round(ry - knob / 2), Math.round(knob), Math.round(knob));
    label(ctx, String(n), tx + tw + 82 * s, ry, 22 * s, INK, 'center', null);
  });
  const fy = y + 122 * s + 4 * 68 * s;
  drawAvatar(ctx, f.color, 'pawn', x + 56 * s, fy - 22 * s, 44 * s);
  label(ctx, `Liberi: ${free}`, x + 110 * s, fy, 17 * s, free ? '#8a3b2e' : INK, 'left', null);
  const lines = wrapText(ctx, 'Alzando un cursore parte il pedone libero più vicino a quella risorsa; abbassandolo, i pedoni tolti restano liberi. I nuovi pedoni non vengono assegnati da soli.', w - 330 * s, 12.5 * s);
  lines.forEach((line, i) => label(ctx, line, x + 250 * s, fy - 12 * s + i * 17 * s, 12.5 * s, INK, 'left', null));
}

// ---------------------------------------------------------------- Ricerche

// Stato di una ricerca per il giocatore: fatta, in corso, oppure il motivo per cui non si può avviare.
function techState(f, id) {
  const d = TECH_DEF[id], b = freeBuilding(f, d.at), done = !!f.tech[id], busy = !!f.busy[id];
  const lock = done || busy ? null : !b ? 'Richiede: ' + BUILD_DEF[d.at].label : techLock(f, d) || (b.queue.length >= QUEUE_MAX ? 'Coda piena' : null);
  return { d, b, done, busy, lock, poor: !done && !busy && !lock && !canAfford(f, d.cost) };
}

function drawTechNode(ctx, f, id, cx, cy, s) {
  const { d, b, done, busy, lock, poor } = techState(f, id), size = 46 * s;
  if (lock) ctx.globalAlpha = 0.42;
  const r = tinyButton(ctx, cx, cy, size, 'Square', poor);
  drawSpec(ctx, d.icon, f.color, cx, cy - s, size * 0.5);
  ctx.globalAlpha = 1;
  if (done) { // spunta verde
    const bx = cx + size * 0.32, by = cy + size * 0.3, br = 8.5 * s;
    ctx.fillStyle = '#3f9d4a'; ctx.beginPath(); ctx.arc(bx, by, br, 0, Math.PI * 2); ctx.fill();
    ctx.strokeStyle = '#fff'; ctx.lineWidth = 2.4 * s; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    ctx.beginPath(); ctx.moveTo(bx - br * 0.45, by); ctx.lineTo(bx - br * 0.1, by + br * 0.38); ctx.lineTo(bx + br * 0.5, by - br * 0.35); ctx.stroke();
    ctx.lineCap = 'butt';
  } else if (busy) {
    let ratio = 0;
    for (const o of G.buildings) if (o.faction === f) for (const q of o.queue) if (q.tech === id) ratio = q.t / q.total;
    smallBar(ctx, cx - size * 0.42, cy + size * 0.3, size * 0.84, ratio, 0.5 * s, Assets.barFill[f.color]);
  }
  if (r.hover) {
    const note = done ? 'Già ricercata. ' : busy ? 'Ricerca in corso. ' : '';
    UI.tip = { x: cx, y: cy - size * 0.2, title: d.label, cost: done ? null : d.cost, desc: note + d.desc, lock };
  }
  if (r.clicked && !done && !busy) say(lock || command('research', b.id, id));
}

function drawTreeScreen(ctx) {
  const f = G.player, { x, y, w, h, s } = manageFrame(ctx, 'RICERCHE', 980, 660, 'dark');
  const left = x + 40 * s, labelW = 178 * s, colW = (w - 80 * s - labelW) / 3, top = y + 112 * s, rowH = (h - 112 * s - 52 * s) / TREE_ROWS.length;
  // un livello per era: quelli già raggiunti sono accesi
  for (let a = 1; a <= 3; a++) {
    const cx = left + labelW + (a - 1) * colW, on = a <= f.age;
    ctx.fillStyle = on ? 'rgba(255,255,255,0.08)' : 'rgba(0,0,0,0.25)';
    ctx.fillRect(Math.round(cx + 4 * s), Math.round(top - 46 * s), Math.round(colW - 8 * s), Math.round(rowH * TREE_ROWS.length + 50 * s));
    label(ctx, `${['I', 'II', 'III'][a - 1]}  ·  ${AGE_NAME[a]}`, cx + colW / 2, top - 26 * s, 15.5 * s, on ? '#ffe9a3' : '#9aa3b5', 'center');
  }
  // posto di ogni ricerca: riga = edificio che la offre, colonna = era
  const pos = {};
  TREE_ROWS.forEach((kind, r) => {
    for (let a = 1; a <= 3; a++) {
      // chi apre una ricerca del livello successivo sta più a destra: la linea non passa sopra le vicine
      const leads = id => (TECH_IDS.some(o => TECH_DEF[o].req === id && TECH_DEF[o].age > a) ? 1 : 0);
      const ids = TECH_IDS.filter(id => TECH_DEF[id].at === kind && TECH_DEF[id].age === a).sort((p, q) => leads(p) - leads(q));
      ids.forEach((id, i) => { pos[id] = [left + labelW + (a - 0.5) * colW + (i - (ids.length - 1) / 2) * 60 * s, top + (r + 0.5) * rowH]; });
    }
  });
  for (const id of TECH_IDS) { // ogni ricerca è legata a quella che la precede
    const req = TECH_DEF[id].req;
    if (!req) continue;
    ctx.strokeStyle = f.tech[req] ? '#ffe27a' : 'rgba(255,255,255,0.28)'; ctx.lineWidth = Math.max(2, 3 * s);
    ctx.beginPath(); ctx.moveTo(pos[req][0], pos[req][1]); ctx.lineTo(pos[id][0], pos[id][1]); ctx.stroke();
  }
  TREE_ROWS.forEach((kind, r) => {
    const cy = top + (r + 0.5) * rowH, has = !!freeBuilding(f, kind);
    if (!has) ctx.globalAlpha = 0.45;
    drawBuildingThumb(ctx, f.color, kind, left + 26 * s, cy, 44 * s);
    ctx.globalAlpha = 1;
    label(ctx, BUILD_DEF[kind].label, left + 56 * s, cy - (has ? 0 : 8 * s), 13.5 * s, has ? '#f1e6c8' : '#9aa3b5', 'left');
    if (!has) label(ctx, 'da costruire', left + 56 * s, cy + 9 * s, 11.5 * s, '#e79a8a', 'left');
  });
  for (const id of TECH_IDS) drawTechNode(ctx, f, id, pos[id][0], pos[id][1], s);
  label(ctx, 'Clic su una ricerca per avviarla nell\'edificio che la offre  ·  rosso: mancano le risorse  ·  spento: manca un requisito',
    G.W / 2, y + h - 30 * s, 12.5 * s, '#d9cfb0', 'center');
}

// ---------------------------------------------------------------- Costruisci

function drawBuildScreen(ctx) {
  const f = G.player, { x, y, w, h, s } = manageFrame(ctx, 'COSTRUISCI', 840, 690, 'wood');
  label(ctx, 'Scegli l\'edificio, poi dove metterlo (clic, o Frecce e Invio): ci va il pedone libero più vicino, o quelli che hai selezionato.',
    G.W / 2, y + h - 54 * s, 12.5 * s, '#f6ead0', 'center');
  const cw = 176 * s, ch = 130 * s, gx = x + (w - cw * 4) / 2, gy = y + 92 * s;
  BUILD_MENU.forEach((kind, i) => {
    const d = BUILD_DEF[kind], bx = gx + (i % 4) * cw, by = gy + Math.floor(i / 4) * ch, lock = ageLock(f, d), poor = !lock && !canAfford(f, d.cost);
    const r = UI.hit(bx + 4 * s, by + 4 * s, cw - 8 * s, ch - 8 * s);
    drawPanel(ctx, PANEL.woodSlots, bx + 6 * s, by + 6 * s, cw - 12 * s, ch - 12 * s, 0.7 * s);
    if (lock) ctx.globalAlpha = 0.45;
    drawBuildingThumb(ctx, f.color, kind, bx + cw / 2, by + 46 * s, 68 * s);
    label(ctx, d.label, bx + cw / 2, by + 88 * s, 13.5 * s, '#fff', 'center');
    ctx.globalAlpha = 1;
    if (lock) label(ctx, AGE_NAME[d.age], bx + cw / 2, by + 108 * s, 12 * s, '#ff8a7a', 'center');
    else drawCost(ctx, f, d.cost, bx + cw / 2, by + 108 * s, s, true, cw - 16 * s); // centrato, dentro la casella anche con tre risorse
    if (r.hover) {
      drawBrackets(ctx, bx + 4 * s, by + 4 * s, cw - 8 * s, ch - 8 * s, 0.7 * s);
      UI.tip = { x: bx + cw / 2, y: by + 14 * s, title: d.label, cost: d.cost, desc: d.desc, lock: lock || (poor ? 'Risorse insufficienti' : null) };
    }
    if (r.clicked) {
      const why = lock || (poor ? 'Risorse insufficienti' : !mineUnits().some(u => u.type === 'pawn') ? 'Non hai pedoni' : null);
      say(why);
      if (!why) { Sfx.play('click'); UI.panel = null; Ctl.startPlacing(kind); }
    }
  });
}

// ---------------------------------------------------------------- Mercato

function drawMarketScreen(ctx) {
  const f = G.player, { x, y, w, s } = manageFrame(ctx, 'Mercato', 700, 440), market = freeBuilding(f, 'market');
  drawIcon(ctx, XICON('scales'), x + 70 * s, y + 74 * s, 46 * s);
  drawIcon(ctx, RES_ICON.gold, x + w / 2 - 60 * s, y + 74 * s, 40 * s);
  label(ctx, `Oro: ${Math.floor(f.res.gold)}`, x + w / 2 - 34 * s, y + 75 * s, 19 * s, INK, 'left', null);
  if (!market) {
    label(ctx, 'Non hai ancora un Mercato.', G.W / 2, y + 190 * s, 19 * s, '#8a3b2e', 'center', null);
    label(ctx, `Si costruisce dall'${AGE_NAME[BUILD_DEF.market.age]}: schermata Costruisci (tasto C).`, G.W / 2, y + 226 * s, 14.5 * s, INK, 'center', null);
    return;
  }
  ['wood', 'meat', 'stone'].forEach((k, i) => {
    const ry = y + 150 * s + i * 82 * s, sell = sellPrice(f, k), buy = buyPrice(f, k);
    drawIcon(ctx, RES_ICON[k], x + 74 * s, ry, 50 * s);
    label(ctx, RES_LABEL[k], x + 108 * s, ry - 10 * s, 17 * s, INK, 'left', null);
    label(ctx, `ne hai ${Math.floor(f.res[k])}`, x + 108 * s, ry + 12 * s, 12.5 * s, '#8a3b2e', 'left', null);
    // monete su chi vende, freccia in su su chi compra
    if (f.res[k] < 100) ctx.globalAlpha = 0.45;
    if (bigButton(ctx, x + 236 * s, ry - 28 * s, 200 * s, 56 * s, 0.58 * s, `Vendi 100:  +${sell}`, false)) say(command('trade', market.id, 'sell', k));
    drawIcon(ctx, XICON('coins'), x + 236 * s + 26 * s, ry - 2 * s, 30 * s);
    ctx.globalAlpha = f.res.gold < buy ? 0.45 : 1;
    if (bigButton(ctx, x + 452 * s, ry - 28 * s, 200 * s, 56 * s, 0.58 * s, `Compra 100:  −${buy}`, true)) say(command('trade', market.id, 'buy', k));
    drawIcon(ctx, XICON('upgrade'), x + 452 * s + 26 * s, ry - 2 * s, 30 * s);
    ctx.globalAlpha = 1;
  });
  label(ctx, 'I prezzi sono in oro. Ogni scambio li peggiora per un po\'.', G.W / 2, y + 150 * s + 3 * 82 * s - 18 * s, 13 * s, INK, 'center', null);
}

// ---------------------------------------------------------------- Modalità test

// Per provare il gioco, solo nelle partite da soli (F4): risorse in regalo, tempo fermo, nebbia spenta. A tempo
// fermo la visuale si muove e gli ordini si danno lo stesso: partono quando il tempo riparte.
const TEST_GIFT = 1000;
function drawTestScreen(ctx) {
  const f = G.player, { x, y, w, s } = manageFrame(ctx, 'Modalità test', 700, 530), bw = 136 * s, gap = (w - 80 * s - bw * 4) / 3;
  label(ctx, 'Solo nelle partite da soli. Si apre e si chiude con F4.', G.W / 2, y + 66 * s, 13.5 * s, '#8a3b2e', 'center', null);
  RES_KINDS.forEach((k, i) => {
    const bx = x + 40 * s + i * (bw + gap);
    drawIcon(ctx, RES_ICON[k], bx + 26 * s, y + 116 * s, 44 * s);
    label(ctx, String(Math.floor(f.res[k])), bx + 52 * s, y + 117 * s, 17 * s, INK, 'left', null);
    if (bigButton(ctx, bx, y + 144 * s, bw, 54 * s, 0.58 * s, `+${TEST_GIFT}`, false)) f.res[k] += TEST_GIFT;
  });
  if (bigButton(ctx, x + 40 * s, y + 212 * s, w - 80 * s, 54 * s, 0.58 * s, `+${TEST_GIFT} di tutte e quattro`, false)) for (const k of RES_KINDS) f.res[k] += TEST_GIFT;
  if (bigButton(ctx, x + 40 * s, y + 292 * s, w - 80 * s, 54 * s, 0.58 * s, G.frozen ? 'Tempo fermo: fallo ripartire' : 'Ferma il tempo', G.frozen)) G.frozen = !G.frozen;
  if (bigButton(ctx, x + 40 * s, y + 360 * s, w - 80 * s, 54 * s, 0.58 * s, Fog.on ? 'Togli la nebbia: tutta la mappa in vista' : 'Nebbia tolta: rimettila', !Fog.on)) Fog.on = !Fog.on;
  label(ctx, 'A tempo fermo puoi guardare in giro e dare ordini: partono quando lo fai ripartire.', G.W / 2, y + 438 * s, 13 * s, INK, 'center', null);
}

function drawManage(ctx) {
  UI.tip = null;
  if (UI.panel === 'units') drawUnitsScreen(ctx);
  else if (UI.panel === 'send') drawSendScreen(ctx);
  else if (UI.panel === 'workers') drawWorkersScreen(ctx);
  else if (UI.panel === 'build') drawBuildScreen(ctx);
  else if (UI.panel === 'market') drawMarketScreen(ctx);
  else if (UI.panel === 'test') drawTestScreen(ctx);
  else drawTreeScreen(ctx);
  if (UI.tip) drawTooltip(ctx, UI.tip, G.us);
}
