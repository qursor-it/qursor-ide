# Session attention: an unseen dot and desktop notifications — design

Date: 2026-10-14
Branch: `feat/session-attention` (worktree `../factorai-session-attention`, based on `develop`)
Roadmap: items 34 (unread axis, partly) and 35 (desktop notifications), upstream issues #29 / #30 — cited, not closed (this is a fork).

## Intent

The user runs several sessions in parallel and also leaves the app to do other
things. Both matter equally:

- **In the app**: see at a glance which session wants them — a clearly visible
  red dot.
- **Outside the app**: be called back when a session wants them — an OS
  notification plus the taskbar / Dock asking for attention.

Must work on Windows, macOS and Linux.

## Decisions (agreed in brainstorming)

1. **One signal, "this session wants you".** "Finished its turn" and "is
   waiting on a permission prompt" are not distinguished: both read as
   `waiting_input` from the terminal title (ADR-0015), and ADR-0015 rejected
   hooks, the only reliable way to tell them apart. A later ADR can add hooks
   without reworking this design.
2. **Desktop notification only when the window is not focused.** Inside the
   app the red dot is enough; no in-app toast.
3. **Hybrid architecture.** Rust sends the desktop notification (it only needs
   window focus, which it can observe itself). The renderer owns the unseen
   dot (it knows which session is active).
4. **Red is reserved for the unseen dot** (besides errors). `DESIGN.md` is
   amended in the same PR; today it says "a surface that wants attention gets
   blue" (`DESIGN.md:831`).
5. **~1 s settle** before a desktop notification, so a momentary title flip
   does not notify.
6. **Not in v1**: numeric count on the taskbar/Dock icon, sounds, clicking a
   notification to open that specific session (a click only brings the window
   forward — Tauri's desktop notification click handling is not reliable
   across platforms).

## The transition rule

A session **wants you** on exactly these transitions of `TerminalStatus`:

| prev      | next            | wants you |
|-----------|-----------------|-----------|
| `working` | `waiting_input` | yes       |
| `working` | `stopped`       | yes       |
| anything else | anything    | no        |

`unknown → *` (Codex before its first title) never counts. Both
implementations (Rust `wants_you`, TS `wantsYou`) must match this table and
each is tested against it.

## Behaviour

### Unseen dot (renderer, always on)

- **Set** when a wants-you transition happens and the session is *not being
  looked at*: it is not the active session, **or** the window is not focused.
- **Shown** by painting the `StatusDot` red instead of its state colour (title
  `"<label> — unseen"`; amendment 1: a badge on a 6 px mark is not
  visible) on:
  - the session's tab (`SessionTabs`),
  - the session's row and pending rows in the sidebar (`SidebarProject`),
  - the project's avatar (`ProjectIcon`, including the collapsed rail) when any
    of its sessions is unseen. No dot on the project row, the project page
    lists or the session header.
- **Cleared** in two cases (amendment 2):
  - the session becomes, or is, the active route session while the window is
    focused;
  - the session's status goes back to `working` (someone typed into it).
  Not cleared on terminal exit: a `stopped` transition is followed at once by
  `terminal:exit`, and clearing there would erase the dot it set. A stale
  entry for a session with no tab renders nowhere and is cleared when the
  session is opened.
- **Boot title** (amendment 3): Rust never alerts on a terminal's first status
  event, so Claude's boot title `✳` does not notify. The renderer's previous
  status is the store's seed (`working`), so a session that boots to `✳` while
  not active and focused is marked unseen in the app. Accepted asymmetry; it is
  the session you just started, so it is almost always active.
- Not persisted: like the rest of the terminal store, it does not survive a
  restart.

### Desktop notification (Rust, toggleable)

- On a wants-you transition, if the window is not focused and the setting is
  on, start a ~1 s timer. When it fires, send only if the session's last
  status is still the one that triggered it, the window is still unfocused
  and the setting is still on. One notification per transition.
- **Content**: title = project name. Body = "Waiting for you" / "Exited",
  followed by the topic Claude puts in the terminal title when there is one
  (`"✳ Fix login bug"` → `"Waiting for you — Fix login bug"`). Rust does not
  know the session label the sidebar shows (the renderer derives it), hence
  the project name.
- Together with the notification: `request_user_attention(Informational)` on
  the main window. Cleared (`request_user_attention(None)`) when the window
  regains focus.
- A failed send (no notification daemon, permission denied) is logged with
  `warn!` and ignored. The dot and taskbar/Dock attention still work.

### Per platform

| | Notification | Attention | Notes |
|---|---|---|---|
| Windows | Action Center toast | Taskbar flashes until focus | Under `pnpm dev` the toast may lack the app identity; the NSIS build has it. |
| macOS | Notification Center | Dock icon bounces once | Tied to the bundle id: verify on the built `.app`, not the unsigned dev binary. If the user denied notifications in System Settings they drop silently. |
| Linux | D-Bus via `notify-rust` (needs a daemon; GNOME/KDE have one) | Urgency hint: X11 yes, Wayland compositor-dependent | AppImage env leakage does not affect D-Bus. |

### Setting

- `SettingKey::DesktopNotifications`, column key `notifications.desktop`, in
  the SQLite `settings` table (Rust reads it, so ADR-0013 puts it there).
  Unset = on.
- A "Desktop notifications" switch in `SettingsModal`, wired through
  `settingsDraft` and `cmd.setSetting('desktopNotifications', …)`, with the
  hint "On macOS and Linux, notifications also depend on system settings."
- The unseen dot has no setting.

## Architecture

### Rust

- **Plugin**: add `tauri-plugin-notification` to `src-tauri/Cargo.toml`,
  register it in `lib.rs` next to the others (`lib.rs:67-75`). No JS
  capability: the renderer never sends notifications.
