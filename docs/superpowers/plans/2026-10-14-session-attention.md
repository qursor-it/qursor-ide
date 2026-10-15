# Session Attention Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A red "unseen" dot on every session that started wanting you while you weren't looking, plus an OS notification and taskbar/Dock attention when the window is in the background — on Windows, macOS and Linux.

**Architecture:** Hybrid. Rust taps every `terminal:status` event into an `AttentionNotifier` (new `services/attention.rs`) that knows window focus and, after a 1 s settle, sends a desktop notification through `tauri-plugin-notification` and calls `request_user_attention`. The renderer owns the unseen state in a new Zustand `attentionStore`, fed by the same events, the route's active session and window focus, and draws it through a new `unseen` prop on `StatusDot`.

**Tech Stack:** Tauri 2, Rust (parking_lot, dashmap, rusqlite, tracing), tauri-plugin-notification 2, React 19, Zustand, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-10-14-session-attention-design.md` (read it first; this plan amends it in three places, listed under "Spec amendments").

All paths below are relative to the worktree root `E:\Developing\factorai-session-attention`. Rust root: `apps/desktop/src-tauri`. Renderer root: `apps/desktop/src`.

## Global Constraints

- Transition rule, both sides: wants-you iff `prev == working` and `next ∈ {waiting_input, stopped}`.
- Desktop notification only when the main window is not focused; settle `1000 ms`; one notification per transition.
- Setting: Rust `SettingKey::DesktopNotifications`, column `notifications.desktop`, TS key `'desktopNotifications'`. Unset = on; the string `off` = off. Nothing else is ever written.
- No JS capability for notifications: only Rust sends.
- A failed notification send is `warn!`-logged and ignored, never an error to the user.
- Never `unwrap()` outside `setup()`; no `#[allow]`, `as any`, `oxlint-disable`. Clippy `-D warnings`.
- `@factorai/ui` primitives only (`Switch`, `SettingRow` already used in `SettingsModal`).
- Code comments cite `specs/`, an ADR or `DESIGN.md` (use "F32" and "ADR-0081"), never AGENTS.md or `.claude/`.
- Commits prefixed `feat:` / `test:` / `docs:` / `chore:`, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Work only in the worktree `E:\Developing\factorai-session-attention` on branch `feat/session-attention`. Never touch the `develop` checkout.

## Spec amendments (apply to the spec doc in Task 7)

