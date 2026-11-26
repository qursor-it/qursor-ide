//! The app's [`EventSink`]: every core event becomes the Tauri event the
//! renderer already listens for, under the same name and payload
//! (docs/superpowers/specs/2026-11-26-local-core-design.md).

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

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

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DataEvent {
	id: String,
	bytes_b64: String,
}

impl EventSink for TauriSink {
	fn emit(&self, event: CoreEvent) {
		match event {
			CoreEvent::TerminalData { terminal_id, bytes } => self
				.send("terminal:data", DataEvent { id: terminal_id, bytes_b64: B64.encode(bytes) }),
			CoreEvent::TerminalStatus(e) => self.send("terminal:status", e),
			CoreEvent::TerminalExit(e) => self.send("terminal:exit", e),
			CoreEvent::SessionAdopted(e) => self.send("session:adopted", e),
			CoreEvent::SessionWorktree(e) => self.send("session:worktree", e),
			CoreEvent::AgentSessionStart(e) => self.send("agent-session:start", e),
			CoreEvent::DelegationsChanged { project_id } => {
				self.send("delegations:changed", ProjectEvent { project_id })
			}
			CoreEvent::IdeOpenFile(e) => self.send("ide:open-file", e),
			CoreEvent::IdeStatus(e) => self.send("ide:status", e),
			CoreEvent::AgentTerminalRun(e) => self.send("agent-terminal:run", e),
			CoreEvent::AgentTerminalConfirm(e) => self.send("agent-terminal:confirm", e),
			CoreEvent::RoutinesChanged { project_id } => {
				self.send("routines:changed", ProjectEvent { project_id })
			}
			CoreEvent::RoutineFire(e) => self.send("routine:fire", e),
		}
	}
}
