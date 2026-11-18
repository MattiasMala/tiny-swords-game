'use strict';
// Caricamento sprite, definizioni di gioco (unità, edifici, ricerche) e primitive di disegno condivise.

const TILE = 64;
const COLORS = ['Blue', 'Red', 'Yellow', 'Purple', 'Black']; // stesso ordine di ribbon, spade e avatar
const COLOR_NAME = { Blue: 'Blu', Red: 'Rosso', Yellow: 'Giallo', Purple: 'Viola', Black: 'Nero' };
const COLOR_CSS = { Blue: '#46a5bb', Red: '#e6645c', Yellow: '#e3c84e', Purple: '#b981c6', Black: '#7887a0' };
const FONT = 'Fredoka, "Trebuchet MS", Verdana, "DejaVu Sans", sans-serif';
const fontOf = size => `600 ${Math.round(size)}px ${FONT}`;

const Assets = {
  images: new Map(),
  loaded: 0,
  total: ASSET_FILES.length,
  barFill: {}, // riempimento barra piccola tinto per colore di fazione

  load() {
    // il font è un abbellimento: se non si carica restano quelli di sistema
    const font = new FontFace('Fredoka', 'url(fonts/Fredoka.ttf)', { weight: '300 700' });
    const fontReady = font.load().then(f => document.fonts.add(f), () => {});
    const images = ASSET_FILES.map(f => new Promise((resolve, reject) => {
      const im = new Image();
      im.onload = () => { this.loaded++; resolve(); };
      im.onerror = () => reject(new Error('Impossibile caricare assets/' + f));
      im.src = 'assets/' + f.split('/').map(encodeURIComponent).join('/');
      this.images.set(f, im);
    }));
    return Promise.all([fontReady, ...images]).then(() => this.buildDerived());
  },

  buildDerived() {
    const src = img(UIP + 'Bars/SmallBar_Fill.png');
    for (const c of COLORS) {
      const cv = document.createElement('canvas');
      cv.width = src.width; cv.height = src.height;
      const g = cv.getContext('2d');
      g.drawImage(src, 0, 0);
      g.globalCompositeOperation = 'source-atop';
      g.fillStyle = COLOR_CSS[c];
      g.fillRect(0, 0, cv.width, cv.height);
      this.barFill[c] = cv;
    }
  },
};

const UIP = 'UI Elements/UI Elements/';
const STOREP = 'UI Elements/UI Banners from the store page/';
const TERR = 'Terrain/';

// Durante la passata delle ombre (vedi light.js) ogni sprite è sostituito dalla sua sagoma scura.
const SHADOW = {
  on: false, color: '#0e1230', cache: new Map(),
  // La sagoma non dipende dal colore del regno: se ne tiene una sola per forma.
  of(im, path) {
    const key = path.replace(/Blue|Red|Yellow|Purple|Black/g, '*');
    let c = this.cache.get(key);
    if (!c) {
      c = document.createElement('canvas');
      c.width = im.width; c.height = im.height;
      const g = c.getContext('2d');
      g.drawImage(im, 0, 0);
      g.globalCompositeOperation = 'source-in';
      g.fillStyle = this.color;
      g.fillRect(0, 0, c.width, c.height);
      this.cache.set(key, c);
    }
    return c;
  },
};

function image(path) {
  const im = Assets.images.get(path);
  if (!im) throw new Error('Sprite mancante: ' + path);
  return im;
}
function img(path) { return SHADOW.on ? SHADOW.of(image(path), path) : image(path); }

// ---------------------------------------------------------------- sprite sheet

const _sheets = new Map();
function sheet(path, fw, ax, ay, fps = 10) {
  let s = _sheets.get(path);
  if (!s) {
    const im = image(path);
    s = { im, path, fw, fh: im.height, n: Math.round(im.width / fw), ax, ay, fps };
    _sheets.set(path, s);
  }
  return s;
}

function drawSheet(ctx, s, frame, x, y, flip = false, scale = 1) {
  const f = ((frame % s.n) + s.n) % s.n, im = SHADOW.on ? SHADOW.of(s.im, s.path) : s.im;
  x = Math.round(x); y = Math.round(y);
  if (flip || scale !== 1) {
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(flip ? -scale : scale, scale);
    ctx.drawImage(im, f * s.fw, 0, s.fw, s.fh, -s.ax, -s.ay, s.fw, s.fh);
    ctx.restore();
  } else {
    ctx.drawImage(im, f * s.fw, 0, s.fw, s.fh, x - s.ax, y - s.ay, s.fw, s.fh);
  }
}

// ---------------------------------------------------------------- ere

const AGE_NAME = ['', 'Età del Villaggio', 'Età Feudale', 'Età dei Castelli'];
const AGE_SHORT = ['', 'I', 'II', 'III'];

// ---------------------------------------------------------------- unità

