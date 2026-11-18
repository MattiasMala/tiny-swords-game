'use strict';
// Esegue molte partite o scontri in parallelo, un processo per core.
//
// Come modulo:  const { runBatch } = require('./run');  await runBatch(tasks, { overrides, workers })
//   task = { kind: 'match' | 'arena' | 'eco', cfg }   (vedi SIM.run, SIM.arena, SIM.eco in sim.js)
//   ritorna i risultati nello stesso ordine dei task
// Da riga di comando:  node tools/balance/run.js <file tasks.json> [file overrides.json] [processi]
//   tasks.json = [task, ...]; stampa i risultati in JSON, uno per riga

const { fork } = require('child_process');
const os = require('os');

if (process.argv[2] === '--worker') {
  // ---- processo figlio: carica il gioco una volta e gioca ciò che gli arriva
  const { load } = require('./headless');
  let game = null;
  process.on('message', async m => {
    if (m.overrides !== undefined) { game = load(m.overrides); await game.ready(); process.send({ ready: true }); return; }
    try {
      const fn = { match: 'run', arena: 'arena', eco: 'eco' }[m.task.kind];
      const result = game.run(`JSON.stringify(SIM.${fn}(${JSON.stringify(m.task.cfg)}))`);
      process.send({ i: m.i, result: JSON.parse(result) });
    } catch (err) {
      process.send({ i: m.i, error: String(err && err.stack || err).slice(0, 600) });
    }
  });
} else {
  function runBatch(tasks, { overrides = {}, workers = Math.max(1, os.cpus().length - 2), onResult = null } = {}) {
    return new Promise(resolve => {
      const results = new Array(tasks.length);
      let next = 0, done = 0;
      const n = Math.min(workers, tasks.length);
      if (!n) return resolve(results);
      for (let w = 0; w < n; w++) {
        const child = fork(__filename, ['--worker']);
        const feed = () => {
          if (next >= tasks.length) { child.kill(); return; }
          const i = next++;
          child.send({ i, task: tasks[i] });
        };
        child.on('message', m => {
          if (m.ready) return feed();
          results[m.i] = m.error ? { error: m.error } : m.result;
          if (onResult) onResult(m.i, results[m.i]);
          if (++done === tasks.length) resolve(results);
          feed();
        });
        child.send({ overrides });
      }
    });
  }
  module.exports = { runBatch };

  if (require.main === module) {
    const fs = require('fs');
    const tasks = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
    const overrides = process.argv[3] && process.argv[3] !== '-' ? JSON.parse(fs.readFileSync(process.argv[3], 'utf8')) : {};
    runBatch(tasks, { overrides, workers: +process.argv[4] || undefined }).then(rs => { for (const r of rs) console.log(JSON.stringify(r)); });
  }
}
