# Tiny Swords — La Guerra dei Cinque Regni

Strategico in tempo reale in stile Age of Empires, in JavaScript + Canvas 2D, senza dipendenze né build.
Cinque regni tutti contro tutti su un'isola di 288×192 celle; vince l'ultimo castello in piedi.

Usa tutti i 410 PNG di "Tiny Swords (Free Pack)" più 178 sprite aggiuntivi nello stesso stile (`assets/Extra/`).

## Avvio

Da soli: apri `index.html` nel browser (doppio clic).

In rete locale (fino a 5 giocatori):

    python3 server.py 8000 --bind 0.0.0.0     # valori predefiniti: basta "python3 server.py"

1. L'host apre `http://localhost:8000`, preme **MULTIGIOCATORE** e resta nella lobby.
2. Gli altri aprono l'indirizzo stampato dal server (es. `http://192.168.1.8:8000`) e premono **MULTIGIOCATORE**.
3. Ognuno sceglie il regno, l'host la difficoltà dell'IA e poi **INIZIA**. I regni liberi li guida l'IA.

`python3 -m http.server` serve solo file: va bene per giocare da soli, non per il multigiocatore,
perché non può inoltrare messaggi fra i browser. `server.py` fa entrambe le cose con la sola libreria standard.

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
| V · Tab · Backspace | sposta un edificio (clic, oppure frecce e Invio per sceglierlo) · seleziona tutte le truppe · richiamale davanti al castello |
| X · E · F · G | con una selezione: attacco in marcia · esplora (Esploratori) · ferma · punto di raccolta |
| Maiusc durante il piazzamento | continua a piazzare; con le palizzate, trascinando, ne posa una fila |
| Esc · P · Canc | annulla/deseleziona · pausa (in rete: menu, la partita continua) · demolisci |
| M · F1 · F2 · F3 | audio · aiuto · luci (complete, leggere, spente) · risoluzione del disegno (piena, 75%, 50%) |
| F4 | modalità test (solo da soli) |

Gli otto pulsanti sopra la minimappa fanno le stesse cose di U L I K / C V Tab Backspace. Le azioni di ciò che
hai selezionato (attacco in marcia, ferma, punto di raccolta, sposta, demolisci) sono nel pannello della selezione.

**Modalità test** (F4, non nelle partite in rete): una schermata con quattro pulsanti «+1000» (uno per risorsa, più
uno per tutte), «Ferma il tempo» e «Togli la nebbia». A tempo fermo la partita non avanza ma la visuale si muove e
gli ordini si danno lo stesso: partono quando il tempo riparte. Senza nebbia si vede tutta la mappa, nemici
compresi. Ciò che è attivo resta scritto in alto.

Con Ctrl premuto alcuni tasti restano del sistema e la pagina non può toglierglieli. Su Linux e Windows Ctrl+W
chiude la scheda: per questo, a partita in corso, prima di chiudere o ricaricare la pagina il browser chiede
conferma; lì per andare veloci verso l'alto senza quella domanda si usa Ctrl + freccia. Su macOS è il contrario:
Ctrl + frecce sono di Mission Control e del cambio di scrivania, mentre Ctrl + WASD è libero (la scheda la chiude Cmd+W).

- **Risorse**: legna (alberi), oro e pietra (giacimenti), cibo (pecore; le Fattorie ne allevano di nuove).
  I pedoni consegnano al Castello o a un Deposito (legna, oro, pietra) / Fattoria (cibo) vicino.
  Un pedone rende più o meno lo stesso su ogni risorsa; una Fattoria alleva abbastanza pecore per due o tre pedoni.
  Sotto ogni risorsa, in alto, c'è la **rendita attesa al minuto**: per ogni pedone al lavoro, quanto porta a ogni
  viaggio diviso la durata del giro (lavoro, andata e ritorno lungo la strada vera fino al punto di consegna, soste),
  più ciò che rendono i punti conquistati.
- **Posti**: i Pedoni stanno nelle case (10 dal castello, +5 per Casa, al massimo 30); le truppe negli accampamenti
  (6 dal castello, +8 per Accampamento, +2 per ogni edificio che addestra truppe, al massimo 50).
  Con più edifici dello stesso tipo ognuno addestra più in fretta: +20% per ogni edificio oltre il primo, fino al doppio.
