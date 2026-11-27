//! The app's [`EventSink`]: the routine runner's events become the Tauri
//! events the renderer listens for. Everything else happens in the core and
//! reaches the renderer over its socket
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §1).

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tracing::warn;

use crate::services::event_sink::{CoreEvent, EventSink};

pub struct TauriSink {
	app: AppHandle,
}

impl TauriSink {
	pub fn new(app: AppHandle) -> Self {
		Self { app }
	}

	fn send<S: Serialize + Clone>(&self, name: &str, payload: S) {
		let _ = self.app.emit(name, payload);
	}
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProjectEvent {
	project_id: String,
}

impl EventSink for TauriSink {
	fn emit(&self, event: CoreEvent) {
		match event {
			CoreEvent::RoutinesChanged { project_id } => {
				self.send("routines:changed", ProjectEvent { project_id })
			}
			CoreEvent::RoutineFire(e) => self.send("routine:fire", e),
			// The app holds no terminal, IDE bridge or delegation: these come
			// from the core's own sink, never this one.
			CoreEvent::TerminalData { .. }
			| CoreEvent::TerminalStatus(_)
			| CoreEvent::TerminalExit(_)
			| CoreEvent::SessionAdopted(_)
			| CoreEvent::SessionWorktree(_)
			| CoreEvent::AgentSessionStart(_)
			| CoreEvent::DelegationsChanged { .. }
			| CoreEvent::IdeOpenFile(_)
			| CoreEvent::IdeStatus(_)
			| CoreEvent::AgentTerminalRun(_)
			| CoreEvent::AgentTerminalConfirm(_)
			| CoreEvent::Attention(_) => warn!("the app's sink got an event only the core emits"),
		}
	}
}
