'use strict';
// Le misure del bilanciamento, in tabelle. Tutto gira in parallelo sui core della macchina.
//
//   node tools/balance/measure.js arena  [-o overrides.json] [-n semi] [-w processi]
//   node tools/balance/measure.js eco    ...
//   node tools/balance/measure.js duels  ...   strategia contro strategia, due regni soli
//   node tools/balance/measure.js vsai   ...   ogni strategia contro quattro regni dell'IA  [-l normal|hard|easy]
//   node tools/balance/measure.js ai     ...   cinque regni dell'IA fra loro (quanto durano, chi vince, con cosa)
//
// overrides.json = { consts: { NOME: valore }, set: { 'UNIT_DEF.knight.hp': 170 } }   (vedi headless.js)
// -s a,b,c limita le strategie; -j stampa anche il JSON grezzo su file.

const fs = require('fs');
const { runBatch } = require('./run');

const args = process.argv.slice(2), mode = args[0];
const opt = (flag, def) => { const i = args.indexOf(flag); return i < 0 ? def : args[i + 1]; };
const overrides = opt('-o') ? JSON.parse(fs.readFileSync(opt('-o'), 'utf8')) : {};
const N = +opt('-n', 0), workers = +opt('-w', 0) || undefined, level = opt('-l', 'normal');
const ALL = ['knightMonk', 'knights', 'warriors', 'lancers', 'archers', 'archerLancer', 'warriorArcher', 'mixed'];
const strats = opt('-s') ? opt('-s').split(',') : ALL;
const pad = (s, n) => String(s).padEnd(n), num = (v, n = 6) => String(v).padStart(n);
const mean = a => (a.length ? a.reduce((x, y) => x + y, 0) / a.length : 0);
const dump = (name, data) => { if (opt('-j')) fs.writeFileSync(opt('-j'), JSON.stringify(data)); };
// errori dei processi figli: una riga per tipo, con quante volte
const failed = {};
const fail = e => { const k = e.split('\n').slice(0, 5).join(' | '); if (!failed[k]) console.log('ERRORE', e); failed[k] = (failed[k] || 0) + 1; };
process.on('exit', () => { for (const [k, n] of Object.entries(failed)) if (n > 1) console.log(`(lo stesso errore ${n} volte: ${k.slice(0, 120)})`); });

const UNITS = ['warrior', 'lancer', 'archer', 'knight', 'catapult'];

async function arena() {
  // costi veri, con gli overrides applicati: li chiedo al gioco
  const { load } = require('./headless');
  const g = load(overrides); await g.ready();
  const cost = JSON.parse(g.run('JSON.stringify(Object.fromEntries(UNIT_TYPES.map(t => [t, unitValue(t)])))'));
  const seeds = N || 12, budget = 1680;
  const army = t => ({ [t]: Math.max(1, Math.round(budget / cost[t])) });
  const tasks = [], index = [];
  for (const a of UNITS) for (const b of UNITS) if (a < b) for (let s = 0; s < seeds; s++) { tasks.push({ kind: 'arena', cfg: { seed: 1000 + s, a: army(a), b: army(b) } }); index.push(['duel', a, b]); }
  // quanto vale un monaco: lo stesso esercito con e senza, a parità di spesa, contro lo stesso nemico
  for (const t of ['knight', 'warrior', 'lancer']) {
    for (const foe of UNITS.filter(u => u !== 'catapult')) {
      const n = Math.round(budget / cost[t]), monks = 3, fewer = Math.max(1, Math.round((budget - monks * cost.monk) / cost[t]));
      for (let s = 0; s < seeds; s++) {
        tasks.push({ kind: 'arena', cfg: { seed: 2000 + s, a: { [t]: n }, b: army(foe) } }); index.push(['plain', t, foe]);
        tasks.push({ kind: 'arena', cfg: { seed: 2000 + s, a: { [t]: fewer, monk: monks }, b: army(foe) } }); index.push(['monk', t, foe]);
      }
    }
  }
  const rs = await runBatch(tasks, { overrides, workers });
  const acc = {};
  rs.forEach((r, i) => { const k = index[i].join(' '); (acc[k] || (acc[k] = [])).push(r); });
  console.log(`\nSCONTRI A PARITÀ DI SPESA (${budget} risorse a testa, ${seeds} prove). Valore rimasto al vincitore; + vince la riga, - la colonna`);
  console.log(pad('', 10) + UNITS.map(u => num(u, 10)).join(''));
  for (const a of UNITS) {
    let line = pad(a + ' ×' + army(a)[a], 10);
    for (const b of UNITS) {
      if (a === b) { line += num('·', 10); continue; }
      const k = a < b ? `duel ${a} ${b}` : `duel ${b} ${a}`, rr = acc[k];
      const mine = mean(rr.map(r => (a < b ? r.a : r.b))), theirs = mean(rr.map(r => (a < b ? r.b : r.a)));
      line += num((mine > theirs ? '+' : '-') + Math.round(100 * Math.max(mine, theirs)) + '%', 10);
    }
    console.log(line);
  }
  console.log('\nMONACI: stessa spesa, con 3 monaci al posto di qualche soldato. Valore rimasto a chi li ha (senza → con) e al nemico');
  for (const t of ['knight', 'warrior', 'lancer']) for (const foe of UNITS.filter(u => u !== 'catapult')) {
    const p = acc[`plain ${t} ${foe}`], m = acc[`monk ${t} ${foe}`];
    console.log(pad(`${t} contro ${foe}`, 26) + `senza ${num(Math.round(100 * mean(p.map(r => r.a))), 3)}%  con ${num(Math.round(100 * mean(m.map(r => r.a))), 3)}%   nemico: ${num(Math.round(100 * mean(p.map(r => r.b))), 3)}% → ${num(Math.round(100 * mean(m.map(r => r.b))), 3)}%`);
  }
  dump('arena', acc);
}

