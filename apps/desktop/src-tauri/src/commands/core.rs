//! The app's side of qursor-core (docs/superpowers/specs/2026-11-26-local-core-design.md §1-§3):
//! where the renderer finds it, and what the app shows for it.

use tauri::{AppHandle, State};

use crate::core_link::CoreEndpoint;
use crate::error::AppResult;
use crate::services::attention::AttentionNotice;
use crate::state::AppState;

/// Where the core listens and the secret it wants. Waits for the link, which
/// setup starts off the main thread, so the window never waits on the core.
#[tauri::command]
pub async fn core_endpoint(state: State<'_, AppState>) -> AppResult<CoreEndpoint> {
	Ok(state.core.link().await?.endpoint())
}

/// A session wants you and no window is in front (F32): the core decided, the
/// app shows it.
#[tauri::command]
pub fn attention_show(app: AppHandle, notice: AttentionNotice) {
	crate::services::attention::show(&app, &notice);
}
