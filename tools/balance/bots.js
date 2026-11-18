// Gira dentro il contesto del gioco: bot che giocano come un giocatore, cioè solo con i comandi che ha lui
// (execCommand), con le sue regole (nebbia per i pedoni, fuga nelle case, allarme) e senza la rendita dell'IA.
// Ogni strategia ha lo stesso scheletro di economia e cambia in ciò che conta per il bilanciamento:
// quali unità, a che era si ferma, quando attacca. Tutte costruiscono le fattorie che servono ai loro pedoni
// sul cibo (una ogni due o tre); `farms` è il minimo da cui parte la strategia.

const STRATS = {
  // la partita segnalata: quattro fattorie, cibo venduto al mercato, solo cavalieri e monaci
  knightMonk: { farms: 4, army: { knight: 3, monk: 1 }, age: 3, attackAt: 8, techs: ['horseshoes', 'blades'], market: true },
  knights: { farms: 4, army: { knight: 1 }, age: 2, attackAt: 6, techs: ['horseshoes', 'blades'], market: true },
  warriors: { farms: 2, army: { warrior: 1 }, age: 1, attackAt: 8, techs: [] },
  lancers: { farms: 3, army: { lancer: 1 }, age: 2, attackAt: 8, techs: ['blades'], market: true },
  archers: { farms: 2, army: { archer: 1 }, age: 2, attackAt: 10, techs: ['fletching'], market: true },
  archerLancer: { farms: 2, army: { archer: 1, lancer: 1 }, age: 2, attackAt: 10, techs: ['fletching', 'blades'], market: true },
  warriorArcher: { farms: 2, army: { warrior: 1, archer: 1 }, age: 2, attackAt: 10, techs: ['fletching', 'blades'], market: true },
  mixed: { farms: 3, army: { warrior: 2, archer: 2, lancer: 1, knight: 1, monk: 1, catapult: 1 }, age: 3, attackAt: 12, techs: ['blades', 'fletching', 'armor'], market: true },
  // strategie di controllo, per capire da cosa dipende un risultato: la stessa cosa senza monaci, o con altre unità
  knightsLate: { farms: 4, army: { knight: 1 }, age: 3, attackAt: 8, techs: ['horseshoes', 'blades'], market: true },
  warriorMonk: { farms: 4, army: { warrior: 3, monk: 1 }, age: 3, attackAt: 8, techs: ['blades'], market: true },
  lancerMonk: { farms: 4, army: { lancer: 3, monk: 1 }, age: 3, attackAt: 8, techs: ['blades'], market: true },
  archerMonk: { farms: 4, army: { archer: 3, monk: 1 }, age: 3, attackAt: 8, techs: ['fletching'], market: true },
};

