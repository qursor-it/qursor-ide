//! The saved-server address book (F45): rows in SQLite, secrets in the vault.
//!
//! Validation lives here rather than in the command so every writer — the form,
//! an import — goes through the same checks.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::models::{Server, ServerFolder, ServerInput, ServerSource, SshConfigHost, Tunnel};
use crate::services::vault::Vault;

const ENVIRONMENTS: &[&str] = &["dev", "staging", "prod"];
const POLICIES: &[&str] = &["free", "confirm", "readonly"];
const AUTHS: &[&str] = &["password", "key", "keyFile", "agent", "system"];

/// The vault ids a server's secrets live under.
pub fn secret_id(server_id: &str, kind: &str) -> String {
	format!("server:{server_id}:{kind}")
}

pub fn secret_prefix(server_id: &str) -> String {
	format!("server:{server_id}:")
}

fn row_to_server(r: &rusqlite::Row<'_>) -> rusqlite::Result<Server> {
	Ok(Server {
		id: r.get(0)?,
		name: r.get(1)?,
		host: r.get(2)?,
		port: r.get(3)?,
		username: r.get(4)?,
		start_dir: r.get(5)?,
		folder_id: r.get(6)?,
		environment: r.get(7)?,
		agent_policy: r.get(8)?,
		auth: r.get(9)?,
		key_path: r.get(10)?,
		persistent: r.get(11)?,
		has_password: false,
		has_private_key: false,
		has_passphrase: false,
		connected: false,
		source: ServerSource::Local,
	})
}

const COLUMNS: &str = "id, name, host, port, username, start_dir, folder_id, environment, agent_policy, auth, key_path, persistent";

