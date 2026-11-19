//! An agent session using an SSH terminal the human lent it (F45, ADR-0107).
//!
//! **The human wins every turn.** A command is typed only when the terminal is
//! at rest: no full-screen program, no password prompt, no human keystroke in
//! the last moments, no other command of the agent in flight. Typing while the
//! agent's command runs hands the turn back to the human, and the agent is
//! told so.
//!
//! A child module of `terminal` so it reads the handle's private fields; its
//! public face is a handful of `TerminalManager` methods and [`SessionTerminals`],
//! the backend the session's tool server is given.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{now_ms, TerminalId, TerminalKind, TerminalManager};
use crate::error::{AppError, AppResult};
use crate::services::agent_tools::terminals::{key_bytes, TerminalUse};
use crate::services::ssh::sftp::RemoteFiles;
use crate::services::term_capture::{self, Capture};

/// A human keystroke this recent means they are typing: the agent waits.
const HUMAN_GRACE_MS: i64 = 1500;
/// How long the one-off bootstrap probe may take before the shell is declared
/// unable to run agent commands.
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

pub struct CaptureCell {
	cap: Mutex<Capture>,
	changed: Condvar,
}

impl CaptureCell {
	pub fn new(cols: u16, rows: u16) -> Self {
		Self { cap: Mutex::new(Capture::new(cols, rows)), changed: Condvar::new() }
	}

	pub fn feed(&self, bytes: &[u8]) {
		self.cap.lock().feed(bytes);
		self.changed.notify_all();
	}