async function eco() {
  const seeds = N || 8, tasks = [], index = [];
  const cases = [['wood', 6, 0], ['gold', 6, 0], ['stone', 4, 0], ['meat', 6, 0], ['meat', 4, 1], ['meat', 8, 2], ['meat', 8, 4], ['meat', 12, 4], ['meat', 16, 4]];
  for (const [res, pawns, farms] of cases) for (let s = 0; s < seeds; s++) { tasks.push({ kind: 'eco', cfg: { seed: 3000 + s, res, pawns, farms, time: 360 } }); index.push(`${res} ${pawns}p ${farms}f`); }
  const rs = await runBatch(tasks, { overrides, workers });
  const acc = {};
  rs.forEach((r, i) => (acc[index[i]] || (acc[index[i]] = [])).push(r));
  console.log(`\nRENDITA (un regno da solo, 6 minuti, ${seeds} mappe): risorse per pedone al minuto, e in tutto al minuto`);
  for (const [k, rr] of Object.entries(acc)) {
    const per = mean(rr.map(r => r.perPawnMinute)), pawns = rr[0].pawns;
    console.log(pad(k, 16) + num(per.toFixed(1), 7) + ' a pedone' + num(Math.round(per * pawns), 7) + ' in tutto   al lavoro ' + Math.round(100 * mean(rr.map(r => r.busyShare))) + '%' + (rr.some(r => r.refused) ? '   (rifiutato in ' + rr.filter(r => r.refused).length + ')' : ''));
  }
  dump('eco', acc);
}

async function duels() {
  const seeds = N || 8, tasks = [], index = [];
  for (const a of strats) for (const b of strats) if (a < b) for (let s = 0; s < seeds; s++) {
    // a turno chi sta sulla prima base
    const first = s % 2 ? b : a, second = s % 2 ? a : b;
    tasks.push({ kind: 'match', cfg: { seed: 4000 + s, limit: 1800, players: [{ bot: first }, { bot: second }] } }); index.push([a, b, first]);
  }
  let k = 0;
  const rs = await runBatch(tasks, { overrides, workers, onResult: () => { if (++k % 40 === 0) process.stderr.write(`  ${k}/${tasks.length}\n`); } });
  const wins = {}, games = {}, times = {};
  for (const s of strats) { wins[s] = {}; games[s] = {}; }
  rs.forEach((r, i) => {
    if (r.error) { fail(r.error); return; }
    const [a, b] = index[i], w = r.winner < 0 ? null : r.players[r.winner].bot;
    games[a][b] = (games[a][b] || 0) + 1; games[b][a] = (games[b][a] || 0) + 1;
    if (w) { const l = w === a ? b : a; wins[w][l] = (wins[w][l] || 0) + 1; (times[w] || (times[w] = [])).push(r.time); }
  });
  console.log(`\nDUELLI fra strategie (${seeds} partite a coppia, 30 minuti al massimo): vittorie della riga sulla colonna`);
  console.log(pad('', 15) + strats.map(s => num(s.slice(0, 9), 10)).join('') + num('vinte', 8) + num('minuti', 8));
  for (const a of strats) {
    let line = pad(a, 15), tot = 0, all = 0;
    for (const b of strats) {
      if (a === b) { line += num('·', 10); continue; }
      const w = wins[a][b] || 0; tot += w; all += games[a][b] || 0;
      line += num(w + '/' + (games[a][b] || 0), 10);
    }
    console.log(line + num(Math.round(100 * tot / all) + '%', 8) + num(times[a] ? (mean(times[a]) / 60).toFixed(1) : '-', 8));
  }
  dump('duels', rs);
}

