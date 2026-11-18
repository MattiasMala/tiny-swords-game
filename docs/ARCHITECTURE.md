# Tiny Swords (Rust) — architettura

Riscrittura completa del gioco JavaScript in Rust: Bevy 0.20 + WebAssembly per il client, server nativo
autoritativo, simulazione in un crate puro condiviso. Le specifiche del vecchio gioco (regole, numeri, IA, UI)
sono in `docs/spec/*.md`; le note sulle API di Bevy 0.20 in `docs/bevy-0.20-notes.md`.

## Crate

| Crate | Ruolo | Dipendenze |
|---|---|---|
| `crates/sim` (`ts_sim`) | simulazione deterministica a tick fissi, generazione della mappa, IA, nebbia, stanza (lobby/sessioni/presa in carico dell'IA), protocollo di rete | solo `serde`, `postcard` |
| `crates/server` (`ts_server`) | server HTTP + WebSocket nativo (axum/tokio): file statici (`web/`, `assets/`, `fonts/`) e stanze di gioco. Ascolta di default su `0.0.0.0:8000` | `ts_sim` |
| `crates/client` (`ts_client`) | client Bevy (wasm32 nel browser, nativo per sviluppo): rendering HD-2D, input, UI, audio | `ts_sim`, `bevy` |

### Perché così

- **Server autoritativo**: la partita in rete non gira più nel browser dell'host. Chiudere o ricaricare una
  scheda non chiude la partita per nessuno. Ogni client ha un **token di sessione** (in `localStorage`): chi si
  ricollega con lo stesso token riprende il proprio regno, che nel frattempo era guidato dall'IA.
- **Stessa `Room` ovunque**: la logica di stanza (lobby, sessioni, tick, istantanee per giocatore) sta in
  `ts_sim::room` senza I/O. Il server la collega ai WebSocket; il client, in partita da solo, la fa girare in
  locale (nessuna latenza, funziona anche senza rete, modalità test F4). Il client parla sempre e solo il
  protocollo di `ts_sim::proto`, quindi c'è un unico percorso di codice per solo e rete.
- **La mappa non viaggia**: il generatore è deterministico; basta il seme. Viaggiano solo le modifiche
  (alberi abbattuti, giacimenti esauriti, edifici).

## Simulazione (`ts_sim`)

- Tick fisso a 20 Hz (`TICK_DT = 0.05 s`), `f32`, RNG proprio (PCG32) con seme per partita.
- Coordinate: `x` verso est, `y` verso sud, in caselle (1.0 = una casella). L'altezza è un livello intero
  della casella (`0` acqua … `3`), più le rampe/scale.
- Entità in slab con id generazionale (`EntityId(u32)`), griglia spaziale per le query di vicinanza.
- Nebbia: `explored` e `visible` per fazione, aggiornate a 5 Hz.
- Le istantanee per giocatore contengono solo ciò che quel giocatore vede (niente "occhi" sui nemici).

## Rete

- `GET /ws` → WebSocket binario, messaggi `postcard` (`ts_sim::proto::{ClientMsg, ServerMsg}`).
- Il primo messaggio del client è `Hello { token, name }`. Il server risponde con la lobby, oppure, se il
  token appartiene a un regno di una partita in corso, con `Rejoin` + istantanea completa.
- Istantanee a 20 Hz: unità visibili ogni tick, edifici/giacimenti/memoria della nebbia solo quando cambiano.
- Se tutti gli umani sono disconnessi la partita si mette in pausa e riprende appena qualcuno torna.

## Rendering HD-2D

- **Terreno a blocchi** (stile Minecraft): colonne di blocchi per casella, mesh a chunk 32×32 con sole facce
  visibili e occlusione ambientale per vertice; texture prese dal tileset di Tiny Swords (erba, roccia della
  scogliera, sabbia, terra battuta per i sentieri). Acqua come piano con schiuma animata lungo la riva.
- **Sprite 2D in piedi nel mondo 3D** (come Octopath Traveler II): alberi, unità, edifici, decorazioni sono
  quad verticali rivolti alla telecamera, con alpha mask, illuminati dalla scena e che proiettano ombre reali.
  Disegnati a lotti: una mesh dinamica per texture, ricostruita solo per ciò che è inquadrato.
- Telecamera prospettica con campo visivo stretto e inclinazione fissa (effetto diorama), luce direzionale col
  ciclo giorno/notte, nebbia di distanza, bloom, vignettatura, luci puntiformi di notte.

## Generazione della mappa

Pipeline deterministica orientata all'estetica (vedi `crates/sim/src/mapgen/`): forma dell'isola a bassa
risoluzione con contorni "a blocchi" come nel riferimento (`Reference/Map-example.png`), altipiani a terrazze
con pareti di roccia e scale, coste con scogli e isolotti, boschi a gruppi (abeti al centro, latifoglie gialle
e verdi ai bordi), cespugli ai piedi delle pareti, sassi, prati con pecore, sentieri di terra battuta fra i
castelli; poi le regole di gioco (castelli a ≥ 64 caselle, risorse di partenza, punti da conquistare, grandi
giacimenti, rovine e scorci).
