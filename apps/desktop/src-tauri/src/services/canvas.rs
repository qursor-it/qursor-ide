//! Canvas files (F44, ADR-0103): `<project>/canvas/<slug>.canvas`, JSON Canvas
//! 1.0 written by the renderer and, at times, by the project's agent.
//!
//! Rust owns listing, creating and the *conditional* write. The document itself
//! is the renderer's business; the only thing read out of it here is `title`.

use std::fs;
use std::io::Write as _;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::services::ide::scope::resolve_within;
use crate::services::intelligence::sha256_hex;

/// Folder under the project root that holds the canvases.
pub const DIR: &str = "canvas";
const MAX_SLUG: usize = 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CanvasEntry {
	pub project_id: String,
	pub path: String,
	/// Always `/`-separated, so it is the same key on every platform.
	pub rel_path: String,
	pub slug: String,
	pub title: String,
	pub modified_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum WriteOutcome {
	Written {
		sha256: String,
	},
	/// The disk no longer holds what the writer based its edit on. `None` when
	/// the file is gone, which must not be recreated by an autosave.
	#[serde(rename_all = "camelCase")]
	Conflict {
		disk_sha256: Option<String>,
	},
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CanvasRead {
	pub contents: String,
	pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CanvasOrigin {
	pub session_id: String,
	pub rel_path: String,
	pub title: String,
}

/// Latin letters with a diacritic, folded to ASCII. Anything else that is not
/// ASCII alphanumeric becomes a separator, so emoji and CJK fall through to the
/// `canvas` default rather than to a mangled name.
fn fold(c: char) -> Option<&'static str> {
	Some(match c {
		'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
		'ç' | 'ć' | 'č' => "c",
		'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => "e",
		'ì' | 'í' | 'î' | 'ï' | 'ī' => "i",
		'ñ' | 'ń' => "n",
		'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => "o",
		'ù' | 'ú' | 'û' | 'ü' | 'ū' => "u",
		'ý' | 'ÿ' => "y",
		'ś' | 'š' => "s",
		'ž' | 'ź' | 'ż' => "z",
		'ß' => "ss",
		'æ' => "ae",
		'œ' => "oe",
		_ => return None,
	})
}

pub fn slugify(title: &str) -> String {
	let mut out = String::new();
	let mut pending_dash = false;
	for c in title.chars().flat_map(char::to_lowercase) {
		let piece = match fold(c) {
			Some(s) => s.to_string(),
			None if c.is_ascii_alphanumeric() => c.to_string(),
			None => {
				pending_dash = true;
				continue;
			}
		};
		if pending_dash && !out.is_empty() {
			out.push('-');
		}
		pending_dash = false;
		out.push_str(&piece);
	}
	out.truncate(MAX_SLUG);
	let out = out.trim_end_matches('-');
	if out.is_empty() {
		"canvas".into()
	} else {
		out.into()
	}
}

fn title_of(path: &Path, slug: &str) -> String {
	fs::read_to_string(path)
		.ok()
		.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
		.and_then(|v| v.get("title")?.as_str().map(str::to_owned))
		.filter(|t| !t.trim().is_empty())
		.unwrap_or_else(|| slug.to_owned())
}

fn entry(project_id: &str, file: &Path) -> Option<CanvasEntry> {
	let slug = file.file_stem()?.to_str()?.to_owned();
	let modified_ms = fs::metadata(file)
		.ok()?
		.modified()
		.ok()?
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_millis() as i64)
		.unwrap_or(0);
	Some(CanvasEntry {
		project_id: project_id.to_owned(),
		path: file.to_string_lossy().into_owned(),
		rel_path: format!("{DIR}/{slug}.canvas"),
		title: title_of(file, &slug),
		slug,
		modified_ms,
	})
}

/// The project's canvases, newest first. A project with no folder, or no
/// `canvas/` in it, has none: that is not an error.
pub fn list(root: &Path, project_id: &str) -> AppResult<Vec<CanvasEntry>> {
	let Ok(dir) = fs::read_dir(root.join(DIR)) else {
		return Ok(Vec::new());
	};
	let mut out: Vec<CanvasEntry> = dir
		.filter_map(|e| e.ok())
		.map(|e| e.path())
		.filter(|p| p.extension().is_some_and(|x| x == "canvas") && p.is_file())
		.filter_map(|p| entry(project_id, &p))
		.collect();
	out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then_with(|| a.slug.cmp(&b.slug)));
	Ok(out)
}

