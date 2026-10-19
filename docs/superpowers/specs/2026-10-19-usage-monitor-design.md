# Usage monitor: plan limits in a status bar and in the Windows taskbar — design

Date: 2026-10-19
Branch: `feat/usage-monitor`, from `develop` in its own worktree, merged locally into `develop` like recent features — no push, no PR (this fork is not published, ADR-0074). My call, following the recent history.
Prior art: [CodeZeno/Claude-Code-Usage-Monitor](https://github.com/CodeZeno/Claude-Code-Usage-Monitor) (taskbar widget), [steipete/CodexBar](https://github.com/steipete/CodexBar) (data sources).

## Intent

The user runs Claude Code and Codex sessions inside factorai on a subscription
(Claude Max, ChatGPT Plus). They want to know, at a glance and without typing
`/usage`, how much of their plan's limits is left before they hit the wall —
and how much the session they are working on has consumed.

Two places:

- **Inside factorai**: a thin full-width status bar at the bottom of the window.
- **In the Windows taskbar**: a small widget next to the clock, like CodeZeno's,
  visible only while factorai is running.

## Goals

1. For each provider (Claude, Codex): the **5-hour window %** and the
   **weekly %**, each with its **reset time**.
2. For the **session the user is looking at**: tokens used and an **estimated
   cost** (what it would cost at API list prices; a subscriber does not pay it,
   it shows how "heavy" the session is).
3. Color is the only alert: bars are neutral while things are fine, amber when
   close, red when nearly out. No notifications.
4. Stale data is labelled as stale ("updated 40 min ago"), never shown as fresh.
5. Multiple profiles (ADR-0036: one config dir per profile) are supported: each
   profile can be a different account with its own limits.

## Non-goals

- OpenCode, Gemini, Copilot, Cursor or any other provider (explicitly dropped).
- Anything running while factorai is closed (no separate process, no autostart).
- Desktop notifications or sounds when nearing a limit.
- Daily / 5-hour-block / historical cost reports (ccusage-style). Cost is per session only.
- Taskbar widget on macOS / Linux. They get the status bar only.
- Embedding on secondary monitors' taskbars (v1: primary taskbar only).

## Anti-goals (would be a failure even if it works)

- Signing the user out of Claude Code or Codex. factorai **never writes or
  refreshes** `~/.claude/.credentials.json` or `~/.codex/auth.json`, and never
  calls an OAuth token endpoint. Refresh-token rotation would log the CLI out.
- Sending a credential anywhere other than the provider's own host, or logging it.
- Spending the user's quota to measure quota (e.g. CodeZeno's 1-token Haiku
  fallback, or running `claude -p` / `codex exec` to force a token refresh).
- Breaking the user's own Claude Code `statusLine`.
- Hanging factorai's UI because Explorer hung (see the taskbar section).

## Decisions (agreed in brainstorming)

1. **Placement in the app: a full-width status bar** at the bottom of the
   window (option A of three; sidebar footer and top-bar chips were rejected).
   Left: per provider, 5h bar + % + reset, weekly bar + %. Right: tokens and
   estimated cost of the active session. Clicking opens a detail popover.
2. **Taskbar: a widget embedded in the Windows taskbar** next to the clock (as
   CodeZeno does), two rows (Claude, Codex): letter, bar, 5h %, weekly %. Chosen
   over notification-area icons and the progress bar on factorai's taskbar button.
3. **Only while factorai is running.** The widget lives and dies with the app.
4. **Claude data: official route by default, unofficial endpoint opt-in.** A
   setting, **off by default**, lets the user also poll Anthropic's undocumented
   usage endpoint, accepting the ToS grey zone and the 429s. See "Data sources".
5. **Cost is per session** (the one the user is looking at), not per day/window.
6. **Profiles**: kept as a feature (the user has one account today).
7. **Bar colours**: neutral below 70%, amber 70–89%, red from 90% (option A of
   two; an always-on new cyan was rejected). See "The status bar".

## Data sources

All of these are read-only. Research notes with exact field names are in the
brainstorming transcript; the essentials:

### Claude — official (default): the status-line JSON

Claude Code pipes a JSON object to its configured `statusLine` command on
stdin. For Pro/Max subscribers, after the session's first API response, it
contains:

- `rate_limits.five_hour.used_percentage` (0–100), `rate_limits.five_hour.resets_at` (Unix s)
- `rate_limits.seven_day.used_percentage`, `rate_limits.seven_day.resets_at`

Each window may be absent independently. Doc: https://code.claude.com/docs/en/statusline

factorai already builds the `claude` argv (`apps/desktop/src-tauri/src/services/terminal.rs`,
around the `--mcp-config` push). For sessions it spawns, it injects a
`statusLine` (e.g. via `--settings`) whose command forwards the JSON to
factorai and then **runs the user's own `statusLine` command, if any, with the
same stdin, and prints its output** — so the user's status line is unchanged.
How the command reaches factorai (a helper invocation of the factorai binary,
a POST to the existing local agent-tools server, a file under the app data dir…)
is left to the builder, with these constraints: no network beyond 127.0.0.1,
works on Windows without a POSIX shell, fails silently (a failing status-line
command must not disturb the session).

