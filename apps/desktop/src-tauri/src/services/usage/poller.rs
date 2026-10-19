//! One thread that polls every usage source on a fixed tick and records into
//! the store. Each source is a step in `tick`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;

use super::codex_log;
use super::store::UsageStore;
use crate::agents::CODEX;
use crate::db::Db;
use crate::services::profiles;

const TICK: Duration = Duration::from_secs(60);

pub fn spawn(app: AppHandle, store: Arc<UsageStore>, db: Db) {
	let _ = app;
	let spawned = std::thread::Builder::new().name("usage-poller".into()).spawn(move || loop {
		tick(&store, &db);
		std::thread::sleep(TICK);
	});
	if let Err(e) = spawned {
		tracing::warn!("usage poller did not start: {e}");
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
