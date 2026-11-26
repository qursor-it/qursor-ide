# qx — una distro Linux minimale per lo sviluppo agentico — design

Data: 2026-11-21. Stato: direzione approvata dal proprietario (approccio B); design in revisione.
Nome di lavoro: `qx` (provvisorio).

## Intento

Una distro Linux minimale che esiste solo per sviluppare software con agenti.
Gira senza schermo, su una macchina di casa o su un VPS remoto, e la si usa via
SSH. L'interfaccia è una TUI nello spirito di qursor-ide: al centro le tab con
le sessioni degli agenti che stanno girando. Gli agenti lavorano sui progetti e
possono interagire anche con la distro stessa, attraverso strumenti che il
sistema espone; i sandbox (isx) sono uno di questi strumenti, non la base di
tutto.

### Perché

Il proprietario vuole spostare l'ambiente di sviluppo dal PC a macchine
dedicate, che restano accese, lavorano con il portatile chiuso e sono uguali tra
loro. Il criterio di successo è concreto: **tutto lo sviluppo di qursor-ide
passa su questi VPS con questa distro.**

### Anti-obiettivo

Che sia **scomodo da usare**. Se lavorare sul VPS costa più attrito di qursor sul
PC, la distro ha fallito anche se tecnicamente funziona. "Comodo" è definito più
sotto come requisiti verificabili.

## Obiettivi

1. **Sessioni in tab.** `ssh <host>` porta dritto nella TUI con le sessioni aperte,
   ognuna in una tab con il terminale vivo dell'agente e il suo stato
   (lavora / inattiva / ti aspetta / finita). Una riga sempre visibile dice chi ti
   aspetta, anche se sei su un'altra tab. Ogni tab mostra agente, progetto,
   profilo e dove gira (`host` o `sandbox: <nome>`).
2. **Le sessioni sopravvivono alla disconnessione.** Staccarsi o perdere la
   connessione non ferma nulla; riconnettersi riporta esattamente dove si era.
   Più client possono essere agganciati insieme.
3. **Più abbonamenti, come qursor.** Profili per agente (Claude, Codex), ognuno
   una cartella di configurazione; un profilo per progetto; login via SSH.
4. **Gli agenti interagiscono con la distro** tramite strumenti di sistema (CLI +
   MCP) con livelli di permesso; le azioni distruttive chiedono conferma
   all'umano nella TUI.
5. **Sandbox a disposizione.** isx è uno strumento: il progetto ha un default
   (host o sandbox), l'agente può chiedere più isolamento da solo, tornare
   sull'host richiede conferma.
6. **Si sviluppa qursor-ide sulla distro**: toolchain completa, display virtuale
   per la verifica nella finestra vera, build parallele sostenibili.
7. **Una macchina nuova è pronta in pochi minuti**, identica alle altre.

## Non-obiettivi (prima versione)

- Vista di review, diff, file viewer, git graph, editor: il lavoro finito lo
  gestisce l'agente con git/gh (commit, push, PR); la CI è il cancello.
- Modelli locali: non si costruiscono ora, ma l'architettura lascia il posto
  (vedi "Modelli locali, dopo").
- Desktop grafico, uso con schermo e tastiera locali.
- Build Windows/macOS di qursor sulla distro: restano alla CI.
- Più utenti umani sulla stessa macchina.
- Più macchine viste insieme (scelta del designer): ogni macchina è
  indipendente, con i suoi login, progetti e sessioni. Una vista unica su più
  host o la sincronizzazione vengono dopo.

## Requisiti di comodità (verificabili)

- **Un comando per entrare**: `ssh <host>` apre la TUI agganciata, senza comandi
  da ricordare.
- **Connessione che cade non costa nulla**: riaggancio automatico; mosh
  supportato.
- **Digitazione senza ritardo percepibile**, anche su un VPS lontano.
- **Copia-incolla** tra terminale remoto e PC (OSC 52).
- **Notifica quando un agente ti aspetta anche da scollegato** (telefono o PC).
- **Immagini visibili**: gli screenshot degli agenti si aprono nel terminale
  (protocollo grafico kitty/sixel, dove il terminale lo supporta) o tramite link.
