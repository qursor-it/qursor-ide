# Il cuore delle sessioni fuori dall'app (`qursor-core`) — design

Data: 2026-11-26. Stato: approccio scelto dal proprietario (A); design in revisione.
Fase 1 di due: questa porta il cuore in un processo locale; la fase 2 (cloud) mette
lo stesso binario su un VPS.

## Intento

Oggi tutto ciò che fa girare una sessione (PTY, stato dal titolo, adozione dell'id,
attenzione, bridge IDE, server MCP degli agenti) vive dentro il processo Tauri e
muore con lui (ADR-0005). Lo spostiamo in un processo separato, `qursor-core`, che:

- **sopravvive all'app** quando il proprietario lo sceglie alla chiusura, a un crash
  dell'app e al suo aggiornamento;
- è l'unico proprietario dei PTY, e l'app (renderer) ne è un client via WebSocket;
- parla già il protocollo che useranno il telefono e, in fase 2, l'app verso un VPS.

### Perché

Il proprietario vuole sessioni che non dipendono dalla finestra aperta, il telefono
come secondo client e, più avanti, le stesse sessioni su un server sempre acceso.
Un solo binario e un solo protocollo per "locale" e "cloud": cambia solo l'indirizzo
dell'host. Riferimento esplicito: l'architettura daemon/client di Paseo
(github.com/getpaseo/paseo, Apache-2.0). **Regola del proprietario: nei dubbi, si fa
come Paseo.**

### Requisito trasversale: prestazioni

Deve essere ottimizzato e performante: il percorso tasto→eco e il throughput
dell'output non devono peggiorare rispetto a oggi, misurati (vedi "Budget").

## Obiettivi

1. **Processo separato.** Un binario `qursor-core` possiede tutti i terminali:
   `TerminalKind::Agent`, `Shell` e `Ssh` (con connessioni, tunnel e vault). L'app
   non tiene più `TerminalManager`.
2. **Scelta alla chiusura (B).** Chiudendo l'app con sessioni vive, il dialogo chiede
   **"Lasciale girare"** o **"Ferma tutto"** (più Annulla). Con "Lasciale girare"
   l'app esce e il core resta, con un'icona nella tray che dice quante sessioni sono
   accese.
3. **Riaggancio.** Riaprendo l'app, ogni sessione viva ricompare nella sua tab con lo
   schermo com'era (snapshot) e prosegue dal vivo, senza buchi né doppioni.
4. **Più client.** Più finestre o client possono agganciarsi allo stesso terminale.
5. **Percorso dati più corto di oggi.** L'output arriva a xterm come frame WebSocket
   binari, senza base64 e senza eventi Tauri.
6. **Pronto per la fase 2.** Il protocollo non presuppone filesystem condiviso né
   loopback; le sessioni hanno un campo `host` (oggi sempre `local`).

## Non-obiettivi (fase 1)

- Telefono, relay, pairing QR, push: dopo. Qui solo il protocollo che li permetterà.
- VPS / cloud sessions: fase 2.
- Spostare nel core indicizzatore, ricerca, file, git, usage, intelligence: restano
  nell'app (leggono il disco locale; in fase 2 passeranno dal core host per host).
- Routine: lo scheduler resta nell'app e scatta solo con l'app aperta, come oggi.
- Notifiche OS con l'app chiusa: il core decide l'attenzione, l'app la mostra. Con
  l'app chiusa non si vede nulla (lo risolverà la push della fase mobile).
- Separare ulteriormente i PTY in un "terminal worker" che sopravvive al core
  (approccio C): si può staccare dopo senza rifare nulla.

## Anti-obiettivi

- **Agenti dimenticati.** Il motivo dell'ADR-0005 resta valido: nessuna sessione
  sopravvive all'app senza che il proprietario l'abbia scelto in quel momento, e
  quando succede la tray lo dice sempre.
- **Più lento di oggi**, anche solo nel tasto→eco.
- **Processi orfani** quando il core muore o viene fermato: "Ferma tutto" e la
  terminazione del core uccidono sempre l'albero dei processi (job object su
  Windows, process group su Unix), come oggi.

## Vincoli

