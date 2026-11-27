use tauri::{AppHandle, Manager};

use crate::error::AppResult;
use crate::state::AppState;

// Every terminal command is a qursor-core method now; the renderer calls the
// core directly (docs/superpowers/specs/2026-11-26-local-core-design.md §3).

/// The basename of the shell `shell.spawn` runs — `zsh`, `bash`, `fish` — so a
/// chip can be labelled before its process exists and after it is gone (F23 as
/// amended by F24). One value per app: `$SHELL`, else `/bin/zsh`.
#[tauri::command]
pub fn shell_name() -> String {
	crate::services::shell_path::shell_name()
}

/// User confirmed the quit dialog after the close-requested intercept.
/// Stop the core with every terminal it holds and tell Tauri to exit. See
/// ADR-0005.
#[tauri::command]
pub async fn app_quit_confirmed(app: AppHandle) -> AppResult<()> {
	if let Some(state) = app.try_state::<AppState>() {
		state.intelligence.cancel_all();
		state.core.shutdown().await;
		state.taskbar_widget.stop();
	}
	app.exit(0);
	Ok(())
}