- **Ere**: Villaggio → Feudale → dei Castelli, ricercate al Castello; sbloccano edifici, unità e ricerche.
- **Edifici**: Casa, Accampamento, Deposito, Fattoria, Caserma, Palizzata, Arcieria, Scuderia, Fucina, Mercato,
  Torre, Monastero, Officina d'assedio. Si scelgono dalla schermata Costruisci: se non hai pedoni selezionati ci va
  il pedone libero più vicino, o chi sta raccogliendo lì attorno (che poi torna alla sua raccolta).
  **Dove**: Torri e Palizzate solo entro 12 caselle dal castello; tutto il resto entro 40; attorno a un Avamposto
  conquistato, entro 9 caselle, di tutto. Mentre posi, i cerchi tratteggiati mostrano fin dove. Lo stesso vale
  per spostare un edificio (non il castello): è immediato e conserva vita e coda.
- **Palizzate**: affiancate formano lo steccato, una sopra l'altra una fila di pali; si orientano da sole.
  Le tue unità ci passano attraverso, i nemici devono abbatterle.
- **Fra i tuoi edifici**: le tue unità non restano mai chiuse fra edifici tuoi troppo vicini: se non c'è altra
  strada li attraversano. Quelle nemiche no.
- **Punti da conquistare** (una trentina, sparsi): Miniere, Cave, Segherie e Ovili rendono 30 al minuto della loro
  risorsa a chi li tiene, senza Pedoni; attorno agli Avamposti si può costruire. Un punto passa al regno che per
  8 secondi è il solo ad averci accanto delle truppe (i Pedoni non contano); poi resta suo finché un altro non lo prende.
  Sulla minimappa sono rombi del colore di chi li tiene.
- **Grandi giacimenti**: grandi vene d'oro e cave di pietra (1500 e 1800 di risorsa, fino a 8 Pedoni insieme)
  e boschi fitti, lontano dalle basi.
- **Unità**: Pedone, Esploratore, Guerriero, Lanciere, Arciere, Cavaliere, Monaco (cura), Ariete e Catapulta (assedio).
  A parità di spesa: i Lancieri battono i Cavalieri, i Cavalieri gli Arcieri, gli Arcieri i Guerrieri, i Guerrieri
  i Lancieri. Gli Arieti colpiscono solo edifici. Ogni Monaco ha una riserva di cura che si ricarica piano:
  rimette in piedi un esercito durante lo scontro, non lo rende immortale fra uno scontro e l'altro. I soldati
  che cercano un bersaglio preferiscono un Monaco vicino (conta come a 0.6 della sua distanza).
- **Esploratore** (dal Castello): veloce, vede lontano, non combatte, occupa i punti liberi. Con E esplora da
  solo: va ogni volta nella zona mai vista più vicina a piedi, finché ce ne sono.
- **Esercito in marcia**: un gruppo si dispone in formazione verso dove va: davanti chi combatte in mischia e
  gli Arieti, dietro gli Arcieri, poi le Catapulte, in fondo i Monaci. In marcia d'attacco tutti tengono il passo
  del più lento, così arrivano insieme. Gli Arcieri arretrano mentre ricaricano se un nemico da mischia gli arriva
  addosso; i Monaci curano da lontano quanto possono, si scansano dai soldati nemici e seguono i soldati con cui
  sono partiti invece di restare dove l'ordine li aveva portati.
- **Ricerche** (17), in un albero a tre livelli, uno per era (schermata Ricerche): ogni riga è l'edificio che
  le offre; alcune chiedono quella che le precede (Asce → Picconi, Scudi → Corazze, Erbe → Scritture).
- **Schermata Unità**: tutte le tue unità per tipo, con vita e attività. Il "+" tondo di ogni riga addestra quel
  tipo nell'edificio giusto; "−" e "+" quadrati spuntano una unità in meno o in più di quel tipo (oppure si clicca
  sulle caselle). Le unità spuntate si mandano a una risorsa (pedoni), si fermano, si richiamano, si selezionano
  sulla mappa oppure, con la spada **Invia**, si mandano dove si clicca sulla mappa in grande: ci vanno da sole.
