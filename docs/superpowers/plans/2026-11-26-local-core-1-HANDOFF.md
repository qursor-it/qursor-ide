# Handoff — qursor-core, piano 1 (estrazione)

Data: 2026-11-26. Branch: `feat/qursor-core` (worktree `../qursor-core`), partito da
`feat/opencode` @ 79154e9.

- **Design:** `docs/superpowers/specs/2026-11-26-local-core-design.md` (approvato dal proprietario).
- **Piano:** `docs/superpowers/plans/2026-11-26-local-core-1-extraction.md` (Task 0–8).
- **Esecuzione:** subagent-driven-development (un implementer per task + revisore per task, fix
  loop max 5 giri, revisione finale dell'intero branch). Scelta del proprietario.

## Stato

| Task | Stato | Commit |
|---|---|---|
| 0 — misure di partenza | **completo**, revisione pulita (2 giri di fix) | 1af10a1, 23200a7, e7c7076 |
| 1 — `EventSink` al posto di `AppHandle` | **implementato, revisione: Needs fixes** (vedi sotto) | f0c39e8 |
| 2–8 | da fare | — |

Baseline registrata in `specs/10-performance.md` P9 (Windows 11, Core Ultra 7 155H, 5 run):
`throughput_50mb_ms` min/mediana/max 22582/25727/33882; `echo_p95_us` 15879/15901/15912;
`echo_p50_us` mediana 8317.

## Da fare subito: fix del Task 1 (giro 1 di 5)

Riprendere il Task 1 con un implementer, passandogli questi finding, poi revisione mirata.

1. **Critical — `tokio::spawn` chiamato da thread senza runtime → panic, e in release
   `panic = "abort"` chiude l'app.**
   - Il thread `ssh-reaper` (`lib.rs` ~500-507, `std::thread`) chiama `Pool::reap` → `disconnect` →
     `tokio::spawn` (`ssh/pool.rs:59`). Prima `tauri::async_runtime::spawn` funzionava da
     qualsiasi thread. L'app muore ~20 s dopo la chiusura dell'ultima tab di un server SSH.
   - Stessa causa in `ssh/pty.rs:112-117` `close()` ("from any thread"), raggiunto da
     `SshChild::kill` / `SshKiller::kill`, anche dal thread std di `TerminalManager::kill` per un
     genitore con deleghe (`terminal.rs` ~2542-2557).
   - Rompe il test Unix `an_ssh_tab_streams_through_the_manager_and_kill_hangs_it_up` (mai
     eseguito su Windows): fallirà su Linux/macOS.
   - Fix: catturare un `tokio::runtime::Handle` dove il runtime è attivo e usare `handle.spawn`
     (in `Closer` da `bridge()`, in `Pool` o facendo entrare il runtime al thread reaper); poi
     cercare ogni `tokio::spawn` raggiungibile da `std::thread`, `Drop` o killer.
2. **Important — la baseline non misura più lo stesso percorso.** I test `baseline_*` ora contano
   `bytes.len()` senza encode/decode base64. L'effetto sui numeri è <1%, ma la frase in
   `specs/10-performance.md` P9 ("an in-process closure that base64-decodes each data event") e
   il doc comment di `baseline_manager` (`terminal.rs` ~5126-5128) sono falsi. Rimettere
   encode+decode nel closure di test (preferito: stesso costo di `TauriSink`) oppure correggere
   testo e commento.
3. Minor (rimandabili alla revisione finale):
   - `CoreEvent` con `rename_all = "camelCase"` rinomina solo i tag: aggiungere
     `rename_all_fields = "camelCase"` prima che vada sul socket (Task 3/4).
   - Doc di `Runner::start` obsoleto (dice che il runtime non esiste in `setup()`).
   - `current_runtime()` ha un fallback `cfg(test)` in codice di produzione; meglio passare un
     `Handle` a `new`/`with_callbacks` (utile anche al core headless).

Altre note dal Task 1: `pnpm e2e` non è stato eseguito (nel worktree mancava `node_modules`:
fare `pnpm install` e lanciarlo); nessun test Unix eseguito; `cargo test` senza `-j 2` va in
LNK1102 (linker senza memoria) su questa macchina.

## Decisioni prese durante l'esecuzione (ledger)

- **Base del branch = `feat/opencode`**, non `develop`: il piano e il codice delle deleghe
  (ADR-0125/0126) esistono solo lì. La PR andrà su `develop` dopo il merge di `feat/opencode`
  (oppure rebase).
- **Task 0:** i test di baseline stanno in `services/terminal.rs` `mod tests` come `#[ignore]`
  (`make_manager` è privato); comando
  `cargo test -p qursor-ide --release --lib baseline_ -- --ignored --nocapture --test-threads=1`.
- **Confronto del Task 8:** baseline e core eseguiti alternati, 5 run ciascuno, stessa macchina;
  criterio: mediana throughput del core dentro [min, max] della baseline; mediana dei cinque
  `echo_p95_us` del core ≤ mediana baseline (15901) + 1000 us. Motivo: il rumore di ConPTY
  (~50%) supera l'effetto da misurare.
- **Pre-flight del piano** (valgono per i task successivi):
  - T2→T6: `AppError` è solo `Serialize`; `CoreLink` (app) legge gli `rpc_error` in un suo
    `WireError { kind: String, message: Option<String> }`.
  - T4: aggiungere il test `slow_client_closed_over_8mib` (client che non legge, output `yes`:
    connessione chiusa, PTY vivo, memoria del core limitata) — era assegnato per errore al Task 5.
  - T4: `shell.spawn` richiede `ShellSpawnOpts { clientKey, projectId, cwd, cols, rows }`; il test
    inserisce una riga progetto se serve.
  - T5 prima di T6: tra i due l'app reale non funziona (manca `core_endpoint`); il branch è
    rilasciabile solo dopo il Task 6.
  - T6: `env!("CARGO_BIN_EXE_qursor-core")` non esiste tra package diversi → `CoreLink::ensure`
    riceve il percorso del binario come parametro; i test lo cercano in
    `target/<profile>/qursor-core[.exe]` e falliscono con un messaggio chiaro se non è stato
    compilato.
  - T6: in `.github/workflows` c'è solo `windows-release.yml` (più `triage.yml`): lì va la build
    del sidecar; anche il flusso dev (`beforeDevCommand`) deve compilare `qursor-core`.
- **Minor rimandati del Task 0:** `/bin/cat` hard-coded nei rami POSIX; i rami POSIX della
  baseline non sono mai stati eseguiti (farlo su Linux/macOS al Task 8).
- **Drift di formattazione preesistente** in `services/gh/checkout.rs` e `services/jsonl.rs`
  (non nostro): il gate del Task 8 deve sistemarlo in un commit a parte.

## Come riprendere

1. `cd ../qursor-core && git pull` (branch `feat/qursor-core`), `pnpm install`.
2. Il ledger dettagliato è in `.superpowers/sdd/2026-11-26-local-core-1-extraction/` (ignorato da
   git, esiste solo in questo worktree; questo file ne riporta tutto il contenuto utile).
3. Riprendere con subagent-driven-development dal fix del Task 1 (sopra), poi Task 2.
4. Piani 2 e 3 (sopravvivenza, versioni) si scrivono quando il piano 1 è in `develop`.

## Nel checkout principale

In `C:\DEV\Personale\git\qursor-ide` (branch `feat/opencode`) restano due copie non tracciate di
design e piano (`docs/superpowers/specs/2026-11-26-local-core-design.md`,
`docs/superpowers/plans/2026-11-26-local-core-1-extraction.md`): le versioni di riferimento sono
quelle committate qui. Lo spec cloud sessions del 2026-11-21 e il suo piano sono stati cancellati
su richiesta del proprietario.