1. **The dot turns red instead of carrying a corner badge.** Session marks are 6 px (`size-1.5`); a badge on a 6 px dot is not visible. Unseen ⇒ `StatusDot` paints `bg-status-unseen` (red) and its title becomes `"<label> — unseen"`; once seen it returns to green/blue/grey.
2. **Clearing rules are two, not four.** Cleared when the session becomes (or is) the active route session while the window is focused, and when its status goes back to `working`. Not cleared on terminal exit — a `stopped` transition is followed at once by `terminal:exit`, and clearing there would erase the very badge it set. A stale entry for a session with no tab is invisible (no mark renders it) and is cleared the moment the session is opened.
3. **A first status event with no known previous status never alerts in Rust** (the notifier has no entry for a terminal until its first event, so Claude's boot title `✳` does not notify). The renderer's previous status is the store's seed (`working`), so a session that boots to `✳` while *not* active and focused is marked unseen in the app; it is the session you just started, so it is almost always active. Accepted asymmetry, recorded in F32.

## Review Focus

1. **Boot title** — a Claude session spawning and showing `✳` within 300 ms must not produce a desktop notification. Pinned by `observe_ignores_first_event` (Task 2).
2. **Spinner flicker** — `working → waiting_input → working` inside the settle window must not notify. Pinned by `no_send_when_status_moves_before_settle` (Task 2).
3. **Exit right after waiting** — `working → stopped` then `terminal:exit` must keep the red dot in the tab. Pinned by the store test "stopped marks and nothing clears it but viewing" (Task 4) and by not wiring any exit clearing.
4. **Active session while the window is in the background** — the session you are looking at goes idle while you are in another app: red dot on it, cleared when the window regains focus. Pinned by store tests (Task 4) and the smoke test using `tauri://blur` / `tauri://focus` (Task 5).
5. **Setting off** — `notifications.desktop = off` must suppress the OS notification and the taskbar attention but not the red dot. Pinned by `no_send_when_disabled` (Task 2); the dot has no setting by construction.

---

### Task 1: Rust — remember the title's topic

**Files:**
- Modify: `apps/desktop/src-tauri/src/services/osc_title.rs` (add `push_title`, `title_topic`; tests in its `mod tests`)
- Modify: `apps/desktop/src-tauri/src/services/terminal.rs` (`TerminalHandle` struct ~:637, its only literal ~:1835, Claude branch of the reader ~:2134, new `alert_context` on `TerminalManager`)

**Interfaces:**
- Produces: `TitleScanner::push_title(&mut self, bytes: &[u8]) -> Option<(TerminalStatus, Option<String>)>`; `pub fn title_topic(payload: &[u8]) -> Option<String>` in `osc_title`; `pub struct AlertContext { pub project_id: String, pub topic: Option<String> }` and `TerminalManager::alert_context(&self, id: &TerminalId) -> Option<AlertContext>` in `terminal`.

- [ ] **Step 1: Write the failing tests** in `osc_title.rs` `mod tests` (after the existing tests):

```rust
	#[test]
	fn push_title_reads_status_and_topic_together() {
		let mut s = TitleScanner::default();
		assert_eq!(
			s.push_title(IDLE_NAMED),
			Some((TerminalStatus::WaitingInput, Some("Date command".to_string())))
		);
		assert_eq!(
			s.push_title(WORKING),
			Some((TerminalStatus::Working, Some("Claude Code".to_string())))
		);
	}

	#[test]
	fn push_title_ignores_an_empty_title_like_push_does() {
		// A real title, then an empty one in the same chunk: `push` keeps the
		// real one, and `push_title` must agree (F32).
		let mut chunk = IDLE_NAMED.to_vec();
		chunk.extend_from_slice(b"\x1b]0;\x07");
		let mut a = TitleScanner::default();
		let mut b = TitleScanner::default();
		assert_eq!(a.push(&chunk), Some(TerminalStatus::WaitingInput));
		assert_eq!(b.push_title(&chunk).map(|(s, _)| s), Some(TerminalStatus::WaitingInput));
	}

	#[test]
	fn title_topic_drops_the_glyph_and_trims() {
		assert_eq!(title_topic("\u{2733} Fix login bug ".as_bytes()), Some("Fix login bug".to_string()));
		assert_eq!(title_topic("\u{2810} Working on it".as_bytes()), Some("Working on it".to_string()));
		assert_eq!(title_topic("\u{2733}".as_bytes()), None);
		assert_eq!(title_topic(b"   "), None);
	}
```

- [ ] **Step 2: Run to see them fail**

Run: `cd apps/desktop/src-tauri && cargo test --lib osc_title`
Expected: compile error, `no method named push_title` / `cannot find function title_topic`.

- [ ] **Step 3: Implement** in `osc_title.rs`, right after `push` (~:70):

```rust
	/// The status and the topic of the last classifiable title in `bytes` (F32).
	/// Same `scan` and `classify` as `push`, so status detection cannot drift
	/// from it — an empty title is still skipped, which `push_payload` would not
	/// do. The topic is what a desktop notification names the session by.
	pub fn push_title(&mut self, bytes: &[u8]) -> Option<(TerminalStatus, Option<String>)> {
		self.scan(bytes, |payload| classify(payload).map(|s| (s, title_topic(payload))))
	}
```

and as a free function after `classify` (~:140):

```rust
/// A Claude title minus its leading glyph, trimmed: `"✳ Fix login bug"` is
/// `"Fix login bug"` (F32). `None` when nothing is left.
pub fn title_topic(payload: &[u8]) -> Option<String> {
	let text = String::from_utf8_lossy(payload);
	let mut chars = text.trim_start().chars();
	chars.next()?;
	let topic = chars.as_str().trim();
	(!topic.is_empty()).then(|| topic.to_string())
}
```

- [ ] **Step 4: Run** `cargo test --lib osc_title` — Expected: all pass, existing ones included.

- [ ] **Step 5: Store the topic on the handle.** In `terminal.rs`:
  - In `struct TerminalHandle` next to `status: Mutex<TerminalStatus>` (~:680) add:
    ```rust
    	/// The topic of the agent's last title (F32), for the desktop notification.
    	/// `None` until a Claude title arrives; Codex never sets it.
    	topic: Mutex<Option<String>>,
    ```
  - In the only literal `Arc::new(TerminalHandle { … })` (~:1835) add `topic: Mutex::new(None),`.
  - In the Claude branch of `spawn_reader` (~:2134) replace
    ```rust
    if let Some(next) = titles.push(&tmp[..n]) {
    	set_status(&id_r, &handle_r, next, &on_status);
    }
    ```
    with
    ```rust
    if let Some((next, topic)) = titles.push_title(&tmp[..n]) {
    	// Before the status, so a notifier woken by this status reads
    	// this title's topic, not the previous one (F32).
    	*handle_r.topic.lock() = topic;
    	set_status(&id_r, &handle_r, next, &on_status);
    }
    ```
  - If `TitleScanner::push` is now unused outside tests, clippy will flag `dead_code`: keep it and mark it `#[cfg(test)]` only if clippy complains; do not add `#[allow]`.
  - Add near `TerminalStatusEvent` (~:212):
    ```rust
    /// What a desktop notification needs to know about a terminal (F32).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct AlertContext {
    	pub project_id: String,
    	pub topic: Option<String>,
    }
    ```
  - Add to `impl TerminalManager` (next to `working_count`, ~:978):
    ```rust
    	/// The project and title topic of a live terminal, for F32's notification.
    	/// `None` once the terminal is gone.
    	pub fn alert_context(&self, id: &TerminalId) -> Option<AlertContext> {
    		self.terminals.get(id).map(|h| AlertContext {
    			project_id: h.project_id.clone(),
    			topic: h.topic.lock().clone(),
    		})
    	}
    ```

- [ ] **Step 6: Verify** `cargo test --lib` and `cargo clippy --all-targets -- -D warnings` (from `apps/desktop/src-tauri`). Expected: green.

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src-tauri/src/services/osc_title.rs apps/desktop/src-tauri/src/services/terminal.rs
git commit -m "feat: keep the topic of the agent's terminal title"
```

---

### Task 2: Rust — the notifier's decision core and the setting key

**Files:**
- Create: `apps/desktop/src-tauri/src/services/attention.rs`
- Modify: `apps/desktop/src-tauri/src/services/mod.rs` (add `pub mod attention;` in alphabetical order, before `pub mod file_watch;`… i.e. after `pub mod agent_tools;`)
- Modify: `apps/desktop/src-tauri/src/models/mod.rs` (`SettingKey` ~:852-873 and `column()` ~:876-894)

**Interfaces:**
- Consumes: `TerminalId`, `TerminalStatus`, `TerminalStatusEvent` from `crate::services::terminal`.
- Produces:
  - `pub fn wants_you(prev: TerminalStatus, next: TerminalStatus) -> bool`
  - `pub fn desktop_notifications_on(value: Option<&str>) -> bool`
  - `pub fn alert_body(status: TerminalStatus, topic: Option<&str>) -> String`
  - `pub const SETTLE: Duration`
  - `pub trait NotifySink: Send + Sync { fn enabled(&self) -> bool; fn send(&self, id: &TerminalId, status: TerminalStatus); fn clear_attention(&self); }`
  - `pub struct AttentionNotifier` with `new(sink: Arc<dyn NotifySink>, settle: Duration) -> Self`, `observe(&self, e: &TerminalStatusEvent) -> Option<u64>`, `fire(&self, id: &TerminalId, generation: u64)`, `on_status(self: &Arc<Self>, e: &TerminalStatusEvent)`, `set_focused(&self, focused: bool)`.
  - `SettingKey::DesktopNotifications` with column `"notifications.desktop"`.

- [ ] **Step 1: Add the setting key.** In `models/mod.rs` add the variant after `UpdateChannel`:

```rust
	/// Whether a session that wants you raises a desktop notification while the
	/// window is in the background (F32, ADR-0081). Unset → on; `off` → off.
	/// Rust reads it because Rust sends the notification.
	DesktopNotifications,
```

and in `column()` add `SettingKey::DesktopNotifications => "notifications.desktop",`.

- [ ] **Step 2: Write `attention.rs` with its tests first** (the module compiles against `todo!()` bodies only if you prefer; simplest is to write tests and implementation together and run once — but run the tests before committing). Full file:

```rust
//! A session that wants you, while you are somewhere else (F32, ADR-0081).
//!
//! Every `terminal:status` event passes through `AttentionNotifier::on_status`.
//! A wants-you transition while the main window is not focused schedules a
//! check `SETTLE` later; if nothing moved in between, the sink raises a desktop
//! notification and asks the taskbar / Dock for attention. The unseen dot is the
//! renderer's (F32) — this side only knows focus, which is all it needs.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tracing::warn;

use crate::services::terminal::{TerminalId, TerminalStatus, TerminalStatusEvent};

/// How long a wants-you status must hold before it notifies (F32): long
/// enough to swallow a momentary title flip, short enough to feel immediate.
pub const SETTLE: Duration = Duration::from_millis(1000);

/// F32's transition table: from `working` to `waiting_input` or `stopped`.
/// The renderer's `wantsYou` (`lib/attention.ts`) is the same table.
pub fn wants_you(prev: TerminalStatus, next: TerminalStatus) -> bool {
	prev == TerminalStatus::Working
		&& matches!(next, TerminalStatus::WaitingInput | TerminalStatus::Stopped)
}

/// The `notifications.desktop` row as a switch: anything but `off` is on, so
/// an install that never touched it notifies.
pub fn desktop_notifications_on(value: Option<&str>) -> bool {
	value != Some("off")
}

/// The notification's body: what happened, then the agent's topic if any.
pub fn alert_body(status: TerminalStatus, topic: Option<&str>) -> String {
	let what = if status == TerminalStatus::Stopped { "Exited" } else { "Waiting for you" };
	match topic {
		Some(t) => format!("{what} — {t}"),
		None => what.to_string(),
	}
}

/// Where a decided notification goes. A trait so the decision is testable
/// without a window, a database or a notification daemon.
pub trait NotifySink: Send + Sync {
	/// The setting, read at send time.
	fn enabled(&self) -> bool;
	/// Raise the notification and the taskbar / Dock attention.
	fn send(&self, id: &TerminalId, status: TerminalStatus);
	/// The window came back: stop asking for attention.
	fn clear_attention(&self);
}

struct Entry {
	status: TerminalStatus,
	generation: u64,
}

pub struct AttentionNotifier {
	/// The app starts in front, and Tauri only reports changes.
	focused: AtomicBool,
	last: DashMap<TerminalId, Entry>,
	next_generation: AtomicU64,
	sink: Arc<dyn NotifySink>,
	settle: Duration,
}

impl AttentionNotifier {
	pub fn new(sink: Arc<dyn NotifySink>, settle: Duration) -> Self {
		Self {
			focused: AtomicBool::new(true),
			last: DashMap::new(),
			next_generation: AtomicU64::new(1),
			sink,
			settle,
		}
	}

	/// Record `e` and answer the generation to check after the settle, when
	/// this event is a wants-you transition seen while the window is in the
	/// background. A terminal's first event has no previous status and never
	/// qualifies — Claude's boot title is `✳`, and that is not news.
	pub fn observe(&self, e: &TerminalStatusEvent) -> Option<u64> {
		let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
		let prev = self
			.last
			.insert(e.id.clone(), Entry { status: e.status, generation })
			.map(|old| old.status);
		let due = prev.is_some_and(|p| wants_you(p, e.status))
			&& !self.focused.load(Ordering::Relaxed);
		if e.status == TerminalStatus::Stopped && !due {
			// Nothing more will come for this terminal; don't keep its entry.
			self.last.remove(&e.id);
		}
		due.then_some(generation)
	}

	/// The settle check: send only if nothing moved for `id` since
	/// `generation`, the window is still in the background and the setting is on.
	pub fn fire(&self, id: &TerminalId, generation: u64) {
		let current = self.last.get(id).map(|e| (e.generation, e.status));
		let Some((g, status)) = current else { return };
		if status == TerminalStatus::Stopped {
			self.last.remove(id);
		}
		if g != generation || self.focused.load(Ordering::Relaxed) || !self.sink.enabled() {
			return;
		}
		self.sink.send(id, status);
	}

	/// `observe`, then `fire` on a short-lived thread after the settle.
	pub fn on_status(self: &Arc<Self>, e: &TerminalStatusEvent) {
		let Some(generation) = self.observe(e) else { return };
		let me = Arc::clone(self);
		let id = e.id.clone();
		let spawned = std::thread::Builder::new().name("attention-settle".into()).spawn(move || {
			std::thread::sleep(me.settle);
			me.fire(&id, generation);
		});
		if let Err(err) = spawned {
			warn!(%err, "could not schedule the attention check");
		}
	}

	pub fn set_focused(&self, focused: bool) {
		self.focused.store(focused, Ordering::Relaxed);
		if focused {
			self.sink.clear_attention();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use parking_lot::Mutex;
	use TerminalStatus::*;

	#[derive(Default)]
	struct FakeSink {
		disabled: AtomicBool,
		sent: Mutex<Vec<(TerminalId, TerminalStatus)>>,
		cleared: AtomicU64,
	}

	impl NotifySink for FakeSink {
		fn enabled(&self) -> bool {
			!self.disabled.load(Ordering::Relaxed)
		}
		fn send(&self, id: &TerminalId, status: TerminalStatus) {
			self.sent.lock().push((id.clone(), status));
		}
		fn clear_attention(&self) {
			self.cleared.fetch_add(1, Ordering::Relaxed);
		}
	}

	fn ev(id: &str, status: TerminalStatus) -> TerminalStatusEvent {
		TerminalStatusEvent { id: id.to_string(), status, last_activity: 0 }
	}

	fn setup() -> (Arc<FakeSink>, AttentionNotifier) {
		let sink = Arc::new(FakeSink::default());
		let n = AttentionNotifier::new(sink.clone(), Duration::ZERO);
		n.set_focused(false);
		(sink, n)
	}

	#[test]
	fn wants_you_matches_the_f32_table() {
		let all = [Working, WaitingInput, Stopped, Unknown];
		for prev in all {
			for next in all {
				let expected = prev == Working && (next == WaitingInput || next == Stopped);
				assert_eq!(wants_you(prev, next), expected, "{prev:?} -> {next:?}");
			}
		}
	}

	#[test]
	fn setting_is_on_unless_off() {
		assert!(desktop_notifications_on(None));
		assert!(desktop_notifications_on(Some("on")));
		assert!(!desktop_notifications_on(Some("off")));
	}

	#[test]
	fn body_names_the_event_and_the_topic() {
		assert_eq!(alert_body(WaitingInput, Some("Fix login bug")), "Waiting for you — Fix login bug");
		assert_eq!(alert_body(Stopped, None), "Exited");
	}

	#[test]
	fn observe_ignores_first_event() {
		let (_, n) = setup();
		assert_eq!(n.observe(&ev("t", WaitingInput)), None);
	}

	#[test]
	fn sends_once_after_a_wants_you_transition_in_the_background() {
		let (sink, n) = setup();
		n.observe(&ev("t", Working));
		let g = n.observe(&ev("t", WaitingInput)).expect("due");
		n.fire(&"t".to_string(), g);
		assert_eq!(*sink.sent.lock(), vec![("t".to_string(), WaitingInput)]);
	}

	#[test]
	fn no_send_while_focused() {
		let (sink, n) = setup();
		n.set_focused(true);
		n.observe(&ev("t", Working));
		assert_eq!(n.observe(&ev("t", WaitingInput)), None);
		assert!(sink.sent.lock().is_empty());
	}

	#[test]
	fn no_send_when_status_moves_before_settle() {
		let (sink, n) = setup();
		n.observe(&ev("t", Working));
		let g = n.observe(&ev("t", WaitingInput)).expect("due");
		n.observe(&ev("t", Working));
		n.fire(&"t".to_string(), g);
		assert!(sink.sent.lock().is_empty());
	}

	#[test]
	fn no_send_when_focus_returns_before_settle() {
		let (sink, n) = setup();
		n.observe(&ev("t", Working));
		let g = n.observe(&ev("t", WaitingInput)).expect("due");
		n.set_focused(true);
		n.fire(&"t".to_string(), g);
		assert!(sink.sent.lock().is_empty());
	}

	#[test]
	fn no_send_when_disabled() {
		let (sink, n) = setup();
		sink.disabled.store(true, Ordering::Relaxed);
		n.observe(&ev("t", Working));
		let g = n.observe(&ev("t", WaitingInput)).expect("due");
		n.fire(&"t".to_string(), g);
		assert!(sink.sent.lock().is_empty());
	}

	#[test]
	fn stopped_notifies_and_forgets_the_terminal() {
		let (sink, n) = setup();
		n.observe(&ev("t", Working));
		let g = n.observe(&ev("t", Stopped)).expect("due");
		n.fire(&"t".to_string(), g);
		assert_eq!(*sink.sent.lock(), vec![("t".to_string(), Stopped)]);
		assert!(n.last.get("t").is_none());
	}

	#[test]
	fn stopped_while_focused_forgets_the_terminal() {
		let (_, n) = setup();
		n.set_focused(true);
		n.observe(&ev("t", Working));
		assert_eq!(n.observe(&ev("t", Stopped)), None);
		assert!(n.last.get("t").is_none());
	}

	#[test]
	fn focus_clears_attention() {
		let (sink, n) = setup();
		let before = sink.cleared.load(Ordering::Relaxed);
		n.set_focused(true);
		assert_eq!(sink.cleared.load(Ordering::Relaxed), before + 1);
	}
}
```

- [ ] **Step 3: Run** `cd apps/desktop/src-tauri && cargo test --lib attention` — Expected: 12 tests pass. If `TerminalStatus` lacks `Debug` for the assert message, it derives `Debug` already (`terminal.rs:~108`).

- [ ] **Step 4: Run** `cargo clippy --all-targets -- -D warnings`. `AttentionNotifier` and the free functions are unused outside tests until Task 3; if clippy reports `dead_code`, proceed to Task 3 before committing rather than silencing it (commit Tasks 2 and 3 together in that case).

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/services/attention.rs apps/desktop/src-tauri/src/services/mod.rs apps/desktop/src-tauri/src/models/mod.rs
git commit -m "feat: decide when a session that wants you should notify"
```

---

### Task 3: Rust — wire the notifier: plugin, sink, status tap, focus

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml` (dependencies, next to `tauri-plugin-shell = "2"` ~:23)
- Modify: `apps/desktop/src-tauri/src/services/attention.rs` (add `TauriSink`)
- Modify: `apps/desktop/src-tauri/src/services/terminal.rs` (add `with_status_tap`; test in `mod tests`)
- Modify: `apps/desktop/src-tauri/src/state.rs` (`AppState` gains `attention`)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (plugin chain :67-72, manager construction ~:141, `app.manage(AppState{…})` ~:293, `on_window_event` :334)

**Interfaces:**
- Consumes: everything Task 1 and 2 produce.
- Produces: `TerminalManager::with_status_tap(self, tap: impl Fn(&TerminalStatusEvent) + Send + Sync + 'static) -> Self`; `AppState.attention: Arc<AttentionNotifier>`; `pub struct TauriSink` with `TauriSink::new(app: AppHandle) -> Self`.

- [ ] **Step 1: Failing test for the tap** in `terminal.rs` `mod tests`:

```rust
	#[test]
	fn a_status_tap_sees_every_status_event_and_the_emit_still_happens() {
		let emitted = Arc::new(StdMutex::new(Vec::new()));
		let tapped = Arc::new(StdMutex::new(Vec::new()));
		let (e2, t2) = (emitted.clone(), tapped.clone());
		let mgr = TerminalManager::with_callbacks(
			PathBuf::from(NO_STORE),
			Arc::new(|_| {}),
			Arc::new(move |e: TerminalStatusEvent| e2.lock().unwrap().push(e.status)),
			Arc::new(|_| {}),
		)
		.with_status_tap(move |e| t2.lock().unwrap().push(e.status));
		(mgr.on_status)(TerminalStatusEvent {
			id: "x".into(),
			status: TerminalStatus::WaitingInput,
			last_activity: 0,
		});
		assert_eq!(*emitted.lock().unwrap(), vec![TerminalStatus::WaitingInput]);
		assert_eq!(*tapped.lock().unwrap(), vec![TerminalStatus::WaitingInput]);
	}
```

Use whatever name the existing tests module uses for `std::sync::Mutex` (it calls `.lock().unwrap()`, so it is std's; check the `use` lines at the top of `mod tests` ~:2362 and import `std::sync::Mutex as StdMutex` if not already present).

- [ ] **Step 2: Run** `cargo test --lib a_status_tap` — Expected: compile error, no method `with_status_tap`.

- [ ] **Step 3: Implement** in `impl TerminalManager` next to the other `with_*` builders:

```rust
	/// Also hand every status event to `tap`, before it is emitted (F32). A
	/// builder like the others: call it before anything spawns.
	pub fn with_status_tap(
		mut self,
		tap: impl Fn(&TerminalStatusEvent) + Send + Sync + 'static,
	) -> Self {
		let inner = self.on_status.clone();
		self.on_status = Arc::new(move |e| {
			tap(&e);
			inner(e);
		});
		self
	}
```

Run `cargo test --lib a_status_tap` — Expected: PASS.

- [ ] **Step 4: Add the plugin.** `Cargo.toml`: add `tauri-plugin-notification = "2"` after `tauri-plugin-shell = "2"`. `lib.rs` plugin chain: add `.plugin(tauri_plugin_notification::init())` after `.plugin(tauri_plugin_shell::init())`. Do **not** add any `notification:*` permission to `capabilities/default.json` (no JS sends).

- [ ] **Step 5: `TauriSink`** — append to `attention.rs` above `#[cfg(test)]`:

```rust
/// The real sink: the setting from SQLite, the project's name for a title, the
/// notification plugin, and the main window's attention request (F32).
pub struct TauriSink {
	app: tauri::AppHandle,
}

impl TauriSink {
	pub fn new(app: tauri::AppHandle) -> Self {
		Self { app }
	}

	fn main_window(&self) -> Option<tauri::WebviewWindow> {
		use tauri::Manager;
		self.app.get_webview_window("main")
	}
}

impl NotifySink for TauriSink {
	fn enabled(&self) -> bool {
		use tauri::Manager;
		let Some(state) = self.app.try_state::<crate::state::AppState>() else { return false };
		let value = state
			.db
			.with(|conn| crate::services::settings::get(conn, crate::models::SettingKey::DesktopNotifications))
			.ok()
			.flatten();
		desktop_notifications_on(value.as_deref())
	}

	fn send(&self, id: &TerminalId, status: TerminalStatus) {
		use rusqlite::OptionalExtension;
		use tauri::Manager;
		use tauri_plugin_notification::NotificationExt;

		let Some(state) = self.app.try_state::<crate::state::AppState>() else { return };
		let ctx = state.terminals.alert_context(id);
		let title = ctx
			.as_ref()
			.and_then(|c| {
				state
					.db
					.with(|conn| {
						Ok(conn
							.query_row(
								"SELECT display_name FROM projects WHERE id = ?1",
								[&c.project_id],
								|row| row.get::<_, String>(0),
							)
							.optional()?)
					})
					.ok()
					.flatten()
			})
			.unwrap_or_else(|| self.app.package_info().name.clone());
		let body = alert_body(status, ctx.as_ref().and_then(|c| c.topic.as_deref()));

		if let Err(err) = self.app.notification().builder().title(title).body(body).show() {
			// No daemon on this Linux desktop, or notifications denied on macOS:
			// the dot and the attention request still carry the signal (F32).
			warn!(%err, "desktop notification not shown");
		}
		if let Some(window) = self.main_window() {
			if let Err(err) = window.request_user_attention(Some(tauri::UserAttentionType::Informational)) {
				warn!(%err, "could not request user attention");
			}
		}
	}

	fn clear_attention(&self) {
		if let Some(window) = self.main_window() {
			if let Err(err) = window.request_user_attention(None) {
				warn!(%err, "could not clear the attention request");
			}
		}
	}
}
```

If `Db::with`'s closure needs an explicit error conversion for `rusqlite::Error`, follow how `commands/projects.rs` uses `?` inside `db.with` (it relies on `From<rusqlite::Error> for AppError`).

- [ ] **Step 6: Wire it in `lib.rs` and `state.rs`.**
  - `state.rs`: add to `AppState` (with a doc comment):
    ```rust
    	/// Desktop notifications for a session that wants you (F32, ADR-0081).
    	pub attention: Arc<crate::services::attention::AttentionNotifier>,
    ```
  - `lib.rs` in `setup`, before `TerminalManager::for_app(…)` (~:141):
    ```rust
    let attention = Arc::new(services::attention::AttentionNotifier::new(
    	Arc::new(services::attention::TauriSink::new(app.handle().clone())),
    	services::attention::SETTLE,
    ));
    let attention_tap = attention.clone();
    ```
    and append to the manager's builder chain:
    `.with_status_tap(move |e| attention_tap.on_status(e))`
  - Add `attention,` to the `app.manage(AppState { … })` literal (~:293).
  - In `on_window_event` (:334), before the `CloseRequested` branch:
    ```rust
    if let WindowEvent::Focused(focused) = event {
    	// The notifier only notifies a window that is not in front, and stops
    	// asking for attention once it is (F32).
    	if let Some(state) = window.try_state::<AppState>() {
    		state.attention.set_focused(*focused);
    	}
    }
    ```
  - Use `std::sync::Arc` as already imported in `lib.rs` (check its `use` lines).

- [ ] **Step 7: Verify** (from `apps/desktop/src-tauri`): `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`. Expected: green.

- [ ] **Step 8: Commit**

```bash
git add apps/desktop/src-tauri
git commit -m "feat: raise a desktop notification when a background session wants you"
```

---

### Task 4: Renderer — unseen state

**Files:**
- Modify: `packages/types/src/index.ts` (`SettingKey` union ~:391-397)
- Create: `apps/desktop/src/lib/attention.ts`, `apps/desktop/src/lib/attention.test.ts`
- Create: `apps/desktop/src/store/attentionStore.ts`, `apps/desktop/src/store/attentionStore.test.ts`
- Modify: `apps/desktop/src/store/terminalStore.ts` (export `findSessionByTerminal`, ~:200)
- Modify: `apps/desktop/src/routes/__root.tsx` (status listener ~:208-237; new effects)

**Interfaces:**
- Produces:
  - `wantsYou(prev: TerminalStatus | undefined, next: TerminalStatus): boolean` (`@lib/attention`)
  - `desktopNotificationsOf(value: string | null | undefined): boolean`, `desktopNotificationsSetting(on: boolean): string | null` (`@lib/attention`)
  - `useAttentionStore` with state `{ unseen: Record<string, true>; windowFocused: boolean; activeSessionId: string | null }` and actions `observe(sessionId: string, prev: TerminalStatus | undefined, next: TerminalStatus): void`, `setWindowFocused(focused: boolean): void`, `setActiveSession(sessionId: string | null): void`. Keyed by **session id**, not terminal id.
  - `SettingKey` gains `'desktopNotifications'`.

- [ ] **Step 1: Types.** Add `| 'desktopNotifications'` to `SettingKey` in `packages/types/src/index.ts`.

- [ ] **Step 2: Failing tests** `apps/desktop/src/lib/attention.test.ts`:

```ts
import type { TerminalStatus } from '@factorai/types';
import { describe, expect, it } from 'vitest';
import { desktopNotificationsOf, desktopNotificationsSetting, wantsYou } from './attention';

const ALL: TerminalStatus[] = ['working', 'waiting_input', 'stopped', 'unknown'];

describe('wantsYou', () => {
	it('matches the F32 table', () => {
		for (const prev of ALL) {
			for (const next of ALL) {
				const expected = prev === 'working' && (next === 'waiting_input' || next === 'stopped');
				expect(wantsYou(prev, next), `${prev} -> ${next}`).toBe(expected);
			}
		}
	});

	it('never fires without a previous status', () => {
		expect(wantsYou(undefined, 'waiting_input')).toBe(false);
	});
});

describe('desktop notifications setting', () => {
	it('is on unless the row says off', () => {
		expect(desktopNotificationsOf(null)).toBe(true);
		expect(desktopNotificationsOf('off')).toBe(false);
	});

	it('writes off, or deletes the row for on', () => {
		expect(desktopNotificationsSetting(false)).toBe('off');
		expect(desktopNotificationsSetting(true)).toBeNull();
	});
});
```

`apps/desktop/src/store/attentionStore.test.ts`:

```ts
import { beforeEach, describe, expect, it } from 'vitest';
import { useAttentionStore } from './attentionStore';

const reset = () =>
	useAttentionStore.setState({ unseen: {}, windowFocused: true, activeSessionId: null });
const s = () => useAttentionStore.getState();

describe('attentionStore', () => {
	beforeEach(reset);

	it('marks a session that wants you when it is not the active one', () => {
		s().setActiveSession('a');
		s().observe('b', 'working', 'waiting_input');
		expect(s().unseen).toEqual({ b: true });
	});

	it('does not mark the active session while the window is focused', () => {
		s().setActiveSession('a');
		s().observe('a', 'working', 'waiting_input');
		expect(s().unseen).toEqual({});
	});

	it('marks the active session while the window is in the background', () => {
		s().setActiveSession('a');
		s().setWindowFocused(false);
		s().observe('a', 'working', 'waiting_input');
		expect(s().unseen).toEqual({ a: true });
	});

	it('clears the active session when the window comes back', () => {
		s().setActiveSession('a');
		s().setWindowFocused(false);
		s().observe('a', 'working', 'waiting_input');
		s().setWindowFocused(true);
		expect(s().unseen).toEqual({});
	});

	it('clears a session when it becomes active in a focused window', () => {
		s().observe('b', 'working', 'stopped');
		s().setActiveSession('b');
		expect(s().unseen).toEqual({});
	});

	it('does not clear on activation while the window is in the background', () => {
		s().setWindowFocused(false);
		s().observe('b', 'working', 'waiting_input');
		s().setActiveSession('b');
		expect(s().unseen).toEqual({ b: true });
	});

	it('clears when the session goes back to working', () => {
		s().observe('b', 'working', 'waiting_input');
		s().observe('b', 'waiting_input', 'working');
		expect(s().unseen).toEqual({});
	});

	it('stopped marks and nothing clears it but viewing', () => {
		s().observe('b', 'working', 'stopped');
		s().observe('b', 'stopped', 'stopped');
		expect(s().unseen).toEqual({ b: true });
	});

	it('ignores transitions that are not in the table', () => {
		s().observe('b', 'unknown', 'waiting_input');
		s().observe('c', 'waiting_input', 'stopped');
		expect(s().unseen).toEqual({});
	});
});
```

- [ ] **Step 3: Run** `pnpm --filter @factorai/desktop exec vitest run src/lib/attention.test.ts src/store/attentionStore.test.ts` (if the filter name differs, run `pnpm test -- attention` from `apps/desktop`; check `apps/desktop/package.json` `name`). Expected: FAIL, modules not found.

- [ ] **Step 4: Implement** `apps/desktop/src/lib/attention.ts`:

```ts
import type { TerminalStatus } from '@factorai/types';

/**
 * F32's transition table: a session wants you when it goes from `working` to
 * `waiting_input` or `stopped`. Rust's `services::attention::wants_you` is the
 * same table, and each side tests it.
 */
export function wantsYou(prev: TerminalStatus | undefined, next: TerminalStatus): boolean {
	return prev === 'working' && (next === 'waiting_input' || next === 'stopped');
}

/** The `notifications.desktop` row as a switch: anything but `off` is on (F32). */
export function desktopNotificationsOf(value: string | null | undefined): boolean {
	return value !== 'off';
}

/** The row to write: on is the absence of one, like the update channel's stable. */
export function desktopNotificationsSetting(on: boolean): string | null {
	return on ? null : 'off';
}
```

`apps/desktop/src/store/attentionStore.ts`:

```ts
import type { TerminalStatus } from '@factorai/types';
import { wantsYou } from '@lib/attention';
import { create } from 'zustand';

/**
 * Which sessions started wanting you while you were not looking (F32,
 * ADR-0081). Keyed by session id, because every surface that draws the dot is.
 *
 * Not persisted: like the terminals it describes, it does not survive a
 * restart. Not cleared on exit either — `stopped` is followed at once by
 * `terminal:exit`, and clearing there would erase the dot it just set.
 */
interface AttentionState {
	unseen: Record<string, true>;
	/** The app starts in front; Tauri reports only changes. */
	windowFocused: boolean;
	/** The route's session, mirrored here by `__root`. */
	activeSessionId: string | null;
	observe: (sessionId: string, prev: TerminalStatus | undefined, next: TerminalStatus) => void;
	setWindowFocused: (focused: boolean) => void;
	setActiveSession: (sessionId: string | null) => void;
}

function without(unseen: Record<string, true>, sessionId: string): Record<string, true> {
	if (!unseen[sessionId]) return unseen;
	const { [sessionId]: _gone, ...rest } = unseen;
	return rest;
}

export const useAttentionStore = create<AttentionState>()((set) => ({
	unseen: {},
	windowFocused: true,
	activeSessionId: null,
	observe: (sessionId, prev, next) =>
		set((s) => {
			if (next === 'working') return { unseen: without(s.unseen, sessionId) };
			if (!wantsYou(prev, next)) return s;
			if (s.windowFocused && s.activeSessionId === sessionId) return s;
			return { unseen: { ...s.unseen, [sessionId]: true } };
		}),
	setWindowFocused: (focused) =>
		set((s) => ({
			windowFocused: focused,
			unseen: focused && s.activeSessionId ? without(s.unseen, s.activeSessionId) : s.unseen,
		})),
	setActiveSession: (sessionId) =>
		set((s) => ({
			activeSessionId: sessionId,
			unseen: sessionId && s.windowFocused ? without(s.unseen, sessionId) : s.unseen,
		})),
}));
```

If oxlint flags the unused `_gone` binding, it is allowed by the `_` prefix convention already used in `SettingsModal.prefsOf`; if not, use `const rest = { ...unseen }; delete rest[sessionId];`.

- [ ] **Step 5: Run** the two test files again — Expected: PASS.

- [ ] **Step 6: Wire `__root.tsx`.**
  - In `terminalStore.ts` change `function findSessionByTerminal(` to `export function findSessionByTerminal(` (signature unchanged: `(bySession, terminalId) => string | undefined` — confirm the return type at ~:200 and use it as is).
  - Replace the status listener body in the terminal lifecycle effect (~:213):
    ```ts
    events
    	.onTerminalStatus((e) => {
    		// The previous status is read before it is overwritten: the unseen dot
    		// is about the transition, not the state (F32).
    		const terminals = useTerminalStore.getState();
    		const sessionId = findSessionByTerminal(terminals.bySession, e.id);
    		const prev = sessionId ? terminals.bySession[sessionId]?.status : undefined;
    		terminals.setStatus(e.id, e.status);
    		if (sessionId) useAttentionStore.getState().observe(sessionId, prev, e.status);
    	})
    	.then((fn) => {
    		unStatus = fn;
    	});
    ```
    keeping the surrounding `let unStatus` / cleanup exactly as they are.
  - Add two effects next to the other global listeners:
    ```ts
    // F32: the unseen dot needs to know whether you are looking at all.
    useEffect(() => {
    	let unFocus: (() => void) | undefined;
    	let unBlur: (() => void) | undefined;
    	const attention = useAttentionStore.getState();
    	events.onWindowFocus(() => attention.setWindowFocused(true)).then((fn) => {
    		unFocus = fn;
    	});
    	events.onWindowBlur(() => attention.setWindowFocused(false)).then((fn) => {
    		unBlur = fn;
    	});
    	return () => {
    		unFocus?.();
    		unBlur?.();
    	};
    }, []);

    // …and at what: the route's session (F32).
    useEffect(() => {
    	useAttentionStore.getState().setActiveSession(sessionId ?? null);
    }, [sessionId]);
    ```
    `sessionId` is the one already read at `__root.tsx:31`. Import `useAttentionStore` from `@store/attentionStore` and `findSessionByTerminal` from `@store/terminalStore`.

- [ ] **Step 7: Verify** from repo root: `pnpm typecheck && pnpm lint && pnpm test`. Expected: green.

- [ ] **Step 8: Commit**

```bash
git add packages/types apps/desktop/src/lib/attention.ts apps/desktop/src/lib/attention.test.ts apps/desktop/src/store/attentionStore.ts apps/desktop/src/store/attentionStore.test.ts apps/desktop/src/store/terminalStore.ts apps/desktop/src/routes/__root.tsx
git commit -m "feat: track which sessions wanted you while you were not looking"
```

---

### Task 5: Renderer — draw the red dot

**Files:**
- Modify: `packages/ui/src/styles/globals.css` (theme tokens ~:79-96)
- Modify: `apps/desktop/src/components/layout/StatusDot.tsx`
- Create: `apps/desktop/src/components/layout/StatusDot.test.tsx` only if the repo already has component tests with a DOM environment (check for any `*.test.tsx` under `apps/desktop/src/components`); otherwise rely on the smoke test.
- Modify: `apps/desktop/src/lib/sessionGroups.ts` (+ `sessionGroups.test.ts`)
- Modify: `apps/desktop/src/components/layout/SessionTabs.tsx` (tabs memo ~:131-137, tab render ~:595)
- Modify: `apps/desktop/src/components/layout/SidebarProject.tsx` (pending row ~:644, `SessionRow` ~:894, project icon ~:352)
- Modify: `apps/desktop/src/components/layout/ProjectIcon.tsx`
- Modify: `apps/desktop/src/components/layout/Sidebar.tsx` (`statusByProject` ~:213 and wherever it is passed down)
- Modify: `tests/smoke/session-status.spec.ts`

**Interfaces:**
- Consumes: `useAttentionStore` (`unseen: Record<string, true>`), `useSessionMarks()` (`Record<string, SessionMark>` keyed by session id, `SessionMark.projectId`).
- Produces: `StatusDot` prop `unseen?: boolean` (renders `data-unseen="true"` when set); `ProjectIcon` prop `unseen?: boolean`; `projectUnseen(marks: Record<string, { projectId: string }>, unseen: Record<string, true>, projectId: string): boolean`.

- [ ] **Step 1: Token.** In `globals.css`, beside `--color-status-waiting: var(--primary);` add:
  ```css
  	/* A session that wanted you while you were not looking (F32, ADR-0081). */
  	--color-status-unseen: var(--destructive);
  ```

- [ ] **Step 2: `StatusDot`.** Add `unseen?: boolean` to the props (doc: "Wanted you while you were not looking (F32): painted red until seen."), and in the render:
  - class: replace `inBackground ? 'bg-status-background' : COLOR[status]` with `unseen ? 'bg-status-unseen' : inBackground ? 'bg-status-background' : COLOR[status]`;
  - `title={unseen ? \`${LABEL[status]} — unseen\` : LABEL[status]}` (keep however `title` is set today, only extending it);
  - `data-unseen={unseen || undefined}`.
  - Pulse stays tied to `status === 'working'`, so an unseen dot never pulses.

- [ ] **Step 3: Failing test for `projectUnseen`** in `sessionGroups.test.ts`:

```ts
describe('projectUnseen', () => {
	const marks = {
		a: { projectId: 'p1' },
		b: { projectId: 'p1' },
		c: { projectId: 'p2' },
	};

	it('is true when any of the project’s marked sessions is unseen', () => {
		expect(projectUnseen(marks, { b: true }, 'p1')).toBe(true);
	});

	it('ignores other projects and sessions with no mark', () => {
		expect(projectUnseen(marks, { c: true, zz: true }, 'p1')).toBe(false);
	});
});
```

(add `projectUnseen` to the file's existing import from `./sessionGroups`). Run `vitest run src/lib/sessionGroups.test.ts` — Expected: FAIL.

- [ ] **Step 4: Implement** in `sessionGroups.ts` after `projectStatus`:

```ts
/**
 * Whether any of a project's marked sessions wanted you unseen (F32) — the
 * project row's red dot. Over the marks, like `projectStatus`'s callers, so a
 * session whose process already exited still counts while its tab is open.
 */
export function projectUnseen(
	marks: Record<string, { projectId: string }>,
	unseen: Record<string, true>,
	projectId: string,
): boolean {
	for (const [sessionId, mark] of Object.entries(marks)) {
		if (mark.projectId === projectId && unseen[sessionId]) return true;
	}
	return false;
}
```

Run the test — Expected: PASS.

- [ ] **Step 5: Thread `unseen` to every dot.**
  - `SessionTabs.tsx`: `const unseen = useAttentionStore((s) => s.unseen);` next to the other store reads; in the `tabs` memo add `unseen: unseen[t.sessionId] === true,` and `unseen` to its deps; pass it down the same path `status` takes to the tab component and render `<StatusDot status={status} unseen={unseen} />` (~:595).
  - `SidebarProject.tsx`: in the pending list component (where `open = useSessionMarks()` ~:552) and in `SessionRow` read `const unseen = useAttentionStore((s) => s.unseen);` and pass `unseen={unseen[p.sessionId] === true}` (~:644) / `unseen={unseen[session.id] === true}` (~:894).
  - `ProjectIcon.tsx`: add `unseen?: boolean` to `ProjectIconProps` (doc: "Any of the project's sessions wanted you unseen (F32)."), pass it to its `StatusDot`.
  - `Sidebar.tsx`: beside `statusByProject` build
    ```ts
    const unseen = useAttentionStore((s) => s.unseen);
    const unseenByProject = useMemo(() => {
    	const ids = new Set(Object.values(open).map((t) => t.projectId));
    	return new Map([...ids].map((id) => [id, projectUnseen(open, unseen, id)]));
    }, [open, unseen]);
    ```
    and pass `unseenByProject.get(project.id)` wherever `statusByProject.get(…)` is passed, down to `<ProjectIcon … status={liveStatus} unseen={…} />` (`SidebarProject.tsx` ~:352). Follow the existing prop name pattern (`liveStatus` → `liveUnseen`).

- [ ] **Step 6: Smoke test.** Append to `tests/smoke/session-status.spec.ts`:

```ts
async function emitWindow(page: import('@playwright/test').Page, event: 'tauri://blur' | 'tauri://focus') {
	await page.evaluate((event) => window.__FACTORAI_EMIT__?.(event, null), event);
}

test('@smoke a session that wants you while the window is away turns red until seen', async ({
	page,
}) => {
	await openSession(page);
	await emitStatus(page, 'working');

	// Looking at it: going idle is not news.
	await emitStatus(page, 'waiting_input');
	await expect(page.locator('[data-unseen]')).toHaveCount(0);

	// Away: the same transition marks it, on the tab and in the sidebar (F32).
	await emitStatus(page, 'working');
	await emitWindow(page, 'tauri://blur');
	await emitStatus(page, 'waiting_input');
	await expect(page.getByTestId('session-tabs').locator('[data-unseen]')).toHaveCount(1);
	await expect(page.locator('aside [data-unseen]')).not.toHaveCount(0);

	// Back: it is the session in front, so coming back is seeing it.
	await emitWindow(page, 'tauri://focus');
	await expect(page.locator('[data-unseen]')).toHaveCount(0);
});
```

- [ ] **Step 7: Verify** `pnpm typecheck && pnpm lint && pnpm test && pnpm e2e` (the e2e run must not collide with a running `pnpm dev` on port 1420). Expected: green.

- [ ] **Step 8: Commit**

```bash
git add packages/ui/src/styles/globals.css apps/desktop/src tests/smoke/session-status.spec.ts
git commit -m "feat: paint a session red when it wanted you unseen"
```

---

### Task 6: Renderer — the Settings switch

**Files:**
- Modify: `apps/desktop/src/lib/settingsDraft.ts` (`SettingsValues`, `SECTION_FOR`)
- Modify: `apps/desktop/src/lib/settingsDraft.test.ts` (`SAVED` fixture ~:17)
- Modify: `apps/desktop/src/components/settings/SettingsModal.tsx` (sqlite query ~:100-107, save ~:258-283, sessions section ~:560-575, `prefsOf` ~:611)

**Interfaces:**
- Consumes: `desktopNotificationsOf`, `desktopNotificationsSetting` (`@lib/attention`); `cmd.getSetting` / `cmd.setSetting`.
- Produces: `SettingsValues.desktopNotifications: boolean` in section `'sessions'`.

- [ ] **Step 1: Failing test** in `settingsDraft.test.ts`: add `desktopNotifications: true,` to `SAVED`, and:

```ts
	it('puts the desktop notifications switch in Sessions', () => {
		const draft: SettingsValues = { ...SAVED, desktopNotifications: false };
		expect(dirtySections(SAVED, draft)).toEqual(['sessions']);
	});
```

Run `vitest run src/lib/settingsDraft.test.ts` — Expected: FAIL (type error / unknown key).

- [ ] **Step 2: Implement** in `settingsDraft.ts`: add to `SettingsValues`

```ts
	/** Desktop notifications for a session that wants you (F32). SQLite for the
	 *  same reason as the channel: Rust sends them and reads it there. */
	desktopNotifications: boolean;
```

and `desktopNotifications: 'sessions',` to `SECTION_FOR`. Run the test — Expected: PASS.

- [ ] **Step 3: `SettingsModal.tsx`.**
  - sqlite query: add `desktopNotifications: desktopNotificationsOf(await cmd.getSetting('desktopNotifications')),`.
  - save, after the update-channel block:
    ```ts
    // F32: Rust reads it at send time, so nothing else needs telling.
    if (draft.desktopNotifications !== saved.desktopNotifications) {
    	await cmd.setSetting('desktopNotifications', desktopNotificationsSetting(draft.desktopNotifications));
    }
    ```
  - add `desktopNotifications: draft.desktopNotifications,` to the `queryClient.setQueryData(queryKeys.setting('claudeBinaryPath'), { … })` object.
  - `prefsOf`: add `desktopNotifications: _notifications,` to the destructure.
  - sessions section, after the "Restore open tabs on launch" row:
    ```tsx
    <SettingRow
    	label="Desktop notifications"
    	htmlFor="settings-desktop-notifications"
    	description="When a session wants you and the window is in the background. They also depend on your system's notification settings."
    >
    	<Switch
    		id="settings-desktop-notifications"
    		data-testid="settings-desktop-notifications"
    		checked={draft.desktopNotifications}
    		onCheckedChange={(v) => set('desktopNotifications', v)}
    	/>
    </SettingRow>
    ```
  - Import from `@lib/attention`.

- [ ] **Step 4: Verify** `pnpm typecheck && pnpm lint && pnpm test && pnpm e2e`. Expected: green (fix any other `SettingsValues` literal the typechecker points at by adding `desktopNotifications: true`).

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/lib/settingsDraft.ts apps/desktop/src/lib/settingsDraft.test.ts apps/desktop/src/components/settings/SettingsModal.tsx
git commit -m "feat: a switch for desktop notifications"
```

---

### Task 7: Docs — ADR, F32, DESIGN.md, specs

**Files:**
- Create: `specs/adr/0081-a-session-that-wants-you-unseen-dot-and-desktop-notifications.md` (copy the header shape of `specs/adr/0080-tabs-are-grouped-by-project.md`)
- Modify: `specs/05-features.md` (new `## F32 — A session that wants you` after F31 ~:7212 section end; deferred unread note ~:2133)
- Modify: `DESIGN.md` (§ Status Dot ~:700-715; attention rule ~:830-832; token list ~:19-20)
- Modify: `specs/03-backend-rust.md` (settings table ~:1563-1568), `specs/02-data-model.md` (~:492)
- Modify: `specs/roadmap/README.md` (items 34, 35 ~:70-71)
- Modify: `docs/superpowers/specs/2026-10-14-session-attention-design.md` (the three amendments at the top of this plan)

- [ ] **Step 1: ADR-0081.** Status Accepted, date 2026-10-14. Context: roadmap 34/35, ADR-0015's single status source. Decision: one signal (`working → waiting_input|stopped`), no hooks; hybrid split (Rust notifies, renderer owns unseen) and why (Rust needs only focus; the renderer knows the active session); notification only when unfocused, 1 s settle; red reserved for unseen besides errors; setting `notifications.desktop`. Consequences: the transition table lives in two languages (tested on each side); platform table from the spec; click on a notification only focuses the window; "finished" vs "permission" can come later via hooks by superseding ADR-0015.
- [ ] **Step 2: F32** in `specs/05-features.md`: the transition table, the two clearing rules, settle, the setting, the per-platform table, the boot-title asymmetry (amendment 3), surfaces that show the dot (tab, sidebar session row, pending row, project avatar). At ~:2133 change the deferred unread note to say F32 covers "wanted you while unseen" and the never-opened axis remains deferred.
- [ ] **Step 3: DESIGN.md.** Token list: add `status-unseen` = `destructive` red. § Status Dot: add a paragraph "**Unseen red** — a session that wanted you while you were not looking is painted red until you look (F32, ADR-0081); it replaces the state colour rather than badging it, because a badge on a 6px mark is invisible." Amend :830-832 so it reads that attention gets blue, and a session that wanted you *unseen* gets red (ADR-0081).
- [ ] **Step 4:** `03-backend-rust.md` settings table: add row `| desktopNotifications | notifications.desktop | AttentionNotifier (TauriSink) | off, or unset for on (F32, ADR-0081) |`. `02-data-model.md:492`: add `notifications.desktop` to the list of keys. Roadmap: mark 34 "partly (F32: unseen; never-opened deferred)" and 35 "done (F32)" in the table's existing style.
- [ ] **Step 5:** Apply the three amendments to the design doc.
- [ ] **Step 6: Verify** `pnpm format:check && pnpm bytes:check` (run `pnpm format` if needed).
- [ ] **Step 7: Commit**

```bash
git add specs DESIGN.md docs/superpowers/specs
git commit -m "docs: ADR-0081 and F32 for the unseen dot and desktop notifications"
```

---

### Task 8: Full gate and the real window

- [ ] **Step 1: Gate**, from the worktree root, in order:

```bash
pnpm install --frozen-lockfile
pnpm format:check
pnpm bytes:check
pnpm lint
pnpm typecheck
pnpm test
pnpm e2e
pnpm deps:check
pnpm deps:unused
cd apps/desktop/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Expected: all green. `pnpm install --frozen-lockfile` must pass unchanged (no JS dependency was added); `Cargo.lock` changes are expected and committed.

- [ ] **Step 2: Real window on Windows** (`pnpm dev` from the worktree):
  1. Open a project, start a Claude session, send a prompt that takes ~10 s.
  2. Switch to another app before it finishes. Expected: a toast titled with the project name, body "Waiting for you — <topic>"; the taskbar button flashes.
  3. Return. Expected: flashing stops and the active session's dot clears (you are looking at it). Red is checked with a second, non-active session (step 4).
  4. With two sessions, let the non-active one finish while the app is in front. Expected: its tab dot, sidebar row dot and project avatar dot are red, no OS toast. Click its tab: dot returns to blue.
  5. Start a PR review or routine-style prompted session, blur the window, and wait for it to finish. Expected: a toast.
  6. Settings → Sessions → turn "Desktop notifications" off, repeat step 2. Expected: no toast, no flash; red dot still appears.
  7. Screenshot steps 3 and 4 for the PR.
- [ ] **Step 3: macOS / Linux checklist** for the PR body (not run here): steps 2-5 on the built `.app` / AppImage; on Linux note X11 vs Wayland for the urgency hint.
- [ ] **Step 4: Commit** any fix-ups, then stop: opening the PR (into `develop`, without `Closes #`, citing upstream #29/#30) is a separate decision for the user.
