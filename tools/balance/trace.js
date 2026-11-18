'use strict';
// Segue un bot minuto per minuto: risorse, pedoni per lavoro, edifici, esercito. Serve a capire cosa fa.
// Uso: node tools/balance/trace.js <strategia> [seme] [secondi] [livello IA]
const { load } = require('./headless');
(async () => {
  const [bot = 'knightMonk', seed = 7, limit = 900, level = 'normal'] = process.argv.slice(2);
  const game = load();
  await game.ready();
  const out = game.run(`(() => {
    SIM.start({ seed: ${+seed}, players: [{ bot: '${bot}' }, ...Array(4).fill({ bot: 'ai', level: '${level}' })] });
    const f = G.factions[0], lines = [];
    let botT = 0, next = 0;
    while (G.time < ${+limit} && G.factions.filter(x => x.alive).length > 1 && f.alive) {
      updateWorld(0.05);
      if ((botT -= 0.05) <= 0) { botT = 1; for (const x of G.factions) if (x.alive && x.bot) x.bot.think(x); }
      if (G.time >= next) {
        next += 60;
        const jobs = {}; for (const p of f.pawns) { const k = p.order.k === 'gather' ? p.order.res : p.inside ? 'riparo' : p.order.k; jobs[k] = (jobs[k] || 0) + 1; }
        const army = {}; for (const u of f.soldiers) army[u.type] = (army[u.type] || 0) + 1;
        const b = {}; for (const x of G.buildings) if (x.faction === f) { const k = x.kind + (x.done ? '' : '*'); b[k] = (b[k] || 0) + 1; }
        lines.push(String(Math.round(G.time)).padStart(5) + 's  era ' + f.age + '  pedoni ' + f.pawnN + '/' + f.pawnCap + '  truppe ' + f.troopN + '/' + f.troopCap + '  ' + RES_KINDS.map(k => k + ' ' + Math.floor(f.res[k]) + ' (+' + f.rate[k] + ')').join(' ') +
          '\\n        lavori ' + JSON.stringify(jobs) + '  esercito ' + JSON.stringify(army) + (f.bot.state.attacking ? ' ALL\\'ATTACCO' : '') +
          '\\n        edifici ' + JSON.stringify(b) + '  punti ' + G.points.filter(p => p.owner === f).length + '/' + G.points.length + '  nemici vivi ' + G.factions.filter(x => x !== f && x.alive).length);
      }
    }
    lines.push('fine a ' + Math.round(G.time) + 's: ' + (f.alive ? 'vivo' : 'caduto') + ', addestrati ' + JSON.stringify(f.stat.trained) + ', persi ' + JSON.stringify(f.stat.lost));
    return lines.join('\\n');
  })()`);
  console.log(out);
})();
