//! OpenCode 1.x launch and per-session MCP registration (ADR-0118).

use serde_json::{json, Map, Value};

use crate::error::{AppError, AppResult};
use crate::services::agent_tools::SERVER_NAME;

pub const CONFIG_CONTENT_ENV: &str = "OPENCODE_CONFIG_CONTENT";
pub const TOOLS_TOKEN_ENV: &str = "QURSOR_OPENCODE_MCP_TOKEN";

pub fn supports_version(version: Option<&str>) -> bool {
	version.is_none_or(|version| version.starts_with("1."))
}

pub fn argv(initial_prompt: Option<&str>) -> Vec<String> {
	initial_prompt
		.filter(|prompt| !prompt.is_empty())
		.map(|prompt| format!("--prompt={prompt}"))
		.into_iter()
		.collect()
}

pub fn mcp_config(inherited: Option<&str>, port: u16) -> AppResult<String> {
	let invalid =
		|| AppError::InvalidInput(format!("{CONFIG_CONTENT_ENV} must contain a JSON object"));
	let mut config: Map<String, Value> = match inherited.filter(|value| !value.trim().is_empty()) {
		Some(value) => serde_json::from_str(value).map_err(|_| invalid())?,
		None => Map::new(),
	};
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
	Ok(Value::Object(config).to_string())
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
		assert_eq!(argv(Some("--review\nwith spaces")), ["--prompt=--review\nwith spaces"]);
		assert!(argv(None).is_empty());
	}

	#[test]
	fn tool_registration_keeps_other_inline_settings_and_references_the_token() {
		let inherited =
			r#"{"model":"opencode-go/model","mcp":{"other":{"type":"local","command":["test"]}}}"#;
		let config: Value =
			serde_json::from_str(&mcp_config(Some(inherited), 1234).expect("config"))
				.expect("JSON");
		assert_eq!(config["model"], "opencode-go/model");
		assert_eq!(config["mcp"]["other"]["command"][0], "test");
		let qursor = &config["mcp"][SERVER_NAME];
		assert_eq!(qursor["url"], "http://127.0.0.1:1234/mcp");
		assert_eq!(qursor["headers"]["Authorization"], "Bearer {env:QURSOR_OPENCODE_MCP_TOKEN}");
		assert!(mcp_config(Some("not JSON"), 1234).is_err());
	}
}