- **Schermata Lavoratori**: un cursore per risorsa che conta i pedoni assegnati (anche quelli che in quel momento
  sono al riparo: a pericolo passato ci tornano) e la rendita di quella risorsa. Alzandolo parte il pedone
  libero più vicino a quella risorsa; abbassandolo i pedoni tolti restano liberi. I nuovi pedoni non vengono
  assegnati da soli. Un pedone al lavoro non resta mai disoccupato da solo: finita la risorsa in un punto ne
  cerca un'altra dello stesso tipo attorno al regno e, se non ce n'è, aspetta che torni. Da solo sceglie soltanto
  nel territorio già esplorato (un tuo clic destro diretto lo manda ovunque). Per il cibo va sempre alla pecora
  più vicina a dove si trova, e cambia bersaglio se strada facendo ne nasce una molto più vicina.
- **Pedoni al riparo**: un pedone che vede una truppa nemica (non pedoni, monaci o arieti) corre nella casa più
  vicina, 5 per casa; se non c'è posto, nel castello. Lì dentro non si vede e non può essere colpito; sull'edificio
  compare quanti ne ospita. Dopo 4 secondi senza nemici in vista esce e riprende il lavoro. Se la casa viene
  distrutta i pedoni escono. Un tuo ordine diretto lo fa uscire e non scappa più finché attorno non torna la calma.
  Vale anche per i pedoni dell'IA.
- **Truppe**: quelle ferme senza un tuo ordine accorrono da sole, ovunque siano, quando il regno è colpito
  (edifici, pedoni, o chiunque attorno al castello); chi era lontano resta poi di guardia al castello.
  Un attacco prosegue da solo: finito il bersaglio, le truppe passano al nemico più vicino nei dintorni.
- **Mercato**: vendi o compri legna, cibo e pietra in cambio d'oro; ogni scambio peggiora il prezzo, che poi risale.
- **Nebbia di guerra**: vedi solo attorno a unità ed edifici tuoi; il resto resta come l'hai visto l'ultima volta.
- **Luci**: un giorno dura 8 minuti di gioco, con alba, ora dorata, tramonto e notte. Le ombre degli sprite
  girano col sole; di notte edifici e falò fanno luce. Bagliore, sfocatura ai bordi e vignettatura danno
  l'aspetto da diorama. F2 passa da complete a leggere a spente; se il gioco rallenta si alleggeriscono da sole.
- **Risoluzione**: la tela ha un pixel per pixel dello schermo. Se il gioco resta lento anche a luci spente
  (schermi molto densi, schede lente) la risoluzione del disegno scende da sola al 75% e poi al 50%; F3 la cambia a mano.
- **Mappa**: diversa a ogni partita, quattro volte più estesa di prima. Cambiano la forma dell'isola, golfi e
  laghi e il posto dei cinque castelli (mai a meno di 64 caselle l'uno dall'altro). Ogni base parte con legna, oro,
  pietra e un prato di pecore dentro la zona già esplorata. Sparsi in giro: punti da conquistare, grandi giacimenti,
  accampamenti abbandonati e forzieri, rovine, menhir, mulini, relitti e pontili lungo la riva, e molti più
  dettagli a terra (fiori, felci, sassi, ceppi, tronchi, zucche).
- **Rilievi**: un'ottantina di alture dal contorno irregolare, con la parete di roccia sul lato sud (anche a picco
  sull'acqua) e le scale per salirci; sulle più grandi c'è una terrazza di secondo livello. Si sale e si scende
  solo dalle scale, si può costruire anche in alto. L'altura più vicina al centro fra i regni ha il triplo
  dell'oro; oro e pietra anche sulle quindici successive, alberi su tutte. In mare, isolotti e scogli non raggiungibili.
- **IA**: tre difficoltà (Facile, Normale, Difficile) che cambiano economia, dimensione dell'esercito e ritmo degli
  attacchi. Costruisce case e accampamenti quando servono; a Normale e Difficile manda squadre a occupare i punti vicini.

## Multigiocatore: come funziona

- Tutti contro tutti: ogni giocatore guida un regno, le posizioni di partenza sono a sorte.
- La simulazione gira nel browser dell'host, che trasmette lo stato 20 volte al secondo; i client inviano
  solo comandi (ordini, costruzioni, code). Selezione, visuale e nebbia sono locali. `server.py` è un relay WebSocket (`/ws`).
