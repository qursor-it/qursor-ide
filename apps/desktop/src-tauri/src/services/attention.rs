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

use crate::services::terminal::{AlertContext, TerminalId, TerminalStatus, TerminalStatusEvent};

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
	/// What a notification for `id` needs to name it. Read when the status
	/// event arrives, because an exiting terminal is gone from the manager by
	/// the time the settle check fires (F32).
	fn context(&self, id: &TerminalId) -> Option<AlertContext>;
	/// Raise the notification and the taskbar / Dock attention.
	fn send(&self, id: &TerminalId, status: TerminalStatus, ctx: Option<&AlertContext>);
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
		let due =
			prev.is_some_and(|p| wants_you(p, e.status)) && !self.focused.load(Ordering::Relaxed);
		if e.status == TerminalStatus::Stopped && !due {
			// Nothing more will come for this terminal; don't keep its entry.
			self.last.remove(&e.id);
		}
		due.then_some(generation)
	}

	/// The settle check: send only if nothing moved for `id` since
	/// `generation`, the window is still in the background and the setting is on.
	pub fn fire(&self, id: &TerminalId, generation: u64, ctx: Option<AlertContext>) {
		// A stale generation must neither send nor remove (a later event owns the entry).
		let status = match self.last.get(id) {
			Some(e) if e.generation == generation => e.status,
			_ => return,
		};
		if status == TerminalStatus::Stopped {
			self.last.remove_if(id, |_, e| e.generation == generation);
		}
		if self.focused.load(Ordering::Relaxed) || !self.sink.enabled() {
			return;
		}
		self.sink.send(id, status, ctx.as_ref());
	}

	/// `observe`, then `fire` on a short-lived thread after the settle.
	pub fn on_status(self: &Arc<Self>, e: &TerminalStatusEvent) {
		let Some(generation) = self.observe(e) else { return };
		let me = Arc::clone(self);
		let id = e.id.clone();
		// Still on the caller's thread, before the waiter removes the terminal.
		let ctx = self.sink.context(&id);
		let spawned =
			std::thread::Builder::new().name("attention-settle".into()).spawn(move || {
				std::thread::sleep(me.settle);
				me.fire(&id, generation, ctx);
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
			.with(|conn| {
				crate::services::settings::get(
					conn,
					crate::models::SettingKey::DesktopNotifications,
				)
			})
			.ok()
			.flatten();
		desktop_notifications_on(value.as_deref())
	}

	fn context(&self, id: &TerminalId) -> Option<AlertContext> {
		use tauri::Manager;
		let state = self.app.try_state::<crate::state::AppState>()?;
		state.terminals.alert_context(id)
	}

	fn send(&self, _id: &TerminalId, status: TerminalStatus, ctx: Option<&AlertContext>) {
		use rusqlite::OptionalExtension;
		use tauri::Manager;
		use tauri_plugin_notification::NotificationExt;

		let Some(state) = self.app.try_state::<crate::state::AppState>() else { return };
		let title = ctx
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
		let body = alert_body(status, ctx.and_then(|c| c.topic.as_deref()));

		if let Err(err) = self.app.notification().builder().title(title).body(body).show() {
			// No daemon on this Linux desktop, or notifications denied on macOS:
			// the dot and the attention request still carry the signal (F32).
			warn!(%err, "desktop notification not shown");
		}
		if let Some(window) = self.main_window() {
			if let Err(err) =
				window.request_user_attention(Some(tauri::UserAttentionType::Informational))
			{
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

#[cfg(test)]
mod tests {
	use super::*;
	use parking_lot::Mutex;
	use TerminalStatus::*;

	#[derive(Default)]
	struct FakeSink {
		disabled: AtomicBool,
		sent: Mutex<Vec<(TerminalId, TerminalStatus)>>,
		sent_ctx: Mutex<Vec<Option<AlertContext>>>,
		ctx: Mutex<Option<AlertContext>>,
		cleared: AtomicU64,
	}

	impl NotifySink for FakeSink {
		fn enabled(&self) -> bool {
			!self.disabled.load(Ordering::Relaxed)
		}
		fn context(&self, _id: &TerminalId) -> Option<AlertContext> {
			self.ctx.lock().clone()
		}
		fn send(&self, id: &TerminalId, status: TerminalStatus, ctx: Option<&AlertContext>) {
			self.sent.lock().push((id.clone(), status));
			self.sent_ctx.lock().push(ctx.cloned());
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
		assert_eq!(
			alert_body(WaitingInput, Some("Fix login bug")),
			"Waiting for you — Fix login bug"
		);
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
		n.fire(&"t".to_string(), g, None);
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
		n.fire(&"t".to_string(), g, None);
		assert!(sink.sent.lock().is_empty());
	}

	#[test]
	fn no_send_when_focus_returns_before_settle() {
		let (sink, n) = setup();
		n.observe(&ev("t", Working));
		let g = n.observe(&ev("t", WaitingInput)).expect("due");
		n.set_focused(true);
		n.fire(&"t".to_string(), g, None);
		assert!(sink.sent.lock().is_empty());
	}

	#[test]
	fn no_send_when_disabled() {
		let (sink, n) = setup();
		sink.disabled.store(true, Ordering::Relaxed);
		n.observe(&ev("t", Working));
		let g = n.observe(&ev("t", WaitingInput)).expect("due");
		n.fire(&"t".to_string(), g, None);
		assert!(sink.sent.lock().is_empty());
	}

	#[test]
	fn stopped_notifies_and_forgets_the_terminal() {
		let (sink, n) = setup();
		n.observe(&ev("t", Working));
		let g = n.observe(&ev("t", Stopped)).expect("due");
		n.fire(&"t".to_string(), g, None);
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

	#[test]
	fn stale_check_does_not_swallow_a_later_exit() {
		let (sink, n) = setup();
		let id = "t".to_string();
		n.observe(&ev("t", Working));
		let g1 = n.observe(&ev("t", WaitingInput)).expect("due");
		n.observe(&ev("t", Working));
		let g3 = n.observe(&ev("t", Stopped)).expect("due");
		n.fire(&id, g1, None);
		assert!(sink.sent.lock().is_empty());
		assert!(n.last.get("t").is_some());
		n.fire(&id, g3, None);
		assert_eq!(*sink.sent.lock(), vec![("t".to_string(), Stopped)]);
		assert!(n.last.get("t").is_none());
		n.fire(&id, g3, None);
		assert_eq!(sink.sent.lock().len(), 1);
	}

	#[test]
	fn the_context_read_at_observe_time_reaches_send_unchanged() {
		let sink = Arc::new(FakeSink::default());
		let n = Arc::new(AttentionNotifier::new(sink.clone(), Duration::from_millis(200)));
		n.set_focused(false);
		let ctx = AlertContext { project_id: "p1".into(), topic: Some("Fix login".into()) };
		*sink.ctx.lock() = Some(ctx.clone());
		n.observe(&ev("t", Working));
		n.on_status(&ev("t", Stopped));
		// The terminal is gone by the time the settle check runs: a re-read then
		// would find something else.
		*sink.ctx.lock() = None;
		// `send` records `sent_ctx` last, so once it is there the send is whole.
		for _ in 0..300 {
			if !sink.sent_ctx.lock().is_empty() {
				break;
			}
			std::thread::sleep(Duration::from_millis(10));
		}
		assert_eq!(*sink.sent_ctx.lock(), vec![Some(ctx)]);
		assert_eq!(*sink.sent.lock(), vec![("t".to_string(), Stopped)]);
	}
}
