//! One thread that polls every usage source on a fixed tick and records into
//! the store. Each source is a step in `tick`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tauri::AppHandle;

use super::store::UsageStore;
use super::{codex_app_server, codex_log, statusline};
use crate::agents::{CLAUDE, CODEX};
use crate::db::Db;
use crate::services::{agent_cli, profiles, settings};

const TICK: Duration = Duration::from_secs(60);
/// Minimum gap between two `codex app-server` reads of one profile.
const APP_SERVER_EVERY: Duration = Duration::from_secs(300);
const APP_SERVER_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a `codex` binary lookup (found or not) is trusted.
const BINARY_RECHECK: Duration = Duration::from_secs(600);
/// A status-line sink file untouched this long belongs to a session long gone.
const SINK_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// `statusline_dir` is where Claude sessions' status-line helpers drop their
/// readings (`services::usage::statusline`).
pub fn spawn(app: AppHandle, store: Arc<UsageStore>, db: Db, statusline_dir: PathBuf) {
	let _ = app;
	let (poll_store, poll_db) = (store.clone(), db.clone());
	let spawned = std::thread::Builder::new().name("usage-poller".into()).spawn(move || {
		// Epoch: the first tick reads every sink file; the store drops what is
		// older than what it already has.
		let mut sink_since = SystemTime::UNIX_EPOCH;
		loop {
			tick(&poll_store, &poll_db, &statusline_dir, &mut sink_since);
			std::thread::sleep(TICK);
		}
	});
	if let Err(e) = spawned {
		tracing::warn!("usage poller did not start: {e}");
	}
	// Its own thread: a read can take 15 s per profile and must not stall the
	// other sources.
	let spawned =
		std::thread::Builder::new().name("usage-codex-app-server".into()).spawn(move || {
			let mut state = AppServerState::default();
			loop {
				codex_app_server_step(&store, &db, &mut state);
				std::thread::sleep(TICK);
			}
		});
	if let Err(e) = spawned {
		tracing::warn!("codex app-server poller did not start: {e}");
	}
}

fn tick(store: &UsageStore, db: &Db, statusline_dir: &Path, sink_since: &mut SystemTime) {
	let profiles = profiles::all(db);
	codex_log_step(store, &profiles);
	statusline_step(store, &profiles, statusline_dir, sink_since);
}

/// Claude limits from the status-line sink: every file written since the last
/// tick, recorded against the Claude profile whose config directory it names.
/// Files older than `SINK_MAX_AGE` are deleted.
fn statusline_step(
	store: &UsageStore,
	profiles: &[crate::models::Profile],
	dir: &Path,
	since: &mut SystemTime,
) {
	// Taken before the scan, so a file written during it is read next tick.
	let started = SystemTime::now();
	let Ok(entries) = std::fs::read_dir(dir) else {
		*since = started;
		return;
	};
	for entry in entries.flatten() {
		let path = entry.path();
		let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else { continue };
		if started.duration_since(modified).is_ok_and(|age| age > SINK_MAX_AGE) {
			let _ = std::fs::remove_file(&path);
			continue;
		}
		if modified < *since || path.extension().is_none_or(|e| e != "json") {
			continue;
		}
		let parsed = std::fs::read(&path)
			.ok()
			.and_then(|b| serde_json::from_slice(&b).ok())
			.and_then(|v| statusline::parse_sink(&v));
		let Some((config_dir, reading)) = parsed else { continue };
		let config_dir = config_dir.to_string_lossy();
		let profile = profiles
			.iter()
			.find(|p| p.agent == CLAUDE && statusline::same_dir(&p.config_dir, &config_dir));
		if let Some(p) = profile {
			store.record(CLAUDE, p, reading);
		}
	}
	*since = started;
}

/// Codex limits from each Codex profile's newest rollout.
fn codex_log_step(store: &UsageStore, profiles: &[crate::models::Profile]) {
	for p in profiles.iter().filter(|p| p.agent == CODEX && !p.config_dir.trim().is_empty()) {
		if let Some(reading) = codex_log::latest_reading(Path::new(&p.config_dir)) {
			store.record(CODEX, p, reading);
		}
	}
}

/// What the app-server thread remembers between ticks.
#[derive(Default)]
struct AppServerState {
	/// When each profile was last read.
	last: HashMap<String, Instant>,
	/// The last `codex` lookup and when it ran; `None` inside = not installed.
	binary: Option<(Instant, Option<PathBuf>)>,
}

impl AppServerState {
	/// The `codex` binary, looked up at most every `BINARY_RECHECK` (a miss
	/// would otherwise launch login shells on every tick).
	fn binary(&mut self, db: &Db) -> Option<PathBuf> {
		if let Some((at, found)) = &self.binary {
			if at.elapsed() < BINARY_RECHECK {
				return found.clone();
			}
		}
		let found = crate::agents::descriptor(CODEX).and_then(|d| {
			let override_path = settings::binary_override(db, CODEX);
			agent_cli::find_agent_binary(d, override_path.as_deref()).ok()
		});
		self.binary = Some((Instant::now(), found.clone()));
		found
	}
}