Consequence: Claude limits update only while a Claude session in factorai is
running and talking. Otherwise the bar shows the last known value, labelled stale.
Last-known snapshots (every provider and profile) **persist across app
restarts** (my call), so a fresh launch shows stale values rather than "—".
A window whose `resets_at` has passed is shown as reset (0%), not as stale data.

### Claude — unofficial (opt-in setting, off by default)

`GET https://api.anthropic.com/api/oauth/usage`, headers
`Authorization: Bearer <claudeAiOauth.accessToken>` and
`anthropic-beta: oauth-2025-04-20`. The token is read from
`<config dir>/.credentials.json` (`~/.claude` or the profile's
`CLAUDE_CONFIG_DIR`). Response: `five_hour` / `seven_day`
(`{utilization, resets_at}`), per-model weekly (`seven_day_opus`,
`seven_day_sonnet`, or the newer `limits[]` with `kind: weekly_scoped`). Parse
tolerantly: unknown keys ignored, both shapes accepted.

- Poll no more often than every 5 minutes; honour `Retry-After`; back off on 429.
- If the token is expired (`expiresAt`), **do not refresh it**: skip and wait for
  the CLI to refresh it on its next use.
- If the credential is not in the file (e.g. stored elsewhere), the setting
  shows "not available for this profile".
- The setting's description states plainly that the endpoint is undocumented
  and that Anthropic's terms restrict third-party use of Claude.ai credentials.
- When enabled, per-model weekly limits appear in the detail popover.

### Codex — session logs (offline)

Codex rollout files (`<CODEX_HOME>/sessions/YYYY/MM/DD/rollout-*.jsonl`), which
factorai's indexer already scans, contain `event_msg` lines with
`payload.type == "token_count"` carrying `payload.rate_limits`:
`primary` / `secondary` = `{used_percent, window_minutes, resets_at}`, plus
`plan_type`. Classify windows by `window_minutes` (300 = 5h, ≥ 1440 = weekly),
never by slot: Codex has been seen putting the weekly window in `primary`.
The newest snapshot across the profile's rollouts wins.

### Codex — official live read: `codex app-server`

`account/rateLimits/read` over the app-server's stdio JSON-RPC
(`initialize` → `initialized` → request) returns the same data live
(`rateLimits.primary/secondary.{usedPercent, windowDurationMins, resetsAt}`,
`rateLimitsByLimitId` for per-model limits). Doc: https://learn.chatgpt.com/docs/app-server

Used to refresh Codex limits while the app is open even when no Codex session
is running: at startup and then every ~5 minutes, only if the `codex` binary is
installed. Run it with the profile's `CODEX_HOME`, read-only sandbox, no
approvals. If it fails, fall back to the log snapshot silently.

Token refresh (my call): the app-server may refresh Codex's OAuth token itself
if it has expired. This is accepted: it is Codex's own code doing it, and it is
no different from a second Codex session running under the same `CODEX_HOME`,
which factorai already does routinely with parallel sessions. factorai itself
still never reads or writes `auth.json` for Codex.

### Per-session tokens and cost

- **Claude**: sum the `message.usage` of assistant entries in the session's
  transcript (`input_tokens`, `output_tokens`, `cache_creation_input_tokens`,
  `cache_read_input_tokens`), priced per `message.model`.
- **Codex**: the latest `token_count` event's cumulative `total_token_usage`
  (input, cached input, output, reasoning), priced per the session's model.
- A **bundled price table** (USD per million tokens, by model id, cache read /
  cache write / input / output) lives in one file so updating it is a one-line
  change. Unknown model → show tokens, cost "—".
- Display: total tokens (compact, e.g. `1.8M tok`) and `~$4.12`. The popover
  breaks it down (input / output / cache).

## The status bar

- Full width, bottom of the window, below the sidebar and the main column
  (`AppShell.tsx` today has no status bar; `specs/04-frontend.md` still lists a
  stale `layout/StatusBar.tsx`, which this work makes real).
- Left: one group per provider with known data — `Claude 5h ▮▮▮▯ 62% · reset 16:40  week ▮▯▯ 31%`,
  then Codex. A provider whose agent is not installed is hidden. A provider
  installed but with no data yet shows `—`.
- Right: the active session's tokens and estimated cost. Nothing if no session
  is focused.