- **Scorciatoie rimappabili** (come F28 di qursor), per non scontrarsi con il
  terminale del client.
- **Macchina nuova in pochi minuti** (vedi Installazione).

## Vincoli

- Headless, accesso solo SSH; nessuna porta pubblica oltre SSH (meglio dietro
  WireGuard/Tailscale).
- Molti provider VPS non permettono di avviare una ISO propria: l'installazione
  deve poter convertire una Fedora già installata.
- Sviluppare qursor-ide richiede Rust, Node/pnpm, mise, gh, le librerie Linux di
  Tauri (`libwebkit2gtk-4.1-dev` & co., vedi `.github/workflows/quality.yml`), i
  browser di Playwright, e per la verifica nella finestra vera un display X11:
  `scripts/qa/` oggi presuppone X11 + GNOME (`wmctrl`, `gnome-screenshot`,
  `xdotool`).
- Build Rust/Tauri parallele in più worktree: molta CPU/RAM e diversi GB di
  `target/` per worktree.
- isx (github.com/Sanne/incus-spawn) è Java/Quarkus con binari nativi; si integra
  dall'esterno (CLI `--format=json`, `isx mcp`), non a livello di codice.
- Principio di qursor che resta: l'umano supervisiona, decide e fissa le regole;
  ogni azione irreversibile mantiene la sua conferma.

## Approcci considerati

La parte comune è fissa in tutti: base Fedora bootc, demone di sistema `qx` in
Rust. Il punto di scelta era come costruire la TUI con le tab delle sessioni vive.

- **A. TUI da zero** (Rust + ratatui + emulatore di terminale come `vt100` o
  `alacritty_terminal`, PTY posseduti dal demone, TUI via socket). Controllo
  totale e un solo progetto coerente, ma emulare un terminale dentro un terminale
  (riaggancio, copia-incolla, mouse, resize) è il lavoro più lungo e fragile: mesi
  prima di eguagliare la comodità di tmux.
- **B. Sopra zellij** (multiplexer Rust con plugin WASM). Ogni sessione è una tab
  zellij; barra di stato, riga "chi ti aspetta" e creazione sessioni sono plugin
  nostri che parlano con `qx`. Riaggancio, emulazione, copia-incolla, layout e
  scorciatoie sono già maturi: comodità dal primo giorno. Rischio: i limiti
  dell'API dei plugin.
- **Scartato: tmux + script.** Costa poco ma è "Debian con tmux", proprio ciò che
  la distro deve battere.

**Scelto: B**, dal proprietario, con una prova usa e getta prima di impegnarsi
(Fase 0). Se l'API dei plugin non regge, si ripiega su A riusando il demone.

Base: considerata anche Arch (modello Omarchy: base esistente + repo di pacchetti
+ CLI + migrazioni + skill per agenti). Si prende la filosofia di Omarchy ma non
la base: per un server remoto, dove una rottura costa la console del provider,
serve una base stabile e immutabile con rollback. Scelta: Fedora bootc (anche i
template di isx sono Fedora).

## Architettura

```
 client (terminale del PC/telefono)
   │ ssh / mosh
   ▼
 zellij (sessione "qx", layout qx)  ◄── plugin qx (WASM): barra sessioni,
   │ tab = PTY dell'agente               riga "ti aspetta", nuova sessione,
   │                                     conferme dei permessi
   ▼
 qxd (demone Rust, systemd --user)  ── socket locale ── CLI `qx`
   │  stato sessioni, profili, progetti, permessi, log azioni
   │  server MCP "sistema" per gli agenti
   ├── git / gh
   ├── isx (sandbox, account, proxy credenziali)
   ├── pacchetti e toolchain (mise, layer bootc)
   ├── display virtuale (Xvfb + WM leggero) per QA grafico
   └── [dopo] runtime modelli locali
```

### Base e installazione

- Immagine Fedora bootc costruita da un `Containerfile` versionato: base minimale
  + Incus + btrfs + isx + zellij + qxd/qx + toolchain di sviluppo comune.
