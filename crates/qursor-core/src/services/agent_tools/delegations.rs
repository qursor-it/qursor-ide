//! Qursor's local parent/child hand-offs (F50).
//!
//! **Delegates are disposable** (ADR-0125). A delegation is listed while it is
//! pending, starting or running. Once it finishes it leaves the list, and its
//! child session is retired: the renderer closes its tab and keeps it out of the
//! lists for the rest of the run. A result stays readable by the parent until the
//! parent has read it once, exits, or [`RESULT_TTL`] passes. Nothing is persisted.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::services::terminal::{SessionStart, SpawnOpts};

const APPROVAL_TIMEOUT: Duration = Duration::from_secs(600);
const RESULT_TTL: Duration = Duration::from_secs(600);
/// Live delegations one session may hold at once.
const MAX_PER_PARENT: usize = 8;

/// Called with a project id whenever that project's list changes.
pub type OnChange = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub struct Delegations {
	inner: Mutex<Inner>,
	on_change: OnceLock<OnChange>,
	on_session: OnceLock<OnChange>,
}

#[derive(Default)]
struct Inner {
	entries: HashMap<String, Delegation>,
	// ponytail: grows by one per finished delegate for the life of the app, a
	// few dozen bytes each; cap it if a run ever delegates thousands of times.
	retired: Vec<(String, RetiredDelegate)>,
}

