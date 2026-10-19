//! One thread that polls every usage source on a fixed tick and records into
//! the store. Each source is a step in `tick`.

use std::collections::HashMap;
use std::path::Path;
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

pub fn spawn(app: AppHandle, store: Arc<UsageStore>, db: Db) {
	let _ = app;
	let spawned = std::thread::Builder::new().name("usage-poller".into()).spawn(move || {
		let mut last_app_server = HashMap::new();
		loop {
			tick(&store, &db, &mut last_app_server);
			std::thread::sleep(TICK);
		}
	});
	if let Err(e) = spawned {
		tracing::warn!("usage poller did not start: {e}");
	}
}

fn tick(store: &UsageStore, db: &Db, last_app_server: &mut HashMap<String, Instant>) {
	let profiles = profiles::all(db);
	codex_log_step(store, &profiles);
	codex_app_server_step(store, db, &profiles, last_app_server);
}

/// Codex limits from each Codex profile's newest rollout.
fn codex_log_step(store: &UsageStore, profiles: &[crate::models::Profile]) {
	for p in profiles.iter().filter(|p| p.agent == CODEX && !p.config_dir.trim().is_empty()) {
		if let Some(reading) = codex_log::latest_reading(Path::new(&p.config_dir)) {
			store.record(CODEX, p, reading);
		}
	}
}

/// Live Codex limits through `codex app-server`, at most every five minutes
/// per profile. Skipped when no `codex` binary is found; failures are only
/// logged, the rollout reading above stays in place.
fn codex_app_server_step(
	store: &UsageStore,
	db: &Db,
	profiles: &[crate::models::Profile],
	last: &mut HashMap<String, Instant>,
) {
	let due = |p: &crate::models::Profile, last: &HashMap<String, Instant>| {
		p.agent == CODEX
			&& !p.config_dir.trim().is_empty()
			&& last.get(&p.id).is_none_or(|t| t.elapsed() >= APP_SERVER_EVERY)
	};
	let due_now: Vec<_> = profiles.iter().filter(|p| due(p, last)).collect();
	if due_now.is_empty() {
		return;
	}
	let Some(descriptor) = crate::agents::descriptor(CODEX) else { return };
	let override_path = settings::binary_override(db, CODEX);
	let Ok(bin) = agent_cli::find_agent_binary(descriptor, override_path.as_deref()) else {
		return;
	};
	for p in due_now {
		last.insert(p.id.clone(), Instant::now());
		match codex_app_server::read(&bin, Path::new(&p.config_dir), APP_SERVER_TIMEOUT) {
			Ok(reading) => {
				store.record(CODEX, p, reading);
			}
			Err(e) => tracing::debug!("codex app-server read failed for {}: {e}", p.name),
		}
	}
}
