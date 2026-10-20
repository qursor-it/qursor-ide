//! The usage monitor's read side.

use std::path::PathBuf;

use rusqlite::OptionalExtension;
use tauri::State;

use crate::agents;
use crate::commands::sessions::transcript_path;
use crate::error::{AppError, AppResult};
use crate::models::{ProviderUsage, SessionUsage};
use crate::services::usage::session_cost;
use crate::state::AppState;

/// The latest known limits per profile, with any window that has passed its
/// reset already shown as reset.
#[tauri::command]
pub fn get_usage(state: State<'_, AppState>) -> AppResult<Vec<ProviderUsage>> {
	Ok(state.usage.snapshot(crate::services::usage::now_ms()))
}

/// Tokens and estimated list-price cost of one session, from its transcript.
/// `None` when the session is unknown or its transcript is gone.
#[tauri::command]
pub async fn session_usage(
	state: State<'_, AppState>,
	session_id: String,
) -> AppResult<Option<SessionUsage>> {
	let db = state.db.clone();
	let claude_dir = state.claude_dir.clone();
	crate::commands::off_main(move || {
		let row = db.with(|conn| {
			conn.query_row(
				"SELECT d.key, s.subagent_of, s.transcript_path, pr.agent, pr.config_dir
				 FROM sessions s
				 JOIN discovered_projects d ON d.id = s.discovered_id
				 JOIN profiles pr ON pr.id = d.profile_id
				 WHERE s.id = ?1",
				[&session_id],
				|r| {
					Ok((
						r.get::<_, String>(0)?,
						r.get::<_, Option<String>>(1)?,
						r.get::<_, Option<String>>(2)?,
						r.get::<_, String>(3)?,
						r.get::<_, String>(4)?,
					))
				},
			)
			.optional()
			.map_err(AppError::from)
		})?;
		let Some((key, parent, recorded, agent, config_dir)) = row else { return Ok(None) };
		let path = resolve_path(
			&config_dir,
			&claude_dir,
			&key,
			parent.as_deref(),
			recorded.as_deref(),
			&session_id,
		);
		if !path.is_file() {
			return Ok(None);
		}
		let mut usage = if agent == agents::CODEX {
			session_cost::codex(&path)
		} else {
			session_cost::claude(&path)
		};
		usage.session_id = session_id;
		Ok(Some(usage))
	})
	.await
}

/// The transcript file. The recorded path first: a Codex rollout sits under a
/// date directory only the indexer knows. Claude's is derived from the store key
/// under the **profile's** config dir (the indexer reads each profile there),
/// falling back to the global Claude dir only when the profile has none.
fn resolve_path(
	config_dir: &str,
	fallback: &std::path::Path,
	key: &str,
	parent: Option<&str>,
	recorded: Option<&str>,
	session_id: &str,
) -> PathBuf {
	if let Some(p) = recorded {
		return PathBuf::from(p);
	}
	let dir = if config_dir.is_empty() { fallback } else { std::path::Path::new(config_dir) };
	transcript_path(dir, key, parent, session_id)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_session_under_a_second_profile_dir_resolves() {
		let global = tempfile::tempdir().unwrap();
		let second = tempfile::tempdir().unwrap();
		let dir = second.path().join("projects/-p");
		std::fs::create_dir_all(&dir).unwrap();
		std::fs::write(dir.join("s1.jsonl"), "").unwrap();
		let cfg = second.path().to_string_lossy().to_string();
		let path = resolve_path(&cfg, global.path(), "-p", None, None, "s1");
		assert!(path.is_file());
		// An empty profile dir falls back to the global one.
		let path = resolve_path("", global.path(), "-p", None, None, "s1");
		assert!(path.starts_with(global.path()));
	}
}
