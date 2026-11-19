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
use crate::services::terminal::{LinkDto, LinkMode, TerminalId};
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
	state.tunnels.stop_server(&id);
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

pub(crate) fn target_for(s: &Server, vault: &Vault) -> AppResult<Target> {
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
	state.tunnels.stop_server(&server_id);
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

/// Lend an SSH terminal to an agent session (F45). One session per terminal.
#[tauri::command]
pub fn agent_link_terminal(
	state: State<'_, AppState>,
	terminal_id: String,
	session_id: String,
	mode: LinkMode,
) -> AppResult<()> {
	state.terminals.link_agent(&terminal_id, &session_id, mode)
}

/// Take a terminal back from its agent. The agent's next call on it is refused.
#[tauri::command]
pub fn agent_unlink_terminal(state: State<'_, AppState>, terminal_id: String) {
	state.terminals.unlink_agent(&terminal_id);
}

#[tauri::command]
pub fn agent_links(state: State<'_, AppState>) -> Vec<LinkDto> {
	state.terminals.agent_links()
}

/// The workspace project a server's agent sessions run in (F45): a folder of
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

/// The human's answer to an agent command waiting for confirmation (ADR-0108).
/// `command` set and different from the request runs the edited line instead.
#[tauri::command]
pub fn agent_terminal_answer(
	state: State<'_, AppState>,
	id: String,
	run: bool,
	command: Option<String>,
) -> AppResult<()> {
	use crate::services::agent_policy::Answer;
	let answer = match (run, command.filter(|c| !c.trim().is_empty())) {
		(false, _) => Answer::Refuse,
		(true, Some(c)) => Answer::RunEdited(c),
		(true, None) => Answer::Run,
	};
	if state.confirmations.answer(&id, answer) {
		Ok(())
	} else {
		Err(AppError::NotFound("that confirmation is no longer waiting".into()))
	}
}

/// The last decisions about agent commands on one server, newest first.
#[tauri::command]
pub fn agent_audit(
	state: State<'_, AppState>,
	server_id: String,
) -> AppResult<Vec<crate::services::agent_policy::AuditRow>> {
	state.db.with(|c| Ok(crate::services::agent_policy::recent(c, &server_id, 200)?))
}

/// The SFTP backend for the browser: the same pooled login as the server's tabs.
fn files(state: &AppState) -> crate::services::ssh::sftp::RemoteFiles {
	let (db, vault) = (state.db.clone(), state.vault.clone());
	crate::services::ssh::sftp::RemoteFiles::new(
		state.ssh.clone(),
		std::sync::Arc::new(move |id| {
			let server = db.with(|c| servers::get(c, id))?;
			target_for(&server, &vault)
		}),
	)
}

/// One directory of a server, for the SFTP browser (F46). Empty `path` is the
/// login directory.
#[tauri::command]
pub async fn sftp_list(
	state: State<'_, AppState>,
	server_id: String,
	path: String,
) -> AppResult<crate::services::ssh::sftp::RemoteListing> {
	files(&state)
		.with(&server_id, |_, s| async move { crate::services::ssh::sftp::list(&s, &path).await })
		.await
}

/// Copy a remote file to a local path the human chose.
#[tauri::command]
pub async fn sftp_download(
	state: State<'_, AppState>,
	server_id: String,
	remote: String,
	local: String,
) -> AppResult<()> {
	files(&state)
		.with(&server_id, |_, s| async move {
			let mut from =
				s.open(remote.as_str()).await.map_err(|e| AppError::Remote(e.to_string()))?;
			let mut to = tokio::fs::File::create(&local).await?;
			tokio::io::copy(&mut from, &mut to).await?;
			Ok(())
		})
		.await
}

/// Copy a local file into a remote directory, under its own name. An existing
/// remote file of that name is not overwritten.
#[tauri::command]
pub async fn sftp_upload(
	state: State<'_, AppState>,
	server_id: String,
	local: String,
	remote_dir: String,
) -> AppResult<String> {
	let name = std::path::Path::new(&local)
		.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.ok_or_else(|| AppError::InvalidInput("choose a file".into()))?;
	let remote = format!("{}/{}", remote_dir.trim_end_matches('/'), name);
	files(&state)
		.with(&server_id, |_, s| async move {
			if s.try_exists(remote.as_str()).await.unwrap_or(false) {
				return Err(AppError::InvalidInput(format!("{remote} already exists")));
			}
			let mut from = tokio::fs::File::open(&local).await?;
			let mut to =
				s.create(remote.as_str()).await.map_err(|e| AppError::Remote(e.to_string()))?;
			tokio::io::copy(&mut from, &mut to).await?;
			tokio::io::AsyncWriteExt::shutdown(&mut to).await?;
			Ok(remote)
		})
		.await
}

#[tauri::command]
pub async fn sftp_mkdir(
	state: State<'_, AppState>,
	server_id: String,
	path: String,
) -> AppResult<()> {
	files(&state)
		.with(&server_id, |_, s| async move {
			s.create_dir(path.as_str()).await.map_err(|e| AppError::Remote(e.to_string()))
		})
		.await
}

#[tauri::command]
pub async fn sftp_rename(
	state: State<'_, AppState>,
	server_id: String,
	from: String,
	to: String,
) -> AppResult<()> {
	files(&state)
		.with(&server_id, |_, s| async move {
			s.rename(from.as_str(), to.as_str()).await.map_err(|e| AppError::Remote(e.to_string()))
		})
		.await
}

/// Delete a file, or an empty directory. Asked for by the renderer only after
/// the human confirmed; never recursive.
#[tauri::command]
pub async fn sftp_remove(
	state: State<'_, AppState>,
	server_id: String,
	path: String,
	is_dir: bool,
) -> AppResult<()> {
	files(&state)
		.with(&server_id, |_, s| async move {
			let r = if is_dir {
				s.remove_dir(path.as_str()).await
			} else {
				s.remove_file(path.as_str()).await
			};
			r.map_err(|e| AppError::Remote(e.to_string()))
		})
		.await
}

/// A server's saved tunnels, each saying whether it is listening (F47).
#[tauri::command]
pub fn list_tunnels(
	state: State<'_, AppState>,
	server_id: String,
) -> AppResult<Vec<crate::models::Tunnel>> {
	let list = state.db.with(|c| servers::list_tunnels(c, &server_id))?;
	Ok(list
		.into_iter()
		.map(|t| crate::models::Tunnel { running: state.tunnels.is_running(&t.id), ..t })
		.collect())
}

#[tauri::command]
pub fn save_tunnel(
	state: State<'_, AppState>,
	tunnel: crate::models::Tunnel,
) -> AppResult<crate::models::Tunnel> {
	// A changed port or destination takes effect on the next start.
	state.tunnels.stop(&tunnel.id);
	state.db.with(|c| servers::save_tunnel(c, &tunnel))
}

#[tauri::command]
pub fn delete_tunnel(state: State<'_, AppState>, id: String) -> AppResult<()> {
	state.tunnels.stop(&id);
	state.db.with(|c| servers::delete_tunnel(c, &id))
}

/// Start listening on the tunnel's local port. An unknown host key or a failed
/// login is reported now, not on the first connection through it.
#[tauri::command]
pub async fn tunnel_start(state: State<'_, AppState>, id: String) -> AppResult<()> {
	let t = state.db.with(|c| servers::get_tunnel(c, &id))?;
	let server = state.db.with(|c| servers::get(c, &t.server_id))?;
	if server.auth == "system" {
		return Err(AppError::InvalidInput(
			"tunnels need a library login, not the system ssh client".into(),
		));
	}
	let (pool, db, vault) = (state.ssh.clone(), state.db.clone(), state.vault.clone());
	let server_id = server.id.clone();
	let conn = move || {
		let (pool, db, vault, server_id) =
			(pool.clone(), db.clone(), vault.clone(), server_id.clone());
		async move {
			let id = server_id.clone();
			pool.connection(
				&server_id,
				|| async move {
					let server = db.with(|c| servers::get(c, &id))?;
					target_for(&server, &vault)
				},
				None,
			)
			.await
		}
	};
	conn().await?;
	state.tunnels.start(&t.id, &t.server_id, t.local_port, t.dest_host, t.dest_port, conn).await
}

#[tauri::command]
pub fn tunnel_stop(state: State<'_, AppState>, id: String) {
	state.tunnels.stop(&id);
}
