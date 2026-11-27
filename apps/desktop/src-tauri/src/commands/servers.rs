//! The Server mode's command surface (F45): what reads or writes the address
//! book without a login. Listing, saving and deleting servers, the vault,
//! shells, SFTP and tunnels need the SSH pool or the vault, which qursor-core
//! owns (docs/superpowers/specs/2026-11-26-local-core-design.md §5a); the
//! renderer calls those on the core.

use tauri::State;

use crate::error::AppResult;
use crate::models::{ServerFolder, SshConfigHost};
use crate::services::servers;
use crate::state::AppState;

#[tauri::command]
pub fn create_server_folder(
	state: State<'_, AppState>,
	name: String,
	project_id: Option<String>,
) -> AppResult<ServerFolder> {
	state.db.with(|c| servers::create_folder(c, &name, project_id.as_deref()))
}

/// Write a fresh id into a duplicate or id-less `.server` file (F45).
#[tauri::command]
pub fn fix_server_file(
	state: State<'_, AppState>,
	project_id: String,
	rel_path: String,
) -> AppResult<()> {
	state.db.with(|c| servers::fix_file(c, &project_id, &rel_path))
}

#[tauri::command]
pub fn rename_server_folder(state: State<'_, AppState>, id: String, name: String) -> AppResult<()> {
	state.db.with(|c| servers::rename_folder(c, &id, &name))
}

#[tauri::command]
pub fn delete_server_folder(state: State<'_, AppState>, id: String) -> AppResult<()> {
	state.db.with(|c| servers::delete_folder(c, &id))
}

/// The concrete hosts of `~/.ssh/config`, for the import list.
#[tauri::command]
pub fn ssh_config_hosts() -> Vec<SshConfigHost> {
	servers::read_user_ssh_config()
}

/// The workspace project a server's agent sessions run in (F46): a folder of
/// the app's own, one per server, so its sessions have a transcript home, show
/// in the sidebar and resume like any other. Created on first use.
#[tauri::command]
pub fn server_agent_project(
	state: State<'_, AppState>,
	server_id: String,
) -> AppResult<crate::models::Project> {
	let server = state.db.with(|c| servers::get(c, &server_id))?;
	let dir = state.data_dir.join("servers").join(&server.id);
	std::fs::create_dir_all(&dir)?;
	let project = crate::commands::projects::add_project_in(&state.db, &dir.to_string_lossy())?;
	let name = format!("{} (server)", server.name);
	state.db.with(|c| {
		c.execute(
			"UPDATE projects SET display_name = ?2 WHERE id = ?1",
			rusqlite::params![project.id, name],
		)?;
		Ok(())
	})?;
	Ok(crate::models::Project { display_name: name, ..project })
}

/// The last decisions about agent commands on one server, newest first.
#[tauri::command]
pub fn agent_audit(
	state: State<'_, AppState>,
	server_id: String,
) -> AppResult<Vec<crate::services::agent_policy::AuditRow>> {
	state.db.with(|c| Ok(crate::services::agent_policy::recent(c, &server_id, 200)?))
}