/// Live Codex limits through `codex app-server`, at most every five minutes
/// per profile. Skipped when no `codex` binary is found; failures are only
/// logged, the rollout reading stays in place.
fn codex_app_server_step(store: &UsageStore, db: &Db, state: &mut AppServerState) {
	let profiles = profiles::all(db);
	let due_now: Vec<_> = profiles
		.iter()
		.filter(|p| {
			p.agent == CODEX
				&& !p.config_dir.trim().is_empty()
				&& state.last.get(&p.id).is_none_or(|t| t.elapsed() >= APP_SERVER_EVERY)
		})
		.collect();
	if due_now.is_empty() {
		return;
	}
	let Some(bin) = state.binary(db) else { return };
	for p in due_now {
		state.last.insert(p.id.clone(), Instant::now());
		match codex_app_server::read(&bin, Path::new(&p.config_dir), APP_SERVER_TIMEOUT) {
			Ok(reading) => {
				store.record(CODEX, p, reading);
			}
			Err(e) => tracing::debug!("codex app-server read failed for {}: {e}", p.name),
		}
	}
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;
	use crate::models::Profile;

	fn profile(id: &str, agent: &str, dir: &str) -> Profile {
		Profile {
			id: id.into(),
			agent: agent.into(),
			name: id.into(),
			config_dir: dir.into(),
			is_default: false,
			is_app_default: false,
			missing: false,
			created_at: 0,
		}
	}

	fn sink_file(dir: &Path, name: &str, config_dir: &str, used: f64, at: i64) -> PathBuf {
		let path = dir.join(name);
		let body = json!({"configDir": config_dir, "observedAt": at,
			"rateLimits": {"five_hour": {"used_percentage": used, "resets_at": 4_000_000_000_i64}}});
		std::fs::write(&path, body.to_string()).unwrap();
		path
	}

	#[test]
	fn sink_files_are_recorded_against_the_matching_claude_profile() {
		let tmp = tempfile::tempdir().unwrap();
		let store = UsageStore::load(tmp.path().join("usage.json"));
		let sink = tmp.path().join("sink");
		std::fs::create_dir_all(&sink).unwrap();
		let (work_dir, other_dir) = (tmp.path().join("work"), tmp.path().join("other"));
		let profiles = vec![
			profile("codex", CODEX, &work_dir.to_string_lossy()),
			profile("work", CLAUDE, &format!("{}/", work_dir.to_string_lossy())),
			profile("other", CLAUDE, &other_dir.to_string_lossy()),
		];
		sink_file(&sink, "s1.json", &work_dir.to_string_lossy(), 42.0, 100);
		sink_file(&sink, "unknown.json", "/nowhere", 99.0, 100);

		let mut since = SystemTime::UNIX_EPOCH;
		statusline_step(&store, &profiles, &sink, &mut since);

		let snap = store.snapshot(0);
		assert_eq!(snap.len(), 1, "{snap:?}");
		assert_eq!(snap[0].agent, CLAUDE);
		assert_eq!(snap[0].profile_id, "work");
		assert_eq!(snap[0].five_hour.as_ref().unwrap().used_percent, 42.0);
		assert!(since > SystemTime::UNIX_EPOCH);
	}

	#[test]
	fn files_older_than_the_last_tick_are_skipped_and_week_old_ones_deleted() {
		let tmp = tempfile::tempdir().unwrap();
		let store = UsageStore::load(tmp.path().join("usage.json"));
		let sink = tmp.path().join("sink");
		std::fs::create_dir_all(&sink).unwrap();
		let dir = tmp.path().join("cfg");
		let profiles = vec![profile("p", CLAUDE, &dir.to_string_lossy())];
		let seen = sink_file(&sink, "seen.json", &dir.to_string_lossy(), 10.0, 100);
		let old = sink_file(&sink, "old.json", &dir.to_string_lossy(), 20.0, 200);
		let set_age = |p: &Path, age: Duration| {
			let f = std::fs::File::options().write(true).open(p).unwrap();
			f.set_modified(SystemTime::now() - age).unwrap();
		};
		set_age(&seen, Duration::from_secs(3600));
		set_age(&old, SINK_MAX_AGE + Duration::from_secs(60));

		let mut since = SystemTime::now() - Duration::from_secs(60);
		statusline_step(&store, &profiles, &sink, &mut since);

		assert!(store.snapshot(0).is_empty(), "nothing new since the last tick");
		assert!(seen.exists());
		assert!(!old.exists(), "a week-old sink file is deleted");
	}
}