pub fn list(conn: &Connection) -> AppResult<Vec<Server>> {
	let mut st = conn.prepare(&format!(
		"SELECT {COLUMNS} FROM servers ORDER BY sort_order, name COLLATE NOCASE"
	))?;
	let rows = st.query_map([], row_to_server)?.collect::<Result<Vec<_>, _>>()?;
	Ok(rows)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Server> {
	conn.query_row(&format!("SELECT {COLUMNS} FROM servers WHERE id = ?1"), [id], row_to_server)
		.optional()?
		.ok_or_else(|| AppError::NotFound(format!("server {id}")))
}

pub fn list_folders(conn: &Connection) -> AppResult<Vec<ServerFolder>> {
	let mut st = conn
		.prepare("SELECT id, name FROM server_folders ORDER BY sort_order, name COLLATE NOCASE")?;
	let rows = st
		.query_map([], |r| Ok(ServerFolder { id: r.get(0)?, name: r.get(1)? }))?
		.collect::<Result<Vec<_>, _>>()?;
	Ok(rows)
}

/// Fill the `has_*` flags from the vault. Never reads a secret's value.
pub fn with_secret_flags(mut s: Server, vault: &Vault) -> Server {
	s.has_password = vault.has(&secret_id(&s.id, "password"));
	s.has_private_key = vault.has(&secret_id(&s.id, "key"));
	s.has_passphrase = vault.has(&secret_id(&s.id, "passphrase"));
	s
}

fn trimmed(v: &Option<String>) -> Option<String> {
	v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

pub(crate) fn validate(input: &ServerInput) -> AppResult<()> {
	let bad = |m: &str| Err(AppError::InvalidInput(m.to_string()));
	if input.name.trim().is_empty() {
		return bad("a server needs a name");
	}
	let host = input.host.trim();
	if host.is_empty() || host.chars().any(|c| c.is_whitespace() || c == '@' || c == '/') {
		return bad("the host is a name or an address, without user@ or a path");
	}
	if input.username.trim().is_empty() {
		return bad("a server needs a user");
	}
	if input.port == 0 {
		return bad("the port must be between 1 and 65535");
	}
	if !ENVIRONMENTS.contains(&input.environment.as_str()) {
		return bad("unknown environment");
	}
	if !POLICIES.contains(&input.agent_policy.as_str()) {
		return bad("unknown agent policy");
	}
	if !AUTHS.contains(&input.auth.as_str()) {
		return bad("unknown authentication");
	}
	if input.auth == "keyFile" && trimmed(&input.key_path).is_none() {
		return bad("choose the key file");
	}
	if let Some(dir) = trimmed(&input.start_dir) {
		if !dir.starts_with('/') && !dir.starts_with('~') {
			return bad("the start folder is an absolute remote path");
		}
	}
	Ok(())
}

/// Insert or update a server and write the secrets it was given.
pub fn save(
	conn: &Connection,
	vault: &Vault,
	input: &ServerInput,
	now_ms: i64,
) -> AppResult<Server> {
	validate(input)?;
	// Secrets first: a vault that refuses (locked, no keychain) must not leave a
	// row behind that promises a password it does not have.
	let id = input.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
	for (kind, value) in [
		("password", &input.password),
		("key", &input.private_key),
		("passphrase", &input.passphrase),
	] {
		match value.as_deref() {
			None => {}
			Some("") => vault.remove(&secret_id(&id, kind))?,
			Some(v) => vault.put(&secret_id(&id, kind), v.as_bytes())?,
		}
	}
	let start_dir = trimmed(&input.start_dir);
	let key_path = if input.auth == "keyFile" { trimmed(&input.key_path) } else { None };
	let folder = trimmed(&input.folder_id);
	if input.id.is_some() {
		let n = conn.execute(
			"UPDATE servers SET name=?2, host=?3, port=?4, username=?5, start_dir=?6, folder_id=?7,
			 environment=?8, agent_policy=?9, auth=?10, key_path=?11, persistent=?12 WHERE id=?1",
			params![
				id,
				input.name.trim(),
				input.host.trim(),
				input.port,
				input.username.trim(),
				start_dir,
				folder,
				input.environment,
				input.agent_policy,
				input.auth,
				key_path,
				input.persistent
			],
		)?;
		if n == 0 {
			return Err(AppError::NotFound(format!("server {id}")));
		}
	} else {
		conn.execute(
			"INSERT INTO servers (id, name, host, port, username, start_dir, folder_id, environment,
			 agent_policy, auth, key_path, created_at, persistent) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
			params![
				id,
				input.name.trim(),
				input.host.trim(),
				input.port,
				input.username.trim(),
				start_dir,
				folder,
				input.environment,
				input.agent_policy,
				input.auth,
				key_path,
				now_ms,
				input.persistent
			],
		)?;
	}
	Ok(with_secret_flags(get(conn, &id)?, vault))
}

pub fn delete(conn: &Connection, vault: &Vault, id: &str) -> AppResult<()> {
	vault.remove_prefix(&secret_prefix(id))?;
	conn.execute("DELETE FROM servers WHERE id = ?1", [id])?;
	Ok(())
}

pub fn create_folder(conn: &Connection, name: &str) -> AppResult<ServerFolder> {
	let name = name.trim();
	if name.is_empty() {
		return Err(AppError::InvalidInput("a folder needs a name".into()));
	}
	let id = uuid::Uuid::new_v4().to_string();
	conn.execute("INSERT INTO server_folders (id, name) VALUES (?1, ?2)", params![id, name])?;
	Ok(ServerFolder { id, name: name.to_string() })
}

pub fn rename_folder(conn: &Connection, id: &str, name: &str) -> AppResult<()> {
	let name = name.trim();
	if name.is_empty() {
		return Err(AppError::InvalidInput("a folder needs a name".into()));
	}
	conn.execute("UPDATE server_folders SET name = ?2 WHERE id = ?1", params![id, name])?;
	Ok(())
}

/// Deleting a folder keeps its servers; they move to the top level.
pub fn delete_folder(conn: &Connection, id: &str) -> AppResult<()> {
	conn.execute("DELETE FROM server_folders WHERE id = ?1", [id])?;
	Ok(())
}

pub fn list_tunnels(conn: &Connection, server_id: &str) -> AppResult<Vec<Tunnel>> {
	let mut st = conn.prepare(
		"SELECT id, server_id, label, local_port, dest_host, dest_port FROM server_tunnels
		 WHERE server_id = ?1 ORDER BY local_port",
	)?;
	let rows = st
		.query_map([server_id], |r| {
			Ok(Tunnel {
				id: r.get(0)?,
				server_id: r.get(1)?,
				label: r.get(2)?,
				local_port: r.get(3)?,
				dest_host: r.get(4)?,
				dest_port: r.get(5)?,
				running: false,
			})
		})?
		.collect::<Result<Vec<_>, _>>()?;
	Ok(rows)
}

pub fn get_tunnel(conn: &Connection, id: &str) -> AppResult<Tunnel> {
	conn.query_row(
		"SELECT id, server_id, label, local_port, dest_host, dest_port FROM server_tunnels WHERE id = ?1",
		[id],
		|r| {
			Ok(Tunnel {
				id: r.get(0)?,
				server_id: r.get(1)?,
				label: r.get(2)?,
				local_port: r.get(3)?,
				dest_host: r.get(4)?,
				dest_port: r.get(5)?,
				running: false,
			})
		},
	)
	.optional()?
	.ok_or_else(|| AppError::NotFound(format!("tunnel {id}")))
}

pub(crate) fn validate_tunnel(t: &Tunnel) -> AppResult<()> {
	let host = t.dest_host.trim();
	if t.local_port == 0
		|| t.dest_port == 0
		|| host.is_empty()
		|| host.contains(char::is_whitespace)
	{
		return Err(AppError::InvalidInput(
			"a tunnel needs a local port, a destination host and port".into(),
		));
	}
	Ok(())
}

pub fn save_tunnel(conn: &Connection, t: &Tunnel) -> AppResult<Tunnel> {
	validate_tunnel(t)?;
	let host = t.dest_host.trim();
	let id = if t.id.is_empty() { uuid::Uuid::new_v4().to_string() } else { t.id.clone() };
	conn.execute(
		"INSERT INTO server_tunnels (id, server_id, label, local_port, dest_host, dest_port)
		 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
		 ON CONFLICT(id) DO UPDATE SET label=?3, local_port=?4, dest_host=?5, dest_port=?6",
		params![id, t.server_id, t.label.trim(), t.local_port, host, t.dest_port],
	)?;
	get_tunnel(conn, &id)
}

pub fn delete_tunnel(conn: &Connection, id: &str) -> AppResult<()> {
	conn.execute("DELETE FROM server_tunnels WHERE id = ?1", [id])?;
	Ok(())
}

/// The concrete `Host` blocks of an OpenSSH client config, for import.
///
/// shortcut: reads `Host`, `HostName`, `User`, `Port`, `IdentityFile`,
/// `ProxyJump`/`ProxyCommand` only, and follows no `Include`; upgrade when an
/// import misses hosts people actually have.
pub fn parse_ssh_config(text: &str) -> Vec<SshConfigHost> {
	let mut out: Vec<SshConfigHost> = Vec::new();
	let mut current: Vec<SshConfigHost> = Vec::new();
	let flush = |current: &mut Vec<SshConfigHost>, out: &mut Vec<SshConfigHost>| {
		out.append(current);
	};
	for line in text.lines() {
		let line = line.trim();
		if line.is_empty() || line.starts_with('#') {
			continue;
		}
		let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
			Some((k, v)) => {
				(k.to_ascii_lowercase(), v.trim().trim_start_matches('=').trim().trim_matches('"'))
			}
			None => continue,
		};
		match key.as_str() {
			"host" => {
				flush(&mut current, &mut out);
				current = value
					.split_whitespace()
					.filter(|a| !a.contains(['*', '?', '!']))
					.map(|alias| SshConfigHost {
						alias: alias.to_string(),
						host: alias.to_string(),
						port: 22,
						username: None,
						identity_file: None,
						needs_system_ssh: false,
					})
					.collect();
			}
			"match" => flush(&mut current, &mut out),
			"hostname" => current.iter_mut().for_each(|h| h.host = value.to_string()),
			"user" => current.iter_mut().for_each(|h| h.username = Some(value.to_string())),
			"port" => {
				if let Ok(p) = value.parse() {
					current.iter_mut().for_each(|h| h.port = p);
				}
			}
			"identityfile" => current.iter_mut().for_each(|h| {
				h.identity_file.get_or_insert_with(|| value.to_string());
			}),
			"proxyjump" | "proxycommand" => {
				current.iter_mut().for_each(|h| h.needs_system_ssh = true)
			}
			_ => {}
		}
	}
	flush(&mut current, &mut out);
	out
}

