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
		let spawned =
			std::thread::Builder::new().name("attention-settle".into()).spawn(move || {
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
