//! Files on a server over SFTP (F47): the browser beside an SSH tab, and the
//! file tools an agent gets on the servers whose terminals it was lent.
//!
//! **A channel per operation** on the server's one connection (ADR-0105): an
//! SFTP session is a subsystem channel, cheap to open, and keeping none means
//! none can outlive the connection it rode on.
//!
//! **A write is atomic and keeps the file's mode**: the bytes go to a temporary
//! name beside the target, which is then renamed over it with `mv -f` — SFTP v3
//! has no rename that replaces.
//!
//! **An agent writes only what it read** (F47): `agent_read` remembers each
//! file's size and mtime per session, and a write or edit refuses when the
//! file changed since — a human edited it, or it was never read.

use std::future::Future;
use std::sync::Arc;

use dashmap::DashMap;
use russh_sftp::client::SftpSession;
use serde::Serialize;

use super::pool::Pool;
use super::{quote, Connection, Target};
use crate::error::{AppError, AppResult};

/// Entries listed per directory; the rest are counted, not sent.
const MAX_ENTRIES: usize = 2000;
/// The largest file read as text, for the agent or the viewer.
pub const MAX_TEXT: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
	pub name: String,
	pub is_dir: bool,
	pub is_symlink: bool,
	pub size: u64,
	/// Seconds since the epoch.
	pub mtime: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteListing {
	/// The directory, canonical (`~` and `..` resolved by the server).
	pub path: String,
	pub entries: Vec<RemoteEntry>,
	pub truncated: bool,
}

fn sftp_err(e: impl std::fmt::Display) -> AppError {
	AppError::Remote(format!("sftp: {e}"))
}

pub async fn open(conn: &Connection) -> AppResult<SftpSession> {
	let ch = conn.channel_open_session().await.map_err(sftp_err)?;
	ch.request_subsystem(true, "sftp").await.map_err(sftp_err)?;
	SftpSession::new(ch.into_stream()).await.map_err(sftp_err)
}

pub async fn list(s: &SftpSession, path: &str) -> AppResult<RemoteListing> {
	let path = s.canonicalize(if path.is_empty() { "." } else { path }).await.map_err(sftp_err)?;
	let mut entries: Vec<RemoteEntry> = s
		.read_dir(path.as_str())
		.await
		.map_err(sftp_err)?
		.map(|e| {
			let m = e.metadata();
			RemoteEntry {
				name: e.file_name(),
				is_dir: m.is_dir(),
				is_symlink: m.is_symlink(),
				size: m.size.unwrap_or(0),
				mtime: m.mtime,
			}
		})
		.filter(|e| e.name != "." && e.name != "..")
		.collect();
	entries.sort_by(|a, b| {
		b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
	});
	let truncated = entries.len() > MAX_ENTRIES;
	entries.truncate(MAX_ENTRIES);
	Ok(RemoteListing { path, entries, truncated })
}

/// `(size, mtime)`: what "unchanged since read" is judged on.
async fn stamp(s: &SftpSession, path: &str) -> AppResult<Option<(u64, u32)>> {
	match s.metadata(path).await {
		Ok(m) => Ok(Some((m.size.unwrap_or(0), m.mtime.unwrap_or(0)))),
		Err(russh_sftp::client::error::Error::Status(st))
			if st.status_code == russh_sftp::protocol::StatusCode::NoSuchFile =>
		{
			Ok(None)
		}
		Err(e) => Err(sftp_err(e)),
	}
}

