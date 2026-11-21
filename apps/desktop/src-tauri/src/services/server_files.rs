//! Portable servers (F51): one JSON file per server in
//! `<project>/servers/**/<slug>.server`. This module only reads, scans and
//! formats; it never touches the database or rewrites an id.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::{AppError, AppResult};
use crate::models::{ProblemKind, Server, ServerFileProblem, ServerInput, ServerSource, Tunnel};
use crate::services::canvas::slugify;
use crate::services::ide::scope::resolve_within;
use crate::services::intelligence::sha256_hex;

const DIR: &str = "servers";
const EXT: &str = "server";

pub struct ScanProject {
	pub id: String,
	pub root: PathBuf,
}

pub struct FileServer {
	pub project_id: String,
	/// `servers/clienti/acme.server`, always with `/`.
	pub rel_path: String,
	/// Canonical path.
	pub abs_path: PathBuf,
	pub sha256: String,
	pub server: Server,
	pub tunnels: Vec<Tunnel>,
	pub raw: Map<String, Value>,
}

pub struct FileFolder {
	pub id: String,
	pub project_id: String,
	pub name: String,
}

pub struct Scan {
	pub servers: Vec<FileServer>,
	pub folders: Vec<FileFolder>,
	pub problems: Vec<ServerFileProblem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileDoc {
	id: Option<String>,
	name: String,
	host: String,
	user: String,
	login: String,
	#[serde(default = "default_port")]
	port: u16,
	start_dir: Option<String>,
	#[serde(default = "default_env")]
	environment: String,
	#[serde(default = "default_policy")]
	agent_policy: String,
	key_path: Option<String>,
	#[serde(default)]
	persistent: bool,
	#[serde(default)]
	tunnels: Vec<FileTunnel>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileTunnel {
	id: String,
	#[serde(default)]
	label: String,
	local_port: u16,
	dest_host: String,
	dest_port: u16,
}

fn default_port() -> u16 {
	22
}
fn default_env() -> String {
	"dev".into()
}
fn default_policy() -> String {
	"free".into()
}

pub fn tunnel_app_id(server_id: &str, file_tunnel_id: &str) -> String {
	format!("{server_id}/{file_tunnel_id}")
}

pub fn split_tunnel_id(id: &str) -> Option<(&str, &str)> {
	id.split_once('/')
}

pub fn folder_id(project_id: &str, rel_dir: &str) -> String {
	format!("p:{project_id}:{rel_dir}")
}

pub fn parse_folder_id(id: &str) -> Option<(&str, &str)> {
	id.strip_prefix("p:")?.split_once(':')
}

pub fn expand_home(p: &str) -> String {
	let rest = if p == "~" { Some("") } else { p.strip_prefix("~/") };
	match (rest, dirs::home_dir()) {
		(Some(rest), Some(h)) => h.join(rest).to_string_lossy().into_owned(),
		_ => p.to_string(),
	}
}

fn tilde(p: &str) -> String {
	let Some(h) = dirs::home_dir() else { return p.to_string() };
	match Path::new(p).strip_prefix(&h) {
		Ok(rest) => format!("~/{}", join_slash(rest)),
		Err(_) => p.to_string(),
	}
}

fn join_slash(p: &Path) -> String {
	let parts: Vec<_> = p.components().map(|c| c.as_os_str().to_string_lossy()).collect();
	parts.join("/")
}

struct Parsed {
	server: Server,
	tunnels: Vec<Tunnel>,
	raw: Map<String, Value>,
}

enum Bad {
	Broken(String),
	NoId,
}

fn parse(
	project_id: &str,
	rel_path: &str,
	folder: Option<String>,
	bytes: &[u8],
) -> Result<Parsed, Bad> {
	let broken = |e: &dyn std::fmt::Display| Bad::Broken(e.to_string());
	let raw: Map<String, Value> = serde_json::from_slice(bytes).map_err(|e| broken(&e))?;
	let doc: FileDoc =
		serde_json::from_value(Value::Object(raw.clone())).map_err(|e| broken(&e))?;
	let input = ServerInput {
		id: doc.id.clone(),
		name: doc.name,
		host: doc.host,
		port: doc.port,
		username: doc.user,
		start_dir: doc.start_dir,
		folder_id: folder,
		environment: doc.environment,
		agent_policy: doc.agent_policy,
		auth: doc.login,
		// Kept as written (`~/…`); the login site expands it.
		key_path: doc.key_path,
		persistent: doc.persistent,
		password: None,
		private_key: None,
		passphrase: None,
	};
	crate::services::servers::validate(&input).map_err(|e| match e {
		AppError::InvalidInput(m) => Bad::Broken(m),
		e => Bad::Broken(e.to_string()),
	})?;
	let id = match doc.id.as_deref().map(str::trim) {
		None | Some("") => return Err(Bad::NoId),
		Some(id) if id.contains('/') => {
			return Err(Bad::Broken("the id cannot contain '/'".into()))
		}
		Some(id) => id.to_string(),
	};
	let mut seen = HashSet::new();
	if let Some(t) = doc.tunnels.iter().find(|t| !seen.insert(t.id.as_str())) {
		return Err(Bad::Broken(format!("two tunnels share the id {}", t.id)));
	}
	let tunnels: Vec<Tunnel> = doc
		.tunnels
		.into_iter()
		.map(|t| Tunnel {
			id: tunnel_app_id(&id, &t.id),
			server_id: id.clone(),
			label: t.label,
			local_port: t.local_port,
			dest_host: t.dest_host,
			dest_port: t.dest_port,
			running: false,
		})
		.collect();
	for t in &tunnels {
		crate::services::servers::validate_tunnel(t).map_err(|e| broken(&e))?;
	}
	let server = Server {
		id,
		name: input.name,
		host: input.host,
		port: input.port,
		username: input.username,
		start_dir: input.start_dir,
		folder_id: input.folder_id,
		environment: input.environment,
		agent_policy: input.agent_policy,
		auth: input.auth,
		key_path: input.key_path,
		persistent: input.persistent,
		has_password: false,
		has_private_key: false,
		has_passphrase: false,
		connected: false,
		source: ServerSource::Project {
			project_id: project_id.to_string(),
			rel_path: rel_path.to_string(),
		},
	};
	Ok(Parsed { server, tunnels, raw })
}

/// `.server` files under `dir`, skipping dot-folders. Unreadable folders are
/// skipped: a half-visible tree must not take the whole scan down.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
	let Ok(rd) = fs::read_dir(dir) else { return };
	for e in rd.flatten() {
		let path = e.path();
		let dot = e.file_name().to_string_lossy().starts_with('.');
		if path.is_dir() {
			// A symlinked folder could lead out of the project.
			if !dot && !e.file_type().is_ok_and(|t| t.is_symlink()) {
				walk(&path, out);
			}
		} else if !dot && path.extension().is_some_and(|x| x == EXT) {
			out.push(path);
		}
	}
}

pub fn scan(
	projects: &[ScanProject],
	prior_owner: &HashMap<String, PathBuf>,
	db_ids: &HashSet<String>,
) -> Scan {
	let mut seen = HashSet::new();
	let mut found: Vec<FileServer> = Vec::new();
	let mut problems = Vec::new();
	for p in projects {
		let mut files = Vec::new();
		walk(&p.root.join(DIR), &mut files);
		let mut files: Vec<(String, PathBuf)> = files
			.into_iter()
			.filter_map(|f| Some((join_slash(f.strip_prefix(&p.root).ok()?), f)))
			.collect();
		files.sort();
		for (rel_path, file) in files {
			let Ok(canon) = dunce::canonicalize(&file) else { continue };
			if !seen.insert(canon.clone()) {
				continue;
			}
			let escapes = resolve_within(&p.root.join(DIR), &canon.to_string_lossy()).is_err();
			let problem = |kind, message: String| ServerFileProblem {
				project_id: p.id.clone(),
				rel_path: rel_path.clone(),
				kind,
				message,
			};
			if escapes {
				problems.push(problem(
					ProblemKind::Broken,
					"the file resolves outside the project's servers folder".into(),
				));
				continue;
			}
			let bytes = match fs::read(&canon) {
				Ok(b) => b,
				Err(e) => {
					problems.push(problem(ProblemKind::Broken, e.to_string()));
					continue;
				}
			};
			let folder = rel_path
				.strip_prefix("servers/")
				.and_then(|r| r.rsplit_once('/'))
				.map(|(d, _)| folder_id(&p.id, d));
			match parse(&p.id, &rel_path, folder, &bytes) {
				Ok(x) => found.push(FileServer {
					project_id: p.id.clone(),
					rel_path,
					abs_path: canon,
					sha256: sha256_hex(&bytes),
					server: x.server,
					tunnels: x.tunnels,
					raw: x.raw,
				}),
				Err(Bad::NoId) => problems.push(problem(
					ProblemKind::NoId,
					"the file has no id; add one to use it".into(),
				)),
				Err(Bad::Broken(m)) => problems.push(problem(ProblemKind::Broken, m)),
			}
		}
	}

	// One id is one server: the previous owner wins, else the first found.
	let mut winner: HashMap<String, usize> = HashMap::new();
	for (i, f) in found.iter().enumerate() {
		let prior = prior_owner.get(&f.server.id).is_some_and(|p| *p == f.abs_path);
		if prior || !winner.contains_key(&f.server.id) {
			winner.insert(f.server.id.clone(), i);
		}
	}
	let mut servers = Vec::new();
	let mut folders: Vec<FileFolder> = Vec::new();
	for (i, f) in found.into_iter().enumerate() {
		let in_db = db_ids.contains(&f.server.id);
		if in_db || winner[&f.server.id] != i {
			let message = if in_db {
				"the id is already used by a server in the local address book"
			} else {
				"another file already defines a server with this id"
			};
			problems.push(ServerFileProblem {
				project_id: f.project_id,
				rel_path: f.rel_path,
				kind: ProblemKind::Duplicate,
				message: message.into(),
			});
			continue;
		}
		if let Some(fid) = &f.server.folder_id {
			if !folders.iter().any(|x| &x.id == fid) {
				let name = parse_folder_id(fid).map(|(_, d)| d.to_string()).unwrap_or_default();
				folders.push(FileFolder {
					id: fid.clone(),
					project_id: f.project_id.clone(),
					name,
				});
			}
		}
		servers.push(f);
	}
	Scan { servers, folders, problems }
}

/// The file text for `s`: known fields over `raw`, unknown ones kept.
pub fn to_json(raw: &Map<String, Value>, s: &Server, tunnels: &[Tunnel]) -> String {
	let mut m = raw.clone();
	let mut set = |k: &str, v: Option<Value>| match v {
		Some(v) => {
			m.insert(k.into(), v);
		}
		None => {
			m.remove(k);
		}
	};
	set("id", Some(json!(s.id)));
	set("name", Some(json!(s.name)));
	set("host", Some(json!(s.host)));
	set("port", Some(json!(s.port)));
	set("user", Some(json!(s.username)));
	set("startDir", s.start_dir.as_ref().map(|d| json!(d)));
	set("environment", Some(json!(s.environment)));
	set("agentPolicy", Some(json!(s.agent_policy)));
	set("login", Some(json!(s.auth)));
	set("keyPath", s.key_path.as_deref().map(|k| json!(tilde(k))));
	set("persistent", Some(json!(s.persistent)));
	let old = raw.get("tunnels").and_then(Value::as_array);
	let ts = tunnels.iter().map(|t| {
		let local = split_tunnel_id(&t.id).map_or(t.id.as_str(), |(_, l)| l);
		let mut o = old
			.and_then(|a| a.iter().find(|x| x.get("id").and_then(Value::as_str) == Some(local)))
			.and_then(|x| x.as_object().cloned())
			.unwrap_or_default();
		for (k, v) in [
			("id", json!(local)),
			("label", json!(t.label)),
			("localPort", json!(t.local_port)),
			("destHost", json!(t.dest_host)),
			("destPort", json!(t.dest_port)),
		] {
			o.insert(k.into(), v);
		}
		Value::Object(o)
	});
	set("tunnels", Some(Value::Array(ts.collect())));
	// A map of plain values cannot fail to serialise.
	format!("{}\n", serde_json::to_string_pretty(&Value::Object(m)).unwrap_or_default())
}

/// A free `servers/<rel_dir>/<slug>.server` path; does not create it.
pub fn new_file_path(root: &Path, rel_dir: &str, name: &str) -> AppResult<PathBuf> {
	let rel = Path::new(rel_dir);
	if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
		return Err(AppError::InvalidInput("the folder must be inside servers/".into()));
	}
	let dir = root.join(DIR).join(rel);
	// Through a symlinked folder the new file would land outside the project.
	let mut probe = dir.as_path();
	while !probe.exists() {
		probe = probe.parent().unwrap_or(root);
	}
	resolve_within(&root.join(DIR), &probe.to_string_lossy())?;
	// `slugify` falls back to "canvas"; a server falls back to "server".
	let mut base = slugify(name);
	if base == "canvas" && !name.trim().eq_ignore_ascii_case("canvas") {
		base = "server".into();
	}
	for n in 1..=1000 {
		let slug = if n == 1 { base.clone() } else { format!("{base}-{n}") };
		let file = dir.join(format!("{slug}.{EXT}"));
		if !file.exists() {
			return Ok(file);
		}
	}
	Err(AppError::InvalidInput(format!("no free file name for {name}")))
}