struct Delegation {
	/// The caller's session id as its MCP endpoint was bound: what authorises it.
	parent: String,
	/// The parent as the renderer knows it, after its own adoption.
	parent_view: String,
	/// The child's session id: the delegation id until its agent adopts its own.
	session_id: String,
	launch: SessionStart,
	created: Instant,
	created_ms: i64,
	state: State,
	/// When it reached a final state; the clock for [`RESULT_TTL`].
	finished: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Final {
	Declined,
	Ended,
	Failed,
	Completed,
}

enum State {
	Pending,
	Approved,
	Starting,
	Running,
	Done(Final, Option<String>),
}

/// One live delegation, as `list_delegations` returns it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationDto {
	pub id: String,
	pub parent_session_id: String,
	pub session_id: String,
	pub project_id: String,
	pub agent: String,
	pub prompt: String,
	pub status: &'static str,
	pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RetiredDelegate {
	pub session_id: String,
	pub parent_session_id: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DelegationList {
	pub active: Vec<DelegationDto>,
	pub retired: Vec<RetiredDelegate>,
}

impl Delegations {
	/// Where changes are announced; set once at startup.
	pub fn set_on_change(&self, cb: OnChange) {
		let _ = self.on_change.set(cb);
	}

	/// Told every session id a delegate runs under, provisional and adopted, so
	/// it can be kept out of the lists for good (ADR-0126); set once at startup.
	pub fn set_on_session(&self, cb: OnChange) {
		let _ = self.on_session.set(cb);
	}

	fn delegate_session(&self, id: &str) {
		if let Some(cb) = self.on_session.get() {
			cb(id);
		}
	}

	fn changed(&self, project: &str) {
		if let Some(cb) = self.on_change.get() {
			cb(project);
		}
	}

	/// `launch.parent_session_id`, when the caller sets it, is the parent as the
	/// renderer knows it now; `parent` is the id its endpoint was bound to.
	pub fn request(&self, parent: &str, mut launch: SessionStart) -> AppResult<SessionStart> {
		{
			let mut inner = self.inner.lock();
			inner.sweep();
			if inner.entries.contains_key(parent) {
				return Err(AppError::InvalidInput(
					"delegated sessions cannot start other sessions".into(),
				));
			}
			let live = inner.entries.values().filter(|e| e.parent == parent && e.is_live()).count();
			if live >= MAX_PER_PARENT {
				return Err(AppError::InvalidInput(format!(
					"this session already has {MAX_PER_PARENT} delegations in progress"
				)));
			}
			let id = Uuid::new_v4().to_string();
			let parent_view = launch.parent_session_id.take().unwrap_or_else(|| parent.into());
			launch.delegation_id = Some(id.clone());
			launch.parent_session_id = Some(parent_view.clone());
			inner.entries.insert(
				id.clone(),
				Delegation {
					parent: parent.into(),
					parent_view,
					session_id: id,
					launch: launch.clone(),
					created: Instant::now(),
					created_ms: crate::epoch_ms(),
					state: if launch.auto_approved { State::Approved } else { State::Pending },
					finished: None,
				},
			);
		}
		self.changed(&launch.project_id);
		Ok(launch)
	}

	pub fn is_child(&self, id: &str) -> bool {
		self.inner.lock().entries.contains_key(id)
	}

	pub fn decide(
		&self,
		id: &str,
		approve: bool,
		parent_live: impl FnOnce(&str) -> bool,
	) -> AppResult<Option<SessionStart>> {
		let (project, launch) = {
			let mut inner = self.inner.lock();
			let entry = inner.entries.get_mut(id).ok_or_else(unavailable)?;
			entry.expire();
			if !matches!(entry.state, State::Pending) {
				return Err(unavailable());
			}
			let launch = if approve && parent_live(&entry.parent) {
				entry.state = State::Approved;
				Some(entry.launch.clone())
			} else {
				entry.finish(Final::Declined, None);
				None
			};
			(entry.launch.project_id.clone(), launch)
		};
		self.changed(&project);
		Ok(launch)
	}

	pub fn begin(
		&self,
		opts: &SpawnOpts,
		parent_live: impl FnOnce(&str) -> bool,
	) -> AppResult<bool> {
		let mut inner = self.inner.lock();
		let Some(entry) = inner.entries.get_mut(&opts.session_id) else { return Ok(false) };
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
			entry.finish(Final::Declined, None);
			return Err(unavailable());
		}
		entry.state = State::Starting;
		drop(inner);
		self.delegate_session(&opts.session_id);
		Ok(true)
	}

	pub fn started(&self, id: &str, success: bool) {
		let project = {
			let mut inner = self.inner.lock();
			let Some(entry) = inner.entries.get_mut(id) else { return };
			if !matches!(entry.state, State::Starting) {
				return;
			}
			if success {
				entry.state = State::Running;
			} else {
				entry.finish(Final::Failed, None);
			}
			let project = entry.launch.project_id.clone();
			if !success {
				inner.retire(id);
			}
			project
		};
		self.changed(&project);
	}

	/// A child's or a parent's agent handed back its own session id (ADR-0062).
	pub fn adopted(&self, provisional: &str, adopted: &str) {
		let mut projects = Vec::new();
		let mut child = false;
		{
			let mut inner = self.inner.lock();
			for (id, entry) in inner.entries.iter_mut() {
				if id == provisional {
					entry.session_id = adopted.into();
					child = true;
				} else if entry.parent == provisional {
					entry.parent_view = adopted.into();
				} else {
					continue;
				}
				projects.push(entry.launch.project_id.clone());
			}
		}
		if child {
			self.delegate_session(adopted);
		}
		projects.dedup();
		for project in projects {
			self.changed(&project);
		}
	}

	/// The delegates a session has running, by the session id their PTY was
	/// spawned under: closed before their parent, so none outlives it.
	pub fn children_of(&self, parent: &str) -> Vec<String> {
		self.inner
			.lock()
			.entries
			.iter()
			.filter(|(_, e)| {
				e.parent == parent && matches!(e.state, State::Starting | State::Running)
			})
			.map(|(id, _)| id.clone())
			.collect()
	}

	/// A PTY exited: a child that had not finished has ended, and a parent's
	/// unstarted requests and unread results go with it.
	pub fn exited(&self, id: &str) {
		let mut projects = Vec::new();
		{
			let mut inner = self.inner.lock();
			if let Some(entry) = inner.entries.get_mut(id) {
				if matches!(entry.state, State::Starting | State::Running) {
					entry.finish(Final::Ended, None);
				}
				projects.push(entry.launch.project_id.clone());
				inner.retire(id);
			}
			let children: Vec<String> = inner
				.entries
				.iter()
				.filter(|(_, e)| {
					e.parent == id && !matches!(e.state, State::Starting | State::Running)
				})
				.map(|(child, _)| child.clone())
				.collect();
			for child in children {
				if let Some(entry) = inner.entries.remove(&child) {
					projects.push(entry.launch.project_id);
				}
			}
		}
		projects.dedup();
		for project in projects {
			self.changed(&project);
		}
	}

	/// What the parent sees. A final state is handed over once and then forgotten.
	pub fn get(&self, parent: &str, id: &str) -> AppResult<Value> {
		let mut inner = self.inner.lock();
		inner.sweep();
		let entry = inner
			.entries
			.get_mut(id)
			.filter(|entry| entry.parent == parent)
			.ok_or_else(unavailable)?;
		entry.expire();
		let agent = entry.launch.agent.clone();
		let (status, summary) = match &entry.state {
			State::Pending => ("pendingApproval", None),
			State::Approved | State::Starting => ("starting", None),
			State::Running => ("running", None),
			State::Done(end, summary) => (end.as_str(), summary.clone()),
		};
		if entry.finished.is_some() {
			inner.entries.remove(id);
		}
		Ok(json!({ "id": id, "agent": agent, "status": status, "summary": summary }))
	}

	/// The child publishes its result. The caller then stops its PTY.
	pub fn complete(&self, child: &str, summary: &str) -> AppResult<Value> {
		let project = {
			let mut inner = self.inner.lock();
			let entry = inner.entries.get_mut(child).ok_or_else(unavailable)?;
			if !matches!(entry.state, State::Starting | State::Running) {
				return Err(unavailable());
			}
			entry.finish(Final::Completed, Some(summary.into()));
			let project = entry.launch.project_id.clone();
			inner.retire(child);
			project
		};
		self.changed(&project);
		Ok(json!({ "status": "completed" }))
	}

	pub fn list(&self, project: &str) -> DelegationList {
		let mut inner = self.inner.lock();
		inner.sweep();
		let mut active: Vec<DelegationDto> = inner
			.entries
			.iter()
			.filter(|(_, e)| e.launch.project_id == project && e.is_live())
			.map(|(id, e)| DelegationDto {
				id: id.clone(),
				parent_session_id: e.parent_view.clone(),
				session_id: e.session_id.clone(),
				project_id: e.launch.project_id.clone(),
				agent: e.launch.agent.clone(),
				prompt: e.launch.prompt.clone(),
				status: match e.state {
					State::Pending => "pendingApproval",
					State::Running => "running",
					_ => "starting",
				},
				created_at: e.created_ms,
			})
			.collect();
		active.sort_by_key(|d| d.created_at);
		let retired =
			inner.retired.iter().filter(|(p, _)| p == project).map(|(_, r)| r.clone()).collect();
		DelegationList { active, retired }
	}
}

impl Inner {
	/// Remember a finished child's session so the renderer can drop it.
	fn retire(&mut self, id: &str) {
		let Some(entry) = self.entries.get(id) else { return };
		let retired = RetiredDelegate {
			session_id: entry.session_id.clone(),
			parent_session_id: entry.parent_view.clone(),
		};
		if !self.retired.iter().any(|(_, r)| *r == retired) {
			self.retired.push((entry.launch.project_id.clone(), retired));
		}
	}