- ADR da superare o toccare: **ADR-0005** e Q10 in `specs/07-open-questions.md`
  (superati), **ADR-0020** (il dialogo cambia trigger e pulsanti), ADR-0111 invariato.
  Nuovo ADR: il prossimo libero (oggi ADR-0127).
- `specs/10-performance.md` P1: **nessuna modifica può distruggere, staccare o
  ricreare un xterm del pool.** Il riaggancio crea gli xterm del pool all'avvio
  come oggi; non li ricrea mai.
- AGENTS.md: niente `unwrap()` fuori da `setup()`, `anyhow` dentro / `thiserror` al
  confine, tipi cross-boundary scritti a mano in `packages/types` (niente codegen,
  quindi niente zod come in Paseo), Windows nativo x64 supportato (ADR-0074).
- Build dev con identificatore suo (ADR-0024): ha la sua cartella dati, quindi il
  suo core. Un core per cartella dati.

## Approcci considerati

| | Approccio | Pro | Contro |
|---|---|---|---|
| **A (scelto)** | Binario `qursor-core` separato; il renderer si collega **direttamente** via WebSocket su 127.0.0.1 con token | Percorso dati più corto di oggi (niente base64 né IPC Tauri); stesso protocollo di telefono e VPS | Il renderer cambia di più (`Terminal.tsx`, store) |
| B | Core separato, l'app Tauri fa da ponte su named pipe / socket Unix | Renderer quasi invariato | Un salto in più, resta il base64, il telefono vorrà comunque un secondo protocollo |
| C | Core + terminal worker separato (come Paseo) | I PTY sopravvivono anche al riavvio del core | Due processi e due protocolli; rimandato, si stacca da A |

Il proprietario ha scelto A.

## Design

### 1. Processo e ciclo di vita

- **Crate.** Nuovo membro del workspace Cargo, `crates/qursor-core` (lib + bin). Ci
  si spostano `services/terminal.rs` (+ `terminal/agent_use.rs`), `child_env.rs`,
  `osc_title.rs`, `term_capture.rs`, la logica di decisione di `attention.rs`, il
  bridge IDE (`services/ide/*`) e il server MCP degli agenti
  (`agent_tools/*`, deleghe incluse), e tutto SSH (`services/ssh/*`: pool, pty,
  tunnel, sftp, vault) con i comandi dei server. L'app non dipende più da questi moduli; i tipi
  del protocollo stanno nel crate core (lato Rust) e in `packages/types` (lato TS).
  Il layout esatto dei moduli è del builder.
- **Record.** All'avvio il core scrive `<data_dir>/core/core.json`, solo per il
  proprietario (0600 su Unix; su Windows la cartella dati utente è già per utente):
  `pid`, `protocolVersion`, `appVersion`, `port`, `secret` (32 byte casuali),
  `binaryPath`. Un lock di istanza unica per cartella dati.
- **Avvio.** L'app, all'avvio, legge il record: se il core risponde (connessione +
  `hello` con il segreto), lo usa; altrimenti ne avvia uno. Il core parte
  **staccato** (non figlio dell'app: su Windows `DETACHED_PROCESS` +
  `CREATE_NEW_PROCESS_GROUP` e fuori dal job object dell'app; su Unix `setsid`), così
  un crash o un kill dell'app non lo trascina via.
- **Copia per versione (come Paseo).** Il core non gira dalla cartella di
  installazione: l'app copia il binario in `<data_dir>/core/bin/<version>-<hash>/` e
  lo lancia da lì. Così l'installer di Windows può sostituire l'app mentre il core
  gira. Una copia si cancella quando nessun record la nomina.
- **Uscita da solo.** Il core esce quando non ha né PTY né client connessi da 30 s
  (valore del builder), e su `shutdown`. Un core lasciato acceso con zero sessioni
  non resta in giro.
- **Kill.** `shutdown { keepTerminals: false }` = comportamento di oggi di
  `kill_all` (deleghe prima, 500 ms di grazia, albero dei processi). Il backstop
  `Drop` resta nel core. Se il core muore, i suoi PTY muoiono con lui (job object /
  process group): niente orfani.

### 2. Chiusura dell'app (scelta B)

- Il dialogo compare quando c'è **almeno una sessione agente viva** (non solo
  "che lavora", come in ADR-0020: ora la domanda è "tenerle o fermarle", che ha
  senso anche per sessioni ferme al prompt). Pulsanti: **Lasciale girare**,
  **Ferma tutto**, Annulla. Una casella "Ricorda la scelta" la rende predefinita;
  si cambia dalle impostazioni. (**Mia scelta**, da confermare: senza la casella il
  dialogo comparirebbe a ogni chiusura.)