// from = edificio che la addestra; age = era minima; time = secondi di addestramento.
// bonus = moltiplicatore del danno contro un tipo; vsBuilding = contro gli edifici.
// Le unità senza `dir` usano gli sprite aggiuntivi di assets/Extra (misure in EXTRA.units).
const UNIT_DEF = {
  pawn: {
    label: 'Pedone', dir: 'Pawn', fw: 192, ax: 96, ay: 134, avatar: 4, barY: 78,
    hp: 50, speed: 92, sight: 5, cost: { meat: 50 }, time: 13, from: 'castle', age: 1,
    desc: 'Raccoglie risorse, costruisce e ripara.',
  },
  // non combatte: corre, vede lontano e occupa i punti liberi (vedi updateScout)
  scout: {
    label: 'Esploratore', barY: 78, scout: true,
    hp: 60, speed: 165, sight: 10, cost: { meat: 40 }, time: 12, from: 'castle', age: 1,
    desc: 'Veloce e con la vista lunga: esplora la mappa, anche da solo (tasto E), e occupa i punti liberi. Non combatte.',
  },
  warrior: {
    label: 'Guerriero', dir: 'Warrior', fw: 192, ax: 96, ay: 134, avatar: 0, barY: 92, foot: true,
    hp: 130, speed: 98, dmg: 13, range: 52, cd: 0.95, hitFrame: 2, block: 0.25, aggro: 300, sight: 6,
    cost: { meat: 58, gold: 40 }, time: 15, from: 'barracks', age: 1, bonus: { lancer: 1.5 }, vsBuilding: 1.6,
    desc: 'Fante con scudo, disponibile da subito. Forte contro Lancieri ed edifici, teme gli Arcieri.',
  },
  lancer: {
    label: 'Lanciere', dir: 'Lancer', fw: 320, ax: 160, ay: 196, avatar: 1, barY: 92, foot: true,
    hp: 110, speed: 118, dmg: 20, range: 88, cd: 1.25, hitFrame: 1, block: 0.2, aggro: 320, sight: 6,
    cost: { meat: 60, gold: 40 }, time: 17, from: 'barracks', age: 2, bonus: { knight: 1.5, archer: 1.1, monk: 1.5 },
    desc: 'Colpisce da lontano con la lancia. Forte contro i Cavalieri, teme i Guerrieri.',
  },
  archer: {
    label: 'Arciere', dir: 'Archer', fw: 192, ax: 96, ay: 134, avatar: 2, barY: 92, foot: true,
    hp: 70, speed: 102, dmg: 12, range: 330, cd: 1.5, hitFrame: 5, aggro: 390, sight: 7,
    cost: { wood: 38, gold: 32 }, time: 17, from: 'archery', age: 2, bonus: { warrior: 1.7 }, vsBuilding: 0.5,
    desc: 'Tira da lontano. Forte contro i Guerrieri, fragile in mischia: teme i Cavalieri.',
  },
  knight: {
    label: 'Cavaliere', barY: 106, hp: 190, speed: 150, dmg: 18, range: 58, cd: 1.1, hitFrame: 3, aggro: 340, sight: 7,
    cost: { meat: 80, gold: 60 }, time: 20, from: 'stable', age: 2, bonus: { archer: 1.6, catapult: 2, pawn: 1.3 },
    desc: 'Cavalleria veloce e resistente. Travolge Arcieri e Catapulte, teme i Lancieri.',
  },
  // faith = riserva di cura: ogni cura ne spende quanta vita rende; si ricarica di faithRegen al secondo
  monk: {
    label: 'Monaco', dir: 'Monk', fw: 192, ax: 96, ay: 134, avatar: 3, barY: 78,
    hp: 65, speed: 94, heal: 40, range: 170, cd: 1.8, hitFrame: 6, sight: 6, faith: 100, faithRegen: 0.4,
    cost: { gold: 80 }, time: 22, from: 'monastery', age: 3,
    desc: 'Cura le unità alleate ferite. Ha una riserva di 100 punti di cura che si ricarica piano. I nemici vicini lo preferiscono ad altri bersagli.',
  },
  ram: {
    label: 'Ariete', barY: 100, siege: true, onlyBuildings: true,
    hp: 340, speed: 62, dmg: 55, range: 60, cd: 2.2, hitFrame: 2, aggro: 330, sight: 5,
    cost: { wood: 160, gold: 60 }, time: 26, from: 'siege', age: 3, vsBuilding: 4,
    desc: 'Sfonda edifici e mura. Non può colpire le unità; le frecce gli fanno poco.',
  },
  catapult: {
    label: 'Catapulta', barY: 74, siege: true, splash: 72,
    hp: 110, speed: 58, dmg: 42, range: 430, cd: 4.2, hitFrame: 2, aggro: 450, sight: 8,
    cost: { wood: 140, gold: 120 }, time: 28, from: 'siege', age: 3, vsBuilding: 2.2,
    desc: 'Lancia massi a grande distanza: colpiscono tutti i nemici in una zona.',
  },
};
const UNIT_TYPES = Object.keys(UNIT_DEF);

const LANCE_DIRS = ['Up', 'UpRight', 'Right', 'DownRight', 'Down'];
const PAWN_TOOLS = ['Axe', 'Pickaxe', 'Knife', 'Hammer'];
const PAWN_LOADS = ['Wood', 'Gold', 'Meat', 'Stone'];

