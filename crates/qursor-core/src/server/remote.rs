//! The address book's live half, the SFTP browser and the tunnels: everything
//! that needs the SSH pool, which only the core holds
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §5a). Each method is
//! the app's command of the same purpose, by its wire name.

use serde::Deserialize;
use serde_json::Value;

use super::{parse, to_value, Shared};
use crate::error::{AppError, AppResult};
use crate::models::{Server, ServerBook, ServerInput, ServerSource, Tunnel};
use crate::services::servers;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerParams {
	server_id: String,
}

#[derive(Deserialize)]
struct IdParams {
	id: String,
}

impl Shared {
	fn flagged(&self, s: Server) -> Server {
		let connected = self.core.ssh.is_connected(&s.id);
		Server { connected, ..servers::with_secret_flags(s, &self.core.vault) }
	}

	/// `None` when `method` is not one of these.
	pub(super) async fn remote_rpc(&self, method: &str, params: Value) -> Option<AppResult<Value>> {
		Some(match method {
			"server.list" => self.list_servers(),
			"server.save" => self.save_server(params),
			"server.delete" => self.delete_server(params),
			"server.disconnect" => parse::<ServerParams>(params).map(|p| {
				self.core.tunnels.stop_server(&p.server_id);
				self.core.terminals.kill_ssh_for(&p.server_id);
				self.core.ssh.disconnect(&p.server_id);
				Value::Null
			}),
			"tunnel.list" => self.list_tunnels(params),
			"tunnel.save" => self.save_tunnel(params),
			"tunnel.delete" => parse::<IdParams>(params).and_then(|p| {
				self.core.tunnels.stop(&p.id);
				self.core.db.with(|c| servers::delete_tunnel(c, &p.id))?;
				Ok(Value::Null)
			}),
			"tunnel.start" => match parse::<IdParams>(params) {
				Ok(p) => self.start_tunnel(p.id).await,
				Err(e) => Err(e),
			},
			"tunnel.stop" => parse::<IdParams>(params).map(|p| {
				self.core.tunnels.stop(&p.id);
				Value::Null
			}),
			m if m.starts_with("sftp.") => self.sftp(m, params).await,
			_ => return None,
		})
	}

	fn list_servers(&self) -> AppResult<Value> {
		// `list` scans the project files; the folders and problems come from that scan.
		let (list, folders) =
			self.core.db.with(|c| Ok((servers::list(c)?, servers::list_folders(c)?)))?;
		to_value(ServerBook {
			folders,
			servers: list.into_iter().map(|s| self.flagged(s)).collect(),
			problems: servers::problems(),
		})
	}

	fn save_server(&self, params: Value) -> AppResult<Value> {
		#[derive(Deserialize)]
		struct P {
			input: ServerInput,
		}
		let P { input } = parse(params)?;
		let now = chrono::Utc::now().timestamp_millis();
		let before = match &input.id {
			Some(id) => self.core.db.with(|c| servers::get(c, id)).ok().map(|s| s.source),
			None => None,
		};
		let saved = self.core.db.with(|c| servers::save(c, &self.core.vault, &input, now))?;
		if before.is_some_and(|b| home_changed(&b, &saved.source)) {
			self.core.tunnels.stop_server(&saved.id);
		}
		// New credentials or a new address take effect on the next login; an idle
		// connection is dropped so that login happens now, an open tab keeps its own.
		if !self.core.terminals.has_ssh_for(&saved.id) {
			self.core.ssh.disconnect(&saved.id);
		}
		to_value(self.flagged(saved))
	}

	fn delete_server(&self, params: Value) -> AppResult<Value> {
		let IdParams { id } = parse(params)?;
		self.core.tunnels.stop_server(&id);
		self.core.terminals.kill_ssh_for(&id);
		self.core.ssh.disconnect(&id);
		self.core.db.with(|c| servers::delete(c, &self.core.vault, &id))?;
		Ok(Value::Null)
	}

	/// A server's saved tunnels, each saying whether it is listening (F48).
	fn list_tunnels(&self, params: Value) -> AppResult<Value> {
		let ServerParams { server_id } = parse(params)?;
		let list = self.core.db.with(|c| servers::list_tunnels(c, &server_id))?;
		to_value(
			list.into_iter()
				.map(|t| Tunnel { running: self.core.tunnels.is_running(&t.id), ..t })
				.collect::<Vec<_>>(),
		)
	}

	fn save_tunnel(&self, params: Value) -> AppResult<Value> {
		#[derive(Deserialize)]
		struct P {
			tunnel: Tunnel,
		}
		let P { tunnel } = parse(params)?;
		// A changed port or destination takes effect on the next start.
		self.core.tunnels.stop(&tunnel.id);
		to_value(self.core.db.with(|c| servers::save_tunnel(c, &tunnel))?)
	}

