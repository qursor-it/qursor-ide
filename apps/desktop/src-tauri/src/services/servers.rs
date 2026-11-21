//! The saved-server address book (F45): rows in SQLite, secrets in the vault,
//! and servers kept as `.server` files in the sidebar's projects.
//!
//! Validation lives here rather than in the command so every writer — the form,
//! an import — goes through the same checks.
//!
//! Project files: the scan never writes; only an explicit action does. A write
//! to an existing file is conditional on the sha256 last read (`Conflict`
//! otherwise), a new file never replaces one, and a move writes the new home
//! before removing the old one.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use parking_lot::{Mutex, MutexGuard};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};

use crate::error::{AppError, AppResult};
use crate::models::{
	ProblemKind, Server, ServerFileProblem, ServerFolder, ServerInput, ServerSource, SidebarRow,
	SshConfigHost, Tunnel,
};
use crate::services::intelligence::sha256_hex;
use crate::services::server_files::{self as files, FileFolder, FileServer, ScanProject};
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

/// The last scan of the projects' `.server` files.
#[derive(Default)]
struct Index {
	/// The scanned projects, in sidebar order.
	projects: Vec<ScanProject>,
	servers: Vec<FileServer>,
	folders: Vec<FileFolder>,
	problems: Vec<ServerFileProblem>,
	/// Servers valid in an earlier scan and gone since, so tabs still open on
	/// them resolve. Never listed.
	/// shortcut: retired entries live until app restart instead of until their
	/// last tab/tunnel ends; upgrade if a stale entry ever matters.
	retired: HashMap<String, FileServer>,
}

impl Index {
	fn find(&self, id: &str) -> Option<&FileServer> {
		self.servers.iter().find(|f| f.server.id == id)
	}
	fn root(&self, project_id: &str) -> Option<PathBuf> {
		self.projects.iter().find(|p| p.id == project_id).map(|p| p.root.clone())
	}
}

static INDEX: LazyLock<Mutex<Index>> = LazyLock::new(Default::default);

/// Scan now and hand back the index, still locked.
fn fresh(conn: &Connection) -> AppResult<MutexGuard<'static, Index>> {
	let projects: Vec<ScanProject> = crate::commands::sidebar::list_sidebar_in(conn)?
		.into_iter()
		.flat_map(|r| match r {
			SidebarRow::Project { project, .. } => vec![project],
			SidebarRow::Group { children, .. } => children.into_iter().map(|c| c.project).collect(),
		})
		.filter(|p| !p.real_path.is_empty() && !p.missing)
		.map(|p| ScanProject { id: p.id, root: PathBuf::from(p.real_path) })
		.collect();
	let db_ids: HashSet<String> = conn
		.prepare("SELECT id FROM servers")?
		.query_map([], |r| r.get(0))?
		.collect::<Result<_, _>>()?;
	let mut idx = INDEX.lock();
	// A retired id still belongs to its last file, should it come back.
	let prior = idx
		.retired
		.values()
		.chain(&idx.servers)
		.map(|f| (f.server.id.clone(), f.abs_path.clone()))
		.collect();
	let mut scan = files::scan(&projects, &prior, &db_ids);
	let rank = |p: &str| projects.iter().position(|x| x.id == p);
	scan.servers.sort_by_cached_key(|f| (rank(&f.project_id), f.server.name.to_lowercase()));
	let live: HashSet<String> = scan.servers.iter().map(|f| f.server.id.clone()).collect();
	for f in std::mem::take(&mut idx.servers) {
		if !live.contains(&f.server.id) {
			idx.retired.insert(f.server.id.clone(), f);
		}
	}
	idx.retired.retain(|id, _| !live.contains(id) && !db_ids.contains(id));
	idx.servers = scan.servers;
	idx.folders = scan.folders;
	idx.problems = scan.problems;
	idx.projects = projects;
	Ok(idx)
}

/// Scan the sidebar's projects for `.server` files. Reads only.
pub fn rescan(conn: &Connection) -> AppResult<()> {
	fresh(conn).map(drop)
}

/// A file server by id: the index, else after one rescan; `retired` also
/// answers with a server that has since vanished.
fn file_server(conn: &Connection, id: &str, retired: bool) -> AppResult<Option<FileServer>> {
	if let Some(f) = INDEX.lock().find(id) {
		return Ok(Some(f.clone()));
	}
	let idx = fresh(conn)?;
	Ok(idx.find(id).or(if retired { idx.retired.get(id) } else { None }).cloned())
}

fn project_root(conn: &Connection, project_id: &str) -> AppResult<PathBuf> {
	if let Some(r) = INDEX.lock().root(project_id) {
		return Ok(r);
	}
	fresh(conn)?
		.root(project_id)
		.ok_or_else(|| AppError::NotFound(format!("project {project_id} has no folder on disk")))
}

/// The files that are not listed as servers, from the last scan.
pub fn problems() -> Vec<ServerFileProblem> {
	INDEX.lock().problems.clone()
}

