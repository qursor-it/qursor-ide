//! Model-facing tools for the terminals a human lent to this session (F45,
//! ADR-0107): list them, run a command in one, read its screen, press keys.
//!
//! **Bound to the session by construction.** The backend handed to
//! [`super::AgentTools`] already knows which session it serves; no argument here
//! can name another session's terminal, the same rule `start_agent_tools`
//! applies to routines.

use serde_json::{json, Value};

use crate::services::mcp_wire::{tool_error, tool_text};

/// The longest a single `terminal_run` waits before handing back what it has.
pub const MAX_TIMEOUT_S: u64 = 600;
const DEFAULT_TIMEOUT_S: u64 = 60;

/// What a session may do with its lent terminals. Errors are sentences for the
/// model, saying what to do next.
pub trait TerminalUse: Send + Sync {
	fn list(&self) -> Value;
	fn run(&self, terminal: &str, command: &str, timeout_s: u64) -> Result<Value, String>;
	fn read(&self, terminal: &str) -> Result<Value, String>;
	fn send_keys(&self, terminal: &str, keys: &[String]) -> Result<Value, String>;
}

pub const INSTRUCTIONS: &str = concat!(
	"The human can lend you terminals on remote servers (SSH sessions in Qursor IDE). ",
	"Call `terminals_list` to see which ones you have, on which server and in which directory; ",
	"run commands there with `terminal_run`, never with your local shell or `ssh`. ",
	"Your own working directory and file tools are on this computer, not on those servers. ",
	"A terminal lent in shared mode is the human's own shell: they see every command you type, ",
	"its state (directory, environment, `sudo`) is shared with them, and they can take the turn ",
	"back at any time. Never ask for, type or repeat a password: when a terminal waits for one, ",
	"stop and tell the human. Each server has a policy the human chose; a refusal is final for ",
	"that command.",
);

pub fn definitions() -> Vec<Value> {
	let term = json!({
		"type": "string",
		"description": "The terminal's id, as `terminals_list` gives it.",
	});
	vec![
		json!({
			"name": "terminals_list",
			"description": "The terminals on remote servers the human has lent to this session: \
							id, server, user and host, current directory, whether it is shared \
							with the human or your own, the server's policy for agents, whether it \
							is production, and its state (ready, running a command, in a \
							full-screen program, waiting for a password). Call this first, and \
							again whenever you are unsure where a terminal is.",
			"inputSchema": { "type": "object", "properties": {} },
			"annotations": { "title": "List lent terminals", "readOnlyHint": true },
			"_meta": { "anthropic/searchHint": "run commands in remote server SSH terminals the human lent this session in Qursor IDE" },
		}),
		json!({
			"name": "terminal_run",
			"description": "Type one shell command into a lent terminal on a remote server and \
							wait for it to finish. Returns its output, exit code and the \
							directory the shell is in afterwards. A `cd` persists. Interactive \
							programs (editors, pagers, `top`) are not for this tool: use \
							non-interactive flags (`--no-pager`, `-y` only when the human agreed). \
							If the timeout passes the command keeps running and you get what it \
							printed so far.",
			"inputSchema": {
				"type": "object",
				"properties": {
					"terminal": term,
					"command": { "type": "string", "description": "A POSIX shell command line." },
					"timeout_s": {
						"type": "integer",
						"description": format!("Seconds to wait, 1–{MAX_TIMEOUT_S}. Default {DEFAULT_TIMEOUT_S}."),
					},
				},
				"required": ["terminal", "command"],
			},
			"annotations": { "title": "Run in a remote terminal", "destructiveHint": true },
			"_meta": { "anthropic/searchHint": "run commands in remote server SSH terminals the human lent this session in Qursor IDE" },
		}),
		json!({
			"name": "terminal_read",
			"description": "What is on a lent terminal's screen right now, as text, and its \
							state. Use it to see a full-screen program, a prompt waiting for an \
							answer, or what the human did.",
			"inputSchema": {
				"type": "object",
				"properties": { "terminal": term },
				"required": ["terminal"],
			},
			"annotations": { "title": "Read a remote terminal", "readOnlyHint": true },
			"_meta": { "anthropic/searchHint": "run commands in remote server SSH terminals the human lent this session in Qursor IDE" },
		}),
		json!({
			"name": "terminal_send_keys",
			"description": "Press keys in a lent terminal: answer a `[y/N]`, quit a pager with \
							`q`, interrupt with `<ctrl-c>`. Each item is literal text or one \
							named key: <enter> <tab> <esc> <space> <backspace> <up> <down> \
							<left> <right> <ctrl-c> <ctrl-d> <ctrl-z> <ctrl-l>. Never a password.",
			"inputSchema": {
				"type": "object",
				"properties": {
					"terminal": term,
					"keys": { "type": "array", "items": { "type": "string" } },
				},
				"required": ["terminal", "keys"],
			},
			"annotations": { "title": "Press keys in a remote terminal", "destructiveHint": true },
			"_meta": { "anthropic/searchHint": "run commands in remote server SSH terminals the human lent this session in Qursor IDE" },
		}),
	]
}

