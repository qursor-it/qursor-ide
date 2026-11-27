//! The profile writes the renderer hears about: each one announces
//! `profiles:changed` after the write in `qursor_core::services::profiles`
//! (F25, ADR-0036).

use tauri::Emitter;

pub use qursor_core::services::profiles::*;

use crate::db::Db;
use crate::error::AppResult;
use crate::models::{Profile, ProfileInput};

/// `profiles:changed` — the list is different, so re-read it. Carries no payload
/// on purpose: every consumer wants the whole list (there are a handful of rows),
/// and the indexer's reaction is to re-read profiles and re-arm its watches
/// rather than to patch anything.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProfilesChangedEvent {}

fn announce(app: &tauri::AppHandle) {
	let _ = app.emit("profiles:changed", ProfilesChangedEvent {});
}

pub fn set_app_default_and_announce(
	db: &Db,
	app: &tauri::AppHandle,
	id: &str,
) -> AppResult<Profile> {
	let profile = db.with_mut(|conn| set_app_default(conn, id))?;
	announce(app);
	Ok(profile)
}

// ── The announcing layer ────────────────────────────────────────────────────
//
// Every write goes through one of these. What the event buys is the half a
// renderer cannot do for itself: the indexer re-reads profiles and re-arms its
// watches, which is what makes a new profile's sessions appear at all.

pub fn create_and_announce(
	db: &Db,
	app: &tauri::AppHandle,
	input: &ProfileInput,
	now_ms: i64,
) -> AppResult<Profile> {
	let profile = db.with(|conn| create(conn, input, now_ms))?;
	announce(app);
	Ok(profile)
}

pub fn rename_and_announce(
	db: &Db,
	app: &tauri::AppHandle,
	id: &str,
	name: &str,
) -> AppResult<Profile> {
	let profile = db.with(|conn| rename(conn, id, name))?;
	announce(app);
	Ok(profile)
}

pub fn set_default_and_announce(db: &Db, app: &tauri::AppHandle, id: &str) -> AppResult<Profile> {
	let profile = db.with_mut(|conn| set_default(conn, id))?;
	announce(app);
	Ok(profile)
}

pub fn delete_and_announce(db: &Db, app: &tauri::AppHandle, id: &str) -> AppResult<()> {
	db.with(|conn| delete(conn, id))?;
	announce(app);
	Ok(())
}
