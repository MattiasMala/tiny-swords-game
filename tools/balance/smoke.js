'use strict';
// Prova rapida: una partita e uno scontro, con i tempi.
const { load } = require('./headless');
(async () => {
  const t0 = Date.now(), game = load();
  await game.ready();
  console.log('caricato in', Date.now() - t0, 'ms');
  let t = Date.now();
  const arena = game.run(`SIM.arena({ seed: 1, a: { knight: 6 }, b: { lancer: 8 } })`);
  console.log('arena', JSON.stringify(arena), Date.now() - t, 'ms');
  t = Date.now();
  const bot = process.argv[2] || 'knightMonk';
  const r = game.run(`SIM.run({ seed: 7, limit: ${+process.argv[3] || 2400}, players: [{ bot: '${bot}' }, { bot: 'ai' }, { bot: 'ai' }, { bot: 'ai' }, { bot: 'ai' }] })`);
  console.log('partita', (Date.now() - t) + ' ms', 'tempo di gioco', r.time, 'vincitore', r.winner);
  for (const p of r.players) console.log(JSON.stringify(p));
})();
