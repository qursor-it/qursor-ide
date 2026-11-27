//! Reads and writes of a session's own rows that something outside the command
//! layer needs.
//!
//! Both callers are `TerminalManager`, and both exist for the same reason
//! `settings::claude_binary_override` does: the manager needs a database it
//! should not hold, so it takes callbacks and these are what they close over.
//! See its `session_cwd` and `session_worktree` fields.

use std::path::PathBuf;

use crate::db::Db;
use crate::error::AppResult;

/// The directories a session's transcript records it having run in, **newest
/// first** — its last `cwd` then its first. Usually the same path twice, hence
/// one entry; two when the agent moved into a worktree mid-session (F21).
///
/// **Swallows a read failure into `None`**, following
/// `settings::claude_binary_override`: this sits in front of a spawn, and a
/// database we cannot read has to mean "we know nothing about this session"
/// rather than "refuse to start it". The caller's own cwd is then used, which is
/// exactly the behaviour that predates this function.
///
/// `None` is also the ordinary answer for a session factorai just minted: there
/// is no transcript yet, so there is no row and nothing to recover.
pub fn recorded_cwds(db: &Db, session_id: &str) -> Vec<PathBuf> {
	let row: Option<(Option<String>, Option<String>)> = db
		.with(|conn| {
			Ok(conn
				.query_row(
					"SELECT last_cwd, cwd FROM sessions WHERE id = ?1",
					[session_id],
					|row| Ok((row.get(0)?, row.get(1)?)),
				)
				.ok())
		})
		.ok()
		.flatten();

	let Some((last, first)) = row else {
		return Vec::new();
	};
	// Newest first, and de-duplicated: the two are the same string for every
	// session that never moved, which is almost all of them.
	let mut out: Vec<PathBuf> = Vec::new();
	for candidate in [last, first].into_iter().flatten() {
		let path = PathBuf::from(candidate);
		if !out.contains(&path) {
			out.push(path);
		}
	}
	out
}

/// The store **directory name** the index recorded this session's transcript
/// under, if the indexer has seen it — `discovered_projects.key`.
///
/// This is the authoritative location: the key is the directory Claude actually
/// wrote to, recorded verbatim when the transcript was parsed, and never
/// re-derived from a cwd. It is what [`recorded_cwds`] cannot reconstruct once
/// the agent has `cd`'d into a subdirectory of the folder Claude keyed the store
/// by — `last_cwd` then encodes to a sibling directory that holds no transcript,
/// while the key still points where the file is.
///
/// **Swallows a read failure into `None`**, the same rule as [`recorded_cwds`]:
/// a database we cannot read means "we know nothing about this session", not
/// "refuse to act on it". `None` is also the ordinary answer for a freshly
/// minted session — no transcript, no row, no key.
pub fn recorded_key(db: &Db, session_id: &str) -> Option<String> {
	db.with(|conn| {
		Ok(conn
			.query_row(
				"SELECT d.key
				 FROM sessions s
				 JOIN discovered_projects d ON d.id = s.discovered_id
				 WHERE s.id = ?1",
				[session_id],
				|row| row.get::<_, String>(0),
			)
			.ok())
	})
	.ok()
	.flatten()
}

/// Record which checkout of its repository a session is working in (F21).
///
/// **Only the IDE bridge's signal path calls this**, after the path has been
/// validated against the repository's real worktree list — the table is a record
/// of what the agent said, and the validation is what makes it worth recording.
///
/// A failure is returned rather than swallowed, unlike the reads above: this one
/// is the durable half of a signal whose event is about to be emitted, and
/// emitting after a write that silently failed is how the renderer comes to show
/// a checkout the next reload disagrees with.
pub fn set_worktree(db: &Db, session_id: &str, path: &str, now_ms: i64) -> AppResult<()> {
	db.with(|conn| {
		conn.execute(
			"INSERT INTO session_worktrees(session_id, path, updated_at)
			 VALUES (?1, ?2, ?3)
			 ON CONFLICT(session_id) DO UPDATE SET path = excluded.path,
			                                       updated_at = excluded.updated_at",
			rusqlite::params![session_id, path, now_ms],
		)?;
		Ok(())
	})
}

/// The checkout recorded for a session, if any.
///
/// **Not re-validated here.** A row is a record, not a guarantee: the checkout it
/// names can be removed while the row survives, so the caller checks it against
/// git and falls back. Doing that inside this function would put a `git` call
/// behind what reads like a column read.
pub fn worktree(db: &Db, session_id: &str) -> Option<String> {
	db.with(|conn| {
		let path: Option<String> = conn
			.query_row(
				"SELECT path FROM session_worktrees WHERE session_id = ?1",
				[session_id],
				|row| row.get(0),
			)
			.ok();
		Ok(path)
	})
	.ok()
	.flatten()
}