// anim -> [file, fps]
const UNIT_ANIM = {
  warrior: {
    idle: ['Warrior_Idle', 8], run: ['Warrior_Run', 10],
    attack1: ['Warrior_Attack1', 12], attack2: ['Warrior_Attack2', 12], block: ['Warrior_Guard', 10],
  },
  lancer: { idle: ['Lancer_Idle', 10], run: ['Lancer_Run', 10] },
  archer: { idle: ['Archer_Idle', 8], run: ['Archer_Run', 10], attack: ['Archer_Shoot', 14] },
  monk: { idle: ['Idle', 8], run: ['Run', 10], attack: ['Heal', 12] },
  pawn: { idle: ['Pawn_Idle', 8], run: ['Pawn_Run', 10] },
  scout: { idle: ['Scout_Idle', 8], run: ['Scout_Run', 10] },
  knight: { idle: ['Knight_Idle', 8], run: ['Knight_Run', 12], attack: ['Knight_Attack', 12] },
  // macchine d'assedio: da ferme mostrano il primo fotogramma della marcia (0 fotogrammi al secondo)
  ram: { idle: ['Ram_Run', 0], run: ['Ram_Run', 8], attack: ['Ram_Attack', 7] },
  catapult: { idle: ['Catapult_Run', 0], run: ['Catapult_Run', 8], attack: ['Catapult_Attack', 7] },
};
for (const d of LANCE_DIRS) {
  UNIT_ANIM.lancer['attack' + d] = [`Lancer_${d}_Attack`, 9];
  UNIT_ANIM.lancer['block' + d] = [`Lancer_${d}_Defence`, 10];
}
for (const t of [...PAWN_TOOLS, ...PAWN_LOADS]) {
  UNIT_ANIM.pawn['idle' + t] = [`Pawn_Idle ${t}`, 8];
  UNIT_ANIM.pawn['run' + t] = [`Pawn_Run ${t}`, 10];
}
for (const t of PAWN_TOOLS) UNIT_ANIM.pawn['work' + t] = [`Pawn_Interact ${t}`, 10];

function unitSheet(color, type, anim) {
  const d = UNIT_DEF[type], a = UNIT_ANIM[type][anim], x = EXTRA.units[a[0]];
  if (x) return sheet(`Extra/Units/${color}/${a[0]}.png`, x.fw, x.ax, x.ay, a[1]);
  return sheet(`Units/${color} Units/${d.dir}/${a[0]}.png`, d.fw, d.ax, d.ay, a[1]);
}

function avatarImg(color, type) {
  const n = COLORS.indexOf(color) * 5 + UNIT_DEF[type].avatar + 1;
  return img(`${UIP}Human Avatars/Avatars_${String(n).padStart(2, '0')}.png`);
}
// Il ritratto occupa il centro del frame 256x256.
function drawAvatar(ctx, color, type, x, y, size) {
  ctx.drawImage(avatarImg(color, type), 40, 40, 176, 176, Math.round(x), Math.round(y), Math.round(size), Math.round(size));
}

// ---------------------------------------------------------------- edifici