pub fn read_user_ssh_config() -> Vec<SshConfigHost> {
	let Some(home) = dirs::home_dir() else { return Vec::new() };
	match std::fs::read_to_string(home.join(".ssh").join("config")) {
		Ok(text) => parse_ssh_config(&text),
		Err(_) => Vec::new(),
	}
}

/// `~` at the start of a key path, as OpenSSH writes it.
pub fn expand_home(p: &str) -> std::path::PathBuf {
	match (p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")), dirs::home_dir()) {
		(Some(rest), Some(home)) => home.join(rest),
		_ => Path::new(p).to_path_buf(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ssh_config_import_reads_concrete_hosts_and_skips_patterns() {
		let hosts = parse_ssh_config(
			"Host *\n  ServerAliveInterval 30\n\nHost web01 web\n  HostName web01.shop.it\n  User deploy\n  Port 2222\n  IdentityFile ~/.ssh/shop\n\nHost db01\n  HostName=10.0.0.5\n  ProxyJump bastion\n",
		);
		assert_eq!(hosts.len(), 3);
		assert_eq!(hosts[0].alias, "web01");
		assert_eq!(hosts[0].host, "web01.shop.it");
		assert_eq!(hosts[0].port, 2222);
		assert_eq!(hosts[0].username.as_deref(), Some("deploy"));
		assert_eq!(hosts[0].identity_file.as_deref(), Some("~/.ssh/shop"));
		assert_eq!(hosts[1].alias, "web");
		assert_eq!(hosts[2].host, "10.0.0.5");
		assert!(hosts[2].needs_system_ssh);
	}

	#[test]
	fn the_form_is_validated_before_anything_is_written() {
		let mut input = ServerInput {
			id: None,
			name: "web01".into(),
			host: "deploy@web01".into(),
			port: 22,
			username: "deploy".into(),
			start_dir: None,
			folder_id: None,
			environment: "dev".into(),
			agent_policy: "free".into(),
			auth: "agent".into(),
			key_path: None,
			persistent: false,
			password: None,
			private_key: None,
			passphrase: None,
		};
		assert!(validate(&input).is_err(), "user@ in the host");
		input.host = "web01".into();
		assert!(validate(&input).is_ok());
		input.start_dir = Some("var/www".into());
		assert!(validate(&input).is_err(), "relative start folder");
		input.start_dir = None;
		input.auth = "keyFile".into();
		assert!(validate(&input).is_err(), "keyFile without a path");
	}
}