async function vsai() {
  const seeds = N || 10, tasks = [], index = [];
  for (const a of strats) for (let s = 0; s < seeds; s++) {
    tasks.push({ kind: 'match', cfg: { seed: 5000 + s, limit: 2400, players: [{ bot: a }, ...Array(4).fill({ bot: 'ai', level })] } }); index.push(a);
  }
  let k = 0;
  const rs = await runBatch(tasks, { overrides, workers, onResult: () => { if (++k % 20 === 0) process.stderr.write(`  ${k}/${tasks.length}\n`); } });
  const acc = {};
  rs.forEach((r, i) => { if (r.error) fail(r.error); else (acc[index[i]] || (acc[index[i]] = [])).push(r); });
  console.log(`\nCONTRO 4 REGNI DELL'IA (${level}), ${seeds} partite, 40 minuti al massimo`);
  console.log(pad('strategia', 15) + num('vince', 7) + num('±', 6) + num('cade', 7) + num('in min', 8) + num('cade a', 8) + num('regni', 7) + num('uccide', 8) + num('perde', 7) + '   ucciso con');
  for (const [a, rr] of Object.entries(acc)) {
    const me = rr.map(r => r.players[0]), won = rr.filter(r => r.winner === 0), dead = me.filter(p => !p.alive);
    const downed = mean(rr.map(r => r.players.slice(1).filter(p => p.killer === 0).length));
    const kills = {}; for (const p of me) for (const [w, v] of Object.entries(p.killsWith)) kills[w] = (kills[w] || 0) + v;
    const top = Object.entries(kills).sort((x, y) => y[1] - x[1]).slice(0, 3).map(([w, v]) => w + ' ' + Math.round(v / rr.length)).join(', ');
    const lostValue = mean(me.map(p => p.lostValue)), p = won.length / rr.length;
    // ± = una deviazione standard della quota di vittorie: con poche partite è larga
    console.log(pad(a, 15) + num(won.length + '/' + rr.length, 7) + num('±' + Math.round(100 * Math.sqrt(p * (1 - p) / rr.length)) + '%', 6) + num(dead.length + '/' + rr.length, 7) + num(won.length ? (mean(won.map(r => r.time)) / 60).toFixed(1) : '-', 8) +
      num(dead.length ? (mean(dead.map(p => p.deathT)) / 60).toFixed(1) : '-', 8) + num(downed.toFixed(1), 7) + num(Math.round(mean(me.map(p => Object.values(p.killsWith).reduce((x, y) => x + y, 0)))), 8) + num(Math.round(lostValue), 7) + '   ' + top);
  }
  dump('vsai', rs);
}

async function ai() {
  const seeds = N || 10, tasks = [];
  for (let s = 0; s < seeds; s++) tasks.push({ kind: 'match', cfg: { seed: 6000 + s, limit: 2400, players: Array(5).fill({ bot: 'ai', level }) } });
  const rs = await runBatch(tasks, { overrides, workers });
  const ok = rs.filter(r => !r.error), trained = {}, kills = {};
  for (const r of ok) for (const p of r.players) { for (const [t, n] of Object.entries(p.trained)) trained[t] = (trained[t] || 0) + n; for (const [w, v] of Object.entries(p.killsWith)) kills[w] = (kills[w] || 0) + v; }
  console.log(`\nCINQUE REGNI DELL'IA (${level}), ${seeds} partite, 40 minuti al massimo`);
  console.log('finite con un vincitore: ' + ok.filter(r => r.winner >= 0).length + '/' + ok.length + '   durata media ' + (mean(ok.map(r => r.time)) / 60).toFixed(1) + ' min   regni caduti a partita ' + mean(ok.map(r => r.players.filter(p => !p.alive).length)).toFixed(1));
  console.log('prima caduta a ' + (mean(ok.map(r => Math.min(...r.players.filter(p => p.deathT).map(p => p.deathT), 2400))) / 60).toFixed(1) + ' min   Età Feudale a ' + (mean(ok.flatMap(r => r.players.filter(p => p.age2).map(p => p.age2))) / 60).toFixed(1) + '   Età dei Castelli a ' + (mean(ok.flatMap(r => r.players.filter(p => p.age3).map(p => p.age3))) / 60).toFixed(1));
  const per = o => Object.entries(o).sort((x, y) => y[1] - x[1]).map(([k, v]) => k + ' ' + Math.round(v / ok.length / 5)).join(', ');
  console.log('addestrati per regno: ' + per(trained));
  console.log('valore ucciso per regno, per arma: ' + per(kills));
  dump('ai', rs);
}

({ arena, eco, duels, vsai, ai }[mode] || (() => { console.log('modo sconosciuto: arena | eco | duels | vsai | ai'); }))();