// w, h = ingombro in celle; ax, ay = ancora dello sprite; work = secondi di cantiere per un pedone;
// crop = parte visibile dello sprite (per le miniature); drop = risorse che vi si possono consegnare.
// extra = sprite aggiuntivo in assets/Extra (ancora e ritaglio vengono da EXTRA.buildings).
// pawns, troops = posti che aggiunge per Pedoni e per truppe; defense = opera di difesa, che si può posare
// solo vicino al castello o a un proprio avamposto (vedi zoneBlock).
const BUILD_DEF = {
  castle: {
    label: 'Castello', sprite: 'Castle', w: 5, h: 2, ax: 160, ay: 250, crop: [4, 41, 312, 208],
    hp: 2600, work: 0, sight: 10, pawns: 10, troops: 6, drop: ['wood', 'gold', 'meat', 'stone'], trains: ['pawn', 'scout'], techs: ['age2', 'age3', 'carts', 'masonry'],
    desc: 'Cuore del regno: se cade, hai perso. Addestra Pedoni ed Esploratori e fa avanzare d\'era.',
  },
  house: {
    label: 'Casa', sprites: ['House1', 'House2', 'House3'], w: 2, h: 1, ax: 64, ay: 178, crop: [8, 16, 112, 157],
    hp: 320, cost: { wood: 30 }, work: 9, sight: 4, pawns: 5, age: 1, desc: '+5 Pedoni al massimo. Con i nemici in vista i Pedoni ci si rifugiano.',
  },
  camp: {
    label: 'Accampamento', extra: 'Camp', w: 3, h: 2,
    hp: 420, cost: { wood: 60 }, work: 12, sight: 5, troops: 8, age: 1, desc: '+8 truppe al massimo.',
  },
  depot: {
    label: 'Deposito', extra: 'Depot', w: 2, h: 1,
    hp: 380, cost: { wood: 60 }, work: 12, sight: 5, age: 1, drop: ['wood', 'gold', 'stone'], techs: ['axes', 'picks'],
    desc: 'Punto di consegna per legna, oro e pietra. Costruiscilo vicino a boschi e cave.',
  },
  farm: {
    label: 'Fattoria', extra: 'Farm', w: 3, h: 2,
    hp: 420, cost: { wood: 150 }, work: 14, sight: 5, age: 1, drop: ['meat'], techs: ['breeding'],
    desc: 'Alleva pecore nei dintorni, abbastanza per due o tre pedoni, e fa da punto di consegna per il cibo.',
  },
  barracks: {
    label: 'Caserma', sprite: 'Barracks', w: 3, h: 2, ax: 96, ay: 246, crop: [4, 58, 184, 187],
    hp: 700, cost: { wood: 90 }, work: 18, sight: 5, age: 1, troops: 2, trains: ['warrior', 'lancer'], techs: ['boots'],
    desc: 'Addestra Guerrieri e, dall\'Età Feudale, Lancieri. +2 truppe al massimo.',
  },
  wall: {
    label: 'Palizzata', extra: 'Wall', neutral: true, w: 1, h: 1,
    hp: 300, cost: { wood: 12 }, work: 3, sight: 2, age: 1, defense: true,
    desc: 'Sbarra il passaggio ai nemici; le tue unità ci passano. Solo vicino al castello o a un tuo avamposto. Maiusc + trascina per posarne una fila.',
  },
  archery: {
    label: 'Arcieria', sprite: 'Archery', w: 3, h: 2, ax: 96, ay: 242, crop: [3, 61, 183, 179],
    hp: 600, cost: { wood: 80, gold: 30 }, work: 18, sight: 5, age: 2, troops: 2, trains: ['archer'],
    desc: 'Addestra Arcieri. +2 truppe al massimo.',
  },
  stable: {
    label: 'Scuderia', extra: 'Stable', w: 3, h: 2,
    hp: 650, cost: { wood: 110, gold: 40 }, work: 18, sight: 5, age: 2, troops: 2, trains: ['knight'], techs: ['horseshoes'],
    desc: 'Addestra Cavalieri. +2 truppe al massimo.',
  },
  smith: {
    label: 'Fucina', extra: 'Smith', w: 3, h: 2,
    hp: 650, cost: { wood: 100, stone: 40 }, work: 18, sight: 5, age: 2, techs: ['blades', 'shields', 'fletching', 'armor'],
    desc: 'Migliora armi e armature dell\'esercito.',
  },
  market: {
    label: 'Mercato', extra: 'Market', w: 3, h: 2,
    hp: 600, cost: { wood: 120 }, work: 18, sight: 5, age: 2, techs: ['maps'], trade: true,
    desc: 'Scambia legna, cibo e pietra con oro.',
  },
  tower: {
    label: 'Torre', sprite: 'Tower', w: 2, h: 1, ax: 64, ay: 232, crop: [4, 46, 120, 184],
    hp: 620, cost: { stone: 80, wood: 40 }, work: 16, sight: 9, age: 2, defense: true,
    desc: 'Scaglia frecce sui nemici vicini e vede lontano. Solo vicino al castello o a un tuo avamposto.',
  },
  monastery: {
    label: 'Monastero', sprite: 'Monastery', w: 3, h: 2, ax: 96, ay: 312, crop: [16, 45, 160, 265],
    hp: 600, cost: { wood: 80, gold: 100, stone: 40 }, work: 22, sight: 5, age: 3, troops: 2, trains: ['monk'], techs: ['herbs', 'scriptures'],
    desc: 'Addestra Monaci guaritori. +2 truppe al massimo.',
  },
  siege: {
    label: 'Officina d\'assedio', extra: 'Siege', w: 3, h: 2,
    hp: 650, cost: { wood: 160, gold: 60 }, work: 22, sight: 5, age: 3, troops: 2, trains: ['ram', 'catapult'], techs: ['engineering'],
    desc: 'Costruisce Arieti e Catapulte. +2 truppe al massimo.',
  },
};
for (const d of Object.values(BUILD_DEF)) {
  if (d.extra) { const x = EXTRA.buildings[d.extra]; d.ax = x.ax; d.ay = x.ay; d.crop = x.crop; }
}
const BUILD_KINDS = Object.keys(BUILD_DEF);
const BUILD_MENU = ['house', 'camp', 'depot', 'farm', 'barracks', 'wall', 'archery', 'stable', 'smith', 'market', 'tower', 'monastery', 'siege'];

// variant = quale delle tre case (gli altri edifici hanno un solo aspetto).
function buildingImg(color, kind, variant = 0) {
  const d = BUILD_DEF[kind];
  if (d.extra) return img(d.neutral ? `Extra/Buildings/${d.extra}.png` : `Extra/Buildings/${color}/${d.extra}.png`);
  return img(`Buildings/${color} Buildings/${d.sprites ? d.sprites[variant] : d.sprite}.png`);
}

// ---------------------------------------------------------------- punti da conquistare

// Strutture neutrali sparse sulla mappa (2x2 celle): chi ci tiene accanto delle truppe senza nemici attorno le
// conquista (vedi updatePoints). res = risorsa che rendono a chi le tiene; zone = attorno ci si può costruire.
const POINT_DEF = {
  outpost: { label: 'Avamposto', sprite: 'Outpost', zone: true, desc: 'Chi lo tiene può costruire nei dintorni, torri e palizzate comprese.' },
  mine: { label: 'Miniera', sprite: 'Mine', res: 'gold', desc: 'Rende oro a chi la tiene, senza Pedoni.' },
  quarry: { label: 'Cava', sprite: 'Quarry', res: 'stone', desc: 'Rende pietra a chi la tiene, senza Pedoni.' },
  sawmill: { label: 'Segheria', sprite: 'Sawmill', res: 'wood', desc: 'Rende legna a chi la tiene, senza Pedoni.' },
  ranch: { label: 'Ovile', sprite: 'Ranch', res: 'meat', desc: 'Rende cibo a chi lo tiene, senza Pedoni.' },
};
for (const d of Object.values(POINT_DEF)) { const x = EXTRA.buildings[d.sprite]; d.ax = x.ax; d.ay = x.ay; d.crop = x.crop; }
function pointImg(kind) { return img(`Extra/Buildings/${POINT_DEF[kind].sprite}.png`); }
function bigNodeImg(kind) { return img(`Extra/Resources/${kind === 'gold' ? 'Gold' : 'Stone'}_Big.png`); }

