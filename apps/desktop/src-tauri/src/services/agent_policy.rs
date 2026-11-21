//! What an agent may do in a server's terminals (F46, ADR-0110): the policy
//! the human chose per server, the confirmations it asks for, and the local
//! record of every decision.
//!
//! **Read-only is a fast lane, not a security boundary.** It lets through
//! commands that only look at things and stops the rest; the boundary that
//! actually holds is the Unix user the server is logged into as. The UI says so.

use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use dashmap::DashMap;
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::services::terminal::PolicyRequest;

/// How long a confirmation waits for the human before the command is refused.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(600);

/// The human's answer to a confirmation.
pub enum Answer {
	Run,
	/// Run this instead: the human edited the command.
	RunEdited(String),
	Refuse,
}

/// A confirmation on screen, announced to the renderer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmEvent {
	pub id: String,
	pub server_id: String,
	pub server_name: String,
	pub terminal_id: String,
	pub session_id: String,
	pub command: String,
	pub keys: bool,
	/// A command line the human may edit before it runs.
	pub editable: bool,
	pub production: bool,
}

/// Confirmations waiting for the human, by id.
#[derive(Default)]
pub struct Confirmations {
	waiting: DashMap<String, Sender<Answer>>,
}

impl Confirmations {
	/// Ask, and block until the human answers or the wait runs out.
	pub fn ask(&self, ev: ConfirmEvent, announce: impl Fn(&ConfirmEvent)) -> Answer {
		let (tx, rx) = mpsc::channel();
		self.waiting.insert(ev.id.clone(), tx);
		announce(&ev);
		let answer = rx.recv_timeout(CONFIRM_TIMEOUT).unwrap_or(Answer::Refuse);
		self.waiting.remove(&ev.id);
		answer
	}

	/// The human's click. `false` if nothing was waiting under that id.
	pub fn answer(&self, id: &str, answer: Answer) -> bool {
		match self.waiting.remove(id) {
			Some((_, tx)) => tx.send(answer).is_ok(),
			None => false,
		}
	}
}

/// Programs that only read. Matched on the first word of every command in a
/// pipeline or list; anything else, or any output redirection, is refused.
const READ_ONLY: &[&str] = &[
	"ls",
	"cat",
	"head",
	"tail",
	"less",
	"grep",
	"egrep",
	"fgrep",
	"rg",
	"find",
	"stat",
	"file",
	"wc",
	"du",
	"df",
	"free",
	"uptime",
	"uname",
	"whoami",
	"id",
	"pwd",
	"echo",
	"ps",
	"top",
	"pgrep",
	"lsblk",
	"printenv",
	"which",
	"type",
	"journalctl",
	"cut",
	"tr",
	"diff",
	"md5sum",
	"sort",
	"sha256sum",
	"readlink",
	"realpath",
	"basename",
	"dirname",
	"true",
	"test",
	"[",
];
/// Subcommands that only read, for tools that also write.
const READ_ONLY_SUB: &[(&str, &[&str])] = &[
	(
		"systemctl",
		&["status", "is-active", "is-enabled", "list-units", "list-timers", "show", "cat"],
	),
	("git", &["status", "log", "diff", "show", "rev-parse", "describe", "blame"]),
	("docker", &["ps", "logs", "inspect", "images", "stats", "version", "info"]),
	("kubectl", &["get", "describe", "logs", "top", "version"]),
	("nginx", &["-t", "-T", "-v", "-V"]),
];
/// Options that make a reading program write, delete or run something.
const ACTING_OPTIONS: &[&str] = &[
	"-delete",
	"-exec",
	"-execdir",
	"-ok",
	"-okdir",
	"-fprint",
	"-fprint0",
	"-fprintf",
	"-fls",
	"--output",
	"-o",
	"--vacuum-size",
	"--vacuum-time",
	"--vacuum-files",
	"--rotate",
	"--flush",
	"--relinquish-var",
	"--setup-keys",
];

/// Whether a command line stays within reading. Conservative: anything it
/// cannot classify is not read-only.
pub fn is_read_only(command: &str) -> bool {
	if command.chars().any(char::is_control)
		&& !command.chars().all(|c| !c.is_control() || c == '\t')
	{
		return false;
	}
	if command.contains(['>', '`']) || command.contains("$(") || command.contains("<(") {
		return false;
	}
	command.split(['|', ';', '&']).map(str::trim).filter(|s| !s.is_empty()).all(|segment| {
		let words: Vec<&str> = segment.split_whitespace().collect();
		let Some(first) = words.first() else { return true };
		if words
			.iter()
			.any(|w| ACTING_OPTIONS.iter().any(|o| *w == *o || w.starts_with(&format!("{o}="))))
		{
			return false;
		}
		if READ_ONLY.contains(first) {
			return true;
		}
		READ_ONLY_SUB
			.iter()
			.any(|(tool, subs)| tool == first && words.get(1).is_some_and(|sub| subs.contains(sub)))
	})
}

/// Keys that cannot change anything: interrupt, quit a pager, escape.
pub fn keys_are_harmless(keys: &str) -> bool {
	keys.split_whitespace()
		.all(|k| matches!(k.to_ascii_lowercase().as_str(), "<ctrl-c>" | "q" | "<esc>"))
}

