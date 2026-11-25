//! Qursor's local parent/child hand-offs (F50).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::services::terminal::{SessionStart, SpawnOpts};

const APPROVAL_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_PER_PARENT: usize = 32;

#[derive(Default)]
pub struct Delegations(Mutex<HashMap<String, Delegation>>);

struct Delegation {
	parent: String,
	launch: SessionStart,
	created: Instant,
	state: State,
}

enum State {
	Pending,
	Approved,
	Starting,
	Running,
	Declined,
	Ended,
	Failed,
	Completed(String),
}

impl Delegations {
	pub fn request(&self, parent: &str, mut launch: SessionStart) -> AppResult<SessionStart> {
		let mut entries = self.0.lock();
		if entries.contains_key(parent) {
			return Err(AppError::InvalidInput(
				"delegated sessions cannot start other sessions".into(),
			));
		}
		if entries.values().filter(|entry| entry.parent == parent).count() >= MAX_PER_PARENT {
			return Err(AppError::InvalidInput(
				"this session has reached its delegation limit".into(),
			));
		}
		let id = Uuid::new_v4().to_string();
		launch.delegation_id = Some(id.clone());
		entries.insert(
			id,
			Delegation {
				parent: parent.into(),
				launch: launch.clone(),
				created: Instant::now(),
				state: if launch.auto_approved { State::Approved } else { State::Pending },
			},
		);
		Ok(launch)
	}

	pub fn is_child(&self, id: &str) -> bool {
		self.0.lock().contains_key(id)
	}

	pub fn decide(
		&self,
		id: &str,
		approve: bool,
		parent_live: impl FnOnce(&str) -> bool,
	) -> AppResult<Option<SessionStart>> {
		let mut entries = self.0.lock();
		let entry = entries.get_mut(id).ok_or_else(unavailable)?;
		entry.expire();
		if !matches!(entry.state, State::Pending) {
			return Err(unavailable());
		}
		if !approve || !parent_live(&entry.parent) {
			entry.state = State::Declined;
			return Ok(None);
		}
		entry.state = State::Approved;
		Ok(Some(entry.launch.clone()))
	}

	pub fn begin(
		&self,
		opts: &SpawnOpts,
		parent_live: impl FnOnce(&str) -> bool,
	) -> AppResult<bool> {
		let mut entries = self.0.lock();
		let Some(entry) = entries.get_mut(&opts.session_id) else { return Ok(false) };
		entry.expire();
		let expected = &entry.launch;
		if !matches!(entry.state, State::Approved)
			|| opts.project_id != expected.project_id
			|| opts.agent.as_deref() != Some(expected.agent.as_str())
			|| opts.cwd.as_deref() != Some(expected.cwd.as_str())
			|| opts.initial_prompt.as_deref() != Some(expected.prompt.as_str())
		{
			return Err(unavailable());
		}
		if !parent_live(&entry.parent) {
			entry.state = State::Declined;
			return Err(unavailable());
		}
		entry.state = State::Starting;
		Ok(true)
	}

	pub fn started(&self, id: &str, success: bool) {
		if let Some(entry) = self.0.lock().get_mut(id) {
			if matches!(entry.state, State::Starting) {
				entry.state = if success { State::Running } else { State::Failed };
			}
		}
	}

	pub fn exited(&self, id: &str) {
		let mut entries = self.0.lock();
		if let Some(entry) = entries.get_mut(id) {
			if matches!(entry.state, State::Starting | State::Running) {
				entry.state = State::Ended;
			}
		}
		for entry in entries.values_mut().filter(|entry| entry.parent == id) {
			if matches!(entry.state, State::Pending | State::Approved) {
				entry.state = State::Declined;
			}
		}
	}

	pub fn get(&self, parent: &str, id: &str) -> AppResult<Value> {
		let mut entries = self.0.lock();
		let entry =
			entries.get_mut(id).filter(|entry| entry.parent == parent).ok_or_else(unavailable)?;
		entry.expire();
		let (status, summary) = match &entry.state {
			State::Pending => ("pendingApproval", None),
			State::Approved | State::Starting => ("starting", None),
			State::Running => ("running", None),
			State::Declined => ("declined", None),
			State::Ended => ("ended", None),
			State::Failed => ("failed", None),
			State::Completed(summary) => ("completed", Some(summary)),
		};
		Ok(json!({ "id": id, "agent": entry.launch.agent, "status": status, "summary": summary }))
	}

	pub fn complete(&self, child: &str, summary: &str) -> AppResult<Value> {
		let mut entries = self.0.lock();
		let entry = entries.get_mut(child).ok_or_else(unavailable)?;
		if !matches!(entry.state, State::Starting | State::Running) {
			return Err(unavailable());
		}
		entry.state = State::Completed(summary.into());
		Ok(json!({ "status": "completed" }))
	}
}

impl Delegation {
	fn expire(&mut self) {
		if matches!(self.state, State::Pending | State::Approved)
			&& self.created.elapsed() >= APPROVAL_TIMEOUT
		{
			self.state = State::Declined;
		}
	}
}

fn unavailable() -> AppError {
	AppError::InvalidInput("this delegation is unavailable to the session".into())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn an_automatic_delegation_can_start_once_while_its_parent_is_live() {
		let store = Delegations::default();
		for live in [false, true] {
			let launch = store
				.request(
					"parent",
					SessionStart {
						project_id: "project".into(),
						agent: "opencode".into(),
						prompt: "Review the diff".into(),
						cwd: "/project".into(),
						delegation_id: None,
						auto_approved: true,
					},
				)
				.expect("request");
			let id = launch.delegation_id.expect("id");
			assert_eq!(store.get("parent", &id).expect("status")["status"], "starting");
			let opts = SpawnOpts {
				session_id: id.clone(),
				project_id: launch.project_id,
				cwd: Some(launch.cwd),
				agent: Some(launch.agent),
				initial_prompt: Some(launch.prompt),
				cols: 80,
				rows: 24,
			};
			assert_eq!(
				store.begin(&opts, |parent| live && parent == "parent").ok(),
				live.then_some(true)
			);
			assert!(store.begin(&opts, |_| true).is_err());
			if !live {
				assert_eq!(store.get("parent", &id).expect("status")["status"], "declined");
			}
		}
	}

	#[test]
	fn only_the_approved_child_can_publish_and_only_the_parent_can_read() {
		let store = Delegations::default();
		let launch = store
			.request(
				"parent",
				SessionStart {
					project_id: "project".into(),
					agent: "opencode".into(),
					prompt: "Review the diff".into(),
					cwd: "/project".into(),
					delegation_id: None,
					auto_approved: false,
				},
			)
			.expect("request");
		let id = launch.delegation_id.expect("id");
		assert!(store.complete(&id, "too soon").is_err());
		assert!(store.get("other", &id).is_err());
		assert_eq!(store.get("parent", &id).expect("status")["status"], "pendingApproval");
		assert!(store.decide(&id, true, |parent| parent == "parent").expect("decision").is_some());
		assert!(store.decide(&id, true, |_| true).is_err());
		assert!(store
			.begin(
				&SpawnOpts {
					session_id: id.clone(),
					project_id: "project".into(),
					cwd: Some("/project".into()),
					agent: Some("opencode".into()),
					initial_prompt: Some("Review the diff".into()),
					cols: 80,
					rows: 24,
				},
				|_| true
			)
			.expect("start"));
		assert!(store.complete("other", "wrong session").is_err());
		store.complete(&id, "Review done").expect("result");
		store.exited(&id);
		assert_eq!(store.get("parent", &id).expect("result")["summary"], "Review done");
	}
}
