//! The Server mode's command surface (F44): the address book, the vault, and
//! opening a shell on a server.
//!
//! **No command returns a secret.** The vault's contents go one way: from the
//! form into the vault, and from the vault into an SSH login.

use std::path::PathBuf;

use tauri::State;
use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};
use crate::models::{Server, ServerBook, ServerFolder, ServerInput, SshConfigHost};
use crate::services::servers::{self, secret_id};
use crate::services::ssh::{self, pty, Auth, Target};
use crate::services::terminal::TerminalId;
use crate::services::vault::{Vault, VaultStatus};
use crate::state::AppState;

fn flagged(state: &AppState, s: Server) -> Server {
	let connected = state.ssh.is_connected(&s.id);
	Server { connected, ..servers::with_secret_flags(s, &state.vault) }
}

#[tauri::command]
pub fn list_servers(state: State<'_, AppState>) -> AppResult<ServerBook> {
	let (folders, list) = state.db.with(|c| Ok((servers::list_folders(c)?, servers::list(c)?)))?;
	Ok(ServerBook { folders, servers: list.into_iter().map(|s| flagged(&state, s)).collect() })
}

#[tauri::command]
pub fn save_server(state: State<'_, AppState>, input: ServerInput) -> AppResult<Server> {
	let now = chrono::Utc::now().timestamp_millis();
	let saved = state.db.with(|c| servers::save(c, &state.vault, &input, now))?;
	// New credentials or a new address take effect on the next login; an idle
	// connection is dropped so that login happens now, an open tab keeps its own.
	if !state.terminals.has_ssh_for(&saved.id) {
		state.ssh.disconnect(&saved.id);
	}
	Ok(flagged(&state, saved))
}

#[tauri::command]
pub fn delete_server(state: State<'_, AppState>, id: String) -> AppResult<()> {
	state.terminals.kill_ssh_for(&id);
	state.ssh.disconnect(&id);
	state.db.with(|c| servers::delete(c, &state.vault, &id))
}

#[tauri::command]
pub fn create_server_folder(state: State<'_, AppState>, name: String) -> AppResult<ServerFolder> {
	state.db.with(|c| servers::create_folder(c, &name))
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

#[tauri::command]
pub fn vault_status(state: State<'_, AppState>) -> VaultStatus {
	state.vault.status()
}

#[tauri::command]
pub fn vault_unlock(state: State<'_, AppState>, password: String) -> AppResult<VaultStatus> {
	let password = Zeroizing::new(password);
	state.vault.unlock(&password)?;
	Ok(state.vault.status())
}

#[tauri::command]
pub fn vault_lock(state: State<'_, AppState>) -> VaultStatus {
	state.vault.lock();
	state.vault.status()
}

/// `Some` sets or changes the master password; `None` returns to the keychain.
#[tauri::command]
pub fn vault_set_master_password(
	state: State<'_, AppState>,
	password: Option<String>,
) -> AppResult<VaultStatus> {
	let password = password.map(Zeroizing::new);
	state.vault.set_master_password(password.as_deref().map(String::as_str))?;
	Ok(state.vault.status())
}

fn secret(vault: &Vault, server_id: &str, kind: &str) -> AppResult<Option<Zeroizing<String>>> {
	vault
		.get(&secret_id(server_id, kind))?
		.map(|bytes| {
			String::from_utf8(bytes.to_vec())
				.map(Zeroizing::new)
				.map_err(|_| AppError::Io(format!("the saved {kind} is not text")))
		})
		.transpose()
}

fn target_for(s: &Server, vault: &Vault) -> AppResult<Target> {
	let auth = match s.auth.as_str() {
		"password" => Auth::Password(secret(vault, &s.id, "password")?.ok_or_else(|| {
			AppError::Unauthenticated(format!("no password is saved for {}", s.name))
		})?),
		"key" => Auth::Key {
			pem: secret(vault, &s.id, "key")?.ok_or_else(|| {
				AppError::Unauthenticated(format!("no private key is saved for {}", s.name))
			})?,
			passphrase: secret(vault, &s.id, "passphrase")?,
		},
		"keyFile" => Auth::KeyFile {
			path: servers::expand_home(s.key_path.as_deref().unwrap_or_default()),
			passphrase: secret(vault, &s.id, "passphrase")?,
		},
		"agent" => Auth::Agent,
		other => return Err(AppError::InvalidInput(format!("{other} is not a library login"))),
	};
	Ok(Target { host: s.host.clone(), port: s.port, user: s.username.clone(), auth })
}

/// The `ssh -t` command line for a server on the system client.
fn system_argv(s: &Server) -> Vec<String> {
	let mut v = vec!["ssh".to_string(), "-t".into(), "-p".into(), s.port.to_string()];
	v.push(format!("{}@{}", s.username, s.host));
	if let Some(dir) = s.start_dir.as_deref().filter(|d| !d.is_empty()) {
		v.push(ssh::start_in(dir));
	}
	v
}

/// Open a shell tab on a server.
///
/// An unknown host key comes back as `AppError::HostKey(fingerprint)`; the
/// renderer shows it to the human and calls again with `accept_fingerprint`.
#[tauri::command]
pub async fn server_open_shell(
	state: State<'_, AppState>,
	server_id: String,
	client_key: String,
	cols: u16,
	rows: u16,
	accept_fingerprint: Option<String>,
) -> AppResult<TerminalId> {
	let server = state.db.with(|c| servers::get(c, &server_id))?;
	let label = server.start_dir.clone().unwrap_or_else(|| "~".into());
	if server.auth == "system" {
		return state.terminals.spawn_ssh_command(
			&server.id,
			client_key,
			system_argv(&server),
			cols,
			rows,
		);
	}
	let vault = state.vault.clone();
	let for_target = server.clone();
	let conn = state
		.ssh
		.connection(
			&server.id,
			|| async move { target_for(&for_target, &vault) },
			accept_fingerprint,
		)
		.await?;
	let channel = ssh::open_shell(&conn, cols, rows, server.start_dir.as_deref()).await?;
	let (master, child) = pty::bridge(channel, cols, rows);
	state.terminals.attach_ssh(&server.id, client_key, &label, master, child)
}

/// Close the server's connection and every tab on it.
#[tauri::command]
pub fn server_disconnect(state: State<'_, AppState>, server_id: String) {
	state.terminals.kill_ssh_for(&server_id);
	state.ssh.disconnect(&server_id);
}

/// Where trusted host keys are written: the app's own file, never the user's.
pub fn known_hosts_path(data_dir: &std::path::Path) -> PathBuf {
	data_dir.join("known_hosts")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_system_client_gets_a_quoted_start_folder() {
		let s = Server {
			id: "1".into(),
			name: "web".into(),
			host: "web01".into(),
			port: 2222,
			username: "deploy".into(),
			start_dir: Some("/srv/my app".into()),
			folder_id: None,
			environment: "dev".into(),
			agent_policy: "free".into(),
			auth: "system".into(),
			key_path: None,
			has_password: false,
			has_private_key: false,
			has_passphrase: false,
			connected: false,
		};
		let argv = system_argv(&s);
		assert_eq!(&argv[..5], ["ssh", "-t", "-p", "2222", "deploy@web01"]);
		assert!(argv[5].starts_with("exec /bin/sh -c 'cd '\\''/srv/my app"), "{argv:?}");
	}
}