pub fn is_terminal_tool(name: &str) -> bool {
	matches!(name, "terminals_list" | "terminal_run" | "terminal_read" | "terminal_send_keys")
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
	args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

pub fn call(backend: &dyn TerminalUse, name: &str, args: &Value) -> Value {
	let done = |r: Result<Value, String>| match r {
		Ok(v) => tool_text(&v.to_string()),
		Err(e) => tool_error(&e),
	};
	match name {
		"terminals_list" => tool_text(&backend.list().to_string()),
		"terminal_run" => {
			let (Some(t), Some(c)) = (str_arg(args, "terminal"), str_arg(args, "command")) else {
				return tool_error("terminal_run needs a terminal and a command");
			};
			let timeout = args
				.get("timeout_s")
				.and_then(Value::as_u64)
				.unwrap_or(DEFAULT_TIMEOUT_S)
				.clamp(1, MAX_TIMEOUT_S);
			done(backend.run(t, c, timeout))
		}
		"terminal_read" => match str_arg(args, "terminal") {
			Some(t) => done(backend.read(t)),
			None => tool_error("terminal_read needs a terminal"),
		},
		"terminal_send_keys" => {
			let Some(t) = str_arg(args, "terminal") else {
				return tool_error("terminal_send_keys needs a terminal");
			};
			let keys: Vec<String> = args
				.get("keys")
				.and_then(Value::as_array)
				.map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
				.unwrap_or_default();
			if keys.is_empty() {
				return tool_error("terminal_send_keys needs at least one key");
			}
			done(backend.send_keys(t, &keys))
		}
		_ => tool_error(&format!("no such terminal tool: {name}")),
	}
}

/// Bytes for one `keys` item: a named key, or the text itself.
pub fn key_bytes(item: &str) -> Vec<u8> {
	let named: Option<&[u8]> = match item.to_ascii_lowercase().as_str() {
		"<enter>" => Some(b"\r"),
		"<tab>" => Some(b"\t"),
		"<esc>" => Some(b"\x1b"),
		"<space>" => Some(b" "),
		"<backspace>" => Some(b"\x7f"),
		"<up>" => Some(b"\x1b[A"),
		"<down>" => Some(b"\x1b[B"),
		"<right>" => Some(b"\x1b[C"),
		"<left>" => Some(b"\x1b[D"),
		"<ctrl-c>" => Some(b"\x03"),
		"<ctrl-d>" => Some(b"\x04"),
		"<ctrl-z>" => Some(b"\x1a"),
		"<ctrl-l>" => Some(b"\x0c"),
		_ => None,
	};
	named.map_or_else(|| item.as_bytes().to_vec(), <[u8]>::to_vec)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn named_keys_map_to_their_bytes_and_text_passes_through() {
		assert_eq!(key_bytes("<ctrl-c>"), b"\x03");
		assert_eq!(key_bytes("<Enter>"), b"\r");
		assert_eq!(key_bytes("q"), b"q");
	}

	#[test]
	fn every_defined_tool_is_dispatched() {
		for d in definitions() {
			let name = d["name"].as_str().unwrap_or_default();
			assert!(is_terminal_tool(name), "{name}");
		}
	}
}