- Senza sessioni agente vive: nessun dialogo; le shell del footer vengono chiuse
  (`shutdown` del core se resta vuoto).
- **Lasciale girare:** l'app si scollega ed esce. Le shell del footer seguono la
  stessa scelta (restano anche loro).
- **Ferma tutto:** `shutdown { keepTerminals: false }`, poi uscita.
- Il riavvio dell'updater usa lo stesso dialogo.
- **Crash o kill dell'app** (nessun dialogo): il core tiene le sessioni e mostra la
  tray. Non si perde lavoro e le sessioni restano visibili (**mia scelta**, da
  confermare insieme alla casella "Ricorda la scelta").
- **Tray nel core** (feature Cargo `tray`, attiva solo nella build desktop, spenta
  per il VPS): icona con "N sessioni accese", "Apri qursor", "Ferma tutto ed esci".
  La crate `tray-icon` è già nell'albero tramite Tauri. La tray compare solo
  quando nessuna app è connessa. (**Mia scelta**: Paseo non ha tray nel daemon, ma
  l'anti-obiettivo "agenti dimenticati" la richiede.)

### 3. Protocollo

WebSocket su `127.0.0.1:<porta casuale>`, una connessione per client. Ripreso da
Paseo (`docs/architecture.md` § WebSocket protocol), adattato:

- **Handshake.** Il client manda `hello { secret, clientId, clientType, protocolVersion,
  appVersion, capabilities }`; il core risponde `server_info { hostId, version,
  protocolVersion, features }`. Qualsiasi altro primo frame chiude la connessione.
  Segreto confrontato in tempo costante (pattern già in `agent_tools/server.rs`).
  Header `Origin` ammesso solo per l'origine della webview Tauri e per client
  senza Origin (CLI, futuro).
- **Il renderer ottiene endpoint e segreto** da un comando Tauri (`core_endpoint`);
  il segreto non va mai in localStorage né in log.
- **Testo JSON per il controllo:** RPC con `requestId` e suffissi
  `.request`/`.response`, errori `rpc_error` (convenzione Paseo `rpc-namespacing`).
  Eventi push: stato terminale, uscita, adozione sessione, worktree, deleghe,
  attenzione. Nomi esatti del builder, equivalenti agli eventi Tauri di oggi
  (`terminal:status`, `terminal:exit`, `session:adopted`, `session:worktree`,
  `delegations:changed`, …).
- **Binario per il terminale, in entrambi i sensi:** `opcode(u8) | slot(u16 BE) | payload`.
  Opcode: `Output`, `Input`, `Resize`, `SnapshotBegin`, `SnapshotData`,
  `SnapshotEnd`. Lo slot è assegnato dal core alla sottoscrizione (Paseo usa u8;
  u16 perché si possono avere più di 255 terminali). Anche l'input è binario: niente
  JSON per ogni tasto.
- **Vivacità:** ping/pong JSON ogni 10 s, socket chiuso dopo due finestre senza
  frame in arrivo (regola Paseo, serve già per il telefono).
- **Compatibilità:** schemi solo in aggiunta; i nuovi valori enum si mandano solo ai
  client che dichiarano la capability (regola Paseo).
- **Resize:** vince l'ultimo client che ha interagito davvero; guardare non conta.

### 4. Percorso dati e prestazioni

- **Un solo buffer, nel core.** Thread lettore → coalescer → frame binario a ogni
  client sottoscritto. Finestre di 16 ms (in primo piano) e 100 ms (in background)
  come oggi; il "background" ora lo dice il client con un messaggio di visibilità
  per terminale (sostituisce `UiState::is_backgrounded`). Più client: vale la
  finestra più corta tra quelli che lo guardano.
- **Il PTY non aspetta mai un client lento.** Coda per client: oltre 4 MiB si
  smette di mandargli output dal vivo per quel terminale e lo si rimette in pari con
  uno snapshot; oltre 8 MiB la connessione si chiude (soglie Paseo).
- **Stato dello schermo nel core.** Ogni PTY alimenta un emulatore senza schermo
  che tiene schermo + 10.000 righe di storia, per mandare lo snapshot a chi si
  aggancia (Paseo usa xterm headless + serialize). In Rust la scelta tra
  `alacritty_terminal` e `vt100` è del builder, **decisa con un benchmark** su
  throughput e memoria per terminale. Lo snapshot deve riprodurre cursore, modalità
  mouse e input (Paseo lo documenta come trappola: `terminal-worker.md` § Display
  replay).
- **Niente buchi né doppioni:** ogni output porta un numero di revisione; lo
  snapshot dichiara la revisione a cui è tagliato e il client scarta l'output dal
  vivo già incluso (come Paseo). Snapshot a blocchi da 64 KiB.
- **Renderer:** l'xterm del pool riceve `Uint8Array` direttamente dal socket.
  Una sola sorgente per tutti i terminali (agenti, footer, SSH); gli eventi Tauri
  `terminal:*` spariscono. Il client
  WebSocket è pensato per **più host** fin da subito (più core contemporanei: vedi §6,
  e il VPS in fase 2).

### 5. Dati condivisi con l'app

- Il core apre lo **stesso `factorai.db`** (WAL, sicuro tra processi) per ciò che il
  codice spostato già scrive: adozione delle sessioni, strumenti routine e canvas
  degli agenti. `busy_timeout` impostato. **Solo l'app esegue le migrazioni**; il
  core controlla che il registro `migration:*` di `_meta` coincida con le migrazioni che conosce e, se no, rifiuta di partire e lo
  dice nel log (l'app ne avvia uno della sua versione, §6).
- Le callback che oggi l'app passa a `TerminalManager` (in `lib.rs`: store delle
  routine, store dei canvas, percorso del progetto) diventano funzioni del core sul DB.
- Il lockfile del bridge IDE (`~/.claude/ide/<port>.lock`) lo scrive il core: il
  bridge vive quanto la sessione.

### 5a. SSH nel core (scelta del proprietario)

- Pool `russh`, PTY SSH, tunnel, SFTP, tmux remoto (ADR-0111) e vault
  (ADR-0106) passano nel core. Il terminale prestato all'agente (ADR-0109/0110)
  resta tutto dentro il core: con "Lasciale girare" l'agente non perde il terminale.
- Le tab SSH seguono la stessa scelta di chiusura delle altre; il tmux remoto
  continua a valere in più (sopravvive anche al core).
- **Vault.** Senza master password il core legge la chiave dal portachiavi del
  sistema, come oggi l'app. Con master password lo sblocco lo chiede la UI e la
  password va dal renderer al core sulla connessione autenticata, una volta, mai in
  log né in eventi. **Il vault si richiude quando l'ultima app si scollega**: le
  connessioni già aperte restano, ma nuove connessioni o riconnessioni senza app
  aspettano lo sblocco. Il resto di ADR-0106 vale invariato: nessun segreto va
  dal core al renderer. Il nuovo ADR lo annota come modifica di ADR-0106.

### 5b. Quando nessuna app è connessa

Alcuni flussi oggi finiscono nel renderer. Con "Lasciale girare" il core li gestisce
così (**mie scelte**, coerenti con "l'umano conferma", ADR-0119):

- **Avvio di sessioni e deleghe tra agenti** (`agent-session:start`, conferma
  `ConfirmAgentDelegations`): la richiesta resta **in coda nel core** e la chiamata
  dello strumento dell'agente aspetta, come oggi quando il proprietario non guarda lo
  schermo. Al primo client che si connette il core la ripropone e il renderer
  prosegue come oggi (scelta del profilo, worktree), chiedendo poi al core di
  avviare il PTY. Se la conferma è disattivata (ADR-0121) vale lo stesso: senza
  renderer non si sceglie profilo né worktree, quindi si aspetta il client.
  L'orchestrazione dell'avvio resta nel renderer in fase 1.
- **Conferme di policy sui terminali prestati** (ADR-0110): in coda, come sopra.
- **Bridge IDE `openFile` / `getOpenEditors`:** senza client rispondono con un errore
  chiaro ("nessun editor connesso"). `openFile` viaggia come "apri questo percorso
  sull'host X": il client lo apre se è suo. Non viola la regola 7.1, che vieta di
  presupporre il disco condiviso, non di nominare percorsi dell'host.
- **Strumento usage (`getUsageLimits`):** il core legge `usage.json` e riporta
  l'istante dell'ultimo aggiornamento; il polling resta nell'app.

### 6. Versioni e aggiornamenti (come Paseo)

- L'app usa il core che trova se ha il suo stesso `protocolVersion`.
- Core con protocollo diverso **e sessioni vive:** resta acceso a svuotarsi (le sue
  sessioni continuano, non ne accetta di nuove); l'app ne avvia uno nuovo per le
  sessioni nuove. Al massimo due core per cartella dati. Il vecchio esce quando
  finisce l'ultima sessione. L'app mostra le sessioni di entrambi.
- Core diverso **senza sessioni:** l'app lo ferma e avvia il suo.
- Verso il core vecchio l'app tiene un adattatore della versione precedente con le
  sole operazioni sulle sessioni esistenti: elenco, aggancio con snapshot, input,
  resize, stop. Niente avvio. La prima release non ne ha bisogno (**mia scelta**).
- **Migrazioni del DB compatibili per una versione:** mentre un core vecchio si
  svuota, il nuovo app migra `factorai.db`. Regola nel nuovo ADR: ogni migrazione
  è solo additiva (colonne/tabelle nuove con default), così un core di una versione
  prima continua a scrivere. Una migrazione distruttiva richiede che nessun core
  vecchio sia vivo (l'app lo verifica dai record). (**Mia scelta**, come lo schema
  "solo in aggiunta" del protocollo di Paseo.)
- **Più finestre:** in fase 1 il multi-client è provato a livello di protocollo e
  nel test di integrazione; nessuna seconda finestra nella UI (**mia scelta**).

### 7. Regole per la fase 2 (da rispettare già ora)

1. Niente nel protocollo presuppone che client e core condividano il disco: i
   percorsi viaggiano come stringhe relative all'host, e il core non manda mai "apri
   questo file locale" al client.
2. Ogni sessione ha `host` (oggi `local`) nel modello dati e nei tipi.
3. Autenticazione sempre, anche in loopback.
4. Il client supporta più host contemporanei.

(Nota: avevo proposto che già in fase 1 l'app leggesse file e git tramite il core.
Lo spostiamo in fase 2: in fase 1 restano locali per non allargare il lavoro; la
regola 1 basta perché il protocollo non vada rifatto.)

## Budget (da misurare prima e dopo, in `specs/10-performance.md`)

Il builder misura prima i valori di oggi, poi:

- **Tasto→eco:** p95 non peggiore di oggi di più di 1 ms (stesso host).
- **Throughput:** `cat` di un file da 50 MB in una sessione non più lento di oggi.
- **Riaggancio:** snapshot di un terminale con 10.000 righe visibile in meno di 100 ms.
- **Core a riposo:** CPU ~0; memoria per terminale con storia piena misurata e
  scritta.
- **Avvio app:** nessun peggioramento visibile per l'attesa del core (l'app non
  blocca la finestra mentre il core parte).

## Cosa produce il lavoro oltre al codice

- Nuovo ADR "le sessioni girano in un core fuori dall'app" che supera ADR-0005 (e
  Q10) e modifica ADR-0020.
- Spec aggiornati: `01-architecture.md`, `03-backend-rust.md`, `05-features.md`
  (quit guard, tray), `10-performance.md` (budget e misure).
- Test: framing binario e handshake (unità); integrazione che avvia il core, apre un
  PTY con eco, si scollega, si riaggancia e verifica snapshot + revisioni; il mock
  bridge degli smoke test (`tests/smoke`) impara a simulare il core.

## Lasciato al builder

- Layout dei moduli nel crate core e ordine dell'estrazione.
- `alacritty_terminal` o `vt100`, deciso col benchmark.
- Nomi esatti dei messaggi, purché equivalenti agli eventi di oggi.
- Timeout di uscita a vuoto del core.
- Come il mock degli smoke test simula il socket.
