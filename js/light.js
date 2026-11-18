'use strict';
// Luce in stile HD-2D sopra il mondo: ciclo giorno/notte, ombre degli sprite che girano col sole, luci
// calde di edifici e falò che tremolano, bagliore delle parti chiare, sfocatura "da diorama" in alto e in
// basso, vignettatura e raggi di sole. È solo presentazione: segue G.time, quindi in rete è uguale per tutti.

const DAY_LENGTH = 480; // secondi di gioco per un giorno intero
// Momenti del giorno: [fase 0..1, colore della luce ambiente (r, g, b), forza delle ombre]
const DAY_KEYS = [
  [0.00, [255, 200, 172], 0.20], // alba
  [0.07, [255, 251, 240], 0.30], // giorno
  [0.50, [255, 251, 240], 0.30],
  [0.60, [255, 206, 150], 0.28], // ora dorata
  [0.67, [208, 142, 150], 0.20], // tramonto
  [0.73, [108, 124, 196], 0.14], // notte: più scuro e azzurro, ma leggibile
  [0.95, [108, 124, 196], 0.14],
  [1.00, [255, 200, 172], 0.20],
];
const FLAT_PROPS = new Set(['bones', 'flowers', 'grass', 'lilypads', 'mushrooms', 'reeds', 'boat', 'flowers_red', 'flowers_blue', 'flowers_white',
  'clover', 'pebbles', 'puddle', 'sticks', 'dock', 'shipwreck', 'tall_grass', 'ferns']); // a terra o in acqua: niente ombra
const POST_GLOW = 0.34; // opacità del bagliore delle parti chiare (e dei raggi di sole) steso dalla passata finale