	pub fn resize(&self, cols: u16, rows: u16) {
		self.cap.lock().resize(cols, rows);
	}
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LinkMode {
	/// The human's own shell: both type into it.
	Shared,
	/// An SSH session opened for the agent; the human watches.
	Own,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentLink {
	pub session_id: String,
	pub mode: LinkMode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkDto {
	pub terminal_id: TerminalId,
	pub server_id: String,
	pub session_id: String,
	pub mode: LinkMode,
	pub running: bool,
}

/// Announced when an agent starts and finishes a command, so the tab can say
/// "Claude is running …" while it does (F45).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunEvent {
	pub terminal_id: TerminalId,
	pub session_id: String,
	pub command: String,
	pub running: bool,
}

/// Decides whether an agent command may run on a server (ADR-0108): `Ok(None)`
/// to run it as written, `Ok(Some(edited))` to run what the human edited it
/// into, `Err(reason)` to refuse. May block while the human is asked.
pub type PolicyCb = Arc<dyn Fn(&PolicyRequest) -> Result<Option<String>, String> + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyRequest {
	pub server_id: String,
	pub terminal_id: TerminalId,
	pub session_id: String,
	/// The command line, or the keys for `send_keys`.
	pub command: String,
	pub keys: bool,
}

impl TerminalManager {
	pub fn link_agent(&self, terminal_id: &str, session_id: &str, mode: LinkMode) -> AppResult<()> {
		let h = self
			.terminals
			.get(terminal_id)
			.ok_or_else(|| AppError::NotFound(format!("terminal {terminal_id}")))?;
		if h.kind != TerminalKind::Ssh {
			return Err(AppError::InvalidInput(
				"only an SSH terminal can be lent to an agent".into(),
			));
		}
		let mut link = h.link.lock();
		if let Some(l) = link.as_ref().filter(|l| l.session_id != session_id) {
			return Err(AppError::InvalidInput(format!(
				"this terminal is already lent to another session ({}); take it back first",
				l.session_id
			)));
		}
		*link = Some(AgentLink { session_id: session_id.to_string(), mode });
		Ok(())
	}

	pub fn unlink_agent(&self, terminal_id: &str) {
		if let Some(h) = self.terminals.get(terminal_id) {
			*h.link.lock() = None;
		}
	}

	pub fn agent_links(&self) -> Vec<LinkDto> {
		self.terminals
			.iter()
			.filter_map(|e| {
				let h = e.value();
				let l = h.link.lock().clone()?;
				Some(LinkDto {
					terminal_id: e.key().clone(),
					server_id: h.project_id.clone(),
					session_id: l.session_id,
					mode: l.mode,
					running: h.agent_running.load(Ordering::Relaxed),
				})
			})
			.collect()
	}

	/// The backend a session's tool server is given: this manager, bound to one
	/// session id, so no tool argument can reach another session's terminals.
	pub fn session_terminals(&self, session_id: &str) -> SessionTerminals {
		SessionTerminals { mgr: self.clone(), session_id: session_id.to_string() }
	}

	pub fn set_agent_policy(&mut self, cb: PolicyCb) {
		self.agent_policy = Some(cb);
	}

	pub fn set_remote_files(&mut self, files: Arc<RemoteFiles>) {
		self.remote_files = Some(files);
	}

	pub fn set_agent_run_cb(&mut self, cb: Arc<dyn Fn(AgentRunEvent) + Send + Sync>) {
		self.on_agent_run = cb;
	}
}

pub struct SessionTerminals {
	mgr: TerminalManager,
	session_id: String,
}

type Handle = Arc<super::TerminalHandle>;

impl SessionTerminals {
	fn lent(&self, terminal: &str) -> Result<(Handle, Arc<CaptureCell>, AgentLink), String> {
		let h = self
			.mgr
			.terminals
			.get(terminal)
			.map(|e| e.value().clone())
			.ok_or_else(|| format!("no terminal {terminal}; call terminals_list"))?;
		let link = h
			.link
			.lock()
			.clone()
			.filter(|l| l.session_id == self.session_id)
			.ok_or_else(|| format!("terminal {terminal} is not lent to this session"))?;
		let cap = h.capture.clone().ok_or_else(|| "this terminal cannot be read".to_string())?;
		Ok((h, cap, link))
	}

	fn state(h: &Handle, c: &Capture) -> &'static str {
		if h.agent_running.load(Ordering::Relaxed) {
			"running your command"
		} else if c.asks_for_secret() {
			"waiting for a password — the human must type it"
		} else if c.in_full_screen_program() {
			"in a full-screen program"
		} else if c.has_pending() {
			"still running an earlier command (send <ctrl-c> to interrupt it)"
		} else {
			"ready"
		}
	}

	fn write(h: &Handle, bytes: &[u8]) -> Result<(), String> {
		h.writer.send(bytes.to_vec()).map_err(|_| "the terminal has closed".to_string())
	}

	fn check_policy(
		&self,
		h: &Handle,
		terminal: &str,
		command: &str,
		keys: bool,
	) -> Result<Option<String>, String> {
		match &self.mgr.agent_policy {
			Some(p) => p(&PolicyRequest {
				server_id: h.project_id.clone(),
				terminal_id: terminal.to_string(),
				session_id: self.session_id.clone(),
				command: command.to_string(),
				keys,
			}),
			None => Ok(None),
		}
	}

	/// Wait on the capture until `done` says so or `deadline` passes.
	fn wait<T>(
		cell: &CaptureCell,
		deadline: Instant,
		mut done: impl FnMut(&mut Capture) -> Option<T>,
	) -> Option<T> {
		let mut cap = cell.cap.lock();
		loop {
			if let Some(v) = done(&mut cap) {
				return Some(v);
			}
			let now = Instant::now();
			if now >= deadline {
				return None;
			}
			let slice = (deadline - now).min(Duration::from_millis(200));
			cell.changed.wait_for(&mut cap, slice);
		}
	}

	/// The server, the files backend and the absolute path for a file tool.
	/// A relative path is resolved against the terminal's directory.
	fn file_target(
		&self,
		terminal: &str,
		path: &str,
	) -> Result<(Handle, Arc<RemoteFiles>, String), String> {
		let (h, cell, _) = self.lent(terminal)?;
		let files = self
			.mgr
			.remote_files
			.clone()
			.ok_or_else(|| "files are not available here".to_string())?;
		let path = if path.starts_with('/') {
			path.to_string()
		} else {
			let cwd = cell.cap.lock().cwd.clone().ok_or_else(|| {
				"the terminal's directory is not known yet: use an absolute path, or run a command there first"
					.to_string()
			})?;
			format!("{}/{}", cwd.trim_end_matches('/'), path)
		};
		Ok((h, files, path))
	}

	/// Define the marker functions in this shell, and prove they work.
	fn bootstrap(h: &Handle, cell: &CaptureCell) -> Result<(), String> {
		let nonce = format!("p{}", short_nonce());
		cell.cap.lock().expect(&nonce);
		Self::write(h, term_capture::BOOTSTRAP.as_bytes())?;
		Self::write(h, term_capture::wrap(&nonce, ":").as_bytes())?;
		match Self::wait(cell, Instant::now() + PROBE_TIMEOUT, |c| c.take_finished(&nonce)) {
			Some(_) => Ok(()),
			None => {
				cell.cap.lock().cancel(&nonce);
				Err("this shell did not answer the command markers: it may not be bash, zsh or sh \
					 (fish and csh are not supported yet), or it is not at a prompt. Read the \
					 screen with terminal_read."
					.into())
			}
		}
	}
}

impl TerminalUse for SessionTerminals {
	fn list(&self) -> Value {
		let rows: Vec<Value> = self
			.mgr
			.terminals
			.iter()
			.filter_map(|e| {
				let h = e.value();
				let link = h.link.lock().clone().filter(|l| l.session_id == self.session_id)?;
				let cell = h.capture.clone()?;
				let c = cell.cap.lock();
				Some(json!({
					"terminal": e.key(),
					"server": self.mgr.server_label(&h.project_id),
					"cwd": c.cwd.clone(),
					"mode": link.mode,
					"state": Self::state(h, &c),
				}))
			})
			.collect();
		json!({
			"terminals": rows,
			"note": if rows.is_empty() {
				"No terminal is lent to this session. Ask the human to attach one from a server tab in Qursor IDE."
			} else {
				"Run commands with terminal_run. `cwd` is known after your first command there."
			},
		})
	}