#[cfg(test)]
mod tests {
	use super::*;

	const FULL: &str = r#"{
  "id": "8f3c2a1e",
  "name": "staging",
  "host": "staging.example.com",
  "port": 22,
  "user": "deploy",
  "startDir": "/srv/app",
  "environment": "staging",
  "agentPolicy": "confirm",
  "login": "keyFile",
  "keyPath": "~/.ssh/id_ed25519",
  "persistent": true,
  "x-note": "keep",
  "tunnels": [
    { "id": "c1d2", "label": "db", "localPort": 5433, "destHost": "localhost", "destPort": 5432 }
  ]
}"#;
	const MIN: &str = r#"{"id":"a","name":"n","host":"h","user":"u","login":"agent"}"#;

	fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
		let d = tempfile::tempdir().unwrap();
		for (rel, body) in files {
			let p = d.path().join(rel);
			fs::create_dir_all(p.parent().unwrap()).unwrap();
			fs::write(p, body).unwrap();
		}
		d
	}

	fn sp(id: &str, d: &tempfile::TempDir) -> ScanProject {
		ScanProject { id: id.into(), root: d.path().to_path_buf() }
	}

	fn run(projects: &[ScanProject]) -> Scan {
		scan(projects, &HashMap::new(), &HashSet::new())
	}

	#[test]
	fn parses_a_full_file() {
		let d = project(&[("servers/staging.server", FULL)]);
		let s = run(&[sp("P1", &d)]);
		let f = &s.servers[0];
		assert_eq!(f.server.host, "staging.example.com");
		assert_eq!(f.server.port, 22);
		assert!(f.server.persistent);
		assert_eq!(f.server.folder_id, None);
		assert_eq!(f.rel_path, "servers/staging.server");
		assert_eq!(f.tunnels.len(), 1);
		assert_eq!(f.tunnels[0].id, tunnel_app_id("8f3c2a1e", "c1d2"));
	}

	#[test]
	fn defaults_apply() {
		let d = project(&[("servers/a.server", MIN)]);
		let s = run(&[sp("P1", &d)]);
		let f = &s.servers[0];
		assert_eq!(f.server.port, 22);
		assert_eq!(f.server.environment, "dev");
		assert_eq!(f.server.agent_policy, "free");
		assert!(!f.server.persistent && f.tunnels.is_empty());
	}

	#[test]
	fn sub_folder_is_folder() {
		let d = project(&[("servers/clienti/acme.server", MIN)]);
		let s = run(&[sp("P1", &d)]);
		assert_eq!(s.folders.len(), 1);
		assert_eq!(
			(s.folders[0].name.as_str(), s.folders[0].id.as_str()),
			("clienti", "p:P1:clienti")
		);
		assert_eq!(s.servers[0].server.folder_id.as_deref(), Some("p:P1:clienti"));
		assert_eq!(parse_folder_id("p:P1:a/b"), Some(("P1", "a/b")));
	}

	#[test]
	fn conflict_markers_are_broken() {
		let d = project(&[("servers/a.server", "<<<<<<< HEAD\n{}\n=======\n{}\n>>>>>>> x\n")]);
		let s = run(&[sp("P1", &d)]);
		assert!(s.servers.is_empty());
		assert_eq!(s.problems.len(), 1);
		assert_eq!(s.problems[0].kind, ProblemKind::Broken);
		assert!(!s.problems[0].message.is_empty());
	}

	#[test]
	fn invalid_fields_are_broken() {
		let body =
			r#"{"id":"a","name":"n","host":"h","user":"u","login":"agent","environment":"live"}"#;
		let d = project(&[("servers/a.server", body)]);
		let s = run(&[sp("P1", &d)]);
		assert!(s.servers.is_empty());
		assert_eq!(s.problems[0].kind, ProblemKind::Broken);
	}

	#[test]
	fn missing_id_is_no_id() {
		let body = r#"{"name":"n","host":"h","user":"u","login":"agent"}"#;
		let d = project(&[("servers/a.server", body)]);
		let s = run(&[sp("P1", &d)]);
		assert!(s.servers.is_empty());
		assert_eq!(s.problems[0].kind, ProblemKind::NoId);
	}

	#[test]
	fn duplicate_prefers_prior_owner() {
		let d = project(&[("servers/a.server", MIN), ("servers/b.server", MIN)]);
		let second = dunce::canonicalize(d.path().join("servers/b.server")).unwrap();
		let prior = HashMap::from([("a".to_string(), second)]);
		let s = scan(&[sp("P1", &d)], &prior, &HashSet::new());
		assert_eq!(s.servers[0].rel_path, "servers/b.server");
		assert_eq!(s.problems[0].rel_path, "servers/a.server");
		assert_eq!(s.problems[0].kind, ProblemKind::Duplicate);
	}

	#[test]
	fn duplicate_without_prior_uses_project_then_path_order() {
		let d1 = project(&[("servers/z.server", MIN)]);
		let d2 = project(&[("servers/a.server", MIN)]);
		let s = run(&[sp("P1", &d1), sp("P2", &d2)]);
		assert_eq!(s.servers.len(), 1);
		assert_eq!(s.servers[0].project_id, "P1");
		assert_eq!(s.problems[0].project_id, "P2");
	}

	#[test]
	fn db_id_collision_is_duplicate() {
		let d = project(&[("servers/a.server", MIN)]);
		let s = scan(&[sp("P1", &d)], &HashMap::new(), &HashSet::from(["a".to_string()]));
		assert!(s.servers.is_empty());
		assert_eq!(s.problems[0].kind, ProblemKind::Duplicate);
	}

	#[test]
	fn nested_projects_list_a_file_once() {
		let d = project(&[("sub/servers/x.server", MIN)]);
		let inner = ScanProject { id: "P2".into(), root: d.path().join("sub") };
		let s = run(&[sp("P1", &d), inner]);
		assert_eq!(s.servers.len() + s.problems.len(), 1);
	}

	#[test]
	fn same_file_through_two_roots_is_listed_once() {
		let d = project(&[("servers/x.server", MIN)]);
		let s = run(&[sp("P1", &d), sp("P2", &d)]);
		assert_eq!(s.servers.len() + s.problems.len(), 1);
	}

	#[test]
	fn dot_folders_and_other_files_are_skipped() {
		let d = project(&[("servers/.git/x.server", MIN), ("servers/readme.md", "hi")]);
		let s = run(&[sp("P1", &d)]);
		assert!(s.servers.is_empty() && s.problems.is_empty() && s.folders.is_empty());
	}

	#[test]
	fn round_trip_keeps_unknown_fields() {
		let d = project(&[("servers/staging.server", FULL)]);
		let s = run(&[sp("P1", &d)]);
		let f = &s.servers[0];
		let out = to_json(&f.raw, &f.server, &f.tunnels);
		assert!(out.contains("\"x-note\": \"keep\""), "{out}");
		assert!(out.ends_with("}\n"));
		assert!(out.contains("\"id\": \"c1d2\""), "{out}");
	}

	#[test]
	fn key_path_is_written_with_tilde() {
		let h = dirs::home_dir().unwrap();
		let d = project(&[("servers/a.server", MIN)]);
		let s = run(&[sp("P1", &d)]);
		let mut srv = s.servers[0].server.clone();
		srv.key_path = Some(h.join(".ssh").join("id").to_string_lossy().into_owned());
		assert!(to_json(&Map::new(), &srv, &[]).contains("\"keyPath\": \"~/.ssh/id\""));
		assert_eq!(expand_home("~/.ssh/id"), h.join(".ssh/id").to_string_lossy());
	}

	#[test]
	fn tunnel_unknown_fields_survive() {
		let body = FULL.replace("\"label\": \"db\"", "\"label\": \"db\", \"x-note\": \"t\"");
		let d = project(&[("servers/staging.server", &body)]);
		let s = run(&[sp("P1", &d)]);
		let f = &s.servers[0];
		assert!(to_json(&f.raw, &f.server, &f.tunnels).contains("\"x-note\": \"t\""));
	}

	#[test]
	fn bad_or_duplicate_tunnels_are_broken() {
		let t = |a: &str, b: &str| {
			format!(
				r#"{{"id":"a","name":"n","host":"h","user":"u","login":"agent","tunnels":[{a},{b}]}}"#
			)
		};
		let ok = r#"{"id":"t1","localPort":1,"destHost":"h","destPort":2}"#;
		let zero = r#"{"id":"t2","localPort":0,"destHost":"h","destPort":2}"#;
		let d = project(&[("servers/a.server", &t(ok, zero)), ("servers/b.server", &t(ok, ok))]);
		let s = run(&[sp("P1", &d)]);
		assert!(s.servers.is_empty());
		assert_eq!(s.problems.len(), 2);
		assert!(s.problems.iter().all(|p| p.kind == ProblemKind::Broken));
	}

	#[test]
	fn key_path_keeps_tilde() {
		let body = FULL.to_string();
		let d = project(&[("servers/a.server", &body)]);
		let s = run(&[sp("P1", &d)]);
		assert_eq!(s.servers[0].server.key_path.as_deref(), Some("~/.ssh/id_ed25519"));
	}

	#[cfg(unix)]
	#[test]
	fn symlinks_cannot_escape() {
		let out = project(&[("x.server", MIN)]);
		let d = project(&[("servers/real.server", MIN)]);
		std::os::unix::fs::symlink(out.path(), d.path().join("servers/link")).unwrap();
		std::os::unix::fs::symlink(out.path().join("x.server"), d.path().join("servers/l.server"))
			.unwrap();
		let s = run(&[sp("P1", &d)]);
		assert_eq!(s.servers.len(), 1);
		assert_eq!(s.problems.len(), 1);
		assert!(new_file_path(d.path(), "link", "x").is_err());
	}

	#[test]
	fn new_file_path_suffixes() {
		let d = project(&[("servers/web.server", MIN)]);
		assert!(new_file_path(d.path(), "", "Web").unwrap().ends_with("web-2.server"));
		assert!(new_file_path(d.path(), "..", "Web").is_err());
		assert!(new_file_path(d.path(), "", "🎉").unwrap().ends_with("server.server"));
	}
}