const Light = {
  level: 2,   // 2 = tutto · 1 = ombre e luci, senza passata finale né raggi · 0 = spenta (tasto F2)
  auto: true, // finché il giocatore non sceglie a mano, si alleggerisce da sola se il gioco rallenta
  slow: 0,
  phase: 0, ambient: [255, 255, 255], night: 0, shade: 0.3, lx: 0, ly: 0.4,
  bufs: {}, glow: null, vignette: null, shaft: null,
  get on() { return this.level > 0; },

  cycle() {
    this.level = (this.level + 2) % 3;
    this.auto = false;
    G.toast(['Luci spente', 'Luci leggere: senza sfocatura, bagliore e raggi', 'Luci complete'][this.level]);
  },
  // dt = secondi veri fra due fotogrammi. Tre secondi sotto i 33 fotogrammi al secondo e scende di un livello.
  watch(dt) {
    if (!this.auto || !this.level || dt > 0.25) return; // oltre un quarto di secondo la scheda era nascosta
    this.slow = dt > 0.03 ? this.slow + dt : Math.max(0, this.slow - dt * 2);
    if (this.slow < 3) return;
    this.level--; this.slow = 0;
    G.toast(this.level ? 'Il gioco rallentava: luci alleggerite (F2 per cambiarle)' : 'Il gioco rallentava: luci spente (F2 per riaccenderle)');
  },

  buf(name, w, h) {
    let b = this.bufs[name];
    if (!b) { const c = document.createElement('canvas'); b = this.bufs[name] = { c, g: c.getContext('2d') }; }
    if (b.c.width !== w || b.c.height !== h) { b.c.width = w; b.c.height = h; }
    b.g.imageSmoothingEnabled = true;
    return b;
  },

  // Sprite sfumati disegnati una volta sola: alone di una luce, vignettatura, raggio di sole.
  build() {
    const radial = (size, stops) => {
      const c = document.createElement('canvas'); c.width = c.height = size;
      const g = c.getContext('2d'), r = g.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
      for (const [at, color] of stops) r.addColorStop(at, color);
      g.fillStyle = r; g.fillRect(0, 0, size, size);
      return c;
    };
    this.glow = radial(256, [[0, 'rgba(255,220,160,1)'], [0.35, 'rgba(255,176,92,0.55)'], [0.7, 'rgba(255,140,60,0.16)'], [1, 'rgba(255,120,40,0)']]);
    this.vignette = radial(256, [[0, 'rgba(8,10,28,0)'], [0.62, 'rgba(8,10,28,0)'], [0.86, 'rgba(8,10,28,0.42)'], [1, 'rgba(8,10,28,0.85)']]);
    const c = this.shaft = document.createElement('canvas'); c.width = 64; c.height = 256;
    const g = c.getContext('2d'), across = g.createLinearGradient(0, 0, 64, 0), along = g.createLinearGradient(0, 0, 0, 256);
    across.addColorStop(0, 'rgba(255,244,210,0)'); across.addColorStop(0.5, 'rgba(255,244,210,1)'); across.addColorStop(1, 'rgba(255,244,210,0)');
    g.fillStyle = across; g.fillRect(0, 0, 64, 256);
    along.addColorStop(0, 'rgba(0,0,0,1)'); along.addColorStop(0.7, 'rgba(0,0,0,0.5)'); along.addColorStop(1, 'rgba(0,0,0,0)');
    g.globalCompositeOperation = 'destination-in'; g.fillStyle = along; g.fillRect(0, 0, 64, 256);
  },

  // Ora del giorno: luce ambiente, quanto è notte (0..1), direzione e forza delle ombre.
  tick() {
    const p = this.phase = (G.time / DAY_LENGTH + 0.1) % 1;
    let i = 1;
    while (DAY_KEYS[i][0] < p) i++;
    const [p0, c0, s0] = DAY_KEYS[i - 1], [p1, c1, s1] = DAY_KEYS[i], k = (p - p0) / (p1 - p0);
    this.ambient = c0.map((v, j) => Math.round(v + (c1[j] - v) * k));
    this.shade = s0 + (s1 - s0) * k;
    const lum = (this.ambient[0] * 0.3 + this.ambient[1] * 0.59 + this.ambient[2] * 0.11) / 255;
    this.night = clamp((0.92 - lum) / 0.42, 0, 1);
    // di giorno il sole va da est a ovest: le ombre girano da sinistra a destra, corte a mezzogiorno;
    // di notte c'è la luna, ferma, con ombre tenui
    const day = clamp(p / 0.7, 0, 1), sun = (day - 0.5) * 2;
    this.lx = this.night > 0.9 ? -0.55 : sun * 1.15;
    this.ly = this.night > 0.9 ? 0.34 : 0.26 + 0.2 * sun * sun;
  },

  // Base a terra da cui parte l'ombra di un'entità visibile; null se non ne fa.
  base(e) {
    if (e.isUnit || e.isBuilding || e.isPoint) return e.sy;
    switch (e.draw) {
      case drawTree: return e.wood > 0 ? e.y : null;
      case drawGold: return e.big ? e.sy : e.y + 30;
      case drawStone: return e.big ? e.sy : e.y + 20;
      case drawSheep: return e.y;
      case drawBush: return e.y + 2;
      case drawRock: return e.y + 8;
      case drawProp: return FLAT_PROPS.has(e.name) ? null : e.y;
    }
    return null;
  },

  // Ombre: ogni sprite ridisegnato come sagoma scura, coricata a terra lungo la direzione della luce.
  // Vanno su una tela a mezza risoluzione, poi stesa tutta insieme: così dove si sovrappongono non scuriscono due volte.
  shadows(ctx, list) {
    if (!this.on) return;
    const z = G.zoom, k = z / 2, L = this.buf('shadow', Math.ceil(G.W / 2), Math.ceil(G.H / 2)), g = L.g;
    const ox = -Math.round(G.cam.x * z) / 2, oy = -Math.round(G.cam.y * z) / 2;
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.clearRect(0, 0, L.c.width, L.c.height);
    g.imageSmoothingEnabled = false;
    SHADOW.on = true;
    for (const e of list) {
      const by = this.base(e);
      if (by === null) continue;
      g.setTransform(k, 0, 0, k, ox, oy);
      g.translate(e.x, by);
      g.transform(1, 0, -this.lx, -this.ly, 0, 0);
      g.translate(-e.x, -by);
      e.draw(g, e);
      g.globalAlpha = 1;
    }
    SHADOW.on = false;
    // le nuvole passano sopra: la loro ombra scivola sul terreno
    g.setTransform(k, 0, 0, k, ox, oy);
    g.globalAlpha = 0.5;
    for (const c of G.clouds) g.drawImage(SHADOW.of(c.im, 'cloud' + c.im.src), Math.round(c.x + 90 + this.lx * 60), Math.round(c.y + 260));
    g.globalAlpha = 1;
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = this.shade;
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(L.c, 0, 0, L.c.width * 2, L.c.height * 2);
    ctx.restore();
    ctx.imageSmoothingEnabled = false;
  },

  // Luci accese da ciò che si vede: finestre degli edifici, falò, incendi, il luccichio dell'oro.
  lights(list) {
    const out = [], t = G.time;
    const add = (x, y, r, power, seed, flick) => out.push({ x, y, r, power: power * (1 - flick + flick * (0.5 + 0.5 * Math.sin(t * 9 + seed) * Math.sin(t * 5.3 + seed * 1.7))) });
    for (const e of list) {
      if (e.isBuilding) {
        if (!e.done || e.kind === 'wall') continue;
        const d = e.def;
        add(e.x, e.y - d.ay * 0.32, e.kind === 'castle' ? 340 : d.w > 2 ? 260 : 200, 1, e.id, 0.12);
        if (e.hp < e.maxHp * 0.7) add(e.x, e.y - d.ay * 0.4, 240, 1.2, e.id * 3, 0.5); // brucia
      } else if (e.isPoint) { if (e.owner) add(e.x, e.y - e.def.ay * 0.3, 220, 0.9, e.id, 0.12); } // un punto tenuto da qualcuno ha le luci accese
      else if (e.draw === drawProp && (e.name === 'campfire' || e.name === 'shrine')) add(e.x, e.y - 18, e.name === 'shrine' ? 150 : 290, e.name === 'shrine' ? 0.7 : 1.25, e.x, 0.35);
      else if (e.draw === drawGold && e.amount > 0) add(e.x, e.y, 120, 0.4, e.id, 0.2);
    }
    return out;
  },

  // Sopra il mondo già disegnato (coordinate schermo): luce ambiente dell'ora e luci puntiformi.
  // I raggi di sole li aggiunge la passata finale (vedi shafts), a un quarto della risoluzione.
  apply(ctx, list) {
    if (!this.on) return;
    const W = G.W, H = G.H, z = G.zoom, cx = Math.round(G.cam.x * z), cy = Math.round(G.cam.y * z), n = this.night;
    const lights = this.lights(list);
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.imageSmoothingEnabled = true;
    const [r, g, b] = this.ambient;
    if (r + g + b < 750) { // mappa di luce a bassa risoluzione, moltiplicata sulla scena
      const s = 8, M = this.buf('map', Math.ceil(W / s), Math.ceil(H / s)), m = M.g;
      m.setTransform(1, 0, 0, 1, 0, 0);
      m.globalCompositeOperation = 'source-over'; m.globalAlpha = 1;
      m.fillStyle = `rgb(${r},${g},${b})`; m.fillRect(0, 0, M.c.width, M.c.height);
      m.globalCompositeOperation = 'lighter';
      for (const l of lights) {
        const lr = l.r * z / s;
        m.globalAlpha = clamp(l.power * (0.25 + 0.75 * n), 0, 1);
        m.drawImage(this.glow, (l.x * z - cx) / s - lr, (l.y * z - cy) / s - lr, lr * 2, lr * 2);
      }
      // ingrandita in due passi: sfumata fino a metà risoluzione, poi a blocchi di 2x2 pixel fisici. La mappa è
      // fatta solo di sfumature e la differenza non si vede; ingrandirla sfumata fino in fondo costava quasi il doppio.
      const half = this.buf('mapHalf', M.c.width * s / 2, M.c.height * s / 2);
      half.g.globalCompositeOperation = 'copy';
      half.g.drawImage(M.c, 0, 0, half.c.width, half.c.height);
      ctx.globalCompositeOperation = 'multiply';
      ctx.imageSmoothingEnabled = false;
      ctx.drawImage(half.c, 0, 0, half.c.width * 2, half.c.height * 2);
      ctx.imageSmoothingEnabled = true;
    }
    if (n > 0.02) { // alone caldo attorno alle luci
      ctx.globalCompositeOperation = 'lighter';
      for (const l of lights) {
        const lr = l.r * z * 0.62;
        ctx.globalAlpha = clamp(0.3 * n * l.power, 0, 1);
        ctx.drawImage(this.glow, l.x * z - cx - lr, l.y * z - cy - lr, lr * 2, lr * 2);
      }
    }
    ctx.restore();
    ctx.imageSmoothingEnabled = false;
  },

  // Raggi di sole (solo con le luci complete): sfumature larghe e lente, disegnate a un quarto della risoluzione
  // e aggiunte al bagliore C della passata finale, che le stende sullo schermo insieme a lui. Prima erano sei
  // disegni inclinati a piena risoluzione, alti quanto lo schermo.
  shafts(C, w, h) {
    const sun = (1 - this.night) * (0.45 + 0.55 * Math.abs(this.lx)); // più forti col sole basso
    if (sun <= 0.05) return;
    const W = G.W, H = G.H, S = this.buf('shafts', w, h), g = S.g, lean = -this.lx * 0.5, t = G.clock;
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.globalCompositeOperation = 'source-over';
    g.clearRect(0, 0, w, h);
    for (let i = 0; i < 6; i++) {
      const x = (((i * 0.19 + t * 0.006 + Math.sin(i * 12.7) * 0.08) % 1.2 + 1.2) % 1.2 - 0.1) * W, sw = (0.1 + 0.07 * Math.sin(i * 3.1)) * W;
      g.globalAlpha = 0.085 * sun * (0.6 + 0.4 * Math.sin(t * 0.35 + i * 2.1)) / POST_GLOW; // C si stende con alfa POST_GLOW
      g.setTransform(0.25, 0, lean * 0.25, 0.25, (x - lean * H * 0.5) * 0.25, 0);
      g.drawImage(this.shaft, -sw / 2, -H * 0.05, sw, H * 1.1);
    }
    // la nebbia li copre come quando erano disegnati sotto di lei: destination-out toglie l'opacità della nebbia
    const z = G.zoom / 4;
    g.globalAlpha = 1;
    g.globalCompositeOperation = 'destination-out';
    g.setTransform(z, 0, 0, z, -Math.round(G.cam.x * G.zoom) / 4, -Math.round(G.cam.y * G.zoom) / 4);
    g.drawImage(Fog.cv, 0, 0, World.W * TILE, World.H * TILE);
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.globalCompositeOperation = 'source-over';
    C.g.globalCompositeOperation = 'lighter';
    C.g.drawImage(S.c, 0, 0);
  },

  // Maschera delle strisce sfocate del diorama: opaca sul bordo dello schermo, trasparente verso il centro
  // (1 - t², t = distanza dal bordo in frazione della striscia), in alto per `top` righe e in basso per `bottom`.
  bandMask(g, h, top, bottom) {
    const key = h + ':' + top + ':' + bottom;
    if (this.maskKey === key) return this.mask;
    const m = g.createLinearGradient(0, 0, 0, h);
    for (let i = 0; i <= 8; i++) {
      const t = i / 8, a = `rgba(0,0,0,${(1 - t * t).toFixed(3)})`;
      m.addColorStop(t * top / h, a);
      m.addColorStop(1 - t * bottom / h, a);
    }
    this.maskKey = key; this.mask = m;
    return m;
  },

  // Ultima passata sul fotogramma del mondo, prima dell'interfaccia: sfocatura ai bordi, bagliore, raggi, vignettatura.
  // Le copie sono a un quarto della risoluzione: abbastanza grandi da restare sulla scheda grafica (le tele
  // piccole il browser le tiene in memoria, e copiarci dentro il fotogramma lo costringerebbe ad aspettarlo).
  // Tutto ciò che si può si compone lì, così sulla tela grande vanno solo pochi disegni.
  post(ctx) {
    if (this.level < 2) return;
    const W = G.W, H = G.H, w = Math.ceil(W / 4), h = Math.ceil(H / 4);
    const A = this.buf('quarter', w, h), C = this.buf('bright', w, h);
    A.g.globalCompositeOperation = 'copy'; A.g.drawImage(ctx.canvas, 0, 0, w, h);
    // solo le parti chiare: la scena moltiplicata per se stessa due volte
    C.g.globalCompositeOperation = 'copy'; C.g.drawImage(A.c, 0, 0);
    C.g.globalCompositeOperation = 'multiply'; C.g.drawImage(A.c, 0, 0); C.g.drawImage(A.c, 0, 0);
    this.shafts(C, w, h);
    ctx.imageSmoothingEnabled = true;
    // diorama: in alto e in basso la scena sfuma nella sua copia sfocata. La copia (con una seconda mano spostata
    // di mezzo pixel, che sfoca di più) e la maschera si preparano a un quarto della risoluzione, poi due soli
    // disegni la stendono sulle strisce. Prima erano 28 strisce sovrapposte a piena risoluzione.
    const top = Math.round(h * 0.22), bottom = Math.round(h * 0.18), B = this.buf('bands', w, h), b = B.g;
    b.globalAlpha = 1; b.globalCompositeOperation = 'copy'; b.drawImage(A.c, 0, 0);
    b.globalAlpha = 0.6; b.globalCompositeOperation = 'source-over'; b.drawImage(A.c, 0.5, 0.5);
    b.globalAlpha = 1; b.globalCompositeOperation = 'destination-in';
    b.fillStyle = this.bandMask(b, h, top, bottom); b.fillRect(0, 0, w, h);
    ctx.drawImage(B.c, 0, 0, w, top, 0, 0, w * 4, top * 4);
    ctx.drawImage(B.c, 0, h - bottom, w, bottom, 0, (h - bottom) * 4, w * 4, bottom * 4);
    // bagliore (e raggi), ingrandito in due passi come la mappa di luce: sfumato fino a metà, poi a blocchi di 2x2
    const half = this.buf('brightHalf', w * 2, h * 2);
    half.g.globalCompositeOperation = 'copy'; half.g.drawImage(C.c, 0, 0, w * 2, h * 2);
    ctx.globalCompositeOperation = 'lighter';
    ctx.globalAlpha = POST_GLOW;
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(half.c, 0, 0, w * 4, h * 4);
    ctx.globalCompositeOperation = 'source-over';
    // vignettatura già stesa alla misura dello schermo: ingrandirla a ogni fotogramma costava il triplo
    let V = this.bufs.vignette;
    if (!V || V.c.width !== W || V.c.height !== H) {
      V = this.buf('vignette', W, H);
      V.g.drawImage(this.vignette, -W * 0.12, -H * 0.12, W * 1.24, H * 1.24);
    }
    ctx.globalAlpha = 0.75 + 0.2 * this.night;
    ctx.drawImage(V.c, 0, 0);
    ctx.globalAlpha = 1;
  },
};
Light.build();
