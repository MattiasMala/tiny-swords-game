# Tiny Swords — La Guerra dei Cinque Regni

Strategico in tempo reale in stile Age of Empires, scritto in **Rust** con **Bevy 0.20** e compilato in
**WebAssembly** per il browser. Cinque regni tutti contro tutti su un'isola di 288×192 caselle; vince l'ultimo
castello in piedi.

Il mondo è in stile HD-2D (come Octopath Traveler II): il terreno è fatto di blocchi (altopiani a terrazze con
pareti di roccia, scale, coste), mentre alberi, unità, edifici e oggetti sono gli sprite PNG di "Tiny Swords"
in piedi nel mondo 3D, illuminati dal ciclo giorno/notte e con ombre vere.

Usa i 410 PNG di "Tiny Swords (Free Pack)" più 178 sprite aggiuntivi nello stesso stile (`assets/Extra/`).

## Avvio rapido

Serve Rust (la versione giusta la installa da sola `rustup`, letta da `rust-toolchain.toml`).

    ./build-web.sh                         # Windows: .\build-web.ps1
    cargo run -p ts-server --release       # server su 0.0.0.0:8000

Poi apri `http://localhost:8000`. Il server stampa anche l'indirizzo da dare agli altri giocatori della rete
locale (es. `http://192.168.1.8:8000`): ascolta sempre su `0.0.0.0`, quindi è raggiungibile da ogni computer
della rete.

Opzioni del server: `ts-server [porta] [--bind INDIRIZZO] [--root CARTELLA]` (predefiniti: `8000`, `0.0.0.0`,
la cartella del progetto).

`build-web` compila il client (`crates/client`) in `web/pkg/`: cargo (profilo `wasm-release`), poi
`wasm-bindgen` e `wasm-opt`, che scarica da solo se mancano. Con `NOOPT=1` salta `wasm-opt`: più veloce, file
più grande.

### Multigiocatore

1. Un giocatore apre l'indirizzo del server e preme **MULTIGIOCATORE**: diventa il capo della lobby.
2. Gli altri aprono lo stesso indirizzo e premono **MULTIGIOCATORE**.
3. Ognuno sceglie il regno, il capo sceglie la difficoltà dell'IA e preme **INIZIA**. I regni liberi li guida l'IA.

La partita gira **sul server**, non nel browser di qualcuno: chiudere o ricaricare una scheda non ferma la
partita per gli altri.

**Riconnessione**: ogni browser ha un gettone di sessione salvato nella pagina. Se un giocatore ricarica la
pagina, chiude la scheda o perde la connessione, l'IA guida subito il suo regno; quando torna (stessa pagina,
stesso browser) riprende il suo regno per tutta la durata della partita, con gli ordini dati nel frattempo.
Mentre la connessione è caduta compare «Riconnessione in corso…» e il gioco riprova da solo. Se lo stesso
giocatore apre una seconda scheda, quella nuova prende il posto della vecchia. «Esci» dal menu lascia
definitivamente il regno all'IA. Una partita rimasta senza nessun giocatore collegato viene chiusa dopo 5 minuti.

Chi viene eliminato può restare a guardare. Non si entra a partita iniziata, tranne chi si riconnette.

### Da soli

Dal menu principale: scegli la difficoltà e gioca. La partita da soli gira nel browser, con la stessa
simulazione del server, e funziona anche senza rete (basta servire i file). F4 apre la modalità test.

### Sviluppo (client nativo)

    cargo run -p ts-client -- --solo --seed 7                       # finestra nativa, partita da soli
    cargo run -p ts-client -- --solo --seed 7 --cam 150,100 --zoom 0.7 --time 0.3 --frames 150 --screenshot shot.png

`--cam X,Y` centra la casella (X, Y), `--zoom` avvicina (1 = normale), `--time` è l'ora del giorno 0..1,
`--screenshot` salva un'immagine dopo `--frames` fotogrammi ed esce. Con la variabile `TS_NOFOG=1` non c'è nebbia.

    cargo run -p ts-sim --release --example mapgen_png -- --seed 7 --out mappa.png --scale 3   # mappa vista dall'alto
    cargo run -p ts-sim --release --example mapgen_png -- --check 1000                         # controlla 1000 mappe
    cargo run -p ts-sim --release --example simbench -- --level normal --minutes 30            # 5 IA fra loro
    cargo test -p ts-sim && cargo test -p ts-server

## Come si gioca

Parti con un castello, quattro pedoni e un guerriero. Raccogli, costruisci, avanza d'era, addestra un esercito
e abbatti i castelli degli altri: se cade il tuo, hai perso.