// ---------------------------------------------------------------- ricerche

// at = edificio dove si ricerca; age = era minima (il livello dell'albero); req = ricerca che la precede
// nell'albero; icon = vedi drawSpec.
const TECH_DEF = {
  age2: {
    label: 'Età Feudale', at: 'castle', age: 1, cost: { meat: 250, gold: 100 }, time: 40, icon: ['age', 2],
    desc: 'Sblocca Arcieria, Scuderia, Fucina, Mercato, Torre, Lancieri e nuove ricerche.',
  },
  age3: {
    label: 'Età dei Castelli', at: 'castle', age: 2, req: 'age2', cost: { meat: 400, gold: 300, stone: 100 }, time: 55, icon: ['age', 3],
    desc: 'Sblocca Monastero, Officina d\'assedio e le ricerche avanzate.',
  },
  carts: {
    label: 'Carriole', at: 'castle', age: 2, cost: { wood: 120, meat: 80 }, time: 28, icon: ['x', 'barrow'],
    desc: 'I Pedoni portano 5 risorse in più e camminano più svelti.',
  },
  masonry: {
    label: 'Muratura', at: 'castle', age: 3, cost: { stone: 150, wood: 100 }, time: 36, icon: ['x', 'tower'],
    desc: '+30% di resistenza a tutti gli edifici.',
  },
  axes: {
    label: 'Asce affilate', at: 'depot', age: 1, cost: { meat: 80, wood: 60 }, time: 24, icon: ['x', 'axe'],
    desc: 'I Pedoni tagliano la legna il 30% più in fretta.',
  },
  picks: {
    label: 'Picconi d\'acciaio', at: 'depot', age: 2, req: 'axes', cost: { meat: 80, wood: 60 }, time: 24, icon: ['x', 'pickaxe'],
    desc: 'I Pedoni estraggono oro e pietra il 30% più in fretta.',
  },
  breeding: {
    label: 'Allevamento', at: 'farm', age: 1, cost: { wood: 80, gold: 40 }, time: 24, icon: ['x', 'wheat'],
    desc: 'Le Fattorie allevano il 50% più in fretta.',
  },
  boots: {
    label: 'Marcia forzata', at: 'barracks', age: 2, cost: { meat: 100, gold: 40 }, time: 26, icon: ['x', 'boot'],
    desc: 'Guerrieri, Lancieri e Arcieri si muovono il 12% più veloci.',
  },
  horseshoes: {
    label: 'Ferratura', at: 'stable', age: 2, cost: { meat: 100, gold: 80 }, time: 28, icon: ['x', 'horseshoe'],
    desc: 'I Cavalieri sono il 15% più veloci.',
  },
  blades: {
    label: 'Lame temprate', at: 'smith', age: 2, cost: { meat: 100, gold: 80 }, time: 30, icon: ['x', 'sword'],
    desc: '+4 attacco a Guerrieri, Lancieri e Cavalieri.',
  },
  shields: {
    label: 'Scudi rinforzati', at: 'smith', age: 2, cost: { wood: 120, gold: 60 }, time: 30, icon: ['x', 'shield'],
    desc: 'Guerrieri, Lancieri e Cavalieri subiscono metà del danno dalle frecce.',
  },
  fletching: {
    label: 'Impennaggio', at: 'smith', age: 2, cost: { wood: 100, gold: 60 }, time: 30, icon: ['x', 'arrows'],
    desc: '+3 attacco e più gittata ad Arcieri, Torri e Castello.',
  },
  armor: {
    label: 'Corazze', at: 'smith', age: 3, req: 'shields', cost: { meat: 120, gold: 120 }, time: 36, icon: ['x', 'breastplate'],
    desc: 'Tutti i soldati subiscono 3 danni in meno per colpo.',
  },
  maps: {
    label: 'Cartografia', at: 'market', age: 2, cost: { wood: 80, gold: 60 }, time: 24, icon: ['x', 'spyglass'],
    desc: 'Unità ed edifici vedono 2 caselle più lontano.',
  },
  herbs: {
    label: 'Erbe curative', at: 'monastery', age: 3, cost: { gold: 120 }, time: 30, icon: ['x', 'herbs'],
    desc: 'I Monaci curano il 50% in più e ricaricano la riserva il 50% più in fretta.',
  },
  scriptures: {
    label: 'Sacre scritture', at: 'monastery', age: 3, req: 'herbs', cost: { gold: 100, wood: 60 }, time: 30, icon: ['x', 'book'],
    desc: 'I Monaci curano da più lontano e resistono il doppio.',
  },
  engineering: {
    label: 'Ingegneria', at: 'siege', age: 3, cost: { wood: 150, gold: 120 }, time: 36, icon: ['x', 'anvil'],
    desc: '+25% di danno ad Arieti e Catapulte.',
  },
};
const TECH_IDS = Object.keys(TECH_DEF);