- Installazione: su VPS, `bootc install to-existing-root` sopra una Fedora del
  provider; a casa, ISO generata dalla stessa immagine.
- Aggiornamenti: `qx update` = nuova immagine bootc (atomica), rollback al riavvio
  precedente. Snapshot prima di ogni aggiornamento, nello spirito di Omarchy.
- Strumenti per progetto (versioni di linguaggio) via mise nella home, non
  nell'immagine.

### qxd e la CLI `qx`

- Possiede l'elenco dei progetti (cartelle in `~/projects`, cloni git), dei
  profili e delle sessioni (id, agente, profilo, progetto, dove gira, stato).
- Riusa i servizi di qursor-ide che non dipendono da Tauri, estratti in un crate
  condiviso: `git.rs`, `indexer.rs`, `jsonl.rs`, `search.rs`, `agent_cli.rs`,
  `routines.rs`, `osc_title.rs` (in `apps/desktop/src-tauri/src/services`; 19 file
  su 44 dipendono da Tauri, gli altri sono candidati).
- Stato delle sessioni come F10 di qursor (titolo OSC dell'agente + transcript).
- `qx` è la stessa interfaccia per l'umano e per gli agenti (filosofia Omarchy:
  una CLI unica per cambiare il sistema).

### Sessioni e TUI (zellij)

- Una sessione zellij `qx` per macchina; il login SSH la apre o la riaggancia
  (shell di login o `ForceCommand`, a scelta del builder).
- Ogni sessione agente = una tab zellij il cui comando è l'agente (`claude`,
  `codex`) lanciato da `qx` con il profilo giusto, nella cartella o nel sandbox
  giusti.
- Plugin qx: barra delle tab con simboli di stato, riga "chi ti aspetta", azione
  "nuova sessione" (progetto → profilo → host/sandbox), prompt di conferma per le
  azioni di livello 3.
- Le sessioni zellij sopravvivono alla disconnessione; zellij `resurrect` /
  `qx` ricreano le tab dopo un riavvio (`claude --resume <id>`).

### Strumenti di sistema per gli agenti (MCP + CLI) e permessi

Tre livelli, applicati da qxd, non dall'agente:

1. **Lettura** (stato, log, risorse, elenco sandbox/progetti): libera.
2. **Azioni reversibili** (nuovo sandbox, nuovo worktree, installare un tool
   nella home, avviare un display virtuale): libere, registrate nel log azioni.
