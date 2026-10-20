# Usage Monitor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show Claude's and Codex's plan limits (5-hour and weekly %, reset times) and the active session's tokens and estimated cost in a full-width status bar, and the limits in a widget embedded in the Windows taskbar.

**Architecture:** A Rust `services::usage` module collects limit snapshots from four sources (Claude status-line JSON forwarded by a helper mode of the factorai binary, Codex rollout `token_count` events, `codex app-server`, and the opt-in Anthropic usage endpoint) into one store keyed by (agent, profile). The store persists to a JSON file and emits `usage:changed`; the renderer's `StatusBar` and a Windows-only native taskbar widget both read from it. Session cost is computed on demand from transcripts with a bundled price table.

**Tech Stack:** Rust (Tauri 2, serde_json, chrono, windows-sys, new: `ureq`, `tiny-skia`, `fontdue`), React + Zustand + TanStack Query, Tailwind v4 tokens, Vitest, Playwright smoke.

**Spec:** `docs/superpowers/specs/2026-10-19-usage-monitor-design.md`

## Global Constraints

- Branch `feat/usage-monitor` from `develop`, in its own worktree; merged locally into `develop`. **No push, no PR** (ADR-0074).
- factorai **never writes** `~/.claude/.credentials.json` or `~/.codex/auth.json`, never calls an OAuth token endpoint, never logs a token. `.credentials.json` is read only when the Claude-endpoint setting is on. Codex `auth.json` is never read.
- No quota spent to measure quota: no Messages-API probe, no `claude -p`, no `codex exec`.
- The user's own Claude Code `statusLine` keeps working unchanged.
- Network: only `https://api.anthropic.com` (opt-in), and `127.0.0.1`. Nothing else.
- Unofficial endpoint: poll ≥ 5 min apart per profile, honour `Retry-After`, skip on expired `expiresAt`.
- `codex app-server`: at startup then every 5 min, only if the `codex` binary is found; per Codex profile with its `CODEX_HOME`; flags `-s read-only -a never app-server`; 15 s timeout.
- Bar colours: neutral below 70%, amber 70–89%, red ≥ 90%. Stale after 15 min. A window whose reset time has passed shows 0%.
- Taskbar widget: Windows only, only while factorai runs, primary taskbar only, on by default; fallback to a tray icon if embedding fails.
- Status bar: on by default, hideable (renderer pref). Shows default profile per agent; popover shows all profiles.
- Every new IPC type is mirrored by hand in `packages/types/src/index.ts`, camelCase.
- Gate (quality-gate skill), in order: `pnpm install --frozen-lockfile && pnpm format:check && pnpm bytes:check && pnpm lint && pnpm typecheck && pnpm test && pnpm e2e && pnpm deps:check && pnpm deps:unused`, then `cd apps/desktop/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
- Rust style per backend-conventions skill: `AppResult` at command boundaries, `let _ = app.emit(...)`, named `std::thread`s for loops, no lock held across a child wait, `#[allow]` needs a comment. Tabs for indentation (existing files use tabs).

## Review Focus

1. **A user who already has a `statusLine`** (in `~/.claude/settings.json`, project `.claude/settings.json` or `.claude/settings.local.json`) must see exactly their own status line in a factorai session. → Task 4 test `helper_runs_the_users_statusline_and_prints_its_output`.
2. **No `statusLine` JSON `rate_limits` field** (API-key user, or before the first response) must leave the stored Claude snapshot untouched, not zero it. → Task 4 test `payload_without_rate_limits_is_ignored`.
3. **Codex putting the weekly window in `primary`** must not show a weekly number as the 5-hour one. → Task 2 test `classifies_by_window_minutes_not_slot`.
4. **A snapshot whose window reset time has passed** (app left open overnight) must read 0% with no reset time, not the old 95% red. → Task 1 test `expired_window_reads_as_reset`.
5. **Explorer restarting / hung** must not hang factorai's UI: nothing outside the widget thread `SendMessage`s its window. → Task 9 review checklist item + test `widget_handle_only_posts`.

---

### Task 1: Usage store, IPC types, persistence, event

**Files:**
- Create: `apps/desktop/src-tauri/src/services/usage/mod.rs`, `apps/desktop/src-tauri/src/services/usage/store.rs`
- Create: `apps/desktop/src-tauri/src/commands/usage.rs`
- Modify: `apps/desktop/src-tauri/src/services/mod.rs`, `apps/desktop/src-tauri/src/commands/mod.rs`, `apps/desktop/src-tauri/src/models/mod.rs`, `apps/desktop/src-tauri/src/state.rs:15-45`, `apps/desktop/src-tauri/src/lib.rs:301-314` (manage) and `:387-470` (handler)
- Modify: `packages/types/src/index.ts`, `apps/desktop/src/lib/tauri.ts` (`cmd.getUsage`, `events.onUsageChanged`, mock entries)

