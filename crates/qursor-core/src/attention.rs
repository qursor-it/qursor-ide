//! A session that wants you, while you are somewhere else (F32, ADR-0081).
//!
//! The decision half of the app's `services::attention`, run in the core
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §1): every status
//! event passes through [`AttentionNotifier::on_status`]; a wants-you transition
//! while no client window is focused schedules a check `SETTLE` later, and if
//! nothing moved in between, a composed [`AttentionNotice`] goes out as an event.
//! Showing it — the OS notification, the taskbar — is the app's.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use rusqlite::OptionalExtension;
use serde::Serialize;
use tracing::warn;

use crate::db::Db;
use crate::models::SettingKey;
use crate::services::event_sink::{CoreEvent, EventSink};
use crate::services::terminal::{AlertContext, TerminalId, TerminalStatus, TerminalStatusEvent};

/// How long a wants-you status must hold before it notifies (F32): long
/// enough to swallow a momentary title flip, short enough to feel immediate.
pub const SETTLE: Duration = Duration::from_millis(1000);

/// What the app shows: title and body already composed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionNotice {
	pub session_id: Option<String>,
	pub terminal_id: TerminalId,
	pub title: String,
	pub body: String,
}

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
/// without a database or a socket.
pub trait NotifySink: Send + Sync {
	/// The setting, read at send time.
	fn enabled(&self) -> bool;
	/// What a notification for `id` needs to name it. Read when the status
	/// event arrives, because an exiting terminal is gone from the manager by
	/// the time the settle check fires (F32).
	fn context(&self, id: &TerminalId) -> Option<AlertContext>;
	fn send(&self, id: &TerminalId, status: TerminalStatus, ctx: Option<&AlertContext>);
}

struct Entry {
	status: TerminalStatus,
	generation: u64,
}

pub struct AttentionNotifier {
	/// Reported by the app (`attention.focus`); it starts in front.
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
	}
}

type ContextOf = Box<dyn Fn(&TerminalId) -> Option<AlertContext> + Send + Sync>;

/// The core's sink: the setting and the project's name from SQLite, the
/// terminal's context from the manager, the notice out as a [`CoreEvent`].
pub struct EventNotify {
	pub db: Db,
	pub events: Arc<dyn EventSink>,
	/// Set once the manager exists; the notifier is built before it.
	pub context: std::sync::OnceLock<ContextOf>,
}

impl NotifySink for EventNotify {
	fn enabled(&self) -> bool {
		let value = self
			.db
			.with(|conn| crate::services::settings::get(conn, SettingKey::DesktopNotifications))
			.ok()
			.flatten();
		desktop_notifications_on(value.as_deref())
	}

	fn context(&self, id: &TerminalId) -> Option<AlertContext> {
		self.context.get().and_then(|f| f(id))
	}

	fn send(&self, id: &TerminalId, status: TerminalStatus, ctx: Option<&AlertContext>) {
		let title = ctx
			.and_then(|c| {
				self.db
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
			// The app's product name, which `TauriSink` read from its package info.
			.unwrap_or_else(|| "qursor-ide".to_string());
		let body = alert_body(status, ctx.and_then(|c| c.topic.as_deref()));
		self.events.emit(CoreEvent::Attention(AttentionNotice {
			session_id: ctx.and_then(|c| c.session_id.clone()),
			terminal_id: id.clone(),
			title,
			body,
		}));
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
	fn stale_check_does_not_swallow_a_later_exit() {
		let (sink, n) = setup();
		let id = "t".to_string();
		n.observe(&ev("t", Working));
		let g1 = n.observe(&ev("t", WaitingInput)).expect("due");
		n.observe(&ev("t", Working));
		let g3 = n.observe(&ev("t", Stopped)).expect("due");
		n.fire(&id, g1, None);
		assert!(sink.sent.lock().is_empty());
		n.fire(&id, g3, None);
		assert_eq!(*sink.sent.lock(), vec![("t".to_string(), Stopped)]);
		n.fire(&id, g3, None);
		assert_eq!(sink.sent.lock().len(), 1);
	}

	#[test]
	fn the_context_read_at_observe_time_reaches_send_unchanged() {
		let sink = Arc::new(FakeSink::default());
		let n = Arc::new(AttentionNotifier::new(sink.clone(), Duration::from_millis(200)));
		n.set_focused(false);
		let ctx = AlertContext {
			project_id: "p1".into(),
			session_id: Some("s1".into()),
			topic: Some("Fix login".into()),
		};
		*sink.ctx.lock() = Some(ctx.clone());
		n.observe(&ev("t", Working));
		n.on_status(&ev("t", Stopped));
		*sink.ctx.lock() = None;
		for _ in 0..300 {
			if !sink.sent_ctx.lock().is_empty() {
				break;
			}
			std::thread::sleep(Duration::from_millis(10));
		}
		assert_eq!(*sink.sent_ctx.lock(), vec![Some(ctx)]);
	}

	struct Collect(Mutex<Vec<CoreEvent>>);
	impl EventSink for Collect {
		fn emit(&self, event: CoreEvent) {
			self.0.lock().push(event);
		}
	}

	#[test]
	fn the_core_sink_names_the_project_and_respects_the_setting() {
		let dir = tempfile::TempDir::new().unwrap();
		let db = Db::open(dir.path()).unwrap();
		db.with(|c| {
			c.execute(
				"INSERT INTO projects(id, display_name, real_path, opened_at) VALUES ('p1', 'Shop', '/p', 0)",
				[],
			)?;
			Ok(())
		})
		.unwrap();
		let events = Arc::new(Collect(Mutex::new(Vec::new())));
		let sink =
			EventNotify { db: db.clone(), events: events.clone(), context: Default::default() };
		let ctx = AlertContext {
			project_id: "p1".into(),
			session_id: Some("s1".into()),
			topic: Some("Fix login".into()),
		};
		assert!(sink.enabled());
		sink.send(&"t1".to_string(), WaitingInput, Some(&ctx));
		let sent = events.0.lock();
		let CoreEvent::Attention(notice) = &sent[0] else { panic!("{sent:?}") };
		assert_eq!(
			*notice,
			AttentionNotice {
				session_id: Some("s1".into()),
				terminal_id: "t1".into(),
				title: "Shop".into(),
				body: "Waiting for you — Fix login".into(),
			}
		);
		drop(sent);
		db.with(|c| {
			crate::services::settings::set(c, SettingKey::DesktopNotifications, Some("off"))
		})
		.unwrap();
		assert!(!sink.enabled());
	}
}