function makeBot(name, f, opts) {
  if (name === 'idle') return { think() {}, state: {}, strat: {} }; // un regno che non fa nulla da solo
  const S = { ...STRATS[name], ...opts };
  if (!STRATS[name]) throw new Error('Strategia sconosciuta: ' + name);
  const st = { t: 0, target: null, attacking: false, scout: 0, lastJobs: -99 };
  const types = Object.keys(S.army);
  const need = [...new Set(types.map(t => UNIT_DEF[t].from))];
  const count = kind => (f.has[kind] || 0) + (f.sites[kind] || 0);
  const can = cost => canAfford(f, cost);
  const sites = () => Object.values(f.sites).reduce((a, b) => a + b, 0);

  function build(kind) {
    const d = BUILD_DEF[kind];
    if (f.age < d.age || !can(d.cost) || sites() >= 2) return false;
    const spot = findSpot(kind, f.base.x, f.base.y + 1, 4);
    if (!spot) return false;
    const x = (spot[0] + d.w / 2) * TILE, y = (spot[1] + d.h / 2) * TILE;
    const pawns = f.pawns.filter(p => !p.inside && p.order.k !== 'build').sort((a, b) => Math.hypot(a.x - x, a.y - y) - Math.hypot(b.x - x, b.y - y));
    if (!pawns.length) return false;
    return execCommand(f, 'place', kind, spot[0], spot[1], pawns.slice(0, d.work > 15 ? 2 : 1).map(p => p.id)) === null;
  }
  function research(id) {
    const d = TECH_DEF[id];
    if (f.tech[id] || f.busy[id] || f.age < d.age || (d.req && !f.tech[d.req]) || !can(d.cost)) return false;
    const b = freeBuilding(f, d.at);
    return !!b && b.queue.length < 2 && execCommand(f, 'research', b.id, id) === null;
  }
  function train(type) {
    const d = UNIT_DEF[type], b = freeBuilding(f, d.from);
    return !!b && f.age >= d.age && b.queue.length < 2 && can(d.cost) && execCommand(f, 'train', b.id, type) === null;
  }
  // ciò che la strategia vuole comprare adesso e non può ancora: l'esercito non deve mangiarsi quelle risorse
  function saving() {
    if (f.age < S.age) return TECH_DEF[f.age === 1 ? 'age2' : 'age3'].cost;
    for (const k of need) if (!count(k)) return BUILD_DEF[k].cost;
    return null;
  }
  function spare(cost) {
    const hold = saving();
    if (!hold || f.soldiers.length < 3) return true;
    return RES_KINDS.every(k => !(cost[k] > 0) || f.res[k] - cost[k] >= (hold[k] || 0));
  }

  // quanti pedoni per risorsa: in proporzione a ciò che la strategia spende, con un minimo su legna e pietra
  function jobs() {
    const want = { wood: 60, gold: 40, meat: 90, stone: 8 };
    for (const [t, w] of Object.entries(S.army)) for (const [k, v] of Object.entries(UNIT_DEF[t].cost)) want[k] += v * w * 2;
    const hold = saving();
    if (hold) for (const [k, v] of Object.entries(hold)) want[k] += v * 0.6;
    for (const k of RES_KINDS) if (f.res[k] > 500) want[k] *= 0.4;
    const workers = f.pawns.filter(p => p.order.k !== 'build' && p.order.k !== 'repair').length;
    const total = RES_KINDS.reduce((s, k) => s + want[k], 0), share = {};
    for (const k of RES_KINDS) share[k] = Math.floor(workers * want[k] / total);
    // finché c'è da costruire serve legna; la pietra solo a chi punta all'Età dei Castelli
    const building = count('farm') < farmsWanted() || need.some(k => !count(k)) || f.troopCap < 30;
    share.wood = Math.max(share.wood, f.res.wood > 400 ? 1 : building ? Math.ceil(workers * 0.25) : 2);
    if (S.age === 3 && f.age === 2 && f.res.stone < 150) share.stone = Math.max(share.stone, 2);
    const fixed = share.wood + share.stone, rest = Math.max(0, workers - fixed), mg = share.meat + share.gold || 1;
    share.gold = Math.floor(rest * share.gold / mg);
    share.meat = rest - share.gold;
    st.meat = share.meat;
    const now = k => f.pawns.filter(p => p.order.k === 'gather' && p.order.res === k).length;
    for (const k of RES_KINDS) if (now(k) > share[k]) execCommand(f, 'jobs', k, share[k]); // prima chi cala, poi chi cresce
    for (const k of RES_KINDS) if (now(k) < share[k]) execCommand(f, 'jobs', k, share[k]);
    // se una risorsa non ha posti liberi, i pedoni rimasti senza lavoro vanno dove ce n'è
    for (const k of ['meat', 'wood', 'gold', 'stone']) {
      const idle = f.pawns.filter(p => p.order.k === 'idle' && !p.inside).length;
      if (idle) execCommand(f, 'jobs', k, now(k) + idle);
    }
  }

  // una fattoria alleva per due o tre pedoni: tante quante ne servono a chi è sul cibo, almeno quelle della strategia
  function farmsWanted() { return Math.max(S.farms, Math.min(6, Math.ceil((st.meat || 0) / 2.5))); }

  function think() {
    st.t++;
    const c = f.castle;
    // --- pedoni, case (posti per i pedoni) e accampamenti (posti per le truppe)
    const pawnGoal = f.age === 1 ? 14 : f.age === 2 ? 18 : 20, comingPawns = queued(f, t => t === 'pawn'), comingTroops = queued(f, t => t !== 'pawn');
    if (f.pawnN + comingPawns < Math.min(pawnGoal, f.pawnCap) && c.queue.length < 2) train('pawn');
    if (!f.sites.house && f.pawnCap < PAWN_MAX && f.pawnN + comingPawns >= f.pawnCap - 3 && pawnGoal > f.pawnCap - 3) build('house');
    if (!f.sites.camp && f.troopCap < TROOP_MAX && f.troopN + comingTroops >= f.troopCap - 3) build('camp');
    if (st.t - st.lastJobs >= 5) { st.lastJobs = st.t; jobs(); }

    // --- edifici ed ere, nell'ordine in cui servono
    if (count('farm') < Math.min(farmsWanted(), 1 + Math.floor(st.t / 50))) build('farm');
    const first = need.find(k => BUILD_DEF[k].age === 1);
    if (first && !count(first) && count('farm')) build(first);
    if (f.age < S.age) research(f.age === 1 ? 'age2' : 'age3');
    for (const k of need) if (!count(k) && f.age >= BUILD_DEF[k].age) build(k);
    if (S.market && f.age >= 2 && !count('market') && need.every(k => BUILD_DEF[k].age > f.age || count(k))) build('market');
    for (const id of S.techs) {
      const at = TECH_DEF[id].at;
      if (f.age >= BUILD_DEF[at].age && !count(at) && f.soldiers.length >= 4) build(at);
      if (f.soldiers.length >= 4) research(id);
    }
    // una seconda fabbrica quando le risorse avanzano
    const main = UNIT_DEF[types[0]].from;
    if (count(main) === 1 && f.has[main] && f.age >= S.age && RES_KINDS.every(k => f.res[k] > 250)) build(main);

    // --- mercato: il cibo e la legna che avanzano diventano oro
    const market = f.has.market && freeBuilding(f, 'market');
    if (market && f.res.gold < 150) {
      const extra = ['meat', 'wood', 'stone'].find(k => f.res[k] > 350);
      if (extra) execCommand(f, 'trade', market.id, 'sell', extra);
    }

    // --- esercito: il tipo più indietro rispetto alle proporzioni volute
    const have = t => f.soldiers.filter(u => u.type === t).length + queued(f, q => q === t);
    if (f.troopN + comingTroops < f.troopCap) {
      const open = types.filter(t => f.age >= UNIT_DEF[t].age && f.has[UNIT_DEF[t].from]);
      open.sort((a, b) => have(a) / S.army[a] - have(b) / S.army[b]);
      for (const t of open) { if (spare(UNIT_DEF[t].cost) && train(t)) break; }
    }

    // --- esplorazione: il guerriero iniziale fa il giro dei dintorni, così i pedoni trovano altre risorse
    const scout = f.soldiers.find(u => u.type === 'warrior' && u.order.k === 'idle' && !u.target);
    if (scout && !st.attacking && st.scout < 10 && st.t > 20 && st.t % 12 === 0) {
      const a = st.scout++ * 2.4, p = World.nearestPassable(c.x + Math.cos(a) * 1500, c.y + Math.sin(a) * 1100);
      execCommand(f, 'order', [scout.id], 'move', p.x, p.y, 0);
    }
    // --- punti da conquistare: tre soldati liberi a casa vanno a prendere il più vicino che non è ancora suo
    if (!st.attacking && st.t % 30 === 15) {
      const squad = f.soldiers.filter(u => u.def.dmg && !u.def.onlyBuildings && u.order.k === 'idle' && !u.target && Math.hypot(u.x - c.x, u.y - c.y) < 900);
      let best = null, bd = 2200 + 600 * f.age;
      for (const p of G.points) { const d = Math.hypot(p.x - c.x, p.y - c.y); if (p.owner !== f && d < bd) { bd = d; best = p; } }
      if (best && squad.length >= 4) execCommand(f, 'order', squad.slice(0, 3).map(u => u.id), 'amove', best.x, best.y1 + 40, 0);
    }
    // chi ha finito lontano da casa (un punto preso, un inseguimento) torna
    if (!st.attacking && st.t % 30 === 0) {
      const away = f.soldiers.filter(u => u.order.k === 'idle' && !u.target && Math.hypot(u.x - c.x, u.y - c.y) >= 900 && !G.points.some(p => p.owner !== f && Math.hypot(p.x - u.x, p.y - u.y) < CAPTURE_RADIUS));
      if (away.length) execCommand(f, 'order', away.map(u => u.id), 'move', c.x, c.y1 + 130, 0);
    }

    // --- attacco: tutti insieme sul regno più vicino; poi i rinforzi li raggiungono
    const fighters = f.soldiers.filter(u => u.def.dmg || u.def.heal);
    const home = fighters.filter(u => u.order.k === 'idle' && !u.target && Math.hypot(u.x - c.x, u.y - c.y) < 900);
    if (!st.target || !st.target.alive) { st.target = nearestEnemyFaction(f); if (st.attacking && st.target) st.resend = true; }
    const go = units => execCommand(f, 'order', units.map(u => u.id), 'amove', st.target.castle.x, st.target.castle.y1 + 40, 0);
    if (st.target) {
      if (!st.attacking && home.filter(u => u.def.dmg).length >= S.attackAt) { st.attacking = true; go(home); }
      else if (st.attacking && (st.resend || st.t % 15 === 0)) {
        st.resend = false;
        // chi è rimasto fermo lontano da casa, e i rinforzi quando sono almeno tre
        const idle = fighters.filter(u => u.order.k === 'idle' && !u.target);
        const away = idle.filter(u => Math.hypot(u.x - c.x, u.y - c.y) >= 900);
        if (away.length) go(away);
        if (home.length >= 3) go(home);
        if (!fighters.some(u => Math.hypot(u.x - c.x, u.y - c.y) >= 900) && home.filter(u => u.def.dmg).length < S.attackAt) st.attacking = false; // ondata finita: si ricomincia
      }
    }
  }
  return { think, state: st, strat: S };
}