/// Write `bytes` to `path` atomically, keeping the existing file's mode and,
/// where the login may, its owner and group. A symlink is written through, not
/// replaced: the target it points to is what changes.
pub async fn write_atomic(
	conn: &Connection,
	s: &SftpSession,
	path: &str,
	bytes: &[u8],
) -> AppResult<()> {
	use russh_sftp::protocol::{FileAttributes, OpenFlags};
	use tokio::io::AsyncWriteExt;
	let path = match s.canonicalize(path).await {
		Ok(real) => real,
		// A new file: nothing to resolve yet.
		Err(_) => path.to_string(),
	};
	let existing = s.metadata(path.as_str()).await.ok();
	let tmp = format!("{path}.qursor-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
	let mut attrs = FileAttributes::empty();
	// Born with the target's mode (or owner-only for a new file), never wider.
	attrs.permissions =
		Some(existing.as_ref().and_then(|m| m.permissions).unwrap_or(0o600) & 0o7777);
	{
		let mut f = s
			.open_with_flags_and_attributes(
				tmp.as_str(),
				OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE | OpenFlags::EXCLUDE,
				attrs.clone(),
			)
			.await
			.map_err(sftp_err)?;
		f.write_all(bytes).await.map_err(sftp_err)?;
		f.shutdown().await.map_err(sftp_err)?;
	}
	// Mode again (a umask may have narrowed it), then owner and group, which
	// only a privileged login may set: best effort, the mode is not.
	if let Err(e) = s.set_metadata(tmp.as_str(), attrs).await {
		let _ = s.remove_file(tmp.as_str()).await;
		return Err(sftp_err(e));
	}
	if let Some(m) = &existing {
		let mut owner = FileAttributes::empty();
		owner.uid = m.uid;
		owner.gid = m.gid;
		let _ = s.set_metadata(tmp.as_str(), owner).await;
	}
	let moved = exec(conn, &format!("mv -f -- {} {}", quote(&tmp), quote(&path))).await;
	if moved.is_err() {
		let _ = s.remove_file(tmp.as_str()).await;
	}
	moved
}

/// Run one short command, require exit 0, and return its standard output.
async fn exec_output(conn: &Connection, command: &str) -> AppResult<String> {
	let mut ch = conn.channel_open_session().await.map_err(sftp_err)?;
	ch.exec(true, command).await.map_err(sftp_err)?;
	let (mut code, mut out) = (None, Vec::new());
	while let Some(msg) = ch.wait().await {
		match msg {
			russh::ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
			russh::ChannelMsg::Data { data } => out.extend_from_slice(&data),
			_ => {}
		}
	}
	match code {
		Some(0) => Ok(String::from_utf8_lossy(&out).into_owned()),
		_ => Err(AppError::Remote(format!("`{command}` failed"))),
	}
}

/// Run one short command and require exit 0.
async fn exec(conn: &Connection, command: &str) -> AppResult<()> {
	let mut ch = conn.channel_open_session().await.map_err(sftp_err)?;
	ch.exec(true, command).await.map_err(sftp_err)?;
	let mut code = None;
	let mut err = Vec::new();
	while let Some(msg) = ch.wait().await {
		match msg {
			russh::ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
			russh::ChannelMsg::ExtendedData { data, .. } => err.extend_from_slice(&data),
			_ => {}
		}
	}
	match code {
		Some(0) => Ok(()),
		_ => Err(AppError::Remote(format!(
			"`{command}` failed: {}",
			String::from_utf8_lossy(&err).trim()
		))),
	}
}

pub type TargetFn = Arc<dyn Fn(&str) -> AppResult<Target> + Send + Sync>;

/// SFTP on the pooled connections, and the agents' read stamps.
pub struct RemoteFiles {
	pool: Arc<Pool>,
	target: TargetFn,
	/// `(session, server, path)` → what the file looked like when that session
	/// last read it.
	reads: DashMap<(String, String, String), Option<(u64, u32)>>,
}

impl RemoteFiles {
	pub fn new(pool: Arc<Pool>, target: TargetFn) -> Self {
		Self { pool, target, reads: DashMap::new() }
	}

	/// Run `f` with the server's connection and a fresh SFTP session on it.
	pub async fn with<T, F, Fut>(&self, server_id: &str, f: F) -> AppResult<T>
	where
		F: FnOnce(Arc<Connection>, SftpSession) -> Fut,
		Fut: Future<Output = AppResult<T>>,
	{
		let target = self.target.clone();
		let id = server_id.to_string();
		let conn = self.pool.connection(server_id, || async move { target(&id) }, None).await?;
		let s = open(&conn).await?;
		let out = f(conn, s).await;
		out
	}

	/// Read a text file for an agent, and remember what it looked like.
	pub fn agent_read(&self, session: &str, server_id: &str, path: &str) -> AppResult<String> {
		let (text, st) = tauri::async_runtime::block_on(self.with(server_id, |_, s| async move {
			let st = stamp(&s, path)
				.await?
				.ok_or_else(|| AppError::NotFound(format!("{path} does not exist")))?;
			if st.0 > MAX_TEXT {
				return Err(AppError::InvalidInput(format!(
					"{path} is {} bytes; read it with a command instead",
					st.0
				)));
			}
			let bytes = s.read(path).await.map_err(sftp_err)?;
			let text = String::from_utf8(bytes)
				.map_err(|_| AppError::InvalidInput(format!("{path} is not text")))?;
			Ok((text, st))
		}))?;
		self.reads.insert((session.into(), server_id.into(), path.into()), Some(st));
		Ok(text)
	}

	/// Write a whole file for an agent: a new file, or one it read and that has
	/// not changed since.
	pub fn agent_write(
		&self,
		session: &str,
		server_id: &str,
		path: &str,
		content: &str,
	) -> AppResult<()> {
		let key = (session.to_string(), server_id.to_string(), path.to_string());
		let read = self.reads.get(&key).map(|r| *r);
		tauri::async_runtime::block_on(self.with(server_id, |conn, s| async move {
			let now = stamp(&s, path).await?;
			match (now, read) {
				(None, _) => {}
				(Some(_), None) => {
					return Err(AppError::InvalidInput(format!(
						"{path} exists and you have not read it; read it first"
					)))
				}
				(Some(n), Some(r)) if Some(n) != r => {
					return Err(AppError::InvalidInput(format!(
						"{path} changed since you read it (someone else edited it); read it again"
					)))
				}
				_ => {}
			}
			write_atomic(&conn, &s, path, content.as_bytes()).await
		}))?;
		// The new content is what this session now knows.
		let st = tauri::async_runtime::block_on(
			self.with(server_id, |_, s| async move { stamp(&s, path).await }),
		)?;
		self.reads.insert(key, st);
		Ok(())
	}
}

impl RemoteFiles {
	/// Whether a persistent tab's tmux pane is at rest (F49): not in a
	/// full-screen program and running nothing but its shell. Asked of tmux
	/// itself, because tmux draws its own alternate screen whatever the pane does.
	pub fn tmux_at_rest(&self, server_id: &str, session: &str) -> AppResult<Result<(), String>> {
		let cmd = format!(
			"tmux display-message -p -t {} '#{{alternate_on}} #{{pane_current_command}}'",
			quote(session)
		);
		let out = tauri::async_runtime::block_on(
			self.with(server_id, |conn, _| async move { exec_output(&conn, &cmd).await }),
		)?;
		Ok(tmux_verdict(&out))
	}

	/// A tmux session's text history, for reading a command's output back (F49).
	pub fn tmux_capture(&self, server_id: &str, session: &str) -> AppResult<String> {
		let cmd = format!("tmux capture-pane -p -J -S -3000 -t {}", quote(session));
		tauri::async_runtime::block_on(
			self.with(server_id, |conn, _| async move { exec_output(&conn, &cmd).await }),
		)
	}

	/// The server has tmux (F49): a persistent tab is only tmux-backed if so.
	pub async fn has_tmux(&self, server_id: &str) -> bool {
		self.with(server_id, |conn, _| async move {
			exec_output(&conn, "command -v tmux >/dev/null 2>&1").await.map(|_| ())
		})
		.await
		.is_ok()
	}

	/// Replace `old` with `new` in a file this session read and that has not
	/// changed since; `old` must occur exactly once unless `all`.
	pub fn agent_edit(
		&self,
		session: &str,
		server_id: &str,
		path: &str,
		old: &str,
		new: &str,
		all: bool,
	) -> AppResult<usize> {
		if old.is_empty() || old == new {
			return Err(AppError::InvalidInput(
				"old_string must be non-empty and differ from new_string".into(),
			));
		}
		let key = (session.to_string(), server_id.to_string(), path.to_string());
		let read = self.reads.get(&key).map(|r| *r).ok_or_else(|| {
			AppError::InvalidInput(format!("read {path} with file_read before editing it"))
		})?;
		let (count, st) =
			tauri::async_runtime::block_on(self.with(server_id, |conn, s| async move {
				if stamp(&s, path).await? != read {
					return Err(AppError::InvalidInput(format!(
						"{path} changed since you read it (someone else edited it); read it again"
					)));
				}
				let text = String::from_utf8(s.read(path).await.map_err(sftp_err)?)
					.map_err(|_| AppError::InvalidInput(format!("{path} is not text")))?;
				let count = text.matches(old).count();
				match (count, all) {
					(0, _) => {
						return Err(AppError::InvalidInput(format!(
							"old_string was not found in {path}"
						)))
					}
					(n, false) if n > 1 => {
						return Err(AppError::InvalidInput(format!(
						"old_string occurs {n} times in {path}; give more context or set replace_all"
					)))
					}
					_ => {}
				}
				let edited = if all { text.replace(old, new) } else { text.replacen(old, new, 1) };
				write_atomic(&conn, &s, path, edited.as_bytes()).await?;
				Ok((count, stamp(&s, path).await?))
			}))?;
		self.reads.insert(key, st);
		Ok(count)
	}
}

/// `alternate_on current_command`, as tmux prints it, judged.
fn tmux_verdict(line: &str) -> Result<(), String> {
	let mut parts = line.split_whitespace();
	let alternate = parts.next() == Some("1");
	let command = parts.next().unwrap_or_default();
	if alternate {
		return Err(format!("in a full-screen program ({command})"));
	}
	if !matches!(
		command,
		"bash" | "zsh" | "sh" | "dash" | "ksh" | "ash" | "fish" | "-bash" | "-zsh"
	) {
		return Err(format!("still running `{command}`"));
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_tmux_pane_is_at_rest_only_at_a_shell_prompt() {
		assert!(tmux_verdict("0 bash\n").is_ok());
		assert!(tmux_verdict("1 vim\n").unwrap_err().contains("full-screen"));
		assert!(tmux_verdict("0 sleep\n").unwrap_err().contains("sleep"));
	}
	use crate::services::ssh::{connect, test_sshd, Auth, KnownHosts, SshError};

	#[test]
	fn list_write_and_conflicts_against_a_real_sshd() {
		let d = test_sshd::start();
		let known = KnownHosts { read: vec![], ours: d.dir.path().join("known_hosts") };
		let target = Target {
			host: "127.0.0.1".into(),
			port: d.port,
			user: d.user.clone(),
			auth: Auth::KeyFile { path: d.key.clone(), passphrase: None },
		};
		// Trust the key once, the way the human does.
		tauri::async_runtime::block_on(async {
			if let Err(SshError::UnknownHost { fingerprint }) = connect(&target, &known, None).await
			{
				connect(&target, &known, Some(fingerprint)).await.expect("trust");
			}
		});
		let pool = Arc::new(Pool::new(known));
		let (port, user, key) = (d.port, d.user.clone(), d.key.clone());
		let files = RemoteFiles::new(
			pool,
			Arc::new(move |_| {
				Ok(Target {
					host: "127.0.0.1".into(),
					port,
					user: user.clone(),
					auth: Auth::KeyFile { path: key.clone(), passphrase: None },
				})
			}),
		);
		let dir = d.dir.path().join("work");
		std::fs::create_dir_all(&dir).expect("dir");
		let file = dir.join("app.conf");
		std::fs::write(&file, "port=80\n").expect("seed");
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).expect("mode");
		}
		let path = file.to_string_lossy().to_string();

		let listing = tauri::async_runtime::block_on(files.with("s", |_, s| {
			let p = dir.to_string_lossy().to_string();
			async move { list(&s, &p).await }
		}))
		.expect("list");
		assert_eq!(listing.entries.len(), 1);
		assert_eq!(listing.entries[0].name, "app.conf");

		assert!(files.agent_write("a", "s", &path, "x").is_err(), "not read yet");
		assert_eq!(files.agent_read("a", "s", &path).expect("read"), "port=80\n");
		files.agent_write("a", "s", &path, "port=8080\n").expect("write after read");
		assert_eq!(std::fs::read_to_string(&file).expect("disk"), "port=8080\n");
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			let mode = std::fs::metadata(&file).expect("meta").permissions().mode() & 0o777;
			assert_eq!(mode, 0o640, "the file's mode survives the atomic write");
		}
		// A human edits the file behind the agent's back.
		std::thread::sleep(std::time::Duration::from_millis(1100));
		std::fs::write(&file, "port=9090 # human\n").expect("human");
		let err = files.agent_write("a", "s", &path, "port=1\n").expect_err("conflict");
		assert!(err.to_string().contains("changed since you read it"), "{err}");
		assert_eq!(std::fs::read_to_string(&file).expect("disk"), "port=9090 # human\n");
		// An edit is held to the same rule, and replaces exactly what it names.
		assert_eq!(files.agent_read("a", "s", &path).expect("re-read"), "port=9090 # human\n");
		files.agent_edit("a", "s", &path, "9090", "9191", false).expect("edit");
		assert_eq!(std::fs::read_to_string(&file).expect("disk"), "port=9191 # human\n");
		assert!(files.agent_edit("a", "s", &path, "nothing-here", "x", false).is_err());
		// A new file needs no read.
		let new = dir.join("new.txt").to_string_lossy().to_string();
		files.agent_write("a", "s", &new, "hello\n").expect("new file");
		assert_eq!(std::fs::read_to_string(&new).expect("disk"), "hello\n");
	}
}