pub fn create(root: &Path, project_id: &str, title: &str) -> AppResult<CanvasEntry> {
	if !root.is_dir() {
		return Err(AppError::InvalidInput(format!(
			"project folder is missing: {}",
			root.display()
		)));
	}
	let title = title.trim();
	let dir = root.join(DIR);
	fs::create_dir_all(&dir)?;
	let doc = serde_json::json!({ "title": title, "nodes": [], "edges": [] });
	let body = format!("{}\n", serde_json::to_string_pretty(&doc)?);

	let base = slugify(title);
	for n in 1..=1000 {
		let slug = if n == 1 { base.clone() } else { format!("{base}-{n}") };
		let file = dir.join(format!("{slug}.canvas"));
		// `create_new` is the collision check: it fails instead of overwriting.
		match fs::OpenOptions::new().write(true).create_new(true).open(&file) {
			Ok(mut f) => {
				f.write_all(body.as_bytes())?;
				return entry(project_id, &file)
					.ok_or_else(|| AppError::Io(format!("cannot stat {}", file.display())));
			}
			Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
			Err(e) => return Err(e.into()),
		}
	}
	Err(AppError::InvalidInput(format!("too many canvases named {base}")))
}

pub fn read(path: &Path) -> AppResult<CanvasRead> {
	let bytes = fs::read(path).map_err(|e| match e.kind() {
		std::io::ErrorKind::NotFound => AppError::NotFound(format!("path {}", path.display())),
		_ => AppError::Io(format!("{}: {e}", path.display())),
	})?;
	let sha256 = sha256_hex(&bytes);
	let contents = String::from_utf8(bytes)
		.map_err(|_| AppError::InvalidInput(format!("not UTF-8: {}", path.display())))?;
	Ok(CanvasRead { contents, sha256 })
}

/// Write `contents` only if the disk still holds what `expected_sha256` names.
///
/// shortcut: the check and the write are not one atomic step, so an agent that
/// writes in between loses to us; upgrade to a lock if that is ever seen.
pub fn write(
	root: &Path,
	path: &Path,
	contents: &str,
	expected_sha256: Option<&str>,
) -> AppResult<WriteOutcome> {
	let target = resolve_within(root, &path.to_string_lossy())?;
	if let Some(expected) = expected_sha256 {
		match fs::read(&target) {
			Ok(disk) => {
				let disk_sha = sha256_hex(&disk);
				if disk_sha != expected {
					return Ok(WriteOutcome::Conflict { disk_sha256: Some(disk_sha) });
				}
			}
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
				return Ok(WriteOutcome::Conflict { disk_sha256: None });
			}
			Err(e) => return Err(AppError::Io(format!("{}: {e}", target.display()))),
		}
	}
	crate::services::files::write_file(&target.to_string_lossy(), contents)?;
	Ok(WriteOutcome::Written { sha256: sha256_hex(contents.as_bytes()) })
}

pub fn get_session(
	conn: &Connection,
	project_id: &str,
	rel_path: &str,
) -> AppResult<Option<String>> {
	Ok(conn
		.query_row(
			"SELECT cs.session_id FROM canvas_sessions cs
			 JOIN sessions s ON s.id = cs.session_id
			 WHERE cs.project_id = ?1 AND cs.rel_path = ?2",
			params![project_id, rel_path],
			|r| r.get(0),
		)
		.optional()?)
}

/// The canvas a session is linked to (the reverse of [`get_session`]), if any.
pub fn canvas_for_session(
	conn: &Connection,
	project_id: &str,
	session_id: &str,
) -> AppResult<Option<String>> {
	Ok(conn
		.query_row(
			"SELECT rel_path FROM canvas_sessions
			 WHERE project_id = ?1 AND session_id = ?2
			 ORDER BY updated_at DESC LIMIT 1",
			params![project_id, session_id],
			|r| r.get(0),
		)
		.optional()?)
}

pub fn set_session(
	conn: &Connection,
	project_id: &str,
	rel_path: &str,
	session_id: &str,
	now_ms: i64,
) -> AppResult<()> {
	conn.execute(
		"INSERT INTO canvas_sessions (project_id, rel_path, session_id, updated_at)
		 VALUES (?1, ?2, ?3, ?4)
		 ON CONFLICT(project_id, rel_path)
		 DO UPDATE SET session_id = excluded.session_id, updated_at = excluded.updated_at",
		params![project_id, rel_path, session_id, now_ms],
	)?;
	Ok(())
}