	/// Start listening on the tunnel's local port. An unknown host key or a
	/// failed login is reported now, not on the first connection through it.
	async fn start_tunnel(&self, id: String) -> AppResult<Value> {
		let t = self.core.db.with(|c| servers::get_tunnel(c, &id))?;
		let server = self.core.db.with(|c| servers::get(c, &t.server_id))?;
		if server.auth == "system" {
			return Err(AppError::InvalidInput(
				"tunnels need a library login, not the system ssh client".into(),
			));
		}
		let (pool, db, vault) =
			(self.core.ssh.clone(), self.core.db.clone(), self.core.vault.clone());
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
						servers::target_for(&server, &vault)
					},
					None,
				)
				.await
			}
		};
		conn().await?;
		self.core
			.tunnels
			.start(&t.id, &t.server_id, t.local_port, t.dest_host, t.dest_port, conn)
			.await?;
		Ok(Value::Null)
	}

	/// The SFTP browser (F47), on the same pooled login as the server's tabs.
	/// Local paths are this host's: the browser runs beside the core in phase 1.
	async fn sftp(&self, method: &str, params: Value) -> AppResult<Value> {
		#[derive(Deserialize)]
		#[serde(rename_all = "camelCase")]
		struct P {
			server_id: String,
			#[serde(default)]
			path: String,
			#[serde(default)]
			remote: String,
			#[serde(default)]
			local: String,
			#[serde(default)]
			remote_dir: String,
			#[serde(default)]
			from: String,
			#[serde(default)]
			to: String,
			#[serde(default)]
			is_dir: bool,
		}
		let p: P = parse(params)?;
		let remote_err = |e: russh_sftp::client::error::Error| AppError::Remote(e.to_string());
		let files = &self.core.files;
		match method {
			// Empty `path` is the login directory.
			"sftp.list" => to_value(
				files
					.with(&p.server_id, |_, s| async move {
						crate::services::ssh::sftp::list(&s, &p.path).await
					})
					.await?,
			),
			"sftp.download" => {
				files
					.with(&p.server_id, |_, s| async move {
						let mut from = s.open(p.remote.as_str()).await.map_err(remote_err)?;
						let mut to = tokio::fs::File::create(&p.local).await?;
						tokio::io::copy(&mut from, &mut to).await?;
						Ok(())
					})
					.await?;
				Ok(Value::Null)
			}
			// Under its own name; an existing remote file is not overwritten.
			"sftp.upload" => {
				let name = std::path::Path::new(&p.local)
					.file_name()
					.map(|n| n.to_string_lossy().into_owned())
					.ok_or_else(|| AppError::InvalidInput("choose a file".into()))?;
				let remote = format!("{}/{}", p.remote_dir.trim_end_matches('/'), name);
				let local = p.local;
				let remote = files
					.with(&p.server_id, |_, s| async move {
						if s.try_exists(remote.as_str()).await.unwrap_or(false) {
							return Err(AppError::InvalidInput(format!("{remote} already exists")));
						}
						let mut from = tokio::fs::File::open(&local).await?;
						let mut to = s.create(remote.as_str()).await.map_err(remote_err)?;
						tokio::io::copy(&mut from, &mut to).await?;
						tokio::io::AsyncWriteExt::shutdown(&mut to).await?;
						Ok(remote)
					})
					.await?;
				Ok(Value::String(remote))
			}
			"sftp.mkdir" => {
				files
					.with(&p.server_id, |_, s| async move {
						s.create_dir(p.path.as_str()).await.map_err(remote_err)
					})
					.await?;
				Ok(Value::Null)
			}
			"sftp.rename" => {
				files
					.with(&p.server_id, |_, s| async move {
						s.rename(p.from.as_str(), p.to.as_str()).await.map_err(remote_err)
					})
					.await?;
				Ok(Value::Null)
			}
			// Asked for only after the human confirmed; never recursive.
			"sftp.remove" => {
				files
					.with(&p.server_id, |_, s| async move {
						if p.is_dir {
							s.remove_dir(p.path.as_str()).await
						} else {
							s.remove_file(p.path.as_str()).await
						}
						.map_err(remote_err)
					})
					.await?;
				Ok(Value::Null)
			}
			other => Err(AppError::NotFound(format!("method {other}"))),
		}
	}
}

/// Another home gives a server's tunnels other ids, orphaning running ones.
fn home_changed(before: &ServerSource, after: &ServerSource) -> bool {
	let home = |s: &ServerSource| match s {
		ServerSource::Local => None,
		ServerSource::Project { project_id, .. } => Some(project_id.clone()),
	};
	home(before) != home(after)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_change_of_home_is_noticed() {
		use crate::models::ServerSource::{Local, Project};
		let p = |id: &str, rel: &str| Project {
			project_id: id.into(),
			rel_path: rel.into(),
			sha256: rel.into(),
		};
		assert!(home_changed(&Local, &p("P1", "a")));
		assert!(home_changed(&p("P1", "a"), &Local));
		assert!(home_changed(&p("P1", "a"), &p("P2", "a")));
		assert!(!home_changed(&p("P1", "a"), &p("P1", "b")));
		assert!(!home_changed(&Local, &Local));
	}
}
