# qursor-core, piano 1 di 3: il core possiede tutti i terminali — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tutti i terminali (agente, footer, SSH) girano nel processo `qursor-core`; il renderer li usa via WebSocket con frame binari; il comportamento visibile resta quello di oggi (alla chiusura si uccide tutto), il percorso dati è più corto e misurato.

**Architecture:** Il codice di `TerminalManager` e la sua chiusura transitiva escono da `factorai_lib` in un crate di libreria senza Tauri, `crates/qursor-core`, che ha anche il binario `qursor-core`. Le emit Tauri diventano un `EventSink`; il binario lo implementa come broadcast WebSocket. L'app avvia il core, passa endpoint e segreto al renderer, e tiene un piccolo client Rust per i conteggi (quit guard, widget taskbar) e lo `shutdown`.

**Tech Stack:** Rust 1.98, tokio, tokio-tungstenite 0.30 (già in uso), portable-pty, rusqlite; React 19 + TS, xterm.

**Spec:** `docs/superpowers/specs/2026-11-26-local-core-design.md`

**Piani successivi (si scrivono quando questo è in `main`):**
- **Piano 2 — sopravvivenza:** stato schermo `vt100` nel core + snapshot con revisioni, riaggancio, dialogo di chiusura B con "Ricorda la scelta", tray (feature `tray`), politica crash, code "nessun client" (§5b), vault che si richiude all'ultima disconnessione, ADR che supera ADR-0005.
- **Piano 3 — versioni:** copia per versione in `<data_dir>/core/bin/`, due core con drain, adattatore della versione precedente, regola migrazioni additive.

## Global Constraints

- Il crate `crates/qursor-core` **non dipende da `tauri`** né da alcun `tauri-plugin-*` (verifica: `cargo tree -p qursor-core | grep -i tauri` vuoto).
- Niente `unwrap()` fuori da `setup()`/test; `anyhow` dentro, `AppError` (thiserror) al confine. Clippy `-D warnings`.
- Tipi che attraversano il confine: Rust `#[serde(rename_all = "camelCase")]`, mirror a mano in `packages/types`. Niente codegen.
- **Nessun xterm del pool viene distrutto, staccato o ricreato** (`specs/10-performance.md` P1).
- WebSocket solo su `127.0.0.1`; primo frame `hello` con segreto (32 byte casuali, hex) confrontato in tempo costante; altrimenti chiusura.
- Frame binario: `opcode u8 | slot u16 BE | payload`. Opcode: `1 Output`, `2 Input`, `3 Resize` (payload `cols u16 BE, rows u16 BE`), `4..=6` riservati agli snapshot del piano 2.
- Coalescing come oggi: 16 ms in primo piano, 100 ms in background, chunk massimo 32 KiB.
- Coda per client: oltre 8 MiB in attesa la connessione si chiude (lo snapshot a 4 MiB è del piano 2).
- Ping/pong JSON ogni 10 s lato client; due finestre senza frame in arrivo = connessione chiusa.
- `protocolVersion = 1`. Record in `<data_dir>/core/core.json`, lock in `<data_dir>/core/core.lock`.
- In questo piano: ultimo client scollegato → 10 s di grazia (ricarica della webview, ADR-0059) → `kill_all()` e uscita. Equivale al kill-on-quit di oggi.
- Windows nativo x64, macOS, Linux: tutti e tre compilano e passano i test.
- Commit piccoli con prefisso (`feat:`, `refactor:`, `test:`, `docs:`), su un branch nel suo worktree da `origin/main`.

## Review Focus

