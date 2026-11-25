//! OpenCode 1.x launch and per-session MCP registration (ADR-0118).

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Map, Value};

use crate::error::{AppError, AppResult};
use crate::services::agent_tools::SERVER_NAME;

pub const CONFIG_CONTENT_ENV: &str = "OPENCODE_CONFIG_CONTENT";
pub const TOOLS_TOKEN_ENV: &str = "QURSOR_OPENCODE_MCP_TOKEN";

pub fn supports_version(version: Option<&str>) -> bool {
	version.is_none_or(|version| version.starts_with("1."))
}

pub fn argv(resume: Option<&str>, initial_prompt: Option<&str>) -> Vec<String> {
	let mut args = Vec::new();
	if let Some(id) = resume {
		args.push(format!("--session={id}"));
	}
	args.extend(
		initial_prompt
			.filter(|prompt| !prompt.is_empty())
			.map(|prompt| format!("--prompt={prompt}")),
	);
	args
}

pub fn launch_config(
	inherited: Option<&str>,
	port: Option<u16>,
	plugin: Option<&str>,
) -> AppResult<String> {
	let invalid =
		|| AppError::InvalidInput(format!("{CONFIG_CONTENT_ENV} must contain a JSON object"));
	let mut config: Map<String, Value> = match inherited.filter(|value| !value.trim().is_empty()) {
		Some(value) => serde_json::from_str(value).map_err(|_| invalid())?,
		None => Map::new(),
	};
	if let Some(plugin) = plugin {
		config
			.entry("plugin")
			.or_insert_with(|| json!([]))
			.as_array_mut()
			.ok_or_else(invalid)?
			.push(json!(plugin));
	}
	if let Some(port) = port {
		let servers = config.entry("mcp").or_insert_with(|| json!({}));
		let servers = servers.as_object_mut().ok_or_else(invalid)?;
		servers.insert(
			SERVER_NAME.into(),
			json!({
				"type": "remote",
				"url": format!("http://127.0.0.1:{port}/mcp"),
				"enabled": true,
				"oauth": false,
				"headers": { "Authorization": format!("Bearer {{env:{TOOLS_TOKEN_ENV}}}") },
			}),
		);
	}
	Ok(Value::Object(config).to_string())
}

pub fn is_session_id(id: &str) -> bool {
	id.strip_prefix("ses_").is_some_and(|suffix| {
		!suffix.is_empty()
			&& suffix.len() <= 64
			&& suffix.bytes().all(|b| b.is_ascii_alphanumeric())
	})
}

pub fn database_path(root: &Path) -> PathBuf {
	root.join("opencode").join("opencode.db")
}

fn open(root: &Path) -> rusqlite::Result<Connection> {
	let connection = Connection::open_with_flags(
		database_path(root),
		OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
	)?;
	connection.busy_timeout(Duration::from_millis(250))?;
	Ok(connection)
}

#[derive(Debug)]
pub struct SessionMeta {
	pub id: String,
	pub directory: String,
	pub title: String,
	pub created_at: i64,
	pub updated_at: i64,
}

pub fn sessions(root: &Path) -> rusqlite::Result<Vec<SessionMeta>> {
	let connection = open(root)?;
	// Early SQLite releases did not track archived sessions.
	let archived: bool = connection.query_row(
		"SELECT EXISTS(SELECT 1 FROM pragma_table_info('session') WHERE name = 'time_archived')",
		[],
		|r| r.get(0),
	)?;
	let sql = "SELECT id, directory, title, time_created, time_updated FROM session WHERE parent_id IS NULL";
	let mut stmt = connection
		.prepare(&format!("{sql}{}", if archived { " AND time_archived IS NULL" } else { "" }))?;
	let rows = stmt.query_map([], |r| {
		Ok(SessionMeta {
			id: r.get(0)?,
			directory: r.get(1)?,
			title: r.get(2)?,
			created_at: r.get(3)?,
			updated_at: r.get(4)?,
		})
	})?;
	rows.filter(|r| r.as_ref().is_err() || r.as_ref().is_ok_and(|r| is_session_id(&r.id))).collect()
}

pub fn session_directory(root: &Path, id: &str) -> Option<String> {
	if !is_session_id(id) {
		return None;
	}
	open(root)
		.ok()?
		.query_row("SELECT directory FROM session WHERE id = ?1 AND parent_id IS NULL", [id], |r| {
			r.get(0)
		})
		.optional()
		.ok()
		.flatten()
}

pub const SESSION_SIGNAL_ENV: &str = "QURSOR_OPENCODE_SESSION_SIGNAL";

/// The first root session from this CLI process, scoped to a private temp directory (ADR-0122).
pub struct SessionSignal {
	_dir: tempfile::TempDir,
	path: PathBuf,
	pub plugin: tauri::Url,
}

impl SessionSignal {
	pub fn create() -> std::io::Result<Self> {
		let dir = tempfile::Builder::new().prefix("qursor-opencode-").tempdir()?;
		let script = dir.path().join("qursor-session.mjs");
		std::fs::write(&script, include_str!("opencode-session.mjs"))?;
		let plugin = tauri::Url::from_file_path(&script)
			.map_err(|_| std::io::Error::other("invalid OpenCode plugin path"))?;
		let path = dir.path().join("session-id");
		Ok(Self { _dir: dir, path, plugin })
	}

	pub fn path(&self) -> &Path {
		&self.path
	}

	pub fn read(&self) -> Option<String> {
		let id = std::fs::read_to_string(&self.path).ok()?;
		is_session_id(&id).then_some(id)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_failed_version_probe_does_not_hide_a_resolved_binary() {
		assert!(supports_version(Some("1.18.30")));
		assert!(!supports_version(Some("2.0.0")));
		assert!(supports_version(None));
	}

	#[test]
	fn first_message_is_one_argument_even_when_it_starts_with_a_dash() {
		assert_eq!(argv(None, Some("--review\nwith spaces")), ["--prompt=--review\nwith spaces"]);
		assert!(argv(None, None).is_empty());
	}

	#[test]
	fn tool_registration_keeps_other_inline_settings_and_references_the_token() {
		let inherited = r#"{"model":"opencode-go/model","plugin":["existing"],"mcp":{"other":{"type":"local","command":["test"]}}}"#;
		let config: Value = serde_json::from_str(
			&launch_config(Some(inherited), Some(1234), Some("file:///qursor.mjs"))
				.expect("config"),
		)
		.expect("JSON");
		assert_eq!(config["model"], "opencode-go/model");
		assert_eq!(config["plugin"], json!(["existing", "file:///qursor.mjs"]));
		assert_eq!(config["mcp"]["other"]["command"][0], "test");
		let qursor = &config["mcp"][SERVER_NAME];
		assert_eq!(qursor["url"], "http://127.0.0.1:1234/mcp");
		assert_eq!(qursor["headers"]["Authorization"], "Bearer {env:QURSOR_OPENCODE_MCP_TOKEN}");
		assert!(launch_config(Some("not JSON"), Some(1234), None).is_err());
	}
}