| Comando | Azione |
|---|---|
| Clic · trascina | seleziona unità ed edifici (doppio clic: tutte le unità uguali in vista) |
| Clic destro | muovi, attacca, raccogli, costruisci/ripara, punto di raccolta (con un edificio selezionato) |
| Frecce o WASD · bordo dello schermo · minimappa | sposta la visuale; tenendo Maiusc va al doppio, tenendo Ctrl al quadruplo |
| Rotellina · `+` e `−` | zoom (con la rotellina attorno al mouse, con i tasti attorno al centro dello schermo) |
| Frecce o WASD + Invio o Spazio | nelle schermate (menu compreso): passa da un pulsante all'altro e premilo · posando o spostando un edificio: muovi la sagoma di una casella e posala · sulla mappa d'invio: scegli il punto e manda |
| Maiusc + 1–9 · 1–9 | crea · richiama un gruppo |
| `.` · H · Spazio | prossimo pedone inattivo · castello · centra la selezione |
| U · L · I · K · C | schermate Unità · Lavoratori · Ricerche · Mercato · Costruisci |
| V · Tab · Backspace | sposta un edificio · seleziona tutte le truppe · richiamale davanti al castello |
| X · E · F · G | con una selezione: attacco in marcia · esplora (Esploratori) · ferma · punto di raccolta |
| Maiusc durante il piazzamento | continua a piazzare; con le palizzate, trascinando, ne posa una fila |
| Esc · P · Canc | annulla/deseleziona · pausa (in rete: menu, la partita continua) · demolisci |
| M · F1 · F2 | audio · aiuto · luci (complete, leggere, spente; se il gioco rallenta si alleggeriscono da sole) |
| F4 | modalità test (solo da soli): +1000 risorse, ferma il tempo, togli la nebbia |

Le regole del gioco (risorse, posti, ere, edifici, unità, punti da conquistare, ricerche, IA) sono le stesse
della versione JavaScript precedente e sono descritte per intero in `docs/spec/`.

## La mappa

Diversa a ogni partita e uguale per tutti: viaggia solo il seme, ogni client la rigenera e il server ne controlla
l'impronta. Il generatore (`crates/sim/src/mapgen/`) punta all'aspetto di `Reference/Map-example.png`:

- 30–40 altopiani dai contorni squadrati, alcuni con una terrazza più alta e pareti che cadono a picco sul mare;
  si sale solo dalle scale, in capo alle pareti;
- ogni livello ha il suo verde (tileset 1, 2, 3), i boschi di abeti il loro sottobosco, qualche prato secco;
- boschi fitti di una sola specie e ciuffi di alberi sui bordi, prati aperti dove si muovono le truppe;
- sentieri di terra battuta che curvano fra i castelli vicini e verso i punti da conquistare;
- coste con spiagge, isolotti, scogli e papere; scorci composti con i 48 oggetti di scena (mulini e covoni,
  rovine, relitti e pontili, accampamenti abbandonati, tombe, funghi e fiori).

Le regole di gioco sono garantite e controllate dai test: cinque castelli a ≥ 64 caselle l'uno dall'altro,
partenze eque, tutto ciò che serve raggiungibile, oro sugli altopiani centrali. Una mappa si genera in circa
50 ms. Dettagli in `docs/MAPGEN.md`.

## Codice

| Cartella | Contenuto |
|---|---|
| `crates/sim` | simulazione deterministica a 20 tick al secondo, generazione della mappa, percorsi, IA, nebbia di guerra, stanza di gioco (lobby, sessioni, presa in carico dell'IA), protocollo di rete. Nessuna dipendenza da Bevy |
| `crates/server` | server HTTP + WebSocket (axum/tokio): file statici e partite in rete |
| `crates/client` | client Bevy: terreno a blocchi, acqua, sprite, luci, interfaccia, comandi, audio sintetizzato |
| `web/` | pagina che carica il client (`web/pkg/` è generata da `build-web`) |
| `docs/` | architettura (`ARCHITECTURE.md`), mappa (`MAPGEN.md`), specifiche complete del gioco (`spec/`) |
| `assets/`, `fonts/` | sprite del pacchetto e aggiuntivi, font Fredoka (licenza OFL in `fonts/OFL.txt`) |

## Sprite aggiuntivi

`assets/Extra/` contiene 178 PNG creati per questo gioco nello stile del pacchetto: sette edifici per colore
(Fucina, Mercato, Scuderia, Officina, Deposito, Fattoria, Accampamento), la Palizzata, i cinque punti da
conquistare, Cavaliere, Ariete, Catapulta ed Esploratore animati per i cinque regni, il Pedone con la pietra,
cave di pietra e grandi giacimenti, 48 oggetti di scena e 28 icone. Le immagini di partenza (`tools/raw/`) sono
state generate con modelli di immagini (quale per ciascuna: `tools/raw/SOURCES.md`); `tools/make_sprites.py`
(Python con Pillow e numpy) le ritaglia e le ricolora. Misure e ancoraggi di tutti gli sprite sono in
`docs/spec/assets.json`, che il client incorpora.
