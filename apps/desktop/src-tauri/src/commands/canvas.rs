//! Canvas mode's command surface (F44, ADR-0103).
//!
//! Thin: resolve the project's folder, hop to the blocking pool, call
//! `services::canvas`. `write_canvas` is the only writer and refuses when the
//! disk no longer holds what the caller based its edit on.

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::commands::off_main;
use crate::commands::projects::project_path;
use crate::error::{AppError, AppResult};
use crate::services::canvas::{self, CanvasEntry, CanvasOrigin, CanvasRead, WriteOutcome};
use crate::state::AppState;

/// `canvas:changed` - the open canvas is no longer what the renderer read.
/// Path only; the contents come back through `read_canvas`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasChangedEvent {
	pub path: String,
}

/// A project's canvases. A project whose folder is gone has none.
#[tauri::command]
pub async fn list_canvases(
	state: State<'_, AppState>,
	project_id: String,
) -> AppResult<Vec<CanvasEntry>> {
	let db = state.db.clone();
	off_main(move || {
		let root = db.with(|c| project_path(c, &project_id))?;
		canvas::list(&root, &project_id)
	})
	.await
}

/// Create `canvas/<slug>.canvas` with an empty document. Never overwrites.
#[tauri::command]
pub async fn create_canvas(
	state: State<'_, AppState>,
	project_id: String,
	title: String,
) -> AppResult<CanvasEntry> {
	let db = state.db.clone();
	off_main(move || {
		let root = db.with(|c| project_path(c, &project_id))?;
		canvas::create(&root, &project_id, &title)
	})
	.await
}

fn canvas_path(path: &str) -> AppResult<&Path> {
	let p = Path::new(path);
	if !p.is_absolute() || p.extension().is_none_or(|x| x != "canvas") {
		return Err(AppError::InvalidInput(format!("not a canvas path: {path}")));
	}
	Ok(p)
}

#[tauri::command]
pub async fn read_canvas(path: String) -> AppResult<CanvasRead> {
	off_main(move || canvas::read(canvas_path(&path)?)).await
}

/// Write the canvas if the disk still hashes to `expected_sha256`; otherwise
/// answer `Conflict` and leave the disk alone.
#[tauri::command]
pub async fn write_canvas(
	state: State<'_, AppState>,
	project_id: String,
	path: String,
	contents: String,
	expected_sha256: Option<String>,
) -> AppResult<WriteOutcome> {
	let db = state.db.clone();
	off_main(move || {
		let root = db.with(|c| project_path(c, &project_id))?;
		canvas::write(&root, canvas_path(&path)?, &contents, expected_sha256.as_deref())
	})
	.await
}

/// Watch the open canvas. Replaces whatever canvas was being watched.
#[tauri::command]
pub fn watch_canvas(app: AppHandle, state: State<'_, AppState>, path: String) -> AppResult<()> {
	let emit_path = path.clone();
	state.canvas_watch.watch(canvas_path(&path)?, move || {
		let _ = app.emit("canvas:changed", CanvasChangedEvent { path: emit_path.clone() });
	})
}

#[tauri::command]
pub fn unwatch_canvas(state: State<'_, AppState>, path: String) -> bool {
	state.canvas_watch.unwatch(Path::new(&path))
}

/// The session linked to a canvas, or `None` when there is none or it is gone.
#[tauri::command]
pub async fn get_canvas_session(
	state: State<'_, AppState>,
	project_id: String,
	rel_path: String,
) -> AppResult<Option<String>> {
	let db = state.db.clone();
	off_main(move || db.with(|c| canvas::get_session(c, &project_id, &rel_path))).await
}

#[tauri::command]
pub async fn set_canvas_session(
	state: State<'_, AppState>,
	project_id: String,
	rel_path: String,
	session_id: String,
) -> AppResult<()> {
	let db = state.db.clone();
	off_main(move || {
		db.with(|c| canvas::set_session(c, &project_id, &rel_path, &session_id, crate::epoch_ms()))
	})
	.await
}

/// Canvases that have a live session, for the origin strip in Sessions mode.
#[tauri::command]
pub async fn list_canvas_origins(
	state: State<'_, AppState>,
	project_id: String,
) -> AppResult<Vec<CanvasOrigin>> {
	let db = state.db.clone();
	off_main(move || {
		db.with(|c| {
			let root = project_path(c, &project_id)?;
			canvas::origins(c, &root, &project_id)
		})
	})
	.await
}
