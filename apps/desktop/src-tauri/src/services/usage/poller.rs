//! One thread that polls every usage source on a fixed tick and records into
//! the store. Each source is a step in `tick`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::AppHandle;

use super::store::UsageStore;
use super::{codex_app_server, codex_log};
use crate::agents::CODEX;
use crate::db::Db;
use crate::services::{agent_cli, profiles, settings};

const TICK: Duration = Duration::from_secs(60);
/// Minimum gap between two `codex app-server` reads of one profile.
const APP_SERVER_EVERY: Duration = Duration::from_secs(300);
const APP_SERVER_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a `codex` binary lookup (found or not) is trusted.
const BINARY_RECHECK: Duration = Duration::from_secs(600);

pub fn spawn(app: AppHandle, store: Arc<UsageStore>, db: Db) {
	let _ = app;
	let (poll_store, poll_db) = (store.clone(), db.clone());
	let spawned = std::thread::Builder::new().name("usage-poller".into()).spawn(move || loop {
		tick(&poll_store, &poll_db);
		std::thread::sleep(TICK);
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

fn tick(store: &UsageStore, db: &Db) {
	let profiles = profiles::all(db);
	codex_log_step(store, &profiles);
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
