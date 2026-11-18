'use strict';
// Carica il gioco in Node senza browser, per far giocare i bot a tutta velocità.
// Gli script di js/ girano così come sono dentro un contesto con un DOM finto: niente disegno, niente audio,
// solo la simulazione (updateWorld). Le misure delle immagini sono lette dai PNG veri, perché la durata delle
// animazioni d'attacco dipende dal numero di fotogrammi.
//
// overrides = { consts: { NOME: valore }, set: { 'UNIT_DEF.knight.hp': 170 } }
//   consts: costanti numeriche dichiarate con `NOME = numero` nei sorgenti (sostituite nel testo prima di caricare)
//   set:    proprietà di oggetti del gioco, assegnate dopo il caricamento

const vm = require('vm');
const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..', '..');

// Un oggetto che accetta qualunque chiamata, lettura e scrittura: fa da canvas, contesto 2D, gradiente...
function dummy() {
  const f = function () {};
  const p = new Proxy(f, {
    get(t, k) {
      if (k === Symbol.toPrimitive) return () => 0;
      if (k === 'then') return undefined;
      if (k in t.store) return t.store[k];
      return p;
    },
    set(t, k, v) { t.store[k] = v; return true; },
    apply() { return p; },
    construct() { return p; },
  });
  f.store = Object.create(null);
  return p;
}

function pngSize(file) {
  const fd = fs.openSync(file, 'r'), buf = Buffer.alloc(24);
  fs.readSync(fd, buf, 0, 24, 0);
  fs.closeSync(fd);
  return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) };
}
const sizes = new Map();

function load(overrides = {}) {
  class Image {
    constructor() { this.width = 0; this.height = 0; this.onload = null; this.onerror = null; this._src = ''; }
    get src() { return this._src; }
    set src(v) {
      this._src = v;
      const file = path.join(ROOT, decodeURIComponent(v));
      let s = sizes.get(file);
      if (!s) { s = pngSize(file); sizes.set(file, s); }
      this.width = this.naturalWidth = s.width; this.height = this.naturalHeight = s.height;
      this.complete = true;
      queueMicrotask(() => { if (this.onload) this.onload(); });
    }
  }
  const sandbox = {
    console, performance, queueMicrotask, setTimeout, clearTimeout, setInterval, clearInterval,
    Image,
    FontFace: class { load() { return Promise.resolve(this); } },
    requestAnimationFrame: () => 0,
    innerWidth: 1600, innerHeight: 900, devicePixelRatio: 1,
    addEventListener() {},
    location: { protocol: 'file:', host: '', port: '' },
    document: {
      getElementById: () => dummy(), createElement: () => dummy(), hasFocus: () => false, addEventListener() {},
      fonts: { add() {} },
    },
  };
  sandbox.window = sandbox;
  const ctx = vm.createContext(sandbox);

  const order = [...fs.readFileSync(path.join(ROOT, 'index.html'), 'utf8').matchAll(/<script src="([^"]+)"/g)].map(m => m[1]);
  let source = order.map(f => fs.readFileSync(path.join(ROOT, f), 'utf8')).join('\n;\n');
  for (const [name, value] of Object.entries(overrides.consts || {})) {
    const re = new RegExp(`\\b${name} = -?[\\d.]+`);
    if (!re.test(source)) throw new Error('Costante non trovata nei sorgenti: ' + name);
    source = source.replace(re, `${name} = ${value}`);
  }
  vm.runInContext(source, ctx, { filename: 'game.js' });
  // niente vista: telecamera, nebbia, effetti e suoni non servono alla simulazione
  vm.runInContext(`
    updateView = function () {};
    showEvent = function () {};
    onScreen = function () { return false; };
    Sfx.play = Sfx.at = Sfx.hear = Sfx.unlock = function () {};
  `, ctx);
  for (const [key, value] of Object.entries(overrides.set || {})) {
    vm.runInContext(`${key} = ${JSON.stringify(value)}`, ctx);
  }
  for (const f of ['sim.js', 'bots.js']) vm.runInContext(fs.readFileSync(path.join(__dirname, f), 'utf8'), ctx, { filename: f });
  // code: JavaScript eseguito nel contesto del gioco dopo tutto il resto, per provare una regola diversa
  // ridefinendo una funzione del gioco (per esempio `doHit = function (u) { ... }`) senza toccare i sorgenti
  if (overrides.code) vm.runInContext(overrides.code, ctx, { filename: 'overrides.code' });
  return {
    ctx,
    run: code => vm.runInContext(code, ctx),
    // attende che il caricamento finto degli sprite sia finito (serve prima della prima partita)
    ready: async () => { for (let i = 0; i < 50 && vm.runInContext('G.state', ctx) === 'loading'; i++) await new Promise(r => setImmediate(r)); },
  };
}

module.exports = { load, ROOT };