// ---------------------------------------------------------------- effetti e risorse

const FX = {
  dust1: () => sheet('Particle FX/Dust_01.png', 64, 32, 46, 14),
  dust2: () => sheet('Particle FX/Dust_02.png', 64, 32, 56, 14),
  boom1: () => sheet('Particle FX/Explosion_01.png', 192, 96, 110, 14),
  boom2: () => sheet('Particle FX/Explosion_02.png', 192, 96, 130, 14),
  fire1: () => sheet('Particle FX/Fire_01.png', 64, 32, 62, 10),
  fire2: () => sheet('Particle FX/Fire_02.png', 64, 32, 62, 10),
  fire3: () => sheet('Particle FX/Fire_03.png', 64, 32, 62, 10),
  splash: () => sheet('Particle FX/Water Splash.png', 192, 96, 100, 12),
  heal: color => sheet(`Units/${color} Units/Monk/Heal_Effect.png`, 192, 96, 134, 12),
};

const RES = TERR + 'Resources/';
const XICON = name => `Extra/Icons/${name}.png`;   // icone aggiuntive
const PROP = name => `Extra/Props/${name}.png`;    // oggetti di scena aggiuntivi
const RES_KINDS = ['wood', 'gold', 'meat', 'stone'];
const RES_ICON = { wood: UIP + 'Icons/Icon_02.png', gold: UIP + 'Icons/Icon_03.png', meat: UIP + 'Icons/Icon_04.png', stone: XICON('stone') };
const RES_LABEL = { wood: 'Legna', gold: 'Oro', meat: 'Cibo', stone: 'Pietra' };
function stoneImg(stage) { return img(`Extra/Resources/Stone_${stage}.png`); }
const ICON = n => `${UIP}Icons/Icon_${String(n).padStart(2, '0')}.png`;
const TOOL = n => `${RES}Tools/Tool_0${n}.png`; // 1 martello, 2 ascia, 3 coltello, 4 piccone
const TREE_DEF = [ // per variante: ancora del ceppo e della chioma
  { ay: 240, stumpAy: 238 }, { ay: 248, stumpAy: 244 }, { ay: 170, stumpAy: 231 }, { ay: 168, stumpAy: 227 },
];
function treeSheet(v) { return sheet(`${RES}Wood/Trees/Tree${v + 1}.png`, 192, 96, TREE_DEF[v].ay, 8); }
function stumpImg(v) { return img(`${RES}Wood/Trees/Stump ${v + 1}.png`); }
function goldImg(stage) { return img(`${RES}Gold/Gold Stones/Gold Stone ${stage}.png`); }
function goldShine(stage) { return sheet(`${RES}Gold/Gold Stones/Gold Stone ${stage}_Highlight.png`, 128, 64, 64, 10); }

// ---------------------------------------------------------------- 9-slice / 3-slice

function nineSlice(ctx, im, xs, ys, x, y, w, h, s = 1) {
  let lw = xs[0][1] * s, rw = xs[2][1] * s, th = ys[0][1] * s, bh = ys[2][1] * s;
  if (lw + rw > w) { const k = w / (lw + rw); lw *= k; rw *= k; }
  if (th + bh > h) { const k = h / (th + bh); th *= k; bh *= k; }
  const X = [Math.round(x), Math.round(x + lw), Math.round(x + w - rw), Math.round(x + w)];
  const Y = [Math.round(y), Math.round(y + th), Math.round(y + h - bh), Math.round(y + h)];
  for (let j = 0; j < 3; j++) {
    for (let i = 0; i < 3; i++) {
      const dw = X[i + 1] - X[i], dh = Y[j + 1] - Y[j];
      if (dw > 0 && dh > 0) ctx.drawImage(im, xs[i][0], ys[j][0], xs[i][1], ys[j][1], X[i], Y[j], dw, dh);
    }
  }
}

const G3 = [[0, 64], [128, 64], [256, 64]];    // fogli 320px con celle separate
const G7 = [[0, 128], [192, 64], [320, 128]];  // fogli 448px con celle separate
const G1 = [[0, 64], [64, 64], [128, 64]];     // fogli 192px contigui
// pad = margine trasparente [sinistra, alto, destra, basso] attorno al contenuto visibile
const PANEL = {
  banner: { path: UIP + 'Banners/Banner.png', xs: G7, ys: G7, pad: [28, 60, 44, 17] },
  bannerSlots: { path: UIP + 'Banners/Banner_Slots.png', xs: G1, ys: G1, pad: [5, 5, 2, 4] },
  wood: { path: UIP + 'Wood Table/WoodTable.png', xs: G7, ys: G7, pad: [44, 43, 44, 25] },
  woodSlots: { path: UIP + 'Wood Table/WoodTable_Slots.png', xs: G1, ys: G1, pad: [12, 11, 12, 11] },
  paper: { path: UIP + 'Papers/RegularPaper.png', xs: G3, ys: G3, pad: [12, 20, 12, 19] },
  special: { path: UIP + 'Papers/SpecialPaper.png', xs: G3, ys: G3, pad: [9, 20, 9, 21] },
  storeSlots: { path: STOREP + 'Banner/Slots.png', xs: G3, ys: G3, pad: [43, 8, 40, 17] },
  bigBlue: { path: UIP + 'Buttons/BigBlueButton_Regular.png', xs: G3, ys: G3, pad: [19, 17, 19, 17] },
  bigBlueDown: { path: UIP + 'Buttons/BigBlueButton_Pressed.png', xs: G3, ys: G3, pad: [19, 17, 19, 17] },
  bigRed: { path: UIP + 'Buttons/BigRedButton_Regular.png', xs: G3, ys: G3, pad: [19, 17, 19, 17] },
  bigRedDown: { path: UIP + 'Buttons/BigRedButton_Pressed.png', xs: G3, ys: G3, pad: [19, 17, 19, 17] },
};

