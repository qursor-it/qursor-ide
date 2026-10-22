//! Which sessions an analysis reads, and the signals it gets out of them.
//!
//! **The link is the canonical one**: `projects` -> `discovered_projects` ->
//! `sessions` (ADR-0011), never a path decoded from a directory name. Each
//! session is read from the store of the profile that wrote it (F25), so two
//! identities on one machine are both found.
//!
//! **Only sessions that are over, and only the ones a person had.** A session a
//! terminal is still running is skipped; so is a sub-agent transcript, whose
//! "user" turns are another agent's prompts and which is never an independent
//! observation (F33). Both are counted, so the preview can say what was left out.

use std::collections::HashSet;
use std::path::PathBuf;

use rusqlite::Connection;

use super::evidence::{self, RawSignal};
use super::sha256_hex;
use crate::agents::{self, claude, codex};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::intelligence::{EvidenceLocator, SignalType};

pub const DEFAULT_SESSION_LIMIT: u32 = 20;
pub const MAX_SESSION_LIMIT: u32 = 50;
/// Signals one run carries, across every session.
pub const MAX_SIGNALS_PER_RUN: usize = 120;

/// A requested limit made usable: absent is the default, and nothing exceeds
/// the maximum or falls below one.
pub fn clamp_limit(requested: Option<u32>) -> u32 {
	requested.unwrap_or(DEFAULT_SESSION_LIMIT).clamp(1, MAX_SESSION_LIMIT)
}

#[derive(Debug, Clone)]
pub struct SelectedSession {
	pub id: String,
	pub title: Option<String>,
	pub agent: &'static str,
	pub path: PathBuf,
	pub updated_at: i64,
}

#[derive(Debug, Default)]
pub struct Selection {
	pub sessions: Vec<SelectedSession>,
	pub excluded_running: u32,
	pub excluded_subagents: u32,
	/// Rows whose transcript is not on disk any more.
	pub unreadable: u32,
}

/// One observation with everything needed to store it, show it and send it.
#[derive(Debug, Clone)]
pub struct Signal {
	pub evidence_id: String,
	pub session_id: String,
	pub session_title: Option<String>,
	pub source_agent: &'static str,
	pub signal_type: SignalType,
	pub excerpt: String,
	pub context: Option<String>,
	pub locator: EvidenceLocator,
	pub fingerprint: String,
	pub observed_at: i64,
}

#[derive(Debug, Default)]
pub struct Collected {
	pub session_count: u32,
	/// `(agent, sessions)` for the agents that contributed.
	pub by_agent: Vec<(String, u32)>,
	pub excluded_running: u32,
	pub excluded_subagents: u32,
	pub unreadable: u32,
	pub signals: Vec<Signal>,
	/// More signals were found than [`MAX_SIGNALS_PER_RUN`] allows.
	pub capped: bool,
}

/// The newest `limit` completed top-level sessions of a project.
pub fn select_sessions(
	conn: &Connection,
	project_id: &str,
	limit: usize,
	live: &HashSet<String>,
) -> AppResult<Selection> {
	let mut stmt = conn.prepare(
		"SELECT s.id, s.title, s.updated_at, s.transcript_path, s.subagent_of,
		        d.key, pr.config_dir, pr.agent
		 FROM sessions s
		 JOIN discovered_projects d ON d.id = s.discovered_id
		 JOIN profiles pr ON pr.id = d.profile_id
		 WHERE d.project_id = ?1
		 ORDER BY s.updated_at DESC, s.id",
	)?;
	let mut rows = stmt.query([project_id])?;
	let mut out = Selection::default();
	while let Some(row) = rows.next()? {
		if out.sessions.len() >= limit {
			break;
		}
		let id: String = row.get(0)?;
		let title: Option<String> = row.get(1)?;
		let updated_at: i64 = row.get(2)?;
		let recorded_path: Option<String> = row.get(3)?;
		let subagent_of: Option<String> = row.get(4)?;
		let key: String = row.get(5)?;
		let config_dir: String = row.get(6)?;
		let agent: String = row.get(7)?;

		if subagent_of.is_some() {
			out.excluded_subagents += 1;
			continue;
		}
		if live.contains(&id) {
			out.excluded_running += 1;
			continue;
		}
		let config = PathBuf::from(&config_dir);
		let (agent, path) = match agent.as_str() {
			agents::CLAUDE => (agents::CLAUDE, claude::transcript_path_by_key(&config, &key, &id)),
			agents::CODEX => {
				let path = recorded_path
					.map(PathBuf::from)
					.or_else(|| codex::transcript_path(&config, &id));
				match path {
					Some(path) => (agents::CODEX, path),
					None => {
						out.unreadable += 1;
						continue;
					}
				}
			}
			// An agent this build does not know how to read is left alone, not guessed at.
			_ => continue,
		};
		if !path.is_file() {
			out.unreadable += 1;
			continue;
		}
		// A Codex thread with a parent is a sub-agent run on another's behalf.
		if agent == agents::CODEX
			&& codex::read_meta(&path).is_some_and(|m| m.parent_thread_id.is_some())
		{
			out.excluded_subagents += 1;
			continue;
		}
		out.sessions.push(SelectedSession { id, title, agent, path, updated_at });
	}
	Ok(out)
}