	/// Expire unanswered requests and drop results nobody read in time.
	fn sweep(&mut self) {
		for entry in self.entries.values_mut() {
			entry.expire();
		}
		self.entries.retain(|_, e| e.finished.is_none_or(|t| t.elapsed() < RESULT_TTL));
	}
}

impl Delegation {
	fn is_live(&self) -> bool {
		self.finished.is_none()
	}

	fn finish(&mut self, end: Final, summary: Option<String>) {
		self.state = State::Done(end, summary);
		self.finished = Some(Instant::now());
	}

	fn expire(&mut self) {
		if matches!(self.state, State::Pending | State::Approved)
			&& self.created.elapsed() >= APPROVAL_TIMEOUT
		{
			self.finish(Final::Declined, None);
		}
	}
}

impl Final {
	fn as_str(self) -> &'static str {
		match self {
			Final::Declined => "declined",
			Final::Ended => "ended",
			Final::Failed => "failed",
			Final::Completed => "completed",
		}
	}
}

fn unavailable() -> AppError {
	AppError::InvalidInput("this delegation is unavailable to the session".into())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicUsize, Ordering};

	fn launch(auto_approved: bool) -> SessionStart {
		SessionStart {
			project_id: "project".into(),
			agent: "opencode".into(),
			prompt: "Review the diff".into(),
			cwd: "/project".into(),
			delegation_id: None,
			parent_session_id: None,
			auto_approved,
		}
	}

	fn opts(launch: &SessionStart) -> SpawnOpts {
		SpawnOpts {
			session_id: launch.delegation_id.clone().expect("id"),
			project_id: launch.project_id.clone(),
			cwd: Some(launch.cwd.clone()),
			agent: Some(launch.agent.clone()),
			initial_prompt: Some(launch.prompt.clone()),
			cols: 80,
			rows: 24,
		}
	}

	#[test]
	fn an_automatic_delegation_can_start_once_while_its_parent_is_live() {
		let store = Delegations::default();
		for live in [false, true] {
			let launch = store.request("parent", launch(true)).expect("request");
			let id = launch.delegation_id.clone().expect("id");
			assert_eq!(store.list("project").active.iter().filter(|d| d.id == id).count(), 1);
			assert_eq!(
				store.begin(&opts(&launch), |parent| live && parent == "parent").ok(),
				live.then_some(true)
			);
			assert!(store.begin(&opts(&launch), |_| true).is_err());
			if !live {
				assert_eq!(store.get("parent", &id).expect("status")["status"], "declined");
			}
		}
	}

	#[test]
	fn only_the_approved_child_can_publish_and_only_the_parent_can_read() {
		let store = Delegations::default();
		let launch = store.request("parent", launch(false)).expect("request");
		let id = launch.delegation_id.clone().expect("id");
		assert_eq!(launch.parent_session_id.as_deref(), Some("parent"));
		assert!(store.complete(&id, "too soon").is_err());
		assert!(store.get("other", &id).is_err());
		assert_eq!(store.get("parent", &id).expect("status")["status"], "pendingApproval");
		assert!(store.decide(&id, true, |parent| parent == "parent").expect("decision").is_some());
		assert!(store.decide(&id, true, |_| true).is_err());
		assert!(store.begin(&opts(&launch), |_| true).expect("start"));
		store.started(&id, true);
		assert!(store.complete("other", "wrong session").is_err());
		store.complete(&id, "Review done").expect("result");
		store.exited(&id);
		assert_eq!(store.get("parent", &id).expect("result")["summary"], "Review done");
	}

	#[test]
	fn a_finished_delegate_leaves_the_list_is_retired_and_its_result_is_read_once() {
		let store = Delegations::default();
		let changes = Arc::new(AtomicUsize::new(0));
		let seen = changes.clone();
		store.set_on_change(Arc::new(move |project| {
			assert_eq!(project, "project");
			seen.fetch_add(1, Ordering::Relaxed);
		}));
		let launch = store.request("parent", launch(true)).expect("request");
		let id = launch.delegation_id.clone().expect("id");
		store.begin(&opts(&launch), |_| true).expect("start");
		store.started(&id, true);
		store.adopted(&id, "ses_child");
		store.adopted("parent", "ses_parent");
		let live = &store.list("project").active[0];
		assert_eq!(
			(live.session_id.as_str(), live.parent_session_id.as_str()),
			("ses_child", "ses_parent")
		);
		assert!(store.get("ses_parent", &id).is_err(), "authority stays with the bound caller");

		store.complete(&id, "Done").expect("complete");
		let list = store.list("project");
		assert!(list.active.is_empty());
		assert_eq!(
			list.retired,
			vec![RetiredDelegate {
				session_id: "ses_child".into(),
				parent_session_id: "ses_parent".into()
			}]
		);
		assert_eq!(store.get("parent", &id).expect("result")["summary"], "Done");
		assert!(store.get("parent", &id).is_err(), "a result is handed over once");
		assert!(changes.load(Ordering::Relaxed) >= 4);
	}

	#[test]
	fn the_limit_counts_only_live_delegations() {
		let store = Delegations::default();
		for _ in 0..MAX_PER_PARENT * 2 {
			let launch = store.request("parent", launch(false)).expect("room for another");
			let id = launch.delegation_id.expect("id");
			store.decide(&id, false, |_| true).expect("decline");
		}
		for _ in 0..MAX_PER_PARENT {
			store.request("parent", launch(false)).expect("live");
		}
		assert!(store.request("parent", launch(false)).is_err());
	}

	#[test]
	fn every_session_a_delegate_runs_under_is_reported_and_its_running_children_are_known() {
		let store = Delegations::default();
		let seen = Arc::new(Mutex::new(Vec::new()));
		let sink = seen.clone();
		store.set_on_session(Arc::new(move |id| sink.lock().push(id.to_string())));
		let mut request = launch(true);
		request.parent_session_id = Some("ses_parent".into());
		let launch = store.request("parent", request).expect("request");
		assert_eq!(launch.parent_session_id.as_deref(), Some("ses_parent"));
		let id = launch.delegation_id.clone().expect("id");
		assert!(store.children_of("parent").is_empty(), "not started yet");
		store.begin(&opts(&launch), |_| true).expect("start");
		assert_eq!(store.children_of("parent"), vec![id.clone()]);
		store.adopted(&id, "ses_child");
		assert_eq!(*seen.lock(), vec![id.clone(), "ses_child".to_string()]);
		store.exited(&id);
		assert!(store.children_of("parent").is_empty());
	}

	#[test]
	fn a_parent_that_exits_takes_its_unstarted_requests_and_unread_results() {
		let store = Delegations::default();
		let pending = store.request("parent", launch(false)).expect("request");
		let pending = pending.delegation_id.expect("id");
		store.exited("parent");
		assert!(store.list("project").active.is_empty());
		assert!(!store.is_child(&pending));
	}
}
