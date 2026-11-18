// Gira dentro il contesto del gioco (vedi headless.js): avvia partite fra bot e IA e ne tiene le statistiche.

// Valore di un'unità: la somma di ciò che costa.
function unitValue(type) { return Object.values(UNIT_DEF[type].cost).reduce((a, b) => a + b, 0); }

const SIM = {
  // ---- statistiche: cosa ogni regno ha addestrato, perso e ucciso, e quando è avanzato d'era o è caduto
  hook() {
    const deploy0 = deploy, kill0 = killUnit, tech0 = applyTech, out0 = eliminateFaction;
    deploy = function (b, type) {
      const s = b.faction.stat;
      if (s) s.trained[type] = (s.trained[type] || 0) + 1;
      return deploy0(b, type);
    };
    killUnit = function (u, by) {
      const src = by && by.owner ? by.owner : by, s = u.faction.stat;
      if (s && !u.dead) {
        s.lost[u.type] = (s.lost[u.type] || 0) + 1;
        const k = src && src.faction && src.faction !== u.faction ? src.faction.stat : null;
        if (k) { const w = src.type || src.kind; k.killsWith[w] = (k.killsWith[w] || 0) + unitValue(u.type); }
      }
      return kill0(u, by);
    };
    applyTech = function (f, id) {
      if (f.stat && (id === 'age2' || id === 'age3')) f.stat[id] = Math.round(G.time);
      return tech0(f, id);
    };
    eliminateFaction = function (f, by) {
      if (f.alive && f.stat) { f.stat.deathT = Math.round(G.time); f.stat.killer = by ? by.idx : -1; }
      return out0(f, by);
    };
  },

  // cfg = { seed, players: [{ bot: 'ai' | nome di una strategia, level, opts }] }
  start(cfg) {
    Math.random = mulberry32(cfg.seed >>> 0);
    G.setup = { players: [], level: 'normal' };
    G.demo = true; G.state = 'menu';
    G.reset();
    World.generate((Math.random() * 1e9) | 0);
    G.scenery();
    for (const m of World.meadows) for (let i = 0; i < 3; i++) makeSheep(m);
    for (const t of World.treasures) dropPickup(t.kind, World.cx(t.c), World.cy(t.c) + 6, TREASURE, Infinity, true);
    const order = COLORS.slice().sort(() => Math.random() - 0.5);
    G.factions = cfg.players.map((p, i) => {
      const f = makeFaction(order[i], World.bases[i], p.bot !== 'ai', p.level || 'normal');
      f.idx = i;
      f.stat = { trained: {}, lost: {}, killsWith: {}, age2: 0, age3: 0, deathT: 0, killer: -1, peak: 0 };
      return f;
    });
    for (const f of G.factions) setupFaction(f);
    cfg.players.forEach((p, i) => { G.factions[i].bot = p.bot === 'ai' ? null : makeBot(p.bot, G.factions[i], p.opts || {}); });
    Ctl.reset();
    Fog.reset(false);
  },

  // Gioca una partita fino a un solo regno vivo o a `limit` secondi di gioco.
  run(cfg) {
    this.start(cfg);
    const dt = cfg.dt || 0.05, limit = cfg.limit || 2400;
    let botT = 0, alive = G.factions.length;
    while (G.time < limit && alive > 1) {
      updateWorld(dt);
      if ((botT -= dt) <= 0) {
        botT = 1;
        for (const f of G.factions) {
          if (!f.alive) continue;
          if (f.bot) f.bot.think(f);
          f.stat.peak = Math.max(f.stat.peak, f.soldiers.length);
        }
      }
      alive = 0;
      for (const f of G.factions) if (f.alive) alive++;
    }
    const left = G.factions.filter(f => f.alive);
    return {
      seed: cfg.seed, time: Math.round(G.time), winner: left.length === 1 ? left[0].idx : -1,
      players: G.factions.map((f, i) => ({
        bot: cfg.players[i].bot, level: cfg.players[i].level || 'normal', alive: f.alive, age: f.age,
        pawns: f.pawns.length, soldiers: f.soldiers.length, buildings: G.buildings.filter(b => b.faction === f).length,
        gathered: Math.round(f.gathered), kills: f.kills, razed: f.razed, ...f.stat,
        lostValue: Object.entries(f.stat.lost).reduce((s, [t, n]) => s + unitValue(t) * n, 0), // con i costi in vigore
      })),
    };
  },

  // Scontro in campo aperto, senza economia: due eserciti `a` e `b` ({ tipo: quanti }) si vengono incontro.
  // techs = ricerche già fatte da ciascuno. Ritorna quanta parte del valore iniziale è rimasta a ognuno.
  arena(cfg) {
    Math.random = mulberry32(cfg.seed >>> 0);
    G.setup = { players: [], level: 'normal' };
    G.demo = true; G.state = 'menu';
    G.reset();
    if (!SIM.field || cfg.newMap) { World.generate(12345); SIM.field = true; } else World.resetBlock();
    G.trees = []; G.golds = []; G.stones = []; G.decor = []; G.points = []; World.block.fill(0); World.version++;
    const base = World.bases[0], cx = base.x * TILE, cy = base.y * TILE;
    const sides = [cfg.a, cfg.b].map((army, i) => {
      const f = makeFaction(COLORS[i], World.bases[i], true, 'normal');
      f.idx = i; f.stat = { trained: {}, lost: {}, killsWith: {}, age2: 0, age3: 0, deathT: 0, killer: -1, peak: 0 };
      for (const id of (i ? cfg.techsB : cfg.techsA) || []) { f.tech[id] = true; techMods(f.mods, id); }
      return f;
    });
    G.factions = sides;
    const value = army => Object.entries(army).reduce((s, [t, n]) => s + unitValue(t) * n, 0);
    const worth = f => G.units.reduce((s, u) => s + (u.faction === f && !u.dead ? unitValue(u.type) * u.hp / u.maxHp : 0), 0);
    [cfg.a, cfg.b].forEach((army, i) => {
      const f = sides[i], dir = i ? 1 : -1;
      let k = 0;
      for (const [type, n] of Object.entries(army)) {
        const back = UNIT_DEF[type].heal ? 130 : UNIT_DEF[type].range > 200 ? 90 : 0; // tiratori e monaci dietro
        for (let j = 0; j < n; j++, k++) {
          const u = spawnUnit(f, type, cx + dir * (330 + back + (k % 3) * 34), cy + ((k % 7) - 3) * 44 + rand(-8, 8));
          setOrder(u, UNIT_DEF[type].dmg ? { k: 'amove', x: cx - dir * 200, y: u.y } : { k: 'move', x: cx + dir * 120, y: u.y });
        }
      }
      census(f);
    });
    const start = sides.map(worth), limit = cfg.limit || 90;
    while (G.time < limit) {
      updateWorld(0.05);
      // chi ha finito i nemici a tiro ma ne restano: di nuovo all'attacco verso di loro
      if (Math.round(G.time * 20) % 40 === 0) {
        for (const f of sides) {
          const foe = G.units.find(u => u.faction !== f && !u.dead && UNIT_DEF[u.type].dmg) || G.units.find(u => u.faction !== f && !u.dead);
          if (!foe) continue;
          for (const u of G.units) if (u.faction === f && !u.dead && u.def.dmg && u.order.k === 'idle' && !u.target) setOrder(u, { k: 'amove', x: foe.x, y: foe.y });
        }
      }
      const fighters = sides.map(f => G.units.filter(u => u.faction === f && !u.dead && u.def.dmg).length);
      if (!fighters[0] || !fighters[1]) break;
    }
    const end = sides.map(worth);
    return { time: Math.round(G.time), a: +(end[0] / start[0]).toFixed(3), b: +(end[1] / start[1]).toFixed(3), valueA: value(cfg.a), valueB: value(cfg.b) };
  },

  // Rendita di una risorsa: un regno da solo, `pawns` pedoni tutti su `res` per `time` secondi.
  // farms = fattorie già costruite, depot = un deposito accanto al castello, techs = ricerche già fatte.
  // Ritorna quanto ha raccolto ogni pedone al minuto.
  eco(cfg) {
    this.start({ seed: cfg.seed, players: [{ bot: 'idle' }] });
    const f = G.factions[0], c = f.castle, time = cfg.time || 300;
    for (const id of cfg.techs || []) applyTech(f, id);
    for (let i = 0; i < (cfg.farms || 0); i++) { const s = findSpot('farm', f.base.x, f.base.y + 1, 4); if (s) addBuilding(f, 'farm', s[0], s[1], true); }
    if (cfg.depot) { const s = findSpot('depot', f.base.x, f.base.y + 1, 4); if (s) addBuilding(f, 'depot', s[0], s[1], true); }
    for (const u of f.soldiers) killUnit(u, null);
    while (f.pawns.length < cfg.pawns) { spawnUnit(f, 'pawn', c.x + rand(-120, 120), c.y1 + rand(50, 130)); census(f); }
    updateWorld(0.05);
    const ret = execCommand(f, 'jobs', cfg.res, cfg.pawns), start = f.gathered;
    let working = 0, samples = 0;
    while (G.time < time) {
      updateWorld(0.05);
      if (Math.round(G.time * 20) % 100 === 0) { samples++; working += f.pawns.filter(p => p.order.k === 'gather' && (p.node || p.carry)).length; }
    }
    return { res: cfg.res, pawns: cfg.pawns, farms: cfg.farms || 0, perPawnMinute: +((f.gathered - start) / cfg.pawns / (time / 60)).toFixed(1), busyShare: +(working / samples / cfg.pawns).toFixed(2), refused: ret };
  },
};
SIM.hook();