/// Read the selected sessions and return their signals.
pub fn collect(
	db: &Db,
	project_id: &str,
	limit: u32,
	live: &HashSet<String>,
) -> AppResult<Collected> {
	let selection = db.read(|conn| select_sessions(conn, project_id, limit as usize, live))?;
	collect_from(project_id, selection)
}

/// The reading half, separate so a test can hand it a selection.
pub fn collect_from(project_id: &str, selection: Selection) -> AppResult<Collected> {
	let mut collected = Collected {
		excluded_running: selection.excluded_running,
		excluded_subagents: selection.excluded_subagents,
		unreadable: selection.unreadable,
		..Collected::default()
	};
	let mut per_session: Vec<Vec<Signal>> = Vec::new();
	let mut claude_sessions = 0u32;
	let mut codex_sessions = 0u32;
	for session in &selection.sessions {
		let raw = match evidence::extract_session(session.agent, &session.path) {
			Ok(raw) => raw,
			Err(error) => {
				tracing::debug!(session = %session.id, %error, "transcript unreadable for analysis");
				collected.unreadable += 1;
				continue;
			}
		};
		match session.agent {
			agents::CODEX => codex_sessions += 1,
			_ => claude_sessions += 1,
		}
		per_session.push(raw.into_iter().map(|r| to_signal(project_id, session, r)).collect());
	}
	collected.session_count = claude_sessions + codex_sessions;
	for (agent, count) in [(agents::CLAUDE, claude_sessions), (agents::CODEX, codex_sessions)] {
		if count > 0 {
			collected.by_agent.push((agent.to_string(), count));
		}
	}
	let (signals, capped) = balance(per_session, MAX_SIGNALS_PER_RUN);
	collected.signals = signals;
	collected.capped = capped;
	Ok(collected)
}

fn to_signal(project_id: &str, session: &SelectedSession, raw: RawSignal) -> Signal {
	Signal {
		evidence_id: evidence::evidence_id(project_id, &session.id, &raw.fingerprint),
		session_id: session.id.clone(),
		session_title: session.title.clone(),
		source_agent: session.agent,
		signal_type: raw.signal_type,
		excerpt: raw.excerpt,
		context: raw.context,
		locator: raw.locator,
		fingerprint: raw.fingerprint,
		observed_at: raw.observed_at.unwrap_or(session.updated_at),
	}
}

/// Cut to `cap` signals taking each session's first, then each session's
/// second, and so on, so one noisy transcript cannot crowd out the rest. The
/// result keeps session order, newest first, and each session's own order.
fn balance(per_session: Vec<Vec<Signal>>, cap: usize) -> (Vec<Signal>, bool) {
	let total: usize = per_session.iter().map(Vec::len).sum();
	if total <= cap {
		return (per_session.into_iter().flatten().collect(), false);
	}
	let deepest = per_session.iter().map(Vec::len).max().unwrap_or(0);
	let mut keep: Vec<(usize, usize)> = Vec::with_capacity(cap);
	'rank: for rank in 0..deepest {
		for (session, signals) in per_session.iter().enumerate() {
			if signals.len() > rank {
				if keep.len() == cap {
					break 'rank;
				}
				keep.push((session, rank));
			}
		}
	}
	keep.sort_unstable();
	let mut slots: Vec<Vec<Option<Signal>>> =
		per_session.into_iter().map(|v| v.into_iter().map(Some).collect()).collect();
	let signals = keep.into_iter().filter_map(|(s, r)| slots[s][r].take()).collect();
	(signals, true)
}