	fn run(&self, terminal: &str, command: &str, timeout_s: u64) -> Result<Value, String> {
		let (h, cell, _) = self.lent(terminal)?;
		{
			let c = cell.cap.lock();
			let state = Self::state(&h, &c);
			if state != "ready" {
				return Err(format!(
					"the terminal is {state}; nothing was typed. Current screen:\n{}",
					c.screen_text()
				));
			}
		}
		if now_ms() - h.last_human_input.load(Ordering::Relaxed) < HUMAN_GRACE_MS {
			return Err(
				"the human is typing in this terminal; wait and try again, or ask them".into()
			);
		}
		let edited = self.check_policy(&h, terminal, command, false)?;
		let command = edited.as_deref().unwrap_or(command);
		if h.agent_running.swap(true, Ordering::AcqRel) {
			return Err("another command of yours is already running in this terminal".into());
		}
		let guard =
			Running { h: h.clone(), mgr: self.mgr.clone(), session: self.session_id.clone() };
		(self.mgr.on_agent_run)(AgentRunEvent {
			terminal_id: terminal.to_string(),
			session_id: self.session_id.clone(),
			command: command.to_string(),
			running: true,
		});
		let needs_bootstrap = !cell.cap.lock().bootstrapped;
		if needs_bootstrap {
			Self::bootstrap(&h, &cell)?;
		}
		let nonce = short_nonce();
		cell.cap.lock().expect(&nonce);
		let started = now_ms();
		Self::write(&h, term_capture::wrap(&nonce, command).as_bytes())?;
		enum End {
			Done(term_capture::Finished),
			Secret,
		}
		let deadline = Instant::now() + Duration::from_secs(timeout_s);
		let end = Self::wait(&cell, deadline, |c| {
			if let Some(f) = c.take_finished(&nonce) {
				return Some(End::Done(f));
			}
			c.asks_for_secret().then_some(End::Secret)
		});
		let interrupted = h.last_human_input.load(Ordering::Relaxed) > started;
		drop(guard);
		let mut out = match end {
			Some(End::Done(f)) => json!({
				"exit_code": f.exit,
				"cwd": f.cwd,
				"output": f.output,
				"output_truncated": f.truncated,
			}),
			Some(End::Secret) => json!({
				"status": "waiting for a password",
				"output_so_far": cell.cap.lock().partial(&nonce),
				"note": "Stop here and tell the human: they type the password in the terminal. Never ask for it.",
			}),
			None => json!({
				"status": "still running",
				"output_so_far": cell.cap.lock().partial(&nonce),
				"note": format!("Not finished after {timeout_s}s; it keeps running. Read it with terminal_read, or interrupt with terminal_send_keys <ctrl-c>."),
			}),
		};
		if interrupted {
			out["human"] = json!("the human typed in this terminal while your command ran");
		}
		if edited.is_some() {
			out["edited_by_human"] = json!(command);
		}
		Ok(out)
	}