1. **Ricarica della webview / HMR in dev** (ADR-0059): il socket cade e torna entro 10 s; le sessioni non devono morire e il terminale deve ricevere di nuovo l'output → test in Task 5.
2. **Due connessioni con lo stesso terminale** (una vecchia mezza morta, una nuova dopo reload): l'input non va duplicato e l'output va a entrambe finché la vecchia non chiude → test in Task 5.
3. **Output enorme e continuo** (`yes`, build verbose) con un client lento: il PTY non si blocca e la memoria del core non cresce senza limite (chiusura a 8 MiB) → test in Task 5.
4. **Core già acceso di un'altra istanza / record vecchio dopo un crash** (pid morto, porta occupata da altro): l'app non si collega al processo sbagliato e ne avvia uno nuovo → test in Task 6.
5. **Percorsi Windows con spazi e caratteri non ASCII** nella cartella dati (`C:\Users\Nome Cognome\…`): il core parte e scrive il record → test in Task 6.

---

### Task 0: Misure di partenza

**Files:**
- Create: `apps/desktop/src-tauri/tests/pty_throughput.rs`
- Modify: `specs/10-performance.md` (nuova sezione "P9 — qursor-core", righe di baseline)

**Interfaces:**
- Produces: due misure riproducibili, eseguite di nuovo in Task 8 contro il core: `throughput_50mb_ms` e `echo_p95_us`.

- [ ] **Step 1:** Scrivi `#[test] #[ignore] fn baseline_throughput()` che costruisce un `TerminalManager` con un sink di test (oggi: `make_manager` di `services/terminal.rs` tests, esposto come helper `test_support`), fa `cat` di un file da 50 MB generato in `tempfile`, e misura il tempo fino all'evento `terminal:exit` con tutti i byte ricevuti. Stampa `throughput_50mb_ms=<n>`.
- [ ] **Step 2:** `#[test] #[ignore] fn baseline_echo()`: avvia `cat` interattivo, scrive 200 volte un byte e misura write→primo byte in uscita; stampa `echo_p95_us=<n>`.
- [ ] **Step 3:** Esegui `cargo test -p qursor-ide --release --test pty_throughput -- --ignored --nocapture` tre volte; scrivi la mediana in `specs/10-performance.md` P9 con OS e macchina.
- [ ] **Step 4:** Commit `test: baseline PTY throughput and echo latency before qursor-core`.

### Task 1: `EventSink` al posto di `AppHandle`

**Files:**
- Modify: `apps/desktop/src-tauri/src/services/terminal.rs:24, 885-950` (for_app → new), `services/routines.rs:544-547, 1020-1053` (emit), `services/agent_tools/server.rs:72,84`, `services/ide/server.rs:139,218`, `services/ssh/{pool.rs:59,pty.rs:66-114,tunnel.rs:23-64,sftp.rs:226-336}`, `agents/opencode.rs:160-165`, `services/intelligence/mod.rs` (sposta `sha256_hex` in `services/hash.rs`), `lib.rs:182-410`
- Create: `apps/desktop/src-tauri/src/services/event_sink.rs`, `apps/desktop/src-tauri/src/tauri_sink.rs`