/// This computer's servers, then the project files in sidebar order, by name.
pub fn list(conn: &Connection) -> AppResult<Vec<Server>> {
	let mut st = conn.prepare(&format!(
		"SELECT {COLUMNS} FROM servers ORDER BY sort_order, name COLLATE NOCASE"
	))?;
	let mut rows = st.query_map([], row_to_server)?.collect::<Result<Vec<_>, _>>()?;
	rows.extend(fresh(conn)?.servers.iter().map(|f| f.server.clone()));
	Ok(rows)
}

fn get_row(conn: &Connection, id: &str) -> AppResult<Option<Server>> {
	Ok(conn
		.query_row(&format!("SELECT {COLUMNS} FROM servers WHERE id = ?1"), [id], row_to_server)
		.optional()?)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Server> {
	if let Some(s) = get_row(conn, id)? {
		return Ok(s);
	}
	file_server(conn, id, true)?
		.map(|f| f.server)
		.ok_or_else(|| AppError::NotFound(format!("server {id}")))
}

/// This computer's folders, then the projects' from the last scan (`list`
/// rescans).
pub fn list_folders(conn: &Connection) -> AppResult<Vec<ServerFolder>> {
	let mut st = conn
		.prepare("SELECT id, name FROM server_folders ORDER BY sort_order, name COLLATE NOCASE")?;
	let mut rows = st
		.query_map([], |r| Ok(ServerFolder { id: r.get(0)?, name: r.get(1)?, project_id: None }))?
		.collect::<Result<Vec<_>, _>>()?;
	rows.extend(INDEX.lock().folders.iter().map(|f| ServerFolder {
		id: f.id.clone(),
		name: f.name.clone(),
		project_id: Some(f.project_id.clone()),
	}));
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
	if host.is_empty()
		|| host.starts_with('-')
		|| host.chars().any(|c| c.is_whitespace() || c.is_control() || c == '@' || c == '/')
	{
		return bad("the host is a name or an address, without user@ or a path");
	}
	let user = input.username.trim();
	if user.is_empty() {
		return bad("a server needs a user");
	}
	// A leading '-' would reach the system ssh as an option (`-oProxyCommand=…`).
	if user.starts_with('-') || user.chars().any(|c| c.is_whitespace() || c.is_control()) {
		return bad("the user cannot start with '-' or contain spaces");
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

/// The server the form describes, trimmed as it is stored.
fn from_input(id: &str, input: &ServerInput) -> Server {
	Server {
		id: id.to_string(),
		name: input.name.trim().to_string(),
		host: input.host.trim().to_string(),
		port: input.port,
		username: input.username.trim().to_string(),
		start_dir: trimmed(&input.start_dir),
		folder_id: trimmed(&input.folder_id),
		environment: input.environment.clone(),
		agent_policy: input.agent_policy.clone(),
		auth: input.auth.clone(),
		key_path: if input.auth == "keyFile" { trimmed(&input.key_path) } else { None },
		persistent: input.persistent,
		has_password: false,
		has_private_key: false,
		has_passphrase: false,
		connected: false,
		source: ServerSource::Local,
	}
}

fn insert_row(conn: &Connection, s: &Server, now_ms: i64) -> AppResult<()> {
	conn.execute(
		"INSERT INTO servers (id, name, host, port, username, start_dir, folder_id, environment,
		 agent_policy, auth, key_path, created_at, persistent) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
		params![
			s.id,
			s.name,
			s.host,
			s.port,
			s.username,
			s.start_dir,
			s.folder_id,
			s.environment,
			s.agent_policy,
			s.auth,
			s.key_path,
			now_ms,
			s.persistent
		],
	)?;
	Ok(())
}

/// `rel` of a `p:<project>:<rel>` folder id, `""` for the top of `servers/`.
fn folder_rel(folder_id: &Option<String>) -> &str {
	folder_id.as_deref().and_then(files::parse_folder_id).map_or("", |(_, d)| d)
}

/// The disk still holds what the index last read.
fn check(f: &FileServer) -> AppResult<()> {
	match fs::read(&f.abs_path) {
		Ok(b) if sha256_hex(&b) == f.sha256 => Ok(()),
		_ => {
			Err(AppError::Conflict(format!("{} changed on disk; reload and try again", f.rel_path)))
		}
	}
}

fn write_file(path: &Path, text: &str) -> AppResult<()> {
	crate::services::files::write_file(&path.to_string_lossy(), text).map(drop)
}

/// Write a file that must not exist yet.
fn write_new(path: &Path, bytes: &[u8]) -> AppResult<()> {
	if let Some(d) = path.parent() {
		fs::create_dir_all(d)?;
	}
	fs::OpenOptions::new().write(true).create_new(true).open(path)?.write_all(bytes)?;
	Ok(())
}

/// Rewrite an existing file, conditional on what the index last read.
fn write_back(f: &FileServer, s: &Server, tunnels: &[Tunnel]) -> AppResult<()> {
	check(f)?;
	write_file(&f.abs_path, &files::to_json(&f.raw, s, tunnels))
}

/// A free path in `dir` for a moved file: its own name, `-2`, `-3`… only when
/// taken. `dir` comes from `project_dir`, which checked containment.
fn free_path(dir: &Path, file: &Path) -> AppResult<PathBuf> {
	let stem = file.file_stem().unwrap_or_default().to_string_lossy();
	(1..=1000)
		.map(|n| match n {
			1 => dir.join(format!("{stem}.server")),
			n => dir.join(format!("{stem}-{n}.server")),
		})
		.find(|p| !p.exists())
		.ok_or_else(|| AppError::InvalidInput(format!("no free file name for {stem}")))
}

/// A new file for `s` in `project_id`, in the folder it names.
fn create_file(
	conn: &Connection,
	project_id: &str,
	s: &Server,
	raw: &Map<String, Value>,
	tunnels: &[Tunnel],
) -> AppResult<()> {
	let root = project_root(conn, project_id)?;
	let path = files::new_file_path(&root, folder_rel(&s.folder_id), &s.name)?;
	write_new(&path, files::to_json(raw, s, tunnels).as_bytes())
}

/// Insert or update a server and write the secrets it was given.
pub fn save(
	conn: &Connection,
	vault: &Vault,
	input: &ServerInput,
	now_ms: i64,
) -> AppResult<Server> {
	validate(input)?;
	// Where it lives now: `None` new, `Some(None)` a row, `Some(Some(f))` a file.
	let current = match &input.id {
		None => None,
		Some(id) if get_row(conn, id)?.is_some() => Some(None),
		Some(id) => {
			let mut f = file_server(conn, id, false)?
				.ok_or_else(|| AppError::NotFound(format!("server {id}")))?;
			// The form's own read, not the poll's: a pull while it was open is a conflict.
			if let Some(base) = &input.base_sha {
				f.sha256 = base.clone();
			}
			Some(Some(f))
		}
	};
	// `None` keeps the current home, `Some("")` is this computer.
	let home = match input.home.as_deref().map(str::trim) {
		None => match &current {
			Some(Some(f)) => Some(f.project_id.clone()),
			_ => None,
		},
		Some("") => None,
		Some(p) => Some(p.to_string()),
	};
	let folder_home =
		trimmed(&input.folder_id).map(|f| files::parse_folder_id(&f).map(|(p, _)| p.to_string()));
	if folder_home.is_some_and(|fh| fh != home) {
		return Err(AppError::InvalidInput("the folder belongs to another place".into()));
	}
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
	let s = from_input(&id, input);
	match (current, home) {
		(None, None) => insert_row(conn, &s, now_ms)?,
		(Some(None), None) => {
			let n = conn.execute(
				"UPDATE servers SET name=?2, host=?3, port=?4, username=?5, start_dir=?6, folder_id=?7,
				 environment=?8, agent_policy=?9, auth=?10, key_path=?11, persistent=?12 WHERE id=?1",
				params![
					s.id,
					s.name,
					s.host,
					s.port,
					s.username,
					s.start_dir,
					s.folder_id,
					s.environment,
					s.agent_policy,
					s.auth,
					s.key_path,
					s.persistent
				],
			)?;
			if n == 0 {
				return Err(AppError::NotFound(format!("server {id}")));
			}
		}
		(None, Some(h)) => create_file(conn, &h, &s, &Map::new(), &[])?,
		(Some(Some(f)), Some(h)) if f.project_id == h => {
			let dir = folder_rel(&s.folder_id);
			let now = f.rel_path.strip_prefix("servers/").and_then(|r| r.rsplit_once('/'));
			if now.map_or("", |(d, _)| d) == dir {
				write_back(&f, &s, &f.tunnels)?;
			} else {
				// Another folder: the file moves, keeping its name where free.
				check(&f)?;
				let to = free_path(&project_dir(conn, &h, dir)?, &f.abs_path)?;
				write_new(&to, files::to_json(&f.raw, &s, &f.tunnels).as_bytes())?;
				fs::remove_file(&f.abs_path)?;
			}
		}
		(Some(None), Some(h)) => {
			let tunnels = list_tunnels(conn, &id)?;
			create_file(conn, &h, &s, &Map::new(), &tunnels)?;
			conn.execute("DELETE FROM server_tunnels WHERE server_id = ?1", [&id])?;
			conn.execute("DELETE FROM servers WHERE id = ?1", [&id])?;
		}
		(Some(Some(f)), None) => {
			check(&f)?;
			insert_row(conn, &s, now_ms)?;
			for t in &f.tunnels {
				// A file's `<server>/<tunnel>` id is not a row id.
				save_tunnel(conn, &Tunnel { id: String::new(), ..t.clone() })?;
			}
			fs::remove_file(&f.abs_path)?;
		}
		(Some(Some(f)), Some(h)) => {
			check(&f)?;
			let to = free_path(&project_dir(conn, &h, folder_rel(&s.folder_id))?, &f.abs_path)?;
			write_new(&to, files::to_json(&f.raw, &s, &f.tunnels).as_bytes())?;
			fs::remove_file(&f.abs_path)?;
		}
	}
	rescan(conn)?;
	Ok(with_secret_flags(get(conn, &id)?, vault))
}

pub fn delete(conn: &Connection, vault: &Vault, id: &str) -> AppResult<()> {
	let file = if get_row(conn, id)?.is_some() { None } else { file_server(conn, id, false)? };
	if let Some(f) = &file {
		check(f)?;
	}
	vault.remove_prefix(&secret_prefix(id))?;
	conn.execute("DELETE FROM servers WHERE id = ?1", [id])?;
	if let Some(f) = file {
		fs::remove_file(&f.abs_path)?;
		rescan(conn)?;
		// Gone on purpose: tabs, tunnels and the agent no longer resolve it.
		INDEX.lock().retired.remove(id);
	}
	Ok(())
}

fn folder_name(name: &str) -> AppResult<&str> {
	let name = name.trim();
	if name.is_empty() {
		return Err(AppError::InvalidInput("a folder needs a name".into()));
	}
	Ok(name)
}

/// `<root>/servers/<rel>` of a project, refused unless it stays inside
/// `servers/`. A project folder is one directory level: no `/`, no dot-name.
fn project_dir(conn: &Connection, project_id: &str, rel: &str) -> AppResult<PathBuf> {
	let root = project_root(conn, project_id)?;
	// `new_file_path` refuses `..`, absolute paths and symlinks out of servers/.
	let probe = files::new_file_path(&root, rel, "x")?;
	Ok(probe.parent().unwrap_or(&root).to_path_buf())
}

fn one_level(name: &str) -> AppResult<()> {
	if name.contains(['/', '\\']) || name.starts_with('.') {
		return Err(AppError::InvalidInput("a folder name has no '/' and no leading '.'".into()));
	}
	Ok(())
}

pub fn create_folder(
	conn: &Connection,
	name: &str,
	project_id: Option<&str>,
) -> AppResult<ServerFolder> {
	let name = folder_name(name)?;
	if let Some(p) = project_id {
		one_level(name)?;
		let dir = project_dir(conn, p, name)?;
		fs::create_dir_all(dir.parent().unwrap_or(&dir))?;
		fs::create_dir(&dir)?;
		rescan(conn)?;
		return Ok(ServerFolder {
			id: files::folder_id(p, name),
			name: name.to_string(),
			project_id: Some(p.to_string()),
		});
	}
	let id = uuid::Uuid::new_v4().to_string();
	conn.execute("INSERT INTO server_folders (id, name) VALUES (?1, ?2)", params![id, name])?;
	Ok(ServerFolder { id, name: name.to_string(), project_id: None })
}

pub fn rename_folder(conn: &Connection, id: &str, name: &str) -> AppResult<()> {
	let name = folder_name(name)?;
	if let Some((p, rel)) = files::parse_folder_id(id) {
		one_level(name)?;
		let to_rel =
			rel.rsplit_once('/').map_or(name.to_string(), |(up, _)| format!("{up}/{name}"));
		let from = project_dir(conn, p, rel)?;
		let to = project_dir(conn, p, &to_rel)?;
		if to.exists() {
			return Err(AppError::InvalidInput(format!("{name} already exists")));
		}
		fs::rename(from, to)?;
		return rescan(conn);
	}
	conn.execute("UPDATE server_folders SET name = ?2 WHERE id = ?1", params![id, name])?;
	Ok(())
}

/// Deleting a folder keeps its servers; they move to the top level. In a
/// project they move up one level, never over another file (`-2` instead).
pub fn delete_folder(conn: &Connection, id: &str) -> AppResult<()> {
	if let Some((p, rel)) = files::parse_folder_id(id) {
		let dir = project_dir(conn, p, rel)?;
		let up = project_dir(conn, p, rel.rsplit_once('/').map_or("", |(up, _)| up))?;
		for e in fs::read_dir(&dir)?.flatten() {
			let path = e.path();
			// `file_type` does not follow a symlink, which could copy e.g. a key into the repo.
			let file = e.file_type().is_ok_and(|t| t.is_file());
			if file && path.extension().is_some_and(|x| x == "server") {
				write_new(&free_path(&up, &path)?, &fs::read(&path)?)?;
				fs::remove_file(&path)?;
			}
		}
		// Only when empty: other files stay where the human put them.
		let _ = fs::remove_dir(&dir);
		return rescan(conn);
	}
	conn.execute("DELETE FROM server_folders WHERE id = ?1", [id])?;
	Ok(())
}

pub fn list_tunnels(conn: &Connection, server_id: &str) -> AppResult<Vec<Tunnel>> {
	if get_row(conn, server_id)?.is_none() {
		if let Some(f) = file_server(conn, server_id, true)? {
			return Ok(f.tunnels);
		}
	}
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
	if let Some((server_id, _)) = files::split_tunnel_id(id) {
		return file_server(conn, server_id, true)?
			.and_then(|f| f.tunnels.into_iter().find(|t| t.id == id))
			.ok_or_else(|| AppError::NotFound(format!("tunnel {id}")));
	}
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

/// The file server a tunnel belongs to, or `None` for a row.
fn tunnel_file(conn: &Connection, t: &Tunnel) -> AppResult<Option<FileServer>> {
	let server_id = match files::split_tunnel_id(&t.id) {
		Some((s, _)) => s,
		None if t.id.is_empty() && get_row(conn, &t.server_id)?.is_none() => &t.server_id,
		None => return Ok(None),
	};
	file_server(conn, server_id, false)?
		.map(Some)
		.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))
}

pub fn save_tunnel(conn: &Connection, t: &Tunnel) -> AppResult<Tunnel> {
	validate_tunnel(t)?;
	let host = t.dest_host.trim();
	if let Some(f) = tunnel_file(conn, t)? {
		let sid = f.server.id.clone();
		let id = if t.id.is_empty() {
			files::tunnel_app_id(&sid, &uuid::Uuid::new_v4().to_string())
		} else {
			t.id.clone()
		};
		let new = Tunnel {
			id: id.clone(),
			server_id: sid,
			label: t.label.trim().to_string(),
			dest_host: host.to_string(),
			running: false,
			..t.clone()
		};
		let mut tunnels = f.tunnels.clone();
		match tunnels.iter_mut().find(|x| x.id == id) {
			Some(x) => *x = new,
			None => tunnels.push(new),
		}
		write_back(&f, &f.server, &tunnels)?;
		rescan(conn)?;
		return get_tunnel(conn, &id);
	}
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
	if let Some((server_id, _)) = files::split_tunnel_id(id) {
		let f = file_server(conn, server_id, false)?
			.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
		let tunnels: Vec<Tunnel> = f.tunnels.iter().filter(|t| t.id != id).cloned().collect();
		write_back(&f, &f.server, &tunnels)?;
		return rescan(conn);
	}
	conn.execute("DELETE FROM server_tunnels WHERE id = ?1", [id])?;
	Ok(())
}

/// Give a duplicate or id-less file a fresh id: the one write the scan leaves
/// to the human. Conditional on the bytes just read.
pub fn fix_file(conn: &Connection, project_id: &str, rel_path: &str) -> AppResult<()> {
	let idx = fresh(conn)?;
	let fixable = idx.problems.iter().any(|p| {
		p.project_id == project_id
			&& p.rel_path == rel_path
			&& matches!(p.kind, ProblemKind::Duplicate | ProblemKind::NoId)
	});
	let root = idx.root(project_id);
	drop(idx);
	let (true, Some(root)) = (fixable, root) else {
		return Err(AppError::InvalidInput(format!(
			"{rel_path} is not a duplicate or a file without an id"
		)));
	};
	// `rel_path` is one the scan listed, so it is inside servers/.
	let path = root.join(rel_path);
	let bytes = fs::read(&path)?;
	let mut raw: Map<String, Value> = serde_json::from_slice(&bytes)?;
	raw.insert("id".into(), Value::String(uuid::Uuid::new_v4().to_string()));
	let text = format!("{}\n", serde_json::to_string_pretty(&raw)?);
	if fs::read(&path).map(|b| b != bytes).unwrap_or(true) {
		return Err(AppError::Conflict(format!(
			"{rel_path} changed on disk; reload and try again"
		)));
	}
	write_file(&path, &text)?;
	rescan(conn)
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
pub fn expand_home(p: &str) -> PathBuf {
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
			home: None,
			base_sha: None,
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

	// ── Project files ──

	use crate::db::Db;
	use crate::models::ProblemKind;
	use crate::services::vault::KeyStore;
	use std::path::PathBuf;

	struct MemStore(parking_lot::Mutex<Option<Vec<u8>>>);
	impl KeyStore for MemStore {
		fn load(&self) -> Result<Option<Vec<u8>>, String> {
			Ok(self.0.lock().clone())
		}
		fn save(&self, key: &[u8]) -> Result<(), String> {
			*self.0.lock() = Some(key.to_vec());
			Ok(())
		}
		fn delete(&self) -> Result<(), String> {
			*self.0.lock() = None;
			Ok(())
		}
	}

	/// The index is process-wide: tests that scan take turns.
	static SERIAL: parking_lot::Mutex<()> = parking_lot::const_mutex(());

	struct Env {
		db: Db,
		vault: Vault,
		p1: String,
		root: PathBuf,
		_tmp: tempfile::TempDir,
		_turn: parking_lot::MutexGuard<'static, ()>,
	}

	impl Env {
		fn put(&self, rel: &str, body: &str) {
			let p = self.root.join(rel);
			std::fs::create_dir_all(p.parent().unwrap()).unwrap();
			std::fs::write(p, body).unwrap();
		}
		fn read(&self, rel: &str) -> String {
			std::fs::read_to_string(self.root.join(rel)).unwrap()
		}
		fn list(&self) -> Vec<Server> {
			self.db.with(list).unwrap()
		}
		fn save(&self, input: &ServerInput) -> AppResult<Server> {
			self.db.with(|c| save(c, &self.vault, input, 1))
		}
	}

	fn env() -> Env {
		let turn = SERIAL.lock();
		let tmp = tempfile::tempdir().unwrap();
		let db = Db::open(&tmp.path().join("data")).unwrap();
		let dir = tmp.path().join("proj");
		std::fs::create_dir_all(&dir).unwrap();
		let p = crate::commands::projects::add_project_in(&db, dir.to_str().unwrap()).unwrap();
		let vault = Vault::open(
			tmp.path().join("v.vault"),
			Box::new(MemStore(parking_lot::Mutex::new(None))),
		)
		.unwrap();
		Env { db, vault, p1: p.id, root: PathBuf::from(p.real_path), _tmp: tmp, _turn: turn }
	}

	fn body(id: &str, name: &str) -> String {
		format!(r#"{{"id":"{id}","name":"{name}","host":"h","user":"u","login":"agent"}}"#)
	}

	fn with_tunnel(id: &str, name: &str) -> String {
		format!(
			r#"{{"id":"{id}","name":"{name}","host":"h","user":"u","login":"agent","tunnels":[{{"id":"t1","localPort":5433,"destHost":"localhost","destPort":5432}}]}}"#
		)
	}

	fn input(id: Option<&str>, name: &str, home: Option<&str>) -> ServerInput {
		ServerInput {
			id: id.map(str::to_string),
			name: name.into(),
			host: "h".into(),
			port: 22,
			username: "u".into(),
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
			home: home.map(str::to_string),
			base_sha: None,
		}
	}

	/// The source without its sha, which the tests do not spell out.
	fn project_source(p: &str, rel: &str) -> (String, String) {
		(p.into(), rel.into())
	}

	fn place(s: &ServerSource) -> (String, String) {
		match s {
			ServerSource::Project { project_id, rel_path, .. } => {
				(project_id.clone(), rel_path.clone())
			}
			ServerSource::Local => Default::default(),
		}
	}

	#[test]
	fn file_server_is_listed_and_got() {
		let e = env();
		e.put("servers/web.server", &body("s1", "web"));
		let l = e.list();
		assert_eq!(l.len(), 1);
		assert_eq!(place(&l[0].source), project_source(&e.p1, "servers/web.server"));
		let g = e.db.with(|c| get(c, "s1")).unwrap();
		assert_eq!(place(&g.source), project_source(&e.p1, "servers/web.server"));
	}

	#[test]
	fn save_rewrites_the_file_keeping_unknown_fields() {
		let e = env();
		e.put("servers/web.server", &body("s1", "web").replacen('{', r#"{"x-note":"keep","#, 1));
		e.list();
		let saved = e.save(&input(Some("s1"), "web2", Some(&e.p1))).unwrap();
		assert_eq!(saved.name, "web2");
		let text = e.read("servers/web.server");
		assert!(text.contains("\"web2\"") && text.contains("\"x-note\": \"keep\""), "{text}");
	}

	#[test]
	fn stale_write_is_refused() {
		let e = env();
		e.put("servers/web.server", &body("s1", "web"));
		e.list();
		let changed = body("s1", "pulled");
		e.put("servers/web.server", &changed);
		let r = e.save(&input(Some("s1"), "mine", Some(&e.p1)));
		assert!(matches!(r, Err(AppError::Conflict(_))), "{r:?}");
		assert_eq!(e.read("servers/web.server"), changed);
	}

	#[test]
	fn create_in_project_writes_a_new_file() {
		let e = env();
		let old = body("s1", "web");
		e.put("servers/web.server", &old);
		e.list();
		let saved = e.save(&input(None, "Web", Some(&e.p1))).unwrap();
		assert_eq!(place(&saved.source), project_source(&e.p1, "servers/web-2.server"));
		assert_eq!(e.read("servers/web.server"), old);
		assert!(e.read("servers/web-2.server").contains(&saved.id));
		assert_eq!(e.list().len(), 2);
	}

	#[test]
	fn move_local_to_project_keeps_id_and_tunnels() {
		let e = env();
		let mut i = input(None, "web", None);
		i.password = Some("pw".into());
		let s = e.save(&i).unwrap();
		let t = Tunnel {
			id: String::new(),
			server_id: s.id.clone(),
			label: "db".into(),
			local_port: 5433,
			dest_host: "localhost".into(),
			dest_port: 5432,
			running: false,
		};
		e.db.with(|c| save_tunnel(c, &t)).unwrap();
		let moved = e.save(&input(Some(&s.id), "web", Some(&e.p1))).unwrap();
		assert_eq!(moved.id, s.id);
		assert_eq!(place(&moved.source), project_source(&e.p1, "servers/web.server"));
		let rows: i64 =
			e.db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM servers", [], |r| r.get(0))?))
				.unwrap();
		assert_eq!(rows, 0);
		let text = e.read("servers/web.server");
		assert!(text.contains(&s.id) && text.contains("5433"), "{text}");
		assert!(e.vault.has(&secret_id(&s.id, "password")));
		let ts = e.db.with(|c| list_tunnels(c, &s.id)).unwrap();
		assert_eq!(ts.len(), 1);
		assert!(ts[0].id.starts_with(&format!("{}/", s.id)));
	}

	#[test]
	fn move_project_to_local() {
		let e = env();
		e.put("servers/web.server", &with_tunnel("s1", "web"));
		e.list();
		let moved = e.save(&input(Some("s1"), "web", Some(""))).unwrap();
		assert_eq!(moved.source, ServerSource::Local);
		assert!(!e.root.join("servers/web.server").exists());
		let l = e.list();
		assert_eq!(l.len(), 1);
		assert_eq!((l[0].id.as_str(), &l[0].source), ("s1", &ServerSource::Local));
		let ts = e.db.with(|c| list_tunnels(c, "s1")).unwrap();
		assert_eq!(ts.len(), 1);
		assert_eq!(ts[0].local_port, 5433);
		assert!(!ts[0].id.contains('/'));
	}

	#[test]
	fn delete_removes_the_file() {
		let e = env();
		e.put("servers/web.server", &body("s1", "web"));
		e.list();
		e.db.with(|c| delete(c, &e.vault, "s1")).unwrap();
		assert!(!e.root.join("servers/web.server").exists());
		assert!(e.list().is_empty());
	}

	#[test]
	fn deleted_server_no_longer_resolves() {
		let e = env();
		e.put("servers/web.server", &body("s1", "web"));
		e.list();
		e.db.with(|c| delete(c, &e.vault, "s1")).unwrap();
		assert!(matches!(e.db.with(|c| get(c, "s1")), Err(AppError::NotFound(_))));
	}

	#[test]
	fn stale_base_sha_is_refused_after_a_rescan() {
		let e = env();
		e.put("servers/web.server", &body("s1", "web"));
		let ServerSource::Project { sha256: base, .. } = e.list().remove(0).source else {
			panic!("a project server")
		};
		let changed = body("s1", "pulled");
		e.put("servers/web.server", &changed);
		e.list();
		let mut i = input(Some("s1"), "mine", None);
		i.base_sha = Some(base);
		assert!(matches!(e.save(&i), Err(AppError::Conflict(_))));
		assert_eq!(e.read("servers/web.server"), changed);
	}

	#[test]
	fn a_returning_owner_keeps_its_id_over_a_new_duplicate() {
		let e = env();
		e.put("servers/z.server", &body("s1", "owner"));
		e.list();
		std::fs::remove_file(e.root.join("servers/z.server")).unwrap();
		e.list();
		e.put("servers/z.server", &body("s1", "owner"));
		e.put("servers/a.server", &body("s1", "newcomer"));
		let l = e.list();
		assert_eq!(l.len(), 1);
		assert_eq!(l[0].name, "owner");
	}

	#[cfg(unix)]
	#[test]
	fn deleting_a_folder_skips_symlinks() {
		let e = env();
		let outside = e._tmp.path().join("secret.server");
		std::fs::write(&outside, "secret").unwrap();
		e.put("servers/a/web.server", &body("s1", "web"));
		std::os::unix::fs::symlink(&outside, e.root.join("servers/a/link.server")).unwrap();
		e.list();
		e.db.with(|c| delete_folder(c, &format!("p:{}:a", e.p1))).unwrap();
		assert!(!e.root.join("servers/link.server").exists());
		assert!(e.root.join("servers/web.server").exists());
	}

	#[test]
	fn deleting_a_folder_never_overwrites() {
		let e = env();
		let (inner, outer) = (body("s1", "web"), body("s2", "web"));
		e.put("servers/a/My Web.server", &inner);
		e.put("servers/My Web.server", &outer);
		e.list();
		e.db.with(|c| delete_folder(c, &format!("p:{}:a", e.p1))).unwrap();
		assert_eq!(e.read("servers/My Web.server"), outer);
		assert_eq!(e.read("servers/My Web-2.server"), inner);
		assert!(!e.root.join("servers/a").exists());
	}

	#[test]
	fn project_folders_are_directories() {
		let e = env();
		let f = e.db.with(|c| create_folder(c, "clienti", Some(&e.p1))).unwrap();
		assert_eq!(f.project_id.as_deref(), Some(e.p1.as_str()));
		assert!(e.root.join("servers/clienti").is_dir());
		e.list();
		let fs = e.db.with(list_folders).unwrap();
		assert!(fs.iter().any(|x| x.id == f.id), "{fs:?}");
		e.db.with(|c| rename_folder(c, &f.id, "acme")).unwrap();
		assert!(e.root.join("servers/acme").is_dir());
		assert!(e.db.with(|c| create_folder(c, "../x", Some(&e.p1))).is_err());
	}

	#[test]
	fn tunnel_ids_are_scoped_by_server() {
		let e = env();
		e.put("servers/a.server", &with_tunnel("s1", "a"));
		e.put("servers/b.server", &with_tunnel("s2", "b"));
		e.list();
		let t1 = e.db.with(|c| list_tunnels(c, "s1")).unwrap();
		let t2 = e.db.with(|c| list_tunnels(c, "s2")).unwrap();
		assert_eq!(t1[0].id, "s1/t1");
		assert_eq!(t2[0].id, "s2/t1");
		assert_eq!(e.db.with(|c| get_tunnel(c, "s2/t1")).unwrap().server_id, "s2");
		let mut edit = t2[0].clone();
		edit.local_port = 6000;
		e.db.with(|c| save_tunnel(c, &edit)).unwrap();
		assert!(e.read("servers/b.server").contains("6000"));
		e.db.with(|c| delete_tunnel(c, "s2/t1")).unwrap();
		assert!(e.db.with(|c| list_tunnels(c, "s2")).unwrap().is_empty());
	}

	#[test]
	fn vanished_server_still_resolves() {
		let e = env();
		e.put("servers/web.server", &body("s1", "web"));
		e.list();
		std::fs::remove_file(e.root.join("servers/web.server")).unwrap();
		assert!(e.list().is_empty());
		assert_eq!(e.db.with(|c| get(c, "s1")).unwrap().name, "web");
	}

	#[test]
	fn fix_file_gives_a_duplicate_its_own_id() {
		let e = env();
		e.put("servers/a.server", &body("s1", "a"));
		e.put("servers/b.server", &body("s1", "b"));
		assert_eq!(e.list().len(), 1);
		let p = problems();
		assert_eq!(p.len(), 1);
		assert_eq!(p[0].kind, ProblemKind::Duplicate);
		e.db.with(|c| fix_file(c, &e.p1, &p[0].rel_path)).unwrap();
		assert_eq!(e.list().len(), 2);
		assert!(problems().is_empty());
	}

	#[test]
	fn scan_never_writes() {
		let e = env();
		e.put("servers/a.server", &body("s1", "a"));
		e.put("servers/b.server", &body("s1", "b"));
		e.put("servers/c.server", r#"{"name":"n","host":"h","user":"u","login":"agent"}"#);
		e.put("servers/d.server", "not json");
		let snap = || {
			let mut v: Vec<_> = std::fs::read_dir(e.root.join("servers"))
				.unwrap()
				.flatten()
				.map(|f| {
					let m = f.metadata().unwrap().modified().unwrap();
					(f.path(), m, std::fs::read(f.path()).unwrap())
				})
				.collect();
			v.sort();
			v
		};
		let before = snap();
		e.list();
		e.list();
		assert_eq!(snap(), before);
	}

	#[test]
	fn edit_without_home_keeps_the_file() {
		let e = env();
		e.put("servers/web.server", &body("s1", "web"));
		e.list();
		let saved = e.save(&input(Some("s1"), "renamed", None)).unwrap();
		assert_eq!(place(&saved.source), project_source(&e.p1, "servers/web.server"));
		assert!(e.read("servers/web.server").contains("\"renamed\""));
		let rows: i64 =
			e.db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM servers", [], |r| r.get(0))?))
				.unwrap();
		assert_eq!(rows, 0);
	}

	#[test]
	fn folder_change_moves_the_file_keeping_its_name() {
		let e = env();
		e.put("servers/My Web.server", &body("s1", "web"));
		e.put("servers/a/My Web.server", &body("s2", "other"));
		e.list();
		let mut i = input(Some("s1"), "web", None);
		i.folder_id = Some(format!("p:{}:a", e.p1));
		let saved = e.save(&i).unwrap();
		assert_eq!(place(&saved.source), project_source(&e.p1, "servers/a/My Web-2.server"));
		assert!(!e.root.join("servers/My Web.server").exists());
		assert!(e.read("servers/a/My Web-2.server").contains("\"s1\""));
		assert!(e.read("servers/a/My Web.server").contains("\"s2\""));
	}

	#[test]
	fn move_between_projects_keeps_name_and_unknown_fields() {
		let e = env();
		let dir2 = e._tmp.path().join("proj2");
		std::fs::create_dir_all(&dir2).unwrap();
		let p2 = crate::commands::projects::add_project_in(&e.db, dir2.to_str().unwrap()).unwrap();
		let root2 = PathBuf::from(&p2.real_path);
		e.put(
			"servers/My Web.server",
			&with_tunnel("s1", "web").replacen('{', r#"{"x-note":"keep","#, 1),
		);
		e.list();
		let saved = e.save(&input(Some("s1"), "web", Some(&p2.id))).unwrap();
		assert_eq!(place(&saved.source), project_source(&p2.id, "servers/My Web.server"));
		assert!(!e.root.join("servers/My Web.server").exists());
		let text = std::fs::read_to_string(root2.join("servers/My Web.server")).unwrap();
		assert!(text.contains("\"x-note\": \"keep\"") && text.contains("5433"), "{text}");
		assert_eq!(e.db.with(|c| list_tunnels(c, "s1")).unwrap()[0].id, "s1/t1");
	}

	#[test]
	fn stale_file_refuses_delete_and_tunnel_writes() {
		let e = env();
		e.put("servers/web.server", &with_tunnel("s1", "web"));
		e.vault.put(&secret_id("s1", "password"), b"pw").unwrap();
		e.list();
		let tunnel = e.db.with(|c| list_tunnels(c, "s1")).unwrap().remove(0);
		let changed = with_tunnel("s1", "pulled");
		e.put("servers/web.server", &changed);
		let conflict = |r: AppResult<()>| matches!(r, Err(AppError::Conflict(_)));
		assert!(conflict(e.db.with(|c| delete(c, &e.vault, "s1"))));
		assert!(e.vault.has(&secret_id("s1", "password")), "secrets kept on a refused delete");
		assert!(conflict(e.db.with(|c| save_tunnel(c, &tunnel).map(drop))));
		assert!(conflict(e.db.with(|c| delete_tunnel(c, "s1/t1"))));
		assert_eq!(e.read("servers/web.server"), changed);
	}

	#[test]
	fn fix_file_gives_an_id_less_file_an_id() {
		let e = env();
		e.put("servers/c.server", r#"{"name":"n","host":"h","user":"u","login":"agent"}"#);
		assert!(e.list().is_empty());
		let p = problems();
		assert_eq!(p[0].kind, ProblemKind::NoId);
		e.db.with(|c| fix_file(c, &e.p1, "servers/c.server")).unwrap();
		assert_eq!(e.list().len(), 1);
		assert!(problems().is_empty());
	}
}
