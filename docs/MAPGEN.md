# Generazione della mappa — progetto (owner S1)

Obiettivo dell'utente: la mappa è la cosa più importante. Oggi «non è bella da vedere, sembra una generazione
troppo casuale che non usa bene tutto quello che gli asset danno». Il riferimento è `Reference/Map-example.png`;
l'analisi di cosa lo rende bello è in `docs/spec/09-asset-inventory.md` §11 (leggerla tutta: ogni punto è una
regola). I vincoli di gioco da rispettare sono in `docs/spec/02-world.md` §6 (+ addenda).

Il terreno sarà disegnato a blocchi (stile Minecraft) con sprite 2D in piedi (stile Octopath Traveler II): le
pareti di roccia sono colonne di blocchi alte un livello, i livelli si leggono dal colore dell'erba e dall'ombra.

## Principi

1. **Terrazze, non rumore.** Tre livelli di terra annidati (0 costa/pianura, 1 altopiano, 2 terrazza alta).
   Contorni rettilinei fatti di tratti lunghi (≥ 3 caselle) con gradini occasionali di una casella; nessuna
   sporgenza o buco di una casella. Tecnica: maschere su griglia grossa (celle di 4×4 caselle), ingrandite con
   bordi spostati a tratti di 0–1 casella, poi apertura/chiusura morfologica con elemento quadrato 3×3, poi
   rimozione di componenti piccole. Ogni livello sta dentro quello sotto.
2. **Meno alture, più grandi e leggibili.** 20–40 altopiani di livello 1 (non 80–130 puntini), alcuni con una
   terrazza di livello 2; alcune pareti cadono a picco nel mare (scogliere drammatiche), altrove una striscia di
   pianura separa parete e acqua.
3. **Colore per livello** (tileset): livello 0 = `Tilemap_color3` (smeraldo), livello 1 = `color1`
   (giallo-verde, il più luminoso e il più esteso), livello 2 = `color2` (verde), sottobosco degli abeti = `color5`
   (verde-acqua), qualche prato secco = `color4`. Sabbia (spiagge) solo su alcuni tratti di costa bassa e sentieri
   di terra battuta: colori proposti in `09-asset-inventory.md` §3.7.
4. **Scale/rampe in capo alle pareti**, mai a metà; 1–2 per altopiano (più su quelli grandi), con cespugli ai lati.
   Tutti gli altopiani raggiungibili (o marcati non edificabili se irraggiungibili, come oggi).
5. **Costa viva**: insenature e golfi di 1–2 caselle, isolotti (1×1, 3×1, 3×2) a 1–3 caselle dalla riva con un
   cespuglio, un sasso o niente; sassi nell'acqua (singoli e a coppie) vicino a riva e agli angoli delle scogliere;
   qualche stagno interno con ninfee, canne e una papera.
6. **Alberi a gruppi sui bordi**: ciuffi di 3–6 alberi che si sovrappongono sui bordi dei livelli, agli angoli e
   sulle punte della costa; boschi fitti di una sola specie su terreno `color5`; mai alberi sparsi a caso in mezzo
   ai prati. Specie raggruppate per zona.
7. **Cespugli e sassi come colla**: ai piedi delle pareti, accanto alle rampe, agli angoli degli edifici, sugli
   isolotti. Non riempiono i prati.
8. **Grandi spazi calmi**: ≥ 55–60 % di ogni livello libero da decorazioni (le unità devono muoversi e leggersi).
   Densità alta solo lungo i bordi (campionamento Poisson pesato sulla distanza dai bordi).
9. **Sentieri**: strade di terra battuta (1–2 caselle) che uniscono i castelli fra loro e ai punti da conquistare
   vicini, calcolate con A* a costo (evita salite e boschi, passa dalle rampe), poi lisciate. Una piazzetta di terra
   davanti a ogni castello. Fanno sembrare la mappa costruita, non casuale.
10. **Scorci con gli oggetti di scena** (`assets/Extra/Props`, 48 immagini): composizioni, non coriandoli —
    mulino + covoni + spaventapasseri + zucche vicino ai campi; rovine (arco, muro, colonne, statua, obelisco,
    cerchio di pietre) su altopiani; relitto, barca e pontile sulla riva; pozzo, carretto, botti, casse vicino ai
    villaggi; tenda + falò + legna agli accampamenti abbandonati; tombe e albero secco in un angolo cupo; funghi,
    felci, tronchi caduti, ceppi nei boschi; fiori (rossi, blu, bianchi, trifogli) a macchie nei prati.
11. **Tutti gli asset usati**: le 4 varianti di albero (per zona), i 4 cespugli, i 4 sassi, i 4 sassi nell'acqua,
    le nuvole (ombre di nuvole che passano), la paperella, le pecore, tutti i tileset 1–5.

## Requisiti di gioco (da non rompere)

Quelli di `02-world.md` §6: 288×192 caselle; 5 basi a ≥ 64 caselle con zona piana e libera; raggiungibilità;
scorte di partenza eque nelle stesse direzioni relative; regole sui livelli (pareti invalicabili, cambio di
livello solo dalle scale, edifici su un solo livello); oro sull'altopiano centrale e sui 15 successivi (adattare
a «i più vicini al centro» se gli altopiani sono meno); punti da conquistare, grandi giacimenti, forzieri; niente
di utile in punti irraggiungibili. Determinismo: stesso seme → stessa mappa (il client la rigenera dal seme).

## Strumenti di verifica

- `cargo run -p ts-sim --release --example mapgen_png -- --seed N --out map.png`: vista dall'alto colorata
  (livelli, pareti, rampe, sentieri, alberi, decorazioni come puntini) per controllare composizione e regole.
- Il client nativo: `cargo run -p ts-client -- --solo --seed N --cam X,Y --screenshot shot.png` per la resa vera.
- Test: per 50 semi la generazione riesce, è deterministica e rispetta gli invarianti; tempo < 300 ms in release.