**Interfaces:**
- Produces:
  ```rust
  pub trait EventSink: Send + Sync + 'static { fn emit(&self, event: CoreEvent); }
  #[derive(Debug, Clone, Serialize)] #[serde(tag = "event", content = "payload", rename_all = "camelCase")]
  pub enum CoreEvent {
      TerminalData { terminal_id: String, #[serde(skip)] bytes: Vec<u8> },
      TerminalStatus(TerminalStatusEvent), TerminalExit(TerminalExitEvent),
      SessionAdopted(SessionAdoptedEvent), SessionWorktree(SessionWorktreeEvent),
      AgentSessionStart(SessionStart), DelegationsChanged { project_id: String },
      IdeOpenFile(IdeOpenFileEvent), IdeStatus(IdeStatusEvent),
      AgentTerminalRun(AgentRunEvent), AgentTerminalConfirm(AgentConfirmEvent),
      RoutinesChanged { project_id: String }, RoutineFire(FireEvent),
  }
  impl TerminalManager { pub fn new(sink: Arc<dyn EventSink>, claude_dir: PathBuf, ui: Arc<UiState>) -> Self }
  ```
  `TauriSink` (in `tauri_sink.rs`, resta nell'app) traduce ogni variante nell'evento Tauri di oggi con lo stesso nome e payload (`terminal:data` codifica `bytes` in base64 come fa `emit_data` oggi). `routines::announce`/fire prendono `&dyn EventSink` invece di `&AppHandle`.
- `tauri::async_runtime::spawn` → `tokio::spawn` (Tauri usa già tokio; nei test serve `#[tokio::test]` dove oggi c'è `tauri::async_runtime::block_on`). `sftp.rs` `block_on` → `tokio::runtime::Handle::current().block_on` dentro `spawn_blocking` come oggi. `tauri::Url` → `url::Url` (già nell'albero tramite tauri; aggiungilo come dipendenza diretta).

- [ ] **Step 1:** Scrivi `#[test] fn recording_sink_receives_terminal_data()` in `services/event_sink.rs`: un `RecordingSink(Mutex<Vec<CoreEvent>>)`, un manager con `make_manager` che lo usa, spawn di `printf hi` (Unix) / `cmd /c echo hi` (Windows), attesa dell'uscita; asserisci che i `TerminalData` concatenati contengono `b"hi"` e che c'è esattamente un `TerminalExit`.
- [ ] **Step 2:** `cargo test -p qursor-ide recording_sink` → FAIL (tipo non definito).
- [ ] **Step 3:** Introduci `EventSink`/`CoreEvent`, `TerminalManager::new`, `TauriSink`; sostituisci le 9 emit di `for_app` e quelle di `routines.rs`; aggiorna `lib.rs` e gli helper di test (`make_manager*` alle righe ~3152, 3156, 4348, 5068) per passare un sink.
- [ ] **Step 4:** Rimuovi ogni `tauri::` dai moduli elencati in "Files" tranne `lib.rs`, `tauri_sink.rs`, `attention.rs`, `commands/*`, `usage/poller.rs`, `platform/*`.
- [ ] **Step 5:** `cargo test -p qursor-ide && pnpm e2e` → PASS, nessun cambiamento visibile.
- [ ] **Step 6:** Commit `refactor: terminal events go through an EventSink instead of AppHandle`.

### Task 2: Il crate `qursor-core`

**Files:**
- Modify: `Cargo.toml` (workspace `members = ["apps/desktop/src-tauri", "crates/qursor-core"]`), `apps/desktop/src-tauri/Cargo.toml` (dipende da `qursor-core = { path = "../../../crates/qursor-core" }`; le dipendenze usate solo dal codice spostato passano al core: `vt100`, `russh`, `russh-sftp`, `tokio-tungstenite`, `futures-util`, `croner`, `git2`, `keyring`, `aes-gcm`, `argon2`, `portable-pty`)
- Create: `crates/qursor-core/Cargo.toml`, `crates/qursor-core/src/lib.rs`
- Move (con `git mv`, contenuto invariato salvo i path `crate::`): `error.rs`, `db/`, `models/`, `services/{terminal.rs,terminal/,event_sink.rs,osc_title.rs,child_env.rs,term_capture.rs,shell_path.rs,agent_cli.rs,git.rs,files.rs,sops.rs,settings.rs,routines.rs,canvas.rs,canvas_tools.rs,mcp_wire.rs,jsonl.rs,hash.rs,ide/,agent_tools/,ssh/,vault.rs,profiles.rs,sessions.rs (solo le funzioni lette dal manager: recorded_cwds, recorded_key, worktree, set_worktree; il resto resta),usage/statusline.rs,usage/child_guard.rs}`, `agents/`, la logica di `services/attention.rs` (vedi Task 4), `claude_dir()` ed `epoch_ms()` da `lib.rs`.
- La lista è il punto di partenza: il compilatore decide. Un modulo che si porta dietro `tauri` resta nell'app e si spezza in due (parte pura nel core).

**Interfaces:**
- Produces: `qursor_core::{error::{AppError, AppResult}, db::Db, services::terminal::*, services::event_sink::*, services::ssh, services::vault, agents, epoch_ms, claude_dir}`. `factorai_lib` ri-esporta con `pub use qursor_core::…` i path che i suoi `commands/*` usano, così i comandi non cambiano in questo task.
- La trasferibilità di `#[cfg(test)] mod test_support` (spawn lock, `lib.rs:794-805`) va nel core come `pub mod test_support` dietro feature `test-support`.

- [ ] **Step 1:** Crea il crate vuoto e il membro del workspace; `cargo check --workspace` → PASS.
- [ ] **Step 2:** Sposta i moduli in un unico commit di soli `git mv` + aggiornamento dei path (`git diff -M --stat` deve mostrare rinomine).
- [ ] **Step 3:** `cargo tree -p qursor-core | grep -i tauri` → vuoto.
- [ ] **Step 4:** `cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && pnpm e2e` → PASS.
- [ ] **Step 5:** Commit `refactor: move the terminal manager and its closure into crates/qursor-core`.

### Task 3: Il protocollo

**Files:**
- Create: `crates/qursor-core/src/protocol/{mod.rs,frame.rs,messages.rs}`, `packages/types/src/core.ts` (export da `packages/types/src/index.ts`), `apps/desktop/src/lib/core/frames.ts`, `apps/desktop/src/lib/core/frames.test.ts`

**Interfaces:**
- Produces (Rust):
  ```rust
  pub const PROTOCOL_VERSION: u32 = 1;
  #[repr(u8)] pub enum Op { Output = 1, Input = 2, Resize = 3 }
  pub fn encode(op: Op, slot: u16, payload: &[u8]) -> Vec<u8>;
  pub fn decode(frame: &[u8]) -> Result<(Op, u16, &[u8]), FrameError>; // FrameError: TooShort, UnknownOp(u8), BadResize
  pub fn encode_resize(slot: u16, cols: u16, rows: u16) -> Vec<u8>;
  #[serde(tag = "type", rename_all = "snake_case")]
  pub enum ClientMsg { Hello { secret: String, client_id: String, client_type: ClientType, protocol_version: u32, app_version: String, capabilities: Vec<String> },
                       Ping, Request { request_id: u64, method: String, params: serde_json::Value } }
  pub enum ServerMsg { ServerInfo { host_id: String, version: String, protocol_version: u32, features: Vec<String> },
                       Pong, Response { request_id: u64, result: serde_json::Value },
                       RpcError { request_id: u64, error: AppError }, Event(CoreEvent) }
  pub enum ClientType { App, Renderer, Cli }
  ```
  Campi JSON in camelCase. `host_id`: uuid v4 persistito in `<data_dir>/core/host-id`.
- Produces (TS, mirror a mano): `ClientMsg`, `ServerMsg`, `CoreEvent`, `Op`, e in `frames.ts` `encodeFrame(op, slot, payload: Uint8Array): Uint8Array`, `decodeFrame(buf: ArrayBuffer): { op, slot, payload: Uint8Array }` (payload è una view, nessuna copia), `encodeResize(slot, cols, rows)`.
- Metodi RPC (stringhe `method`), equivalenti ai comandi di oggi: `terminal.spawn` → `{ id, agent, slot }`, `terminal.attach {terminalId}` → `{ slot }`, `terminal.detach {slot}`, `terminal.kill`, `terminal.list`, `terminal.visibility {terminalId, visible}`, `shell.spawn` → `{ id, slot }`, `shell.killForProject`, `session.start` (= `start_session`), `delegations.list`, `delegations.resolve`, `server.openShell`, `server.endTab`, `agentTerminal.link|unlink|answer`, `ide.reportUi`, `ide.mention`, `vault.*` (i comandi vault di oggi), `core.counts` → `{ live, working }`, `core.shutdown {keepTerminals: false}`. L'input viaggia solo come frame `Input`; il resize solo come frame `Resize`.

- [ ] **Step 1:** Test Rust in `frame.rs`: `roundtrip_output` (encode/decode di 32 KiB casuali, slot 513), `resize_payload_is_four_bytes`, `rejects_short_frame` (2 byte → `TooShort`), `rejects_unknown_op` (op 9 → `UnknownOp(9)`). Test in `messages.rs`: `hello_json_shape` asserisce `{"type":"hello","secret":"…","clientId":"…","clientType":"renderer","protocolVersion":1,…}`.
- [ ] **Step 2:** Test TS in `frames.test.ts` con gli stessi casi, più `decode_is_a_view` (il `payload.buffer` è lo stesso `ArrayBuffer`). Più un vettore condiviso: il frame `[1,2,1,104,105]` = Output, slot 513, `"hi"` decodificato uguale nei due linguaggi.
- [ ] **Step 3:** `cargo test -p qursor-core protocol && pnpm --filter desktop test frames` → FAIL.
- [ ] **Step 4:** Implementa.
- [ ] **Step 5:** Stessi comandi → PASS. Commit `feat: qursor-core wire protocol, binary terminal frames and JSON envelopes`.

### Task 4: Il binario `qursor-core`

**Files:**
- Create: `crates/qursor-core/src/bin/qursor-core.rs`, `crates/qursor-core/src/server/{mod.rs,record.rs,client.rs,wiring.rs}`, `crates/qursor-core/src/attention.rs`, `crates/qursor-core/tests/core_ws.rs`
- Modify: `crates/qursor-core/src/services/terminal.rs` (flush per visibilità), `services/ide/ui_state.rs` (alimentato da `ide.reportUi`)

**Interfaces:**
- Consumes: `TerminalManager::new`, `EventSink`, `protocol::*`.
- Produces:
  - CLI: `qursor-core --data-dir <path> [--parent-pid <pid>]`. Stampa su stdout una riga JSON `{"ready":true,"port":N}` quando ascolta, poi niente.
  - `record::Record { pid: u32, protocol_version: u32, app_version: String, port: u16, secret: String }` scritto atomico (file temporaneo + rename) in `<data_dir>/core/core.json`, permessi 0600 su Unix; cancellato all'uscita pulita. Lock esclusivo con `std::fs::File::try_lock` su `core.lock`: se fallisce, il processo esce con codice 3.
  - `wiring::build_manager(data_dir: &Path, sink: Arc<dyn EventSink>) -> anyhow::Result<TerminalManager>`: la catena `with_*` / `set_*` che oggi sta in `lib.rs:182-410`, con le stesse letture dal DB. `routines.mark_started` si sposta dentro il gestore di `terminal.spawn`.
  - `WsSink`: per ogni `CoreEvent` non-data serializza una volta e lo manda a tutti i client autenticati; `TerminalData` va come frame `Output` solo ai client che hanno lo slot di quel terminale (slot assegnato per client, `u16` crescente, mai riusato nella stessa connessione). Ogni client ha una coda `mpsc` illimitata con contatore di byte; oltre 8 MiB la connessione si chiude.
  - Finestra di flush per terminale: 16 ms se almeno un client lo ha `visible`, altrimenti 100 ms (sostituisce `UiState::is_backgrounded` in `flush_window`, `terminal.rs:2698-2700`).
  - `attention.rs`: la logica di decisione di oggi (`AttentionNotifier`) gira nel core e manda `CoreEvent::Attention(AttentionNotice { session_id, terminal_id, title, body })` (aggiungi la variante in Rust e il suo mirror in `packages/types/src/core.ts`), con titolo e corpo già composti come oggi da `TauriSink` (impostazione `DesktopNotifications`, `projects.display_name`, `alert_context`). Se l'impostazione è spenta non manda nulla.
  - Origin ammessi: assente, `tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost`, `http://localhost:1420`.
  - Ultimo client scollegato → dopo 10 s senza riconnessioni `kill_all()` ed esci; con `--parent-pid`, se quel processo muore, stesso comportamento.

- [ ] **Step 1:** Integration test `tests/core_ws.rs` (avvia il server in-process su porta 0 con una `data_dir` temporanea; client tokio-tungstenite):
  - `rejects_wrong_secret`: hello con segreto sbagliato → la connessione si chiude senza `server_info`.
  - `rejects_non_hello_first_frame`: primo frame `ping` → chiusura.
  - `spawn_echo_roundtrip`: `shell.spawn` di una shell, frame `Input` con `echo qx\r`, entro 5 s arriva un `Output` sullo slot restituito che contiene `qx`.
  - `output_only_to_attached`: due client, solo il primo fa attach; il secondo non riceve frame binari.
  - `last_client_grace_then_kill`: con `grace = 200ms` (parametro di test), una shell viva, il client chiude; entro 400 ms il server in-process segnala l'uscita sul suo canale di shutdown e il pid della shell non esiste più.
  - `reconnect_within_grace_keeps_terminals`: il client chiude e si riconnette entro la grazia; `terminal.list` contiene ancora il PTY.
- [ ] **Step 2:** `cargo test -p qursor-core --test core_ws` → FAIL.
- [ ] **Step 3:** Implementa server, record, lock, sink, wiring, attention.
- [ ] **Step 4:** Test unitari `record.rs`: `record_is_owner_only` (cfg unix, mode `0o600`), `second_instance_exits_3` (due `try_lock` sullo stesso file → il secondo fallisce).
- [ ] **Step 5:** `cargo test -p qursor-core` → PASS. Commit `feat: qursor-core binary serves terminals over an authenticated loopback WebSocket`.

### Task 5: Il client nel renderer

**Files:**
- Create: `apps/desktop/src/lib/core/client.ts`, `apps/desktop/src/lib/core/client.test.ts`, `apps/desktop/src/lib/core/mock.ts`
- Modify: `apps/desktop/src/lib/tauri.ts:519-530, 558-577, 602, 901-979` (i wrapper `cmd.*`/`events.*` dei terminali passano dal client), `apps/desktop/src/components/terminal/Terminal.tsx:767-779, 858-914, 1073-1097`, `packages/types` (`Spawned` guadagna `slot`)

**Interfaces:**
- Consumes: `core_endpoint` (Task 6), `frames.ts`, tipi di `packages/types/src/core.ts`.
- Produces:
  ```ts
  export interface CoreClient {
    request<T>(method: string, params?: unknown): Promise<T>;
    on<E extends CoreEvent["event"]>(event: E, cb: (payload: Extract<CoreEvent, { event: E }>["payload"]) => void): () => void;
    attach(terminalId: string, onOutput: (bytes: Uint8Array) => void): Promise<{ slot: number; detach(): void }>;
    write(slot: number, bytes: Uint8Array): void;
    resize(slot: number, cols: number, rows: number): void;
    setVisible(terminalId: string, visible: boolean): void;
  }
  export function coreClient(hostId?: string): CoreClient; // un client per host, oggi solo "local"
  ```
  - `binaryType = "arraybuffer"`; `onmessage` binario → `decodeFrame` → callback dello slot con la view, senza copie né base64.
  - Riconnessione con backoff 100 ms → 2 s; alla riconnessione rifà `hello` e ri-esegue `attach` per ogni terminale ancora attaccato (nuovi slot); le richieste in volo falliscono con errore `Process`.
  - Ping ogni 10 s; due finestre senza frame = chiudi e riconnetti.
  - Gli input da `term.onData` passano per `TextEncoder` una volta e vanno come frame `Input`.
- I wrapper esistenti mantengono nome e firma (`cmd.terminalSpawn`, `events.onTerminalExit`, …), così i ~20 file chiamanti non cambiano. Cambia solo `events.onTerminalData`, che sparisce: `Terminal.tsx` usa `attach` e scrive `term.write(bytes)`.
- In modalità mock (`!isTauri()`), `coreClient()` restituisce `mock.ts`, che inoltra `request` a `mockInvoke` con il nome del comando di oggi e ascolta `mockListen`; `window.__FACTORAI_EMIT__('terminal:data', { terminalId, bytesB64 })` resta valido (il mock decodifica il base64 e chiama la callback dello slot). Gli smoke test non cambiano.

- [ ] **Step 1:** Test in `client.test.ts` con un `FakeWebSocket`:
  - `sends_hello_first` con segreto e `protocolVersion: 1`.
  - `routes_output_by_slot`: due attach (slot 1 e 2), un frame Output per lo slot 2 chiama solo la seconda callback con i byte giusti.
  - `reattaches_after_reconnect`: chiusura del socket, nuovo socket, il client rimanda `hello` e un `terminal.attach` per il terminale; il nuovo slot riceve l'output.
  - `no_duplicate_input_after_reconnect`: una `write` dopo la riconnessione esce una sola volta, sul socket nuovo.
  - `liveness_closes_after_two_silent_windows` (timer finti: 20 s senza frame → `close()` chiamato).
- [ ] **Step 2:** `pnpm --filter desktop test core/client` → FAIL.
- [ ] **Step 3:** Implementa `client.ts`, `mock.ts`, i wrapper in `tauri.ts`, e `Terminal.tsx` (`attachStream` usa `attach`; `term.onData` → `write`; `pushSize` → `resize`; `showOnly` chiama `setVisible`). Il pool degli xterm resta com'è.
- [ ] **Step 4:** `pnpm typecheck && pnpm --filter desktop test && pnpm e2e` → PASS.
- [ ] **Step 5:** Commit `feat: the renderer talks to qursor-core directly over WebSocket`.

### Task 6: L'app avvia il core e smette di possedere i terminali

**Files:**
- Create: `apps/desktop/src-tauri/src/core_link.rs`, `apps/desktop/src-tauri/src/commands/core.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs` (setup, `CloseRequested` a :566-608, registrazione comandi), `state.rs` (rimuovi `terminals`, `ssh`, `tunnels`; aggiungi `core: Arc<CoreLink>`), `commands/terminal.rs`, `commands/servers.rs`, `commands/ide.rs`, `commands/sessions.rs` (eliminati o ridotti a ciò che non tocca terminali), `services/attention.rs` (resta solo la visualizzazione), `platform/taskbar_widget` (conteggi da `CoreLink`), `apps/desktop/src-tauri/tauri.conf.json` (`bundle.externalBin` con `qursor-core`, così l'installer lo porta accanto all'app su tutti e tre gli OS), CI di build (`.github/workflows/release.yml`: il binario del core compilato per ogni target con il suffisso target-triple che Tauri richiede)

**Interfaces:**
- Consumes: protocollo (Task 3), CLI del binario (Task 4).
- Produces:
  - `CoreLink::ensure(data_dir: &Path, app_version: &str) -> anyhow::Result<CoreLink>`: legge `core.json`; se il pid è vivo **e** la connessione con quel segreto riceve `server_info` con `protocolVersion == 1`, lo usa; altrimenti cancella il record e avvia il sidecar con `--data-dir <data_dir> --parent-pid <pid app>`, aspetta la riga `ready` (timeout 10 s) e si collega. Gira fuori dal main thread; la finestra non aspetta.
  - Avvio staccato: Windows `creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW)`; Unix `process_group(0)`.
  - `CoreLink` è un client `ClientType::App`: tiene `live`/`working` aggiornati dagli eventi `terminal.status`/`terminal.exit`, espone `counts() -> (usize, usize)` e `shutdown()` (`core.shutdown {keepTerminals:false}`, attende la chiusura, timeout 3 s).
  - Comando Tauri `core_endpoint() -> CoreEndpoint { url: String, secret: String, hostId: String }` (mirror in `packages/types`); aspetta che `CoreLink` sia pronto.
  - Comando Tauri `attention_show(notice: AttentionNotice)`: notifica OS + `request_user_attention` come `TauriSink` di oggi.
  - `CloseRequested`: stessa logica di oggi con `counts()` al posto di `live_count()`/`working_count()`; la conferma chiama `CoreLink::shutdown()` al posto di `kill_all()`.

- [ ] **Step 1:** Test in `core_link.rs`:
  - `stale_record_with_dead_pid_starts_new_core`: record con pid inesistente → `ensure` avvia un core (binario di test costruito da `cargo build -p qursor-core`, path da `env!("CARGO_BIN_EXE_qursor-core")` in un integration test `apps/desktop/src-tauri/tests/core_link.rs`).
  - `record_pointing_at_foreign_port_is_not_trusted`: record con pid vivo (il test stesso) e porta di un listener TCP che non risponde al protocollo → `ensure` non lo usa e ne avvia uno nuovo.
  - `data_dir_with_spaces_and_unicode`: `data_dir` = `tempdir/"Nome Cognome è"/` → il core parte e scrive il record.
  - `shutdown_kills_terminals`: spawn di una shell tramite il link, `shutdown()`, il pid della shell non esiste più entro 2 s.
- [ ] **Step 2:** `cargo test -p qursor-ide --test core_link` → FAIL.
- [ ] **Step 3:** Implementa; elimina da `factorai_lib` ogni comando che inoltrava a `TerminalManager` (ora sono metodi RPC del core) e la loro registrazione in `lib.rs:704` e vicini; registra `core_endpoint` e `attention_show`. Il renderer chiama `attention_show` all'evento `attention`.
- [ ] **Step 4:** `cargo test --workspace && pnpm e2e` → PASS.
- [ ] **Step 5:** Verifica nella finestra vera (skill `manual-qa`): nuova sessione Claude, scrivi, ridimensiona, footer shell, tab SSH, chiusura con conferma → nessun processo `claude`/`qursor-core` resta (`Get-Process` / `ps`). Ricarica della webview (devtools) → la sessione continua e il terminale torna a ricevere.
- [ ] **Step 6:** Commit `feat: the app starts qursor-core and no longer owns any terminal`.

### Task 7: Documenti

**Files:**
- Create: `specs/adr/0127-sessions-run-in-a-core-outside-the-app.md`
- Modify: `specs/01-architecture.md`, `specs/03-backend-rust.md`, `specs/adr/0005-*.md` e `specs/07-open-questions.md` Q10 (riga "Narrowed by ADR-0127": il processo cambia, la regola no — la sostituzione arriva col piano 2), `AGENTS.md` (layout: `crates/qursor-core`)

- [ ] **Step 1:** ADR-0127: contesto (questo design e l'approccio A), decisione (core separato, WebSocket loopback autenticato, frame binari, l'app non possiede terminali, in questo piano kill-on-quit invariato tramite `shutdown` e grazia di 10 s), conseguenze (sidecar in ogni bundle, due processi in Gestione attività, piano 2 supererà ADR-0005).
- [ ] **Step 2:** Aggiorna gli spec citati; ogni commento nel codice nuovo cita `specs/` o l'ADR-0127, mai AGENTS.md.
- [ ] **Step 3:** Commit `docs: ADR-0127 sessions run in qursor-core`.

### Task 8: Misure finali e gate

**Files:**
- Modify: `apps/desktop/src-tauri/tests/pty_throughput.rs` (stesse misure di Task 0, ora core → client WebSocket → byte ricevuti), `specs/10-performance.md` P9

- [ ] **Step 1:** Porta i due test di Task 0 sul core: client tokio-tungstenite collegato a un core in-process; stessi file e stessi conteggi.
- [ ] **Step 2:** Esegui tre volte in release; scrivi le mediane accanto alla baseline. Criteri: `echo_p95_us` non oltre baseline + 1000; `throughput_50mb_ms` non oltre la baseline. Se un criterio fallisce, fermati e riporta i numeri: niente merge.
- [ ] **Step 3:** Gate completo (skill `quality-gate`): `pnpm install && pnpm format && pnpm lint && pnpm typecheck && pnpm test && pnpm e2e && pnpm deps && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` → tutto PASS.
- [ ] **Step 4:** Commit `test: qursor-core meets the PTY budgets` e PR dal template (skill `pull-requests`), con i numeri prima/dopo nel corpo.