// (x, y, w, h) è il rettangolo visibile del pannello.
function drawPanel(ctx, def, x, y, w, h, s = 1) {
  const [l, t, r, b] = def.pad;
  nineSlice(ctx, img(def.path), def.xs, def.ys, x - l * s, y - t * s, w + (l + r) * s, h + (t + b) * s, s);
}

// Pergamena grande della pagina dello store: angoli inferiori arricciati più larghi di quelli superiori.
function drawStoreBanner(ctx, x, y, w, h, s = 1) {
  const im = img(STOREP + 'Banner/Banner.png');
  x = Math.round(x); y = Math.round(y); w = Math.round(w); h = Math.round(h);
  const c = Math.round(64 * s), cr = Math.round(44 * s), bh = Math.round(98 * s);
  const bl = Math.round(192 * s), br = Math.round(172 * s), mh = h - c - bh;
  ctx.drawImage(im, 0, 0, 64, 64, x, y, c, c);
  ctx.drawImage(im, 256, 0, 64, 64, x + c, y, w - c - cr, c);
  ctx.drawImage(im, 640, 0, 44, 64, x + w - cr, y, cr, c);
  ctx.drawImage(im, 0, 128, 64, 64, x, y + c, c, mh);
  ctx.drawImage(im, 320, 128, 64, 64, x + c, y + c, w - c - cr, mh);
  ctx.drawImage(im, 640, 128, 44, 64, x + w - cr, y + c, cr, mh);
  ctx.drawImage(im, 0, 256, 192, 98, x, y + h - bh, bl, bh);
  ctx.drawImage(im, 256, 256, 64, 98, x + bl, y + h - bh, w - bl - br, bh);
  ctx.drawImage(im, 512, 256, 172, 98, x + w - br, y + h - bh, br, bh);
}

// Striscia orizzontale a 3 pezzi: (x, y) angolo alto-sinistra della riga sorgente scalata, w larghezza totale.
function threeSlice(ctx, im, xs, sy, sh, x, y, w, s = 1) {
  const lw = Math.round(xs[0][1] * s), rw = Math.round(xs[2][1] * s), h = Math.round(sh * s);
  x = Math.round(x); y = Math.round(y); w = Math.round(w);
  ctx.drawImage(im, xs[0][0], sy, xs[0][1], sh, x, y, lw, h);
  if (w - lw - rw > 0) ctx.drawImage(im, xs[1][0], sy, xs[1][1], sh, x + lw, y, w - lw - rw, h);
  ctx.drawImage(im, xs[2][0], sy, xs[2][1], sh, x + w - rw, y, rw, h);
}

function label(ctx, str, x, y, size, color = '#fff', align = 'left', outline = '#1b1f33') {
  ctx.font = fontOf(size);
  ctx.textAlign = align;
  ctx.textBaseline = 'middle';
  if (outline) {
    ctx.lineJoin = 'round';
    ctx.lineWidth = Math.max(2, size / 5);
    ctx.strokeStyle = outline;
    ctx.strokeText(str, x, y);
  }
  ctx.fillStyle = color;
  ctx.fillText(str, x, y);
}
function textWidth(ctx, str, size) {
  ctx.font = fontOf(size);
  return ctx.measureText(str).width;
}

// Nastro grande (5 colori). cx, cy = centro del nastro.
function bigRibbon(ctx, colorIdx, cx, cy, w, s, str, size) {
  threeSlice(ctx, img(UIP + 'Ribbons/BigRibbons.png'), G7, colorIdx * 128, 128, cx - w / 2, cy - 64 * s, w, s);
  if (str) label(ctx, str, cx, cy - 2 * s, size, '#fff', 'center');
}
// Nastro piccolo: 2 varianti per colore.
function smallRibbon(ctx, colorIdx, variant, cx, cy, w, s, str, size) {
  threeSlice(ctx, img(UIP + 'Ribbons/SmallRibbons.png'), G3, (colorIdx * 2 + variant) * 64, 64, cx - w / 2, cy - 33 * s, w, s);
  if (str) label(ctx, str, cx, cy - 1, size, '#fff', 'center');
}
function swordBanner(ctx, colorIdx, cx, cy, w, s, str, size) {
  threeSlice(ctx, img(UIP + 'Swords/Swords.png'), G7, colorIdx * 128, 128, cx - w / 2, cy - 64 * s, w, s);
  if (str) label(ctx, str, cx + 16 * s, cy - 14 * s, size, '#4a3a33', 'center', null);
}
// Nastro della pagina store (riga superiore del foglio per colore).
function storeRibbon(ctx, color, cx, cy, w, s, str, size) {
  threeSlice(ctx, img(`${STOREP}Ribbons/Ribbon_${color}.png`), G7, 0, 128, cx - w / 2, cy - 68 * s, w, s);
  if (str) label(ctx, str, cx, cy, size, '#fff', 'center');
}
function drawPennant(ctx, color, cx, y, s) {
  ctx.drawImage(img(`${STOREP}Ribbons/Ribbon_${color}.png`), 51, 448, 90, 108,
    Math.round(cx - 45 * s), Math.round(y), Math.round(90 * s), Math.round(108 * s));
}

