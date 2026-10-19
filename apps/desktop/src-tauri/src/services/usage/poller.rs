//! One thread that polls every usage source on a fixed tick and records into
//! the store. Each source is a step in `tick`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tauri::AppHandle;

use super::claude_api::{self, Backoff, TokenState};
use super::store::UsageStore;
use super::{codex_app_server, codex_log, now_ms, statusline};
use crate::agents::{CLAUDE, CODEX};
use crate::db::Db;
use crate::models::SettingKey;
use crate::services::{agent_cli, profiles, settings};

const TICK: Duration = Duration::from_secs(60);
/// Minimum gap between two `codex app-server` reads of one profile.
const APP_SERVER_EVERY: Duration = Duration::from_secs(300);
const APP_SERVER_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a `codex` binary lookup (found or not) is trusted.
const BINARY_RECHECK: Duration = Duration::from_secs(600);
/// A status-line sink file untouched this long belongs to a session long gone.
const SINK_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
/// How far before the last tick a sink file's mtime may be and still be read:
/// `statusline::write_sink` writes a temp file then renames it, keeping the
/// temp file's (earlier) mtime.
const SINK_SLACK: Duration = Duration::from_secs(10);

/// `statusline_dir` is where Claude sessions' status-line helpers drop their
/// readings (`services::usage::statusline`).
pub fn spawn(app: AppHandle, store: Arc<UsageStore>, db: Db, statusline_dir: PathBuf) {
	let _ = app;
	let (poll_store, poll_db) = (store.clone(), db.clone());
	let spawned = std::thread::Builder::new().name("usage-poller".into()).spawn(move || {
		// Epoch: the first tick reads every sink file; the store drops what is
		// older than what it already has.
		let mut sink_since = SystemTime::UNIX_EPOCH;
		let mut unmatched = HashSet::new();
		let mut backoff = Backoff::default();
		loop {
			tick(
				&poll_store,
				&poll_db,
				&statusline_dir,
				&mut sink_since,
				&mut unmatched,
				&mut backoff,
			);
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
				guarded("codex app-server", || codex_app_server_step(&store, &db, &mut state));
				std::thread::sleep(TICK);
			}
		});
	if let Err(e) = spawned {
		tracing::warn!("codex app-server poller did not start: {e}");
	}
}

fn tick(
	store: &UsageStore,
	db: &Db,
	statusline_dir: &Path,
	sink_since: &mut SystemTime,
	unmatched: &mut HashSet<String>,
	backoff: &mut Backoff,
) {
	let profiles = profiles::all(db);
	// A rename or a new default shows within a tick, not at the next reading.
	guarded("profile sync", || {
		store.sync_profiles(&profiles);
	});
	guarded("codex log", || codex_log_step(store, &profiles));
	guarded("status line", || {
		statusline_step(store, &profiles, statusline_dir, sink_since, unmatched)
	});
	guarded("claude endpoint", || claude_api_step(store, db, &profiles, backoff));
}

/// Run one poller step; in debug and test builds a panic in it is logged and
/// the poller carries on. Release builds use `panic = "abort"` (Cargo.toml), so
/// there a panic ends the app instead.
fn guarded(step: &str, f: impl FnOnce()) {
	if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
		let message = panic
			.downcast_ref::<&str>()
			.map(|s| s.to_string())
			.or_else(|| panic.downcast_ref::<String>().cloned())
			.unwrap_or_default();
		tracing::error!("usage poller step {step} panicked: {message}");
	}
}