**Interfaces:**
- Produces (models/mod.rs, `#[serde(rename_all = "camelCase")]`):
  - `UsageWindow { used_percent: f64, resets_at: Option<i64> /* epoch ms */ }`
  - `ModelLimit { label: String, window: UsageWindow }`
  - `enum UsageSource { StatusLine, CodexLog, CodexAppServer, ClaudeApi }`
  - `ProviderUsage { agent: String, profile_id: String, profile_name: String, is_default: bool, five_hour: Option<UsageWindow>, weekly: Option<UsageWindow>, per_model: Vec<ModelLimit>, source: UsageSource, updated_at: i64 }`
  - `UsageReading { five_hour: Option<UsageWindow>, weekly: Option<UsageWindow>, per_model: Vec<ModelLimit>, source: UsageSource, observed_at: i64 }` (what a source hands the store; no profile fields)
- Produces (store.rs): `pub struct UsageStore` with
  - `fn load(path: PathBuf) -> Self` (missing/corrupt file → empty)
  - `fn record(&self, agent: &str, profile: &Profile, reading: UsageReading) -> bool` (true if anything changed; persists on change)
  - `fn snapshot(&self, now_ms: i64) -> Vec<ProviderUsage>` (applies the reset rule)
  - `fn set_on_change(&self, f: Arc<dyn Fn(Vec<ProviderUsage>) + Send + Sync>)`
- Produces: `AppState.usage: Arc<UsageStore>`; command `get_usage() -> AppResult<Vec<ProviderUsage>>`; event `usage:changed` with payload `Vec<ProviderUsage>`. Persist file: `<data_dir>/usage.json`.
- TS: `UsageWindow`, `ModelLimit`, `UsageSource = 'statusLine' | 'codexLog' | 'codexAppServer' | 'claudeApi'`, `ProviderUsage`; `cmd.getUsage(): Promise<ProviderUsage[]>`; `events.onUsageChanged(cb)`.

Merge rule in `record`: an older `observed_at` than the stored `updated_at` is dropped; a reading with a `None` window keeps the stored window (sources may report one window only); `per_model` empty keeps the stored list.

- [ ] **Step 1: Write failing tests in `store.rs` `#[cfg(test)] mod tests`**

```rust
#[test] fn record_then_snapshot_round_trips() // five_hour 62.0, weekly 31.0 come back; source and updated_at set
#[test] fn older_reading_is_dropped()         // observed_at 100 after 200 → record returns false, values of 200 kept
#[test] fn missing_window_keeps_stored_one()  // weekly None in 2nd reading → weekly 31.0 still present
#[test] fn expired_window_reads_as_reset()    // resets_at = now-1 → snapshot shows used_percent 0.0, resets_at None
#[test] fn persists_and_reloads()             // record, drop, UsageStore::load(same path) → same snapshot
#[test] fn corrupt_file_loads_empty()         // write "{nope" → load → snapshot empty
#[test] fn change_callback_fires_only_on_change()
```

- [ ] **Step 2: Run `cargo test usage::store` — FAIL (module missing).**
- [ ] **Step 3: Implement types, `UsageStore` (`parking_lot::Mutex<HashMap<(String,String), ProviderUsage>>`, write file via temp + rename), command, `AppState` field, `app.manage`, handler line, and the on-change hook emitting `usage:changed` from `setup()`.**
- [ ] **Step 4: Run `cargo test usage::store` — PASS. Run `pnpm typecheck` — PASS.**
- [ ] **Step 5: Commit** `feat(usage): limit snapshot store, get_usage and usage:changed`

---

### Task 2: Codex limits from rollout logs

**Files:**
- Create: `apps/desktop/src-tauri/src/services/usage/codex_log.rs`, `apps/desktop/src-tauri/src/services/usage/poller.rs`
- Create: `tests/fixtures/codex/limits/rollout-weekly-in-primary.jsonl` (two `token_count` lines; the second has `primary.window_minutes: 10080` and no 300-minute window)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (start poller after `app.manage`)