/// Identifies exactly this set of signals under this session limit.
/// `start_analysis` recomputes it and refuses a request that no longer matches,
/// so what leaves the machine is what the person was shown.
pub fn preview_hash(limit: u32, signals: &[Signal]) -> String {
	let mut ids: Vec<&str> = signals.iter().map(|s| s.evidence_id.as_str()).collect();
	ids.sort_unstable();
	sha256_hex(format!("v1|{limit}|{}", ids.join(",")).as_bytes())
}

/// The project's folder and name, or why there is no analysis to do.
pub fn project_root(conn: &Connection, project_id: &str) -> AppResult<(PathBuf, String)> {
	use rusqlite::OptionalExtension;
	let row: Option<(String, String)> = conn
		.query_row(
			"SELECT real_path, display_name FROM projects WHERE id = ?1",
			[project_id],
			|r| Ok((r.get(0)?, r.get(1)?)),
		)
		.optional()?;
	let (path, name) = row.ok_or_else(|| {
		super::err(super::ErrorCode::ProjectNotFound, format!("project {project_id}"))
	})?;
	Ok((PathBuf::from(path), name))
}

/// A project's folder canonicalised: the root every file operation is confined to.
pub fn canonical_root(root: &std::path::Path) -> AppResult<PathBuf> {
	std::fs::canonicalize(root).map_err(|e| match e.kind() {
		std::io::ErrorKind::NotFound => super::err(
			super::ErrorCode::ProjectMissing,
			format!("the project folder {} is not on disk", root.display()),
		),
		_ => AppError::Io(format!("{}: {e}", root.display())),
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn signal(session: &str, n: usize) -> Signal {
		Signal {
			evidence_id: format!("{session}-{n}"),
			session_id: session.into(),
			session_title: None,
			source_agent: agents::CLAUDE,
			signal_type: SignalType::UserCorrection,
			excerpt: format!("{session} {n}"),
			context: None,
			locator: EvidenceLocator { line: n as u64, ordinal: None, event_id: None },
			fingerprint: format!("fp-{session}-{n}"),
			observed_at: 0,
		}
	}

	#[test]
	fn the_limit_defaults_to_twenty_and_never_exceeds_fifty() {
		assert_eq!(clamp_limit(None), 20);
		assert_eq!(clamp_limit(Some(0)), 1);
		assert_eq!(clamp_limit(Some(35)), 35);
		assert_eq!(clamp_limit(Some(500)), 50);
	}

	#[test]
	fn a_noisy_session_cannot_crowd_out_the_others() {
		let noisy: Vec<Signal> = (0..10).map(|n| signal("a", n)).collect();
		let quiet = vec![signal("b", 0)];
		let (kept, capped) = balance(vec![noisy, quiet], 4);
		assert!(capped);
		let ids: Vec<&str> = kept.iter().map(|s| s.evidence_id.as_str()).collect();
		assert_eq!(ids, ["a-0", "a-1", "a-2", "b-0"], "b's only signal must survive the cap");
	}

	#[test]
	fn under_the_cap_nothing_is_dropped_or_reordered() {
		let (kept, capped) =
			balance(vec![vec![signal("a", 0), signal("a", 1)], vec![signal("b", 0)]], 10);
		assert!(!capped);
		assert_eq!(kept.len(), 3);
		assert_eq!(kept[2].evidence_id, "b-0");
	}

	#[test]
	fn the_preview_hash_depends_on_the_set_and_the_limit_but_not_the_order() {
		let a = vec![signal("a", 0), signal("a", 1)];
		let reordered = vec![signal("a", 1), signal("a", 0)];
		assert_eq!(preview_hash(20, &a), preview_hash(20, &reordered));
		assert_ne!(preview_hash(20, &a), preview_hash(21, &a));
		assert_ne!(preview_hash(20, &a), preview_hash(20, &a[..1]));
	}
}