- Chi viene eliminato può restare a guardare. Se un giocatore esce, il suo regno passa all'IA.
- Se esce l'host la partita finisce per tutti. La scheda dell'host può stare in secondo piano, ma non va chiusa.
- Non si entra a partita iniziata.

## Sprite aggiuntivi

`assets/Extra/` contiene 178 PNG creati per questo gioco nello stile del pacchetto: sette edifici per colore
(Fucina, Mercato, Scuderia, Officina, Deposito, Fattoria, Accampamento), la Palizzata, i cinque punti da
conquistare, Cavaliere, Ariete, Catapulta ed Esploratore animati per i cinque regni, il Pedone con la pietra,
cave di pietra e grandi giacimenti, 48 oggetti di scena e 28 icone.

Le immagini di partenza (`tools/raw/`) sono state generate con modelli di immagini (quale per ciascuna: `tools/raw/SOURCES.md`); `tools/make_sprites.py`
(Python con Pillow e numpy) le ritaglia, le riduce, le porta sulla tavolozza del pacchetto, ricolora le parti
di squadra nei cinque colori e scrive i PNG, `js/extra.js` e `js/manifest.js`. Per rigenerarli:

    python3 tools/make_sprites.py

Il font dell'interfaccia è Fredoka (`fonts/`, licenza OFL in `fonts/OFL.txt`).

## Codice

| File | Contenuto |
|---|---|
| `server.py` | file statici + relay WebSocket per il multigiocatore |
| `js/manifest.js` | elenco dei PNG in `assets/` (generato) |
| `js/extra.js` | misure e ancoraggi degli sprite aggiuntivi (generato) |
| `js/assets.js` | caricamento, definizioni di unità/edifici/ricerche, 9-slice e primitive di disegno |
| `js/world.js` | generazione dell'isola, tile, campi di distanza per il movimento |
| `js/entities.js` | unità e ordini, combattimento, raccolta, edifici, proiettili, effetti |
| `js/factions.js` | economia, ricerche, comandi dei giocatori, IA |
| `js/control.js` | selezione, ordini col mouse, azioni di contesto, piazzamento, telecamera, nebbia di guerra |
| `js/ui.js` | menu, lobby, HUD, pannelli |
| `js/panels.js` | schermate di gestione (Unità, Invia, Lavoratori, Ricerche, Mercato, Costruisci) e pulsanti sopra la minimappa |
| `js/light.js` | luci: ciclo giorno/notte, ombre, luci puntiformi, passata finale |
| `js/net.js` | rete: lobby, istantanee dell'host, stato replicato sui client |
| `js/main.js` | input, ciclo di gioco, rendering |
| `js/sfx.js` | effetti sonori sintetizzati (il pacchetto non contiene audio) |

## Bilanciamento con i bot

`tools/balance/` fa girare il gioco vero in Node, senza browser e senza disegno, per far giocare molti bot in
parallelo e misurare il bilanciamento. Serve solo Node; non è parte del gioco.

| File | Contenuto |
|---|---|
| `headless.js` | carica gli script di `js/` in un contesto con DOM finto; accetta numeri alternativi da provare |
| `sim.js` | partite fra bot e IA, scontri a parità di spesa, misura delle rendite, statistiche |
| `bots.js` | bot che giocano con i comandi del giocatore: una strategia per nome (`knightMonk`, `warriors`, `mixed`...) |
| `run.js` | esegue molte partite in parallelo, un processo per core |
| `measure.js` | le misure in tabella |
| `trace.js` | un bot seguito minuto per minuto |

    node tools/balance/measure.js arena     # scontri fra tipi di unità a parità di spesa, valore dei monaci
    node tools/balance/measure.js eco       # risorse per pedone al minuto
    node tools/balance/measure.js duels     # strategia contro strategia
    node tools/balance/measure.js vsai      # ogni strategia contro quattro regni dell'IA  (-l easy|normal|hard)
    node tools/balance/measure.js ai        # cinque regni dell'IA fra loro
    node tools/balance/trace.js knightMonk  # cosa fa un bot, minuto per minuto

Opzioni: `-n` partite, `-w` processi, `-s` strategie, `-o file.json` numeri da provare senza toccare i sorgenti
(`{ "consts": { "FARM_BREED": 12 }, "set": { "UNIT_DEF.knight.hp": 170 } }`).