- **`services/attention.rs`** — `AttentionNotifier`:
  - `focused: AtomicBool`;
  - `last: DashMap<TerminalId, ...>` — its own view of each terminal: the
    last status plus a generation counter, so `TerminalStatusEvent` and
    `set_status` stay as they are;
  - `fn wants_you(prev, next) -> bool` (pure);
  - `fn on_status(&self, e: &TerminalStatusEvent)` — updates `last` (a terminal's first event never
    alerts), and on a wants-you transition while unfocused schedules the
    settle check. The notification context (project id and the title's topic)
    is captured synchronously here, because an exiting terminal is gone from
    the manager before the check runs. The check sends only if the generation
    is unchanged; a stale check neither sends nor removes state;
  - `fn set_focused(&self, focused: bool)`;
  - sending goes through a small trait (`NotifySink`) so tests inject a fake;
    the real sink calls the plugin and `request_user_attention`, and reads the
    setting and project name.
  - An entry is dropped when its terminal reaches `Stopped`, so the map does
    not grow.
- **Topic**: the notifier needs the topic text of the last Claude title. The
  Claude branch of the PTY reader (`terminal.rs` ~2125) uses
  `titles.push(...)`, which keeps only the status. `TitleScanner` gains a
  sibling `push_title(bytes) -> Option<(TerminalStatus, Option<String>)>`
  built on the same `scan` + `classify`, so status detection is unchanged
  (last *classifiable* title in the chunk wins; empty titles are still
  ignored — `push_payload` would not ignore them, which is why it is not
  used). The Claude branch calls `push_title` and stores the topic on a new
  `TerminalHandle.topic: Mutex<Option<String>>` before `set_status`; the
  notifier reads it when sending. A pure
  `fn title_topic(&[u8]) -> Option<String>` (title minus the leading glyph,
  trimmed, `None` when empty) is unit-tested. Codex is untouched.
- **Wiring**: the `on_status` closure at `terminal.rs:796`, which today only
  emits `terminal:status`, also calls `notifier.on_status(&e)`. The notifier
  lives in `AppState`.
- **Focus**: `on_window_event` (`lib.rs:334`) handles
  `WindowEvent::Focused(f)`: `notifier.set_focused(f)`, and on `true` clears
  the user-attention request.

### Renderer

- **`lib/attention.ts`** — `wantsYou(prev, next)`, pure.
- **`store/attentionStore.ts`** — Zustand, not persisted:
  `unseen: Set<string>`, `windowFocused: boolean`, actions `mark(id)`,
  `clear(id)`, `setWindowFocused(f)`. `windowFocused` is fed by the existing
  `onWindowFocus` / `onWindowBlur` (`lib/tauri.ts:649-651`).
- **Marking**: in the `terminal:status` listener (`routes/__root.tsx`), read
  the previous status from `terminalStore` before `setStatus`; if
  `wantsYou(prev, next)` and the session is not (active and window focused),
  `mark(id)`. If `next === 'working'`, `clear(id)`.
- **Clearing on view**: when the active session changes, or the window gains
  focus, clear the active session if focused. Not cleared on exit (amendment 2).
- **UI**: `StatusDot` gains `unseen?: boolean`, painting the dot itself red
  (`bg-status-unseen`, token `--color-status-unseen: var(--destructive)`),
  title `"<label> — unseen"`, `data-unseen`. Passed by `SessionTabs`,
  `SidebarProject` (session rows and pending rows) and `ProjectIcon` (the
  avatar, including the collapsed rail). No dot on the project row, on the
  `routes/project.tsx` lists or on the session header. A
  `projectUnseen()` next to `projectStatus()` in `lib/sessionGroups.ts` gives
  the project aggregate.
- **Types**: `SettingKey` TS mirror in `packages/types` gains
  `desktopNotifications`.

## Testing

- **Rust (`cargo test`)**: `wants_you` over every status pair; notifier with a
  fake sink — no send if the status goes back to `working` within the settle
  window, if focus returns, or if the setting is off; exactly one send per
  transition; `title_topic`.
- **Vitest**: `wantsYou` against the same table; `attentionStore` marking,
  the two clearing rules, "active but window unfocused"; `projectUnseen()`.
- **Playwright smoke**: extend `tests/smoke/session-status.spec.ts` — the mock
  bridge emits `working → waiting_input` for a non-active session; the red
  dot appears on the tab, the sidebar row and the project avatar; opening the
  session removes it.
- **Real window**: Windows run by the implementer (session works, switch
  away, notification + taskbar flash; come back, red dot; open it, dot
  gone). macOS and Linux: a manual checklist in the PR.
- Full quality gate from `AGENTS.md` before calling it done.

## Documentation (same PR)

- **ADR-0081** "A session that wants you: an unseen dot and desktop
  notifications" — hybrid split and why, one signal within ADR-0015, red for
  unseen, platform differences.
- **`specs/05-features.md`**: new **F32** with the transition table, clearing
  rules, settle, setting and platform behaviour; update the deferred unread
  note (~`:2133`) to point at F32.
- **`DESIGN.md`**: `--status-unseen` token; amend § Status Dot (~`:700`) and
  the attention rule (`:831`).
- **`specs/03-backend-rust.md`, `specs/02-data-model.md`**: the new
  `SettingKey`.
- **Roadmap**: mark items 34 (partly) and 35.

## Risks

- Two copies of the transition rule can drift — mitigated by one table in F32
  and a test per side.
- macOS notification delivery cannot be verified from this Windows machine.
- Wayland may ignore the urgency hint; the notification and the dot still
  carry the signal.
