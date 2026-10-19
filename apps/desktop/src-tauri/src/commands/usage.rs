//! The usage monitor's read side.

use tauri::State;

use crate::error::AppResult;
use crate::models::ProviderUsage;
use crate::state::AppState;

/// The latest known limits per profile, with any window that has passed its
/// reset already shown as reset.
#[tauri::command]
pub fn get_usage(state: State<'_, AppState>) -> AppResult<Vec<ProviderUsage>> {
	Ok(state.usage.snapshot(crate::services::usage::now_ms()))
}