3. **Azioni distruttive o di sistema** (distruggere un sandbox, aggiornare
   l'immagine, cambiare rete o firewall, installare nel sistema): chiedono
   conferma all'umano nella TUI; se nessuno è collegato restano in attesa e
   generano una notifica.

In aggiunta, skill per gli agenti (modello Omarchy) che spiegano come usare `qx`.
Gli agenti non hanno shell root: il sistema si cambia solo tramite `qx`.

**Chi esegue le azioni che richiedono root** (scelta del designer): un piccolo
servizio di sistema separato (`qx-system`, systemd di sistema) che espone solo
un elenco chiuso di operazioni (aggiornamento immagine, firewall/rete,
installazione nel sistema) ed esegue soltanto richieste che qxd gli inoltra già
confermate dall'umano. Nessun sudo per l'utente degli agenti.

**Aggiornamenti e riavvio** (scelta del designer): `qx update` prepara la nuova
immagine bootc senza riavviare; il riavvio è un'azione di livello 3 separata,
che la TUI propone mostrando le sessioni attive. Dopo il riavvio le tab si
ricreano con `--resume`; un comando o una build in corso si perdono, e la
conferma lo dice.

### Sandbox (isx come strumento)

- Ogni progetto ha un default: `host` o `sandbox` (+ modalità di rete isx: piena,
  `--proxy-only`, `--airgap`).
- Default di sistema: `host`, utente non privilegiato, nella cartella del progetto
  o in un worktree, con il sandbox interno di Claude Code.
- Un agente può sempre passare a più isolamento (crea un branch isx e continua
  lì); passare dal sandbox all'host richiede conferma (livello 3).
- Il lavoro esce dal sandbox via git (remote `isx://<istanza>`), gestito
  dall'agente.

### Profili e abbonamenti

- Stesso modello di qursor F25 / ADR-0036: un profilo = nome + agente + cartella
  di configurazione, passata per spawn (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`); il
  profilo di default = nessuna variabile; una sessione ripresa usa il suo profilo;
  una sessione avviata non cambia profilo; un profilo per progetto (ADR-0061).
- Login via SSH: la CLI dell'agente stampa un link, lo si apre su un altro
  dispositivo. `qx profile add`.
- Nei sandbox isx il token lo tiene il proxy di isx sull'host: ogni profilo
  corrisponde a un account isx, e qxd sceglie quello giusto alla creazione del
  sandbox. Deroga controllata ad ADR-0036 (il token resta sull'host, gestito da
  isx). Da verificare: isx con più account Claude in parallelo.
- Consumi per profilo (come F34) in barra.

### Sviluppare qursor-ide sulla distro

- Toolchain nell'immagine: Rust, Node/pnpm, mise, gh, dipendenze Tauri Linux,
  Playwright con browser.
- Display virtuale come strumento (`qx display start`): Xvfb + WM leggero;
  gli script `scripts/qa/` vanno resi funzionanti fuori da GNOME (screenshot con
  `import`/`scrot`, già previsti come fallback).
- sccache condiviso tra worktree; taglia macchina indicativa 8 core / 16–32 GB, da
  misurare nella Fase 3.

### Notifiche

Quando una sessione aspetta l'umano e nessun client è agganciato, qxd invia una
notifica push (servizio a scelta del builder, es. ntfy self-hosted o pubblico).

### Modelli locali, dopo

Non nella prima versione. Posto riservato: un runtime (Ollama/llama.cpp) come
servizio sull'host, avviato con `qx`, raggiungibile dai sandbox tramite il proxy
isx come un'altra API e dall'host come endpoint per i profili; attivo solo su
macchine con GPU.

## Fasi

0. **Prova su zellij (usa e getta, 1–2 giorni).** Plugin che mostra lo stato di 2–3
   sessioni Claude vere e segnala quella che aspetta. Esito: B confermato o
   ripiego su A. Criteri (scelta del designer): B regge se (a) il plugin disegna
   una barra con lo stato per tab, (b) apre una tab con un comando dato, (c) mostra
   una conferma all'umano (va bene anche un pane flottante). Lo stato può venire
   da qxd (transcript + titolo OSC letto da un wrapper del PTY) invece che dal
   plugin: conta comunque come B. Si ripiega su A solo se (a), (b) o (c) sono
   impossibili.
1. **Immagine bootc + accesso comodo.** Containerfile, installazione su VPS
   esistente, `ssh` → zellij `qx`, OSC 52, mosh.
2. **qxd + sessioni + profili.** Crate condiviso estratto da qursor, sessioni in
   tab, stato, riaggancio, profili e login.
3. **Sviluppare qursor sulla distro.** Toolchain, display virtuale, sccache;
   criterio: un ciclo completo issue → PR di qursor fatto interamente sul VPS.
4. **Strumenti di sistema e permessi.** MCP + CLI con tre livelli, conferme nella
   TUI, log azioni, notifiche.
5. **isx come strumento.** Default per progetto, escalation, account per profilo.
6. **Dopo:** modelli locali, ISO per casa, immagini nel terminale.

## Lasciato al builder

- Nome definitivo e repository (probabilmente un repo separato da qursor-ide; il
  crate condiviso va deciso insieme).
- Meccanismo esatto di aggancio al login SSH.
- Protocollo socket qxd ↔ plugin/CLI.
- Servizio di notifica.
- WM leggero per il display virtuale.

## Da verificare

- API dei plugin zellij: lettura titolo/uscita dei pane, rendering della barra,
  apertura tab con comando (Fase 0).
- `bootc install to-existing-root` sui provider di interesse.
- isx: più account Claude in parallelo, comportamento su Fedora bootc
  (Incus + btrfs su un sistema immutabile).
- `scripts/qa/` senza GNOME.
