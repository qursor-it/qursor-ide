//! Where the terminal manager and the routine runner say what happened.
//!
//! Neither knows what listens: the app translates each event into a Tauri
//! event (`tauri_sink`), a headless core forwards it over a socket
//! (specs/10-performance.md P9, docs/superpowers/specs/2026-11-26-local-core-design.md).

use serde::Serialize;

use crate::services::agent_policy::ConfirmEvent as AgentConfirmEvent;
use crate::services::routines::FireEvent;
use crate::services::terminal::{
	AgentRunEvent, IdeOpenFileEvent, IdeStatusEvent, SessionAdoptedEvent, SessionStart,
	SessionWorktreeEvent, TerminalExitEvent, TerminalStatusEvent,
};

pub trait EventSink: Send + Sync + 'static {
	fn emit(&self, event: CoreEvent);
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "payload", rename_all = "camelCase")]
pub enum CoreEvent {
	/// Raw bytes: encoding them for the wire is the sink's business.
	TerminalData {
		terminal_id: String,
		#[serde(skip)]
		bytes: Vec<u8>,
	},
	TerminalStatus(TerminalStatusEvent),
	TerminalExit(TerminalExitEvent),
	SessionAdopted(SessionAdoptedEvent),
	SessionWorktree(SessionWorktreeEvent),
	AgentSessionStart(SessionStart),
	DelegationsChanged {
		project_id: String,
	},
	IdeOpenFile(IdeOpenFileEvent),
	IdeStatus(IdeStatusEvent),
	AgentTerminalRun(AgentRunEvent),
	AgentTerminalConfirm(AgentConfirmEvent),
	RoutinesChanged {
		project_id: String,
	},
	RoutineFire(FireEvent),
}