/// The policy for one request: run, run edited, or refuse with a reason the
/// agent reads. Every decision is recorded.
pub fn decide(
	db: &crate::db::Db,
	confirmations: &Confirmations,
	req: &PolicyRequest,
	announce: impl Fn(&ConfirmEvent),
) -> Result<Option<String>, String> {
	let now = || chrono::Utc::now().timestamp_millis();
	let log = |decision: &str| {
		let _ = db.with(|c| Ok(record(c, req, decision, now())?));
	};
	let server = db
		.with(|c| crate::services::servers::get(c, &req.server_id))
		.map_err(|_| "this server is no longer in the address book".to_string())?;
	match server.agent_policy.as_str() {
		"free" => {
			log("allowed");
			Ok(None)
		}
		"readonly" => {
			let ok =
				if req.keys { keys_are_harmless(&req.command) } else { is_read_only(&req.command) };
			if ok {
				log("allowed (read-only)");
				Ok(None)
			} else {
				log("refused (read-only)");
				Err(format!(
					"{} is read-only for agents: only commands that look at things run here. Nothing \
					 was typed. Tell the human what you would run and why.",
					server.name
				))
			}
		}
		_ => {
			let ev = ConfirmEvent {
				id: uuid::Uuid::new_v4().to_string(),
				server_id: server.id.clone(),
				server_name: server.name.clone(),
				terminal_id: req.terminal_id.clone(),
				session_id: req.session_id.clone(),
				command: req.command.clone(),
				keys: req.keys,
				editable: req.editable,
				production: server.environment == "prod",
			};
			match confirmations.ask(ev, announce) {
				Answer::Run => {
					log("confirmed");
					Ok(None)
				}
				Answer::RunEdited(cmd) => {
					log(&format!("edited to: {cmd}"));
					Ok(Some(cmd))
				}
				Answer::Refuse => {
					log("refused");
					Err("the human refused this command; nothing was typed. Do not retry it \
						 unchanged — ask them what they want instead."
						.into())
				}
			}
		}
	}
}

/// Record a decision in the local audit log (never sent anywhere).
pub fn record(
	conn: &Connection,
	req: &PolicyRequest,
	decision: &str,
	now_ms: i64,
) -> rusqlite::Result<()> {
	conn.execute(
		"INSERT INTO agent_audit (at, server_id, session_id, terminal_id, command, decision)
		 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
		params![now_ms, req.server_id, req.session_id, req.terminal_id, req.command, decision],
	)?;
	Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRow {
	pub at: i64,
	pub server_id: String,
	pub session_id: String,
	pub command: String,
	pub decision: String,
}

pub fn recent(conn: &Connection, server_id: &str, limit: u32) -> rusqlite::Result<Vec<AuditRow>> {
	let mut st = conn.prepare(
		"SELECT at, server_id, session_id, command, decision FROM agent_audit
		 WHERE server_id = ?1 ORDER BY at DESC LIMIT ?2",
	)?;
	let rows = st
		.query_map(params![server_id, limit], |r| {
			Ok(AuditRow {
				at: r.get(0)?,
				server_id: r.get(1)?,
				session_id: r.get(2)?,
				command: r.get(3)?,
				decision: r.get(4)?,
			})
		})?
		.collect();
	rows
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reading_commands_pass_and_anything_that_acts_does_not() {
		for ok in [
			"ls -la /var/www",
			"df -h / | tail -1",
			"systemctl status nginx --no-pager",
			"journalctl -u nginx -n 50 | grep error",
			"git log --oneline -5",
			"du -sh /var/log/* | sort -h",
		] {
			assert!(is_read_only(ok), "{ok}");
		}
		for no in [
			"rm -rf /tmp/x",
			"systemctl restart nginx",
			"echo hi > /etc/motd",
			"sed -i s/a/b/ file",
			"find / -name core -delete",
			"cat x | sudo tee /etc/y",
			"ls; reboot",
			"echo $(rm -rf /)",
			"git push",
			"ls\rrm -rf ~",
			"env rm -rf /",
			"awk 'BEGIN{system(\"id\")}'",
			"sort -o out in",
			"find . -fprint /tmp/x",
			"cat <(id)",
			"journalctl --vacuum-time=1d",
			"git branch -D main",
		] {
			assert!(!is_read_only(no), "{no}");
		}
	}

	#[test]
	fn only_harmless_keys_pass_read_only() {
		assert!(keys_are_harmless("<ctrl-c>"));
		assert!(keys_are_harmless("q"));
		assert!(!keys_are_harmless("y <enter>"));
	}

	#[test]
	fn a_confirmation_returns_the_humans_answer() {
		let c = std::sync::Arc::new(Confirmations::default());
		let c2 = c.clone();
		let answerer = std::thread::spawn(move || {
			for _ in 0..50 {
				if c2.answer("c1", Answer::RunEdited("ls".into())) {
					return true;
				}
				std::thread::sleep(Duration::from_millis(10));
			}
			false
		});
		let ev = ConfirmEvent {
			id: "c1".into(),
			server_id: "s".into(),
			server_name: "web".into(),
			terminal_id: "t".into(),
			session_id: "x".into(),
			command: "rm -rf /tmp/a".into(),
			keys: false,
			editable: true,
			production: true,
		};
		let got = c.ask(ev, |_| {});
		assert!(answerer.join().unwrap_or(false));
		assert!(matches!(got, Answer::RunEdited(cmd) if cmd == "ls"));
		assert!(!c.answer("c1", Answer::Run), "answered once only");
	}
}