	fn read(&self, terminal: &str) -> Result<Value, String> {
		let (h, cell, link) = self.lent(terminal)?;
		let c = cell.cap.lock();
		Ok(json!({
			"state": Self::state(&h, &c),
			"mode": link.mode,
			"cwd": c.cwd.clone(),
			"screen": c.screen_text(),
		}))
	}

	fn file_read(&self, terminal: &str, path: &str) -> Result<Value, String> {
		let (h, files, path) = self.file_target(terminal, path)?;
		self.check_policy(&h, terminal, &format!("cat {path}"), false)?;
		let text =
			files.agent_read(&self.session_id, &h.project_id, &path).map_err(|e| e.to_string())?;
		Ok(json!({ "path": path, "content": text }))
	}

	fn file_write(&self, terminal: &str, path: &str, content: &str) -> Result<Value, String> {
		let (h, files, path) = self.file_target(terminal, path)?;
		self.check_policy(&h, terminal, &format!("write {path} ({} bytes)", content.len()), false)?;
		files
			.agent_write(&self.session_id, &h.project_id, &path, content)
			.map_err(|e| e.to_string())?;
		Ok(json!({ "path": path, "written": content.len() }))
	}

	fn file_edit(
		&self,
		terminal: &str,
		path: &str,
		old: &str,
		new: &str,
		all: bool,
	) -> Result<Value, String> {
		let (h, files, path) = self.file_target(terminal, path)?;
		let shown = |s: &str| s.chars().take(160).collect::<String>();
		self.check_policy(
			&h,
			terminal,
			&format!("edit {path}: «{}» → «{}»", shown(old), shown(new)),
			false,
		)?;
		let n = files
			.agent_edit(&self.session_id, &h.project_id, &path, old, new, all)
			.map_err(|e| e.to_string())?;
		Ok(json!({ "path": path, "replaced": n }))
	}

	fn send_keys(&self, terminal: &str, keys: &[String]) -> Result<Value, String> {
		let (h, cell, _) = self.lent(terminal)?;
		if cell.cap.lock().asks_for_secret() {
			return Err("the terminal is waiting for a password; only the human types it".into());
		}
		self.check_policy(&h, terminal, &keys.join(" "), true)?;
		let bytes: Vec<u8> = keys.iter().flat_map(|k| key_bytes(k)).collect();
		if keys.iter().any(|k| k.eq_ignore_ascii_case("<ctrl-c>")) {
			// An interrupt ends the wrapped line before its end marker can print.
			cell.cap.lock().clear_pending();
		}
		Self::write(&h, &bytes)?;
		std::thread::sleep(Duration::from_millis(300));
		let c = cell.cap.lock();
		Ok(json!({ "state": Self::state(&h, &c), "screen": c.screen_text() }))
	}
}

/// Eight hex digits: short enough not to clutter a shared terminal, unique
/// enough that a marker printed by anything else cannot be mistaken for ours.
fn short_nonce() -> String {
	uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

/// Clears the running flag and announces the end, on every exit path.
struct Running {
	h: Handle,
	mgr: TerminalManager,
	session: String,
}

impl Drop for Running {
	fn drop(&mut self) {
		self.h.agent_running.store(false, Ordering::Release);
		let id = self
			.mgr
			.terminals
			.iter()
			.find(|e| Arc::ptr_eq(e.value(), &self.h))
			.map(|e| e.key().clone());
		if let Some(terminal_id) = id {
			(self.mgr.on_agent_run)(AgentRunEvent {
				terminal_id,
				session_id: self.session.clone(),
				command: String::new(),
				running: false,
			});
		}
	}
}