// Barra grande: (x, y, w) = rettangolo visibile, alto 51*s.
function bigBar(ctx, x, y, w, ratio, s) {
  threeSlice(ctx, img(UIP + 'Bars/BigBar_Base.png'), G3, 0, 64, x - 40 * s, y - 9 * s, w + 80 * s, s);
  const fw = (w - 22 * s) * Math.max(0, Math.min(1, ratio));
  if (fw > 0.5) {
    ctx.drawImage(img(UIP + 'Bars/BigBar_Fill.png'), 0, 22, 64, 21,
      Math.round(x + 11 * s), Math.round(y + 13 * s), Math.round(fw), Math.round(21 * s));
  }
}
// Barra piccola: alta 19*s. fill = immagine/canvas di riempimento.
function smallBar(ctx, x, y, w, ratio, s, fill) {
  threeSlice(ctx, img(UIP + 'Bars/SmallBar_Base.png'), G3, 0, 64, x - 49 * s, y - 22 * s, w + 98 * s, s);
  const fw = (w - 20 * s) * Math.max(0, Math.min(1, ratio));
  if (fw > 0.5) ctx.drawImage(fill, 0, 30, 64, 3, x + 10 * s, y + 8 * s, fw, 6 * s);
}

// Angolari di selezione (Cursor_04) attorno a un rettangolo.
function drawBrackets(ctx, x, y, w, h, s = 1) {
  const im = img(UIP + 'Cursors/Cursor_04.png'), c = 32, d = Math.round(c * s);
  x = Math.round(x); y = Math.round(y); w = Math.round(w); h = Math.round(h);
  ctx.drawImage(im, 0, 0, c, c, x, y, d, d);
  ctx.drawImage(im, 96, 0, c, c, x + w - d, y, d, d);
  ctx.drawImage(im, 0, 96, c, c, x, y + h - d, d, d);
  ctx.drawImage(im, 96, 96, c, c, x + w - d, y + h - d, d, d);
}

function drawIcon(ctx, path, cx, cy, size) {
  const h = Math.round(size / 2);
  ctx.drawImage(img(path), Math.round(cx) - h, Math.round(cy) - h, h * 2, h * 2);
}

// Miniatura di un edificio, contenuta in un quadrato di lato size.
function drawBuildingThumb(ctx, color, kind, cx, cy, size) {
  const [sx, sy, sw, sh] = BUILD_DEF[kind].crop, k = Math.min(size / sw, size / sh);
  ctx.drawImage(buildingImg(color, kind), sx, sy, sw, sh,
    Math.round(cx - sw * k / 2), Math.round(cy - sh * k / 2), Math.round(sw * k), Math.round(sh * k));
}

// Ritratto di un'unità in un quadrato di lato size: l'avatar del pacchetto, o il primo fotogramma
// dell'animazione per le unità che non ne hanno uno.
function drawPortrait(ctx, color, type, x, y, size) {
  if (UNIT_DEF[type].avatar !== undefined) { drawAvatar(ctx, color, type, x, y, size); return; }
  const s = unitSheet(color, type, 'idle'), side = Math.min(s.fw, s.fh) * 0.86;
  ctx.drawImage(s.im, (s.fw - side) / 2, s.ay - side * 0.9, side, side, Math.round(x), Math.round(y), Math.round(size), Math.round(size));
}

// Icona descritta da una "specifica": ['ui', n] icona del pacchetto · ['x', nome] icona aggiuntiva ·
// ['res', tipo] risorsa · ['unit', tipo] ritratto · ['build', edificio] miniatura · ['age', n].
function drawSpec(ctx, spec, color, cx, cy, size) {
  const [kind, ref] = spec;
  if (kind === 'ui') drawIcon(ctx, ICON(ref), cx, cy, size * 1.15);
  else if (kind === 'x') drawIcon(ctx, XICON(ref), cx, cy, size * 1.2);
  else if (kind === 'res') drawIcon(ctx, RES_ICON[ref], cx, cy, size * 1.15);
  else if (kind === 'unit') drawPortrait(ctx, color, ref, cx - size * 0.6, cy - size * 0.62, size * 1.2);
  else if (kind === 'build') drawBuildingThumb(ctx, color, ref, cx, cy, size);
  else if (kind === 'age') {
    drawIcon(ctx, XICON('crown'), cx, cy - size * 0.1, size * 1.1);
    label(ctx, AGE_SHORT[ref], cx, cy + size * 0.3, size * 0.46, '#ffe9a3', 'center');
  }
}

// ---------------------------------------------------------------- util

const rand = (a, b) => a + Math.random() * (b - a);
const randInt = (a, b) => Math.floor(rand(a, b + 1));
const pick = arr => arr[Math.floor(Math.random() * arr.length)];
const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);
function mulberry32(a) {
  return function () {
    a |= 0; a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