/// Canvases that have a live session, for the origin strip on that session.
pub fn origins(conn: &Connection, root: &Path, project_id: &str) -> AppResult<Vec<CanvasOrigin>> {
	let mut stmt = conn.prepare(
		"SELECT cs.session_id, cs.rel_path FROM canvas_sessions cs
		 JOIN sessions s ON s.id = cs.session_id
		 WHERE cs.project_id = ?1 ORDER BY cs.updated_at DESC",
	)?;
	let rows = stmt
		.query_map(params![project_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
		.collect::<rusqlite::Result<Vec<_>>>()?;
	Ok(rows
		.into_iter()
		.map(|(session_id, rel_path)| {
			let slug = Path::new(&rel_path)
				.file_stem()
				.map(|s| s.to_string_lossy().into_owned())
				.unwrap_or_default();
			let title = title_of(&root.join(&rel_path), &slug);
			CanvasOrigin { session_id, rel_path, title }
		})
		.collect())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn slugify_cases() {
		assert_eq!(slugify("Brainstorm MVP"), "brainstorm-mvp");
		assert_eq!(slugify("Idee è Già"), "idee-e-gia");
		assert_eq!(slugify("🎉🎉"), "canvas");
		assert_eq!(slugify("  a--b  "), "a-b");
		let long = slugify(&format!("{}-{}", "a".repeat(59), "b".repeat(40)));
		assert!(long.len() <= 60 && !long.ends_with('-'), "{long}");
	}

	#[test]
	fn create_makes_folder_and_dedupes() {
		let dir = tempfile::tempdir().unwrap();
		let names: Vec<String> =
			(0..3).map(|_| create(dir.path(), "p", "X").unwrap().rel_path).collect();
		assert_eq!(names, ["canvas/x.canvas", "canvas/x-2.canvas", "canvas/x-3.canvas"]);
		let text = fs::read_to_string(dir.path().join("canvas/x.canvas")).unwrap();
		let v: serde_json::Value = serde_json::from_str(&text).unwrap();
		assert_eq!(v, serde_json::json!({"title":"X","nodes":[],"edges":[]}));
	}

	#[test]
	fn create_missing_project_folder_is_refused() {
		let dir = tempfile::tempdir().unwrap();
		assert!(create(&dir.path().join("nope"), "p", "X").is_err());
	}

	#[test]
	fn list_missing_folder_is_empty() {
		let dir = tempfile::tempdir().unwrap();
		assert!(list(dir.path(), "p").unwrap().is_empty());
		assert!(list(&dir.path().join("nope"), "p").unwrap().is_empty());
	}

	#[test]
	fn list_reads_title_and_falls_back() {
		let dir = tempfile::tempdir().unwrap();
		fs::create_dir(dir.path().join(DIR)).unwrap();
		fs::write(dir.path().join("canvas/good.canvas"), r#"{"title":"Nice One"}"#).unwrap();
		fs::write(dir.path().join("canvas/broken.canvas"), "{").unwrap();
		fs::write(dir.path().join("canvas/notes.txt"), "x").unwrap();
		let list = list(dir.path(), "p").unwrap();
		assert_eq!(list.len(), 2);
		let title = |slug: &str| list.iter().find(|e| e.slug == slug).unwrap().title.clone();
		assert_eq!(title("good"), "Nice One");
		assert_eq!(title("broken"), "broken");
	}

	fn sha(s: &str) -> String {
		sha256_hex(s.as_bytes())
	}

	#[test]
	fn write_refuses_on_stale_hash() {
		let dir = tempfile::tempdir().unwrap();
		let file = dir.path().join("canvas/a.canvas");
		fs::create_dir(dir.path().join(DIR)).unwrap();
		fs::write(&file, "v1").unwrap();
		let out = write(dir.path(), &file, "v2", Some(&sha("v1"))).unwrap();
		assert_eq!(out, WriteOutcome::Written { sha256: sha("v2") });
		fs::write(&file, "external").unwrap();
		let out = write(dir.path(), &file, "v3", Some(&sha("v2"))).unwrap();
		assert_eq!(out, WriteOutcome::Conflict { disk_sha256: Some(sha("external")) });
		assert_eq!(fs::read_to_string(&file).unwrap(), "external");
	}

	#[test]
	fn write_missing_file_conflicts() {
		let dir = tempfile::tempdir().unwrap();
		fs::create_dir(dir.path().join(DIR)).unwrap();
		let file = dir.path().join("canvas/gone.canvas");
		let out = write(dir.path(), &file, "v", Some(&sha("old"))).unwrap();
		assert_eq!(out, WriteOutcome::Conflict { disk_sha256: None });
		assert!(!file.exists());
	}

	#[test]
	fn canvas_for_session_is_the_reverse_lookup() {
		let conn = Connection::open_in_memory().unwrap();
		conn.execute_batch(
			"CREATE TABLE canvas_sessions (project_id TEXT NOT NULL, rel_path TEXT NOT NULL,
			 session_id TEXT NOT NULL, updated_at INTEGER NOT NULL, PRIMARY KEY (project_id, rel_path))",
		)
		.unwrap();
		set_session(&conn, "p", "canvas/a.canvas", "s1", 1).unwrap();
		assert_eq!(
			canvas_for_session(&conn, "p", "s1").unwrap().as_deref(),
			Some("canvas/a.canvas")
		);
		assert_eq!(canvas_for_session(&conn, "p", "s2").unwrap(), None);
		assert_eq!(canvas_for_session(&conn, "other", "s1").unwrap(), None);
	}

	#[test]
	fn write_outside_root_refused() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().join("root");
		fs::create_dir(&root).unwrap();
		let evil = root.join("../evil.canvas");
		assert!(write(&root, &evil, "x", None).is_err());
		assert!(!dir.path().join("evil.canvas").exists());
	}
}