**Interfaces:**
- Consumes: `UsageStore::record`, `UsageReading`, `profiles::all(db)`, `codex::rollouts(codex_dir) -> Vec<PathBuf>`.
- Produces: `pub fn latest_reading(codex_dir: &Path) -> Option<UsageReading>` (newest `token_count` with non-null `rate_limits` across the 20 most recently modified rollouts; `source: CodexLog`; `observed_at` from the line's `timestamp`).
- Produces: `pub fn parse_rate_limits(v: &serde_json::Value, observed_at: i64) -> Option<UsageReading>` (shared with Task 3's snake_case shape).
- Produces: `poller::spawn(app: AppHandle, store: Arc<UsageStore>, db: Db)` — named thread `usage-poller`, 60 s tick; each tick runs Codex-log reads for every Codex profile. Tasks 3 and 5 add their sources to this same loop.

Window classification: `window_minutes <= 360` → five_hour; `>= 1440` → weekly; anything else ignored. `resets_at` is Unix seconds → ms.

- [ ] **Step 1: Failing tests in `codex_log.rs`**

```rust
#[test] fn reads_both_windows_from_the_repo_fixture()   // tests/fixtures/codex/sessions/... : assert values present in its token_count line, or None if that fixture has rate_limits null — check and assert accordingly
#[test] fn classifies_by_window_minutes_not_slot()      // limits fixture: weekly Some(used from primary), five_hour None
#[test] fn newest_line_wins()                          // two lines → second's numbers
#[test] fn null_rate_limits_is_none()
#[test] fn resets_at_seconds_become_ms()
```

- [ ] **Step 2: Run `cargo test usage::codex_log` — FAIL.**
- [ ] **Step 3: Implement; reuse `codex::Line` (agents/codex.rs:263) for deserialising; start `poller::spawn` in `setup()`.**
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** `feat(usage): Codex limits from rollout token_count events`

---

### Task 3: Codex live read via `codex app-server`

**Files:**
- Create: `apps/desktop/src-tauri/src/services/usage/codex_app_server.rs`
- Modify: `apps/desktop/src-tauri/src/services/usage/poller.rs`

**Interfaces:**
- Consumes: `services::agent_cli` binary discovery for `codex` (same resolution the spawn uses, incl. `settings::binary_override`), `UsageStore::record`.
- Produces: `pub fn parse_read_response(v: &serde_json::Value, observed_at: i64) -> Option<UsageReading>` — reads `result.rateLimits.{primary,secondary}.{usedPercent, windowDurationMins, resetsAt}` and `result.rateLimitsByLimitId` (entries other than `codex` → `per_model`, label = `limitName` or the id); `source: CodexAppServer`.
- Produces: `pub fn read(codex_bin: &Path, codex_home: &Path, timeout: Duration) -> anyhow::Result<UsageReading>` — spawns `codex -s read-only -a never app-server` with `CODEX_HOME`, writes newline-delimited JSON-RPC: `initialize` (`clientInfo: {name: "factorai", version: <app version>}`), `initialized` notification, `account/rateLimits/read`; reads stdout lines until the response with the matching id; kills the child afterwards in every path.

Poller: run at the first tick and then when ≥ 300 s since the last app-server read for that profile. Errors are logged at `debug` and ignored.

- [ ] **Step 1: Failing tests** — `parses_primary_and_secondary`, `per_limit_id_entries_become_per_model`, `classifies_by_window_duration`, `error_response_is_none`, using inline `serde_json::json!` values with the field names above.
- [ ] **Step 2: Run `cargo test usage::codex_app_server` — FAIL.**
- [ ] **Step 3: Implement parser and `read` (child stdout read on a helper thread with `recv_timeout`; child killed on drop).**
- [ ] **Step 4: Run — PASS. Manual: with Codex logged in, `cargo test -- --ignored live_codex_app_server` (an `#[ignore]` test printing the reading) shows real numbers.**
- [ ] **Step 5: Commit** `feat(usage): live Codex limits through codex app-server`

---

### Task 4: Claude limits through the status line

**Files:**
- Create: `apps/desktop/src-tauri/src/services/usage/statusline.rs`
- Modify: `apps/desktop/src-tauri/src/main.rs` (dispatch before `factorai_lib::run()`), `apps/desktop/src-tauri/src/lib.rs` (pub entry + `TerminalManager` builder call), `apps/desktop/src-tauri/src/services/terminal.rs` (`argv_for` ~L1520-1570, new builder `with_usage_sink_dir`), `apps/desktop/src-tauri/src/services/usage/poller.rs`

**Interfaces:**
- Produces: `pub const HELPER_FLAG: &str = "--usage-statusline";` and `pub fn run_helper(sink_dir: &Path, stdin: &[u8], env_config_dir: Option<&Path>) -> Vec<u8>` (returns what to print). It: parses stdin JSON; if `rate_limits` present writes `<sink_dir>/<session_id>.json` = `{"configDir": <CLAUDE_CONFIG_DIR or ~/.claude>, "observedAt": <now ms>, "rateLimits": <rate_limits>}` via temp + rename; then resolves the user's statusLine and returns its stdout (empty if none). Never panics; any error → returns the user's output or empty.
- Produces: `pub fn user_statusline(config_dir: &Path, cwd: &Path) -> Option<UserStatusLine { command: String, padding: Option<i64> }>` — precedence `cwd/.claude/settings.local.json` > `cwd/.claude/settings.json` > `config_dir/settings.json`; `cwd` from the payload's `workspace.current_dir`, else `cwd`.
- Produces: `pub fn parse_sink(v: &Value) -> Option<(PathBuf /*configDir*/, UsageReading)>` — `five_hour.used_percentage`/`resets_at` (s → ms), `seven_day.*`; `source: StatusLine`.
- Produces: `pub fn settings_arg(exe: &Path, sink_dir: &Path, padding: Option<i64>) -> String` — inline JSON `{"statusLine":{"type":"command","command":"\"<exe>\" --usage-statusline \"<sink_dir>\"", "padding": …}}`.
- `main.rs`: if `argv[1] == HELPER_FLAG`, read all stdin, call `run_helper`, write stdout, exit 0 — before any Tauri code runs.
- `argv_for`: for Claude only and only when `usage_sink_dir` is set, push `--settings` + `settings_arg(current_exe, sink_dir, user padding)` before the session flag.
- Running the user's command: Windows → Git Bash `bash -c` if found (same discovery Claude Code needs), else `cmd /C`; elsewhere `sh -c`; stdin piped, 5 s timeout.
- Poller: each tick, read every `<data_dir>/usage/statusline/*.json` newer than last tick, map `configDir` to a Claude profile by `Profile.config_dir` (path-normalised compare), `store.record`; delete sink files older than 7 days.

- [ ] **Step 1: Failing tests in `statusline.rs`**

```rust
#[test] fn sink_is_written_when_rate_limits_present()     // sink file has configDir and rateLimits
#[test] fn payload_without_rate_limits_is_ignored()       // no file written, output still the user's
#[test] fn helper_runs_the_users_statusline_and_prints_its_output() // settings.json statusLine "echo hi" → output "hi\n" (cfg-gated cmd per OS)
#[test] fn project_local_settings_win_over_user_settings()
#[test] fn parse_sink_converts_seconds_to_ms_and_maps_windows()
#[test] fn settings_arg_is_valid_json_with_quoted_paths()  // path with spaces, e.g. "C:\\Users\\Fyruz DEV\\..."
```
And in `terminal.rs` tests (next to `the_agent_tool_server_is_registered_by_name_in_argv`):
```rust
#[test] fn claude_argv_carries_the_usage_statusline_when_a_sink_is_set() // "--settings" present, JSON has statusLine.command containing "--usage-statusline"
#[test] fn no_settings_flag_without_a_sink_or_for_codex()
```

- [ ] **Step 2: Run `cargo test statusline` and `cargo test claude_argv_carries` — FAIL.**
- [ ] **Step 3: Implement as specified; set the sink dir to `<data_dir>/usage/statusline` in `lib.rs` via `.with_usage_sink_dir(...)`.**
- [ ] **Step 4: Run — PASS. Manual: `pnpm dev`, open a Claude session, send one message; `<data_dir>/usage/statusline/<id>.json` appears and a user statusLine (if configured) still renders.**
- [ ] **Step 5: Commit** `feat(usage): Claude limits forwarded from the status line`

---

### Task 5: Opt-in Anthropic usage endpoint

**Files:**
- Create: `apps/desktop/src-tauri/src/services/usage/claude_api.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml` (`ureq = { version = "2", features = ["json"] }` — rustls default), `apps/desktop/src-tauri/src/models/mod.rs` (`SettingKey::UsageClaudeEndpoint` → column `"usage.claude_endpoint"`, `"on"` = on, unset = off; `SettingKey::UsageTaskbarWidget` → `"usage.taskbar_widget"`, unset = on, `"off"` = off), `packages/types/src/index.ts` (SettingKey union), `apps/desktop/src-tauri/src/services/usage/poller.rs`

**Interfaces:**
- Produces: `pub fn parse_usage_response(v: &Value, observed_at: i64) -> UsageReading` — legacy shape `five_hour`/`seven_day` `{utilization, resets_at (ISO-8601)}` plus `seven_day_opus`/`seven_day_sonnet` → `per_model` ("Opus"/"Sonnet"); new shape `limits[]` with `kind` `session` → five_hour, `weekly_all` → weekly, `weekly_scoped` → per_model labelled `scope.model.display_name`; `percent` or `utilization`; unknown keys ignored; `source: ClaudeApi`.
- Produces: `pub fn read_token(config_dir: &Path, now_ms: i64) -> TokenState { Ok(String), Expired, Missing }` — `claudeAiOauth.accessToken`, `expiresAt` (ms).
- Produces: `pub struct Backoff` with `fn due(&self, profile_id: &str, now_ms: i64) -> bool` and `fn on_result(&mut self, profile_id: &str, now_ms: i64, status: u16, retry_after_s: Option<u64>)` — base interval 300 s; 429 → `max(retry_after, 300 s)` doubling to a 1 h cap; 2xx resets.
- Request: `GET https://api.anthropic.com/api/oauth/usage`, headers `Authorization: Bearer …`, `anthropic-beta: oauth-2025-04-20`, 15 s timeout, run on the poller thread (blocking is fine there).
- Poller: only when `settings::get(UsageClaudeEndpoint) == Some("on")`, per Claude profile, when `Backoff::due`.

- [ ] **Step 1: Failing tests** — `parses_legacy_shape_with_per_model`, `parses_limits_array_shape`, `unknown_keys_are_ignored`, `expired_token_is_reported_not_refreshed` (fixture credentials with `expiresAt` in the past → `Expired`), `missing_file_is_missing`, `backoff_honours_retry_after_and_caps_at_one_hour`, `success_resets_backoff`, `setting_key_columns_are_stable` (the two new `column()` strings).
- [ ] **Step 2: Run `cargo test usage::claude_api` — FAIL.**
- [ ] **Step 3: Implement. The token never appears in any log line or error string.**
- [ ] **Step 4: Run — PASS. `pnpm deps:check` — PASS.**
- [ ] **Step 5: Commit** `feat(usage): opt-in Anthropic usage endpoint`

---

### Task 6: Session tokens and estimated cost

**Files:**
- Create: `apps/desktop/src-tauri/src/services/usage/prices.rs`, `apps/desktop/src-tauri/src/services/usage/session_cost.rs`
- Modify: `apps/desktop/src-tauri/src/commands/usage.rs`, `apps/desktop/src-tauri/src/lib.rs` (handler), `apps/desktop/src-tauri/src/models/mod.rs`, `packages/types/src/index.ts`, `apps/desktop/src/lib/tauri.ts`

**Interfaces:**
- Produces (models, camelCase): `SessionUsage { session_id: String, model: Option<String>, input_tokens: u64, output_tokens: u64, cache_read_tokens: u64, cache_write_tokens: u64, reasoning_tokens: u64, total_tokens: u64, cost_usd: Option<f64> }`.
- Produces: `prices::price_for(model: &str) -> Option<Price { input: f64, output: f64, cache_read: f64, cache_write: f64 }>` (USD per million tokens; prefix match on model id so dated suffixes resolve; one `const TABLE` holding current list prices for the Claude 4.x/5.x and GPT-5.x Codex models).
- Produces: `session_cost::claude(path: &Path) -> SessionUsage` — sums `message.usage.{input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens}` over `type == "assistant"` lines, **deduplicated by `message.id`** (Claude writes one line per content block with the same usage), priced per line's `message.model`; `cost_usd` None if any priced line's model is unknown.
- Produces: `session_cost::codex(path: &Path) -> SessionUsage` — last `token_count` `info.total_token_usage.{input_tokens, cached_input_tokens, output_tokens, reasoning_output_tokens}`; model from `turn_context.model`; cached input priced as cache_read, reasoning as output.
- Command: `session_usage(session_id: String) -> AppResult<Option<SessionUsage>>` resolving the transcript like `commands/sessions.rs:481`, via `spawn_blocking`.
- TS: `SessionUsage`; `cmd.sessionUsage(sessionId)`.

- [ ] **Step 1: Failing tests** — `claude_sums_usage_once_per_message_id`, `claude_prices_by_model`, `unknown_model_has_tokens_but_no_cost`, `codex_reads_last_total_token_usage` (repo fixture `tests/fixtures/codex/sessions/2026/09/22/rollout-…jsonl`, assert its last `total_token_usage` numbers), `price_prefix_matches_dated_ids`.
- [ ] **Step 2: Run `cargo test usage::session_cost usage::prices` — FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** `feat(usage): per-session tokens and estimated cost`

---

### Task 7: Status bar, popover, amber token

**Files:**
- Create: `packages/ui/src/components/ui/popover.tsx` (Radix `@radix-ui/react-popover`, same version line as the other `@radix-ui/*` deps; shaped like `hover-card.tsx`), export from `packages/ui/src/index.ts`
- Create: `apps/desktop/src/components/layout/StatusBar.tsx`, `apps/desktop/src/components/layout/UsagePopover.tsx`, `apps/desktop/src/lib/usage.ts`, `apps/desktop/src/lib/usage.test.ts`, `apps/desktop/src/store/usageStore.ts`, `apps/desktop/src/hooks/useUsageSync.ts`
- Create: `tests/smoke/usage-status-bar.spec.ts`; Modify `tests/smoke/fixtures.ts` (a fixture with usage), `apps/desktop/src/lib/tauri.ts` mock (`get_usage`, `session_usage`)
- Modify: `apps/desktop/src/components/layout/AppShell.tsx` (insert `<StatusBar />` between L285 and L288), `apps/desktop/src/routes/__root.tsx` (mount `useUsageSync`), `apps/desktop/src/store/prefsStore.ts` (`showStatusBar: boolean`, default `true`, in interface, `DEFAULT_PREFS`, `partialize`, `currentPrefs`), `packages/ui/src/styles/globals.css` (`--usage-warn` in `:root` and `[data-theme="light"]`, `--color-usage-warn` in `@theme inline`), `DESIGN.md` § Colors (+ `.impeccable/design.json`), `specs/04-frontend.md` (StatusBar entry)

**Interfaces:**
- Consumes: `cmd.getUsage`, `events.onUsageChanged`, `cmd.sessionUsage`, `useAttentionStore(s => s.activeSessionId)`, `prefsStore.clock24`, `formatRelative`, `formatAbsolute`.
- Produces (`lib/usage.ts`):
  - `type UsageLevel = 'ok' | 'warn' | 'critical'`; `levelOf(percent: number): UsageLevel` (ok < 70, warn 70–89.999, critical ≥ 90)
  - `isStale(updatedAt: number, now: number): boolean` (> 15 min)
  - `formatTokens(n: number): string` → `"950"`, `"12.3k"`, `"1.8M"`
  - `formatCost(usd: number | null): string` → `"~$4.12"`, `"—"`
  - `barRows(usage: ProviderUsage[], installed: AgentId[]): BarRow[]` where `BarRow = { agent: AgentId, usage: ProviderUsage | null }` — one row per installed agent (from `hooks/useInstalledAgents.ts`), Claude before Codex, carrying the default profile's usage or `null` (rendered as `—`); an agent not installed has no row
- `usageStore` (Zustand, not persisted): `{ usage: ProviderUsage[], set(u) }`; `useUsageSync` loads `cmd.getUsage()` then subscribes `events.onUsageChanged` (pattern of `useSessionsSync.ts:47-75`).
- Session usage: TanStack Query key `['sessionUsage', sessionId]`, `refetchInterval: 15_000` while a session is active.
- Bar: `h-6` full width, `border-t`, `text-xs`; per provider `Claude  5h [bar] 62% · 16:40   wk [bar] 31%`; bar fill classes: ok → `bg-muted-foreground`, warn → `bg-usage-warn`, critical → `bg-destructive`; stale → whole group `opacity-60`; right side `1.8M tok · ~$4.12` for the active session; hidden entirely when `showStatusBar` is false. Click → `Popover` with every profile per agent, both windows with `formatAbsolute` reset, per-model limits, source label, "updated N min ago", and the session breakdown.
- `DESIGN.md` amendment: **Usage Amber** `--usage-warn` (dark `oklch(78% 0.15 75)`, light a darker step that reads on a 98% ground, e.g. `oklch(55% 0.13 70)`), single job "a plan limit is getting close"; `destructive` gains the second job "a plan limit is nearly out".

- [ ] **Step 1: Failing Vitest `lib/usage.test.ts`** — `levelOf(69.9)==='ok'`, `levelOf(70)==='warn'`, `levelOf(90)==='critical'`; `isStale` at 15 min exactly false, 15 min + 1 ms true; `formatTokens(950)==='950'`, `(12_300)==='12.3k'`, `(1_800_000)==='1.8M'`; `formatCost(4.123)==='~$4.12'`, `formatCost(null)==='—'`; `barRows` drops non-default profiles, orders claude then codex, gives `usage: null` for an installed agent with no data, and no row for an agent not installed.
- [ ] **Step 2: Run `pnpm --filter desktop test -- usage` — FAIL.**
- [ ] **Step 3: Implement lib, store, sync hook, Popover primitive, StatusBar, UsagePopover, token, pref, docs.**
- [ ] **Step 4: Smoke spec `@smoke usage status bar`** — fixture with Claude 62/31 and Codex 92/16: bar visible; Codex 5h fill has class `bg-destructive`; Claude 5h fill has `bg-muted-foreground`; clicking opens popover listing "Weekly"; with `showStatusBar: false` in prefs the bar is absent. Run `pnpm e2e -- usage-status-bar` — PASS.
- [ ] **Step 5: Run `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — PASS. Manual: `pnpm dev`, real numbers appear after a Claude message.**
- [ ] **Step 6: Commit** `feat(usage): status bar with plan limits and session cost`

---

### Task 8: Usage settings section

**Files:**
- Modify: `apps/desktop/src/lib/settingsDraft.ts:21-51` (`SETTINGS_SECTIONS` + `'usage'` after `'sessions'`), `apps/desktop/src/components/settings/SettingsModal.tsx` (section label, load at ~104-118, save at ~266-293, `prefsOf` ~832), `apps/desktop/src/lib/settingsDraft.test.ts`, `tests/smoke/settings.spec.ts`

**Interfaces:**
- Consumes: `SettingKey` `'usageClaudeEndpoint'`, `'usageTaskbarWidget'` (Task 5), pref `showStatusBar` (Task 7).
- Section "Usage", three `SettingRow` + `Switch`:
  1. "Show the status bar" → pref `showStatusBar`.
  2. "Show usage in the Windows taskbar" → `usageTaskbarWidget` (unset/on ⇄ `"off"`); rendered only on Windows.
  3. "Read Claude limits from Anthropic's usage endpoint" → `usageClaudeEndpoint` (`"on"` ⇄ unset). Description, verbatim: "Polls an undocumented Anthropic endpoint with the login Claude Code saved on this computer, for fresher limits and per-model figures. Anthropic's terms restrict third-party use of Claude.ai credentials; turn this on only if you accept that. factorai only reads the login, never changes or sends it anywhere else."

- [ ] **Step 1: Failing tests** — `settingsDraft.test.ts`: the `usage` section is dirty after toggling each of the three; smoke `settings.spec.ts` `@smoke usage section saves the endpoint toggle`: toggling and saving records a `set_setting` call with `{ key: 'usageClaudeEndpoint', value: 'on' }`.
- [ ] **Step 2: Run — FAIL.** **Step 3: Implement.** **Step 4: Run `pnpm test && pnpm e2e -- settings` — PASS.**
- [ ] **Step 5: Commit** `feat(usage): Usage settings section`

---

### Task 9: Taskbar widget rendering (cross-platform, testable)

**Files:**
- Create: `apps/desktop/src-tauri/src/platform/mod.rs` (if no `platform` module exists), `apps/desktop/src-tauri/src/platform/taskbar_widget/render.rs`, `apps/desktop/src-tauri/src/platform/taskbar_widget/mod.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml` (`tiny-skia`, `fontdue`), `apps/desktop/src-tauri/src/lib.rs` (`mod platform;`)

**Interfaces:**
- Consumes: `ProviderUsage`, the 70/90 thresholds (define `pub const WARN: f64 = 70.0; pub const CRITICAL: f64 = 90.0;` in `services/usage/mod.rs` and use them here).
- Produces: `pub struct WidgetRow { letter: char, five_hour: Option<f64>, weekly: Option<f64>, stale: bool }`, `pub fn rows_from(usage: &[ProviderUsage], now_ms: i64) -> Vec<WidgetRow>` (default profile per agent, Claude then Codex).
- Produces: `pub fn render(rows: &[WidgetRow], font: &fontdue::Font, scale: f32, light_taskbar: bool) -> tiny_skia::Pixmap` — logical size 150×36 at scale 1.0 (two 18 px rows: letter, 52 px bar, `62%`, `w 31%`), premultiplied RGBA, transparent background; text/neutral fill white on dark taskbar, black on light; fill colours: neutral = text colour at 70% alpha, warn = amber `#E8A33D`, critical = `#E5484D`; stale row at 60% alpha.
- Produces: `pub fn load_system_font() -> Option<fontdue::Font>` — Windows `%WINDIR%\Fonts\segoeui.ttf`; elsewhere `None`.

- [ ] **Step 1: Failing tests** — `rows_from_picks_default_profiles_in_order`, `render_has_scaled_size` (scale 1.5 → 225×54), `empty_rows_render_transparent` (all alpha 0), `critical_bar_contains_red_pixels` (a pixel inside the first row's bar area equals the critical colour, premultiplied). Tests use a font loaded from `tests/fixtures/fonts/` — add `LiberationSans-Regular.ttf` copied from `apps/desktop/public/pdfjs/standard_fonts/` (SIL OFL).
- [ ] **Step 2: Run `cargo test taskbar_widget::render` — FAIL.** **Step 3: Implement.** **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** `feat(usage): taskbar widget renderer`

---

### Task 10: Embed the widget in the Windows taskbar, tray fallback, ADR

**Files:**
- Create: `apps/desktop/src-tauri/src/platform/taskbar_widget/windows.rs` (`#[cfg(windows)]`), `apps/desktop/src-tauri/src/platform/taskbar_widget/tray.rs`
- Modify: `apps/desktop/src-tauri/src/platform/taskbar_widget/mod.rs` (public API + non-Windows no-op), `apps/desktop/src-tauri/Cargo.toml` (windows-sys features `Win32_UI_WindowsAndMessaging`, `Win32_Graphics_Gdi`, `Win32_UI_Accessibility`, `Win32_UI_HiDpi`, `Win32_UI_Shell`; tauri feature `tray-icon`), `apps/desktop/src-tauri/src/lib.rs` (start after `app.manage`, stop in the `CloseRequested` quit path at `:343-386`), `apps/desktop/src-tauri/src/services/usage/store.rs` (second on-change subscriber)
- Create: `specs/adr/0084-usage-monitor.md`; Modify: `specs/03-backend-rust.md`, `specs/05-features.md` (feature entry), `specs/04-frontend.md` if not done in Task 7

**Interfaces:**
- Produces (`mod.rs`): `pub struct WidgetHandle` with `pub fn start(app: AppHandle, initial: Vec<ProviderUsage>) -> Option<WidgetHandle>`, `pub fn update(&self, usage: Vec<ProviderUsage>)`, `pub fn stop(&self)`. `update`/`stop` only `PostMessageW` to the widget HWND or send on an `mpsc` then `PostThreadMessageW(WM_APP)`; **never `SendMessage`**. Non-Windows: all no-ops. Started only if `settings::get(UsageTaskbarWidget) != Some("off")`; toggling the setting starts/stops it (listen in `set_setting` for that key).
- Widget thread (`taskbar-widget`): `SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2)`; register class; create `WS_POPUP` with `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`; `FindWindowW("Shell_TrayWnd")` → `TrayNotifyWnd` child; switch style to `WS_CHILD`, `SetParent`; place at `TrayNotifyWnd.left - width - 8`, vertically centred; push pixels with `UpdateLayeredWindow` (`AC_SRC_ALPHA`); `SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, …, WINEVENT_OUTOFCONTEXT)` filtered to taskbar HWNDs and `OBJID_WINDOW`, coalesced with a 50 ms `SetTimer`; `RegisterWindowMessageW("TaskbarCreated")` → destroy and recreate; `WM_LBUTTONUP` → `app.get_webview_window("main")` `unminimize()` + `show()` + `set_focus()`; theme from `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\SystemUsesLightTheme`, re-read every 60 s.
- Fallback: if `Shell_TrayWnd` is missing, `SetParent` fails, `load_system_font()` is `None`, or 2 s after embedding `GetParent(hwnd) != taskbar || !IsWindowVisible(hwnd)`, destroy the window, log once at `warn`, and start `tray::TrayFallback` (Tauri `TrayIconBuilder`, 32×32 icon rendered with `render` of the most critical row, tooltip `Claude 5h 62% · wk 31%\nCodex 5h 18% · wk 16%`, left click focuses main).
- ADR-0084 (format of `specs/adr/0083-*.md`: Date, Status "Accepted for this local fork", Context, Decision, Consequences) records: status-line injection via `--settings` and the helper mode; first credential read (opt-in only, read-only, never refreshed); `codex app-server` refreshing its own token as accepted; `ureq` as the first HTTP client; cross-process `SetParent` is unsupported by Microsoft and may break on a Windows update, tray fallback is the contract; Usage Amber token.

- [ ] **Step 1: Failing test in `mod.rs`** — `widget_handle_only_posts`: on non-Windows `start` returns `None` and `update`/`stop` on a test handle built over an `mpsc` sender never block (call 1000× with no receiver draining — returns immediately). On Windows, an `#[ignore]` test `embeds_into_taskbar` starts the widget and asserts `GetParent(hwnd) == FindWindowW("Shell_TrayWnd")` within 2 s.
- [ ] **Step 2: Run — FAIL.** **Step 3: Implement `windows.rs`, `tray.rs`, wiring, ADR and spec edits.**
- [ ] **Step 4: Run `cargo test` and on Windows `cargo test -- --ignored embeds_into_taskbar` — PASS.**
- [ ] **Step 5: Manual QA (manual-qa skill) on Windows 11:** widget visible left of the tray with real numbers; click focuses factorai; `taskkill /f /im explorer.exe && start explorer` → widget comes back within a few seconds; switch Windows to light mode → text turns dark within 60 s; change display scale 100% → 150% → widget re-renders sharp; toggle the setting off → widget disappears, on → returns; quit factorai → widget gone; kill factorai from Task Manager → widget gone. Screenshot each for the record.
- [ ] **Step 6: Full gate** (Global Constraints) — PASS.
- [ ] **Step 7: Commit** `feat(usage): taskbar widget embedded in the Windows taskbar, tray fallback (ADR-0084)`