- **Bar colours (agreed)**: neutral grey below 70%, amber 70–89%, red from 90%.
  `DESIGN.md` § Colors forbids reusing Session Green (`working`) or Qursor Blue
  (`waiting`) for this, so the bar is colourless until it has something to say.
  This needs a new amber token (dark + light values) and a `DESIGN.md`
  amendment naming amber's single job ("a plan limit is getting close"); the
  red is the existing `destructive` value, given the second job "a plan limit is
  nearly out" in the same amendment. The taskbar widget uses the same three
  states (its neutral is the taskbar's text colour).
- Thresholds (70 / 90) are my call.
- Stale (my call): a value older than 15 minutes gets a muted style and the
  popover says "updated N min ago".
- Click → popover: per provider and per profile, both windows with exact reset
  date/time, per-model weekly limits when known, source of the data
  (status line / log / app-server / API), last update time; session token breakdown.
- Default profile in the bar; all profiles in the popover (my call).
- Hideable from Settings (my call: on by default).

## The taskbar widget (Windows only)

Shape: two rows, `C ▮▮▮▯ 62%  w 31%` and `X ▮▯▯▯ 18%  w 16%`, transparent
background, white or black text following the taskbar theme. Click brings
factorai's main window to the front. Advice from a design review (Fable):

- **Native Win32 layered window, not a WebView.** A second Tauri window
  reparented into Explorer breaks (WebView2 composition and DPI are tied to the
  host thread) and costs ~60–100 MB. Use a `WS_EX_LAYERED | WS_EX_TOOLWINDOW |
  WS_EX_NOACTIVATE` window whose 32bpp premultiplied DIB is pushed with
  `UpdateLayeredWindow`, rasterised with `tiny-skia`; text via `ab_glyph` or
  `fontdue` from a small bundled font subset (ClearType is impossible on
  per-pixel-alpha windows). Re-render only when a number changes.
- **Embedding**: switch the style from `WS_POPUP` to `WS_CHILD`, then
  `SetParent` into `Shell_TrayWnd`, positioned just left of `TrayNotifyWnd`.
  Reposition on `SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE)`, filtered to the
  taskbar windows and coalesced with a ~50 ms timer.
- **Own thread**: a dedicated `std::thread` (not tokio, not tao) owns the
  window, its message loop and the WinEvent hook. Cross-process `SetParent`
  couples its input queue to Explorer's: if Explorer hangs, this thread hangs.
  Therefore **nothing else ever `SendMessage`s it** — updates arrive via a
  channel + `PostMessage`, and the thread holds no lock other code waits on.
- **Explorer restart**: listen for `RegisterWindowMessage("TaskbarCreated")`
  and recreate the window (the old child dies with its parent).
- **DPI**: per-monitor-v2 awareness on the widget thread, scale from
  `GetDpiForWindow(taskbar)`, re-render when the tray height changes.
- **Theme**: `HKCU\…\Themes\Personalize\SystemUsesLightTheme`, re-read
  periodically (broadcasts may not reach a child of Explorer).
- **Lifecycle**: destroyed on clean exit (the existing kill-on-quit path); if
  factorai crashes the kernel destroys the window, nothing dangles.
- **Click**: `WM_LBUTTONUP` → unminimize + focus the `main` window.
- **Code location**: Windows-only module behind `#[cfg(windows)]` with a no-op
  stub elsewhere; `windows-sys` features added as needed (no new `windows` crate).
- **Setting**: "Show usage in the Windows taskbar", on by default on Windows (my call).
- **Fallback (my call, adapted from the review)**: if embedding fails
  (`Shell_TrayWnd` not found, `SetParent` fails, or the widget is not visible
  under the taskbar within ~2 s), log once and fall back to a
  notification-area icon (Tauri `tray-icon` feature) with the most critical %
  drawn in it and the four numbers in the tooltip. No user-facing error.
- **Risk to record in the ADR**: cross-process `SetParent` into the taskbar is
  unsupported by Microsoft; a Windows update can break it without notice. The
  fallback is what keeps the feature alive.

## Architecture sketch (left to the builder in detail)

- Rust owns a **usage service**: collects snapshots from the four sources,
  keyed by (provider, profile), with source and timestamp; emits a Tauri event
  on change; exposes a command for the current state. It feeds both the
  renderer (status bar) and the taskbar widget thread.
- Session cost is computed in Rust from the transcripts the indexer already
  reads, and exposed per session id.
- Renderer: a `StatusBar` component in `components/layout/`, state from the
  event stream (follow `frontend-conventions` for Zustand vs TanStack Query).
- Settings: a new section or entries under an existing one for: show status bar,
  show taskbar widget (Windows), "Use Anthropic's unofficial usage endpoint"
  (off, with the warning text).
- An ADR (next free number, currently 0084) for: reading credentials for the
  first time (only under the opt-in), the statusLine injection, and the taskbar
  embed. Update `specs/` where the contract changes (frontend layout, settings keys,
  backend services).

## Left to the builder

- The statusLine forwarding mechanism (within the constraints above).
- Exact module/file layout, event names, setting keys.
- Polling intervals within the bounds given.
- Price table contents (current list prices for the Claude and OpenAI models
  the user can run) and its file location.
- Test strategy (fixtures for status-line JSON, `token_count` lines, the
  usage-endpoint shapes; the widget is verified manually in the real window).