/// Claude limits from Anthropic's usage endpoint, only while the user has
/// opted in. Per Claude profile, paced by `Backoff`; an expired or missing
/// token skips the profile. Failures are logged by status only.
fn claude_api_step(
	store: &UsageStore,
	db: &Db,
	profiles: &[crate::models::Profile],
	backoff: &mut Backoff,
) {
	let on = db
		.with(|conn| settings::get(conn, SettingKey::UsageClaudeEndpoint))
		.ok()
		.flatten()
		.is_some_and(|v| v == "on");
	if !on {
		return;
	}
	for p in profiles.iter().filter(|p| p.agent == CLAUDE && !p.config_dir.trim().is_empty()) {
		let now = now_ms();
		if !backoff.due(&p.id, now) {
			continue;
		}
		let TokenState::Ok(token) = claude_api::read_token(Path::new(&p.config_dir), now) else {
			continue;
		};
		let fetched = claude_api::fetch(&token);
		backoff.on_result(&p.id, now_ms(), fetched.status, fetched.retry_after_s);
		match fetched.body {
			Some(body) => {
				store.record(CLAUDE, p, claude_api::parse_usage_response(&body, now_ms()));
			}
			None => {
				tracing::debug!("claude usage endpoint for {}: status {}", p.name, fetched.status)
			}
		}
	}
}

/// Claude limits from the status-line sink: every file written since the last
/// tick, recorded against the Claude profile whose config directory it names.
/// Files older than `SINK_MAX_AGE` are deleted. A config directory no profile
/// has is logged once (`unmatched` remembers it), since its readings are lost.
fn statusline_step(
	store: &UsageStore,
	profiles: &[crate::models::Profile],
	dir: &Path,
	since: &mut SystemTime,
	unmatched: &mut HashSet<String>,
) {
	// Taken before the scan, so a file written during it is read next tick.
	let started = SystemTime::now();
	// A sink file keeps its temp file's mtime through the rename, so one renamed
	// just after a scan began can predate `since`. Re-reading is harmless: the
	// store drops a reading it already has.
	let floor = since.checked_sub(SINK_SLACK).unwrap_or(SystemTime::UNIX_EPOCH);
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
		if modified < floor || path.extension().is_none_or(|e| e != "json") {
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
		match profile {
			Some(p) => {
				store.record(CLAUDE, p, reading);
			}
			None => {
				if unmatched.insert(config_dir.to_string()) {
					tracing::warn!(
						"status-line readings for {config_dir} match no Claude profile; ignored"
					);
				}
			}
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
		let mut unmatched = HashSet::new();
		statusline_step(&store, &profiles, &sink, &mut since, &mut unmatched);

		let snap = store.snapshot(0);
		assert_eq!(snap.len(), 1, "{snap:?}");
		assert_eq!(snap[0].agent, CLAUDE);
		assert_eq!(snap[0].profile_id, "work");
		assert_eq!(snap[0].five_hour.as_ref().unwrap().used_percent, 42.0);
		assert!(since > SystemTime::UNIX_EPOCH);
		// The stray file's directory is remembered, so it is logged once.
		assert_eq!(unmatched, HashSet::from(["/nowhere".to_string()]));
	}

	#[test]
	fn a_panicking_step_is_contained() {
		let mut after = false;
		guarded("test", || panic!("boom"));
		guarded("test", || after = true);
		assert!(after);
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
		statusline_step(&store, &profiles, &sink, &mut since, &mut HashSet::new());

		assert!(store.snapshot(0).is_empty(), "nothing new since the last tick");
		assert!(seen.exists());
		assert!(!old.exists(), "a week-old sink file is deleted");
	}

	#[test]
	fn a_file_renamed_just_after_the_last_scan_began_is_still_read() {
		let tmp = tempfile::tempdir().unwrap();
		let store = UsageStore::load(tmp.path().join("usage.json"));
		let sink = tmp.path().join("sink");
		std::fs::create_dir_all(&sink).unwrap();
		let dir = tmp.path().join("cfg");
		let profiles = vec![profile("p", CLAUDE, &dir.to_string_lossy())];
		let mut since = SystemTime::now();
		// Its temp file was written 3 s before that scan; the rename came after.
		let late = sink_file(&sink, "late.json", &dir.to_string_lossy(), 30.0, 100);
		std::fs::File::options()
			.write(true)
			.open(&late)
			.unwrap()
			.set_modified(since - Duration::from_secs(3))
			.unwrap();

		statusline_step(&store, &profiles, &sink, &mut since, &mut HashSet::new());

		assert_eq!(store.snapshot(0).len(), 1);
	}
}
