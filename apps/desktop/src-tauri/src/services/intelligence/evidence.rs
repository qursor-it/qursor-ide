//! Deterministic, offline extraction of *signals* from one session transcript.
//!
//! A signal is something that was observed — a person correcting an agent, a
//! test command failing, the same command run again, the same instruction given
//! twice — never a conclusion about why. The extractor reads the original
//! transcript (ADR-0004, ADR-0053: the FTS index flattens exactly the structure
//! this needs) and does not write to it.
//!
//! **Two transcripts, one extractor.** Claude and Codex write different files
//! (ADR-0060). Each has a small adapter that turns a line into an [`Ev`]; the
//! rules that decide what is a signal live once, in [`Extractor`]. The adapters
//! read raw lines rather than the indexer's `SessionEvent` because the Codex
//! reader there drops every tool call and result, and because a signal needs the
//! line number to point at.
//!
//! **What counts as a person.** A Claude `user` record is often not one: it is
//! how a tool result is delivered, or an injected caveat, or a compaction
//! summary. Only text a human typed becomes [`Ev::Human`]; see [`human_text`].
//!
//! Everything that leaves this module is bounded (a handful of signals per
//! session, 280-character excerpts) and redacted ([`super::redact`]).

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde_json::Value;

use super::{dedupe, redact, sha256_hex};
use crate::models::intelligence::{EvidenceLocator, SignalType};
use crate::services::jsonl::{flatten_message_text, title_display};

/// Longest excerpt, in characters.
pub const EXCERPT_CHARS: usize = 280;
/// Longest context line, in characters.
pub const CONTEXT_CHARS: usize = 160;

/// A line longer than this is skipped: it is a pasted file or a dumped log, not
/// something said.
const MAX_LINE_BYTES: usize = 512 * 1024;
/// Reading stops after this many bytes of one transcript.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const HUMAN_TEXT_CHARS: usize = 4000;
const RESULT_TEXT_CHARS: usize = 2400;
const COMMAND_CHARS: usize = 600;
const MAX_HUMANS_TRACKED: usize = 400;
const MAX_CALLS_TRACKED: usize = 600;
/// A re-run counts as a retry only within this many tool calls of the failure.
const RETRY_WINDOW: usize = 12;

/// Per session and per type. Totals stay small enough that a run's cap, applied
/// across sessions, is a choice rather than an accident of one noisy transcript.
const CAP_CORRECTION: usize = 4;
const CAP_FAILURE: usize = 3;
const CAP_RETRY: usize = 2;
const CAP_REPEAT: usize = 2;

/// One observation, before the collector gives it a session and an id.
#[derive(Debug, Clone)]
pub struct RawSignal {
	pub signal_type: SignalType,
	pub excerpt: String,
	pub context: Option<String>,
	pub locator: EvidenceLocator,
	/// sha256 of the normalised source event.
	pub fingerprint: String,
	/// The event's own timestamp, epoch ms, when the transcript wrote one.
	pub observed_at: Option<i64>,
}

/// The stable id of one observation: derived from the project, the session and
/// the fingerprint, so extracting the same event twice names it the same way.
/// A v5-shaped uuid, minted here and never by the model.
pub fn evidence_id(project_id: &str, session_id: &str, fingerprint: &str) -> String {
	let digest = sha256_hex(
		format!("factorai-evidence-v1|{project_id}|{session_id}|{fingerprint}").as_bytes(),
	);
	let mut bytes = [0u8; 16];
	for (i, byte) in bytes.iter_mut().enumerate() {
		*byte = u8::from_str_radix(&digest[i * 2..i * 2 + 2], 16).unwrap_or(0);
	}
	bytes[6] = (bytes[6] & 0x0f) | 0x50;
	bytes[8] = (bytes[8] & 0x3f) | 0x80;
	uuid::Uuid::from_bytes(bytes).to_string()
}

/// Read one transcript and return its signals, in the order they happened.
///
/// `agent` is `claude` or `codex`; anything else yields nothing rather than a
/// guess. An unreadable file is an `Err` the caller counts as unreadable. A
/// line that is not JSON — the half-written tail of a live transcript — is
/// skipped, as the indexer skips it.
pub fn extract_session(agent: &str, path: &Path) -> std::io::Result<Vec<RawSignal>> {
	let mut extractor = Extractor::default();
	match agent {
		crate::agents::CLAUDE => {
			for_each_json_line(path, |line, value| claude_event(line, &value, &mut extractor))?
		}
		crate::agents::CODEX => {
			for_each_json_line(path, |line, value| codex_event(line, &value, &mut extractor))?
		}
		_ => return Ok(Vec::new()),
	}
	Ok(extractor.finish())
}

// ── Reading ────────────────────────────────────────────────────────────────

fn for_each_json_line(path: &Path, mut f: impl FnMut(u64, Value)) -> std::io::Result<()> {
	let mut reader = BufReader::with_capacity(64 * 1024, File::open(path)?);
	let mut buf: Vec<u8> = Vec::new();
	let mut line_no = 0u64;
	let mut total = 0u64;
	loop {
		buf.clear();
		let read = reader.by_ref().take(MAX_LINE_BYTES as u64 + 1).read_until(b'\n', &mut buf)?;
		if read == 0 {
			return Ok(());
		}
		line_no += 1;
		total += read as u64;
		if buf.len() > MAX_LINE_BYTES && buf.last() != Some(&b'\n') {
			skip_rest_of_line(&mut reader)?;
			continue;
		}
		if total > MAX_FILE_BYTES {
			return Ok(());
		}
		if let Ok(value) = serde_json::from_slice::<Value>(buf.trim_ascii()) {
			f(line_no, value);
		}
	}
}

fn skip_rest_of_line(reader: &mut BufReader<File>) -> std::io::Result<()> {
	loop {
		let (done, used) = {
			let available = reader.fill_buf()?;
			if available.is_empty() {
				(true, 0)
			} else if let Some(i) = available.iter().position(|b| *b == b'\n') {
				(true, i + 1)
			} else {
				(false, available.len())
			}
		};
		reader.consume(used);
		if done {
			return Ok(());
		}
	}
}

fn timestamp_ms(value: &Value) -> Option<i64> {
	let text = value.get("timestamp")?.as_str()?;
	chrono::DateTime::parse_from_rfc3339(text).ok().map(|d| d.timestamp_millis())
}

fn flag(value: &Value, key: &str) -> bool {
	value.get(key).and_then(Value::as_bool) == Some(true)
}

/// Keep the head and the tail of a long text: a failure's cause is usually at
/// the end, a message's point at the start.
fn bounded(text: &str, max: usize) -> String {
	if text.chars().count() <= max {
		return text.to_string();
	}
	let head_len = max / 4;
	let tail_len = max - head_len;
	let head: String = text.chars().take(head_len).collect();
	let tail: String = text.chars().skip(text.chars().count() - tail_len).collect();
	format!("{head}\n…\n{tail}")
}

// ── The events the rules read ──────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Loc {
	line: u64,
	ordinal: Option<i64>,
	event_id: Option<String>,
	ts: Option<i64>,
}

impl Loc {
	fn locator(&self) -> EvidenceLocator {
		EvidenceLocator { line: self.line, ordinal: self.ordinal, event_id: self.event_id.clone() }
	}
}

enum Ev {
	Human { text: String, loc: Loc },
	Assistant { summary: String },
	ToolCall { call_id: String, command: Option<String>, loc: Loc },
	ToolResult { call_id: String, failed: bool, text: String, loc: Loc },
}

// ── Claude ─────────────────────────────────────────────────────────────────

fn claude_event(line: u64, value: &Value, ex: &mut Extractor) {
	// A sidechain event belongs to a sub-agent: its `user` turns are the parent
	// agent's prompts, not a person's.
	if flag(value, "isSidechain") {
		return;
	}
	let loc = Loc {
		line,
		ordinal: None,
		event_id: value.get("uuid").and_then(Value::as_str).map(str::to_owned),
		ts: timestamp_ms(value),
	};
	match value.get("type").and_then(Value::as_str) {
		Some("user") => {
			if flag(value, "isMeta") || flag(value, "isCompactSummary") {
				return;
			}
			match value.pointer("/message/content") {
				Some(Value::String(text)) => {
					if let Some(text) = human_text(text) {
						ex.feed(Ev::Human { text, loc });
					}
				}
				Some(Value::Array(blocks)) => {
					let mut had_result = false;
					let mut texts: Vec<&str> = Vec::new();
					for block in blocks {
						match block.get("type").and_then(Value::as_str) {
							Some("tool_result") => {
								had_result = true;
								ex.feed(Ev::ToolResult {
									call_id: block
										.get("tool_use_id")
										.and_then(Value::as_str)
										.unwrap_or_default()
										.to_string(),
									failed: flag(block, "is_error"),
									text: bounded(
										&flatten_message_text(
											block.get("content").unwrap_or(&Value::Null),
										),
										RESULT_TEXT_CHARS,
									),
									loc: loc.clone(),
								});
							}
							Some("text") => {
								if let Some(text) = block.get("text").and_then(Value::as_str) {
									texts.push(text);
								}
							}
							_ => {}
						}
					}
					// A record that carries a tool result is the tool's reply, whatever
					// else is in it.
					if !had_result {
						if let Some(text) = human_text(&texts.join("\n")) {
							ex.feed(Ev::Human { text, loc });
						}
					}
				}
				_ => {}
			}
		}
		Some("assistant") => {
			let Some(Value::Array(blocks)) = value.pointer("/message/content") else { return };
			for block in blocks {
				match block.get("type").and_then(Value::as_str) {
					Some("text") => {
						if let Some(text) = block.get("text").and_then(Value::as_str) {
							ex.feed(Ev::Assistant { summary: bounded(text, 400) });
						}
					}
					Some("tool_use") => {
						let command = block
							.pointer("/input/command")
							.and_then(Value::as_str)
							.map(|c| bounded(c, COMMAND_CHARS));
						ex.feed(Ev::ToolCall {
							call_id: block
								.get("id")
								.and_then(Value::as_str)
								.unwrap_or_default()
								.to_string(),
							command,
							loc: loc.clone(),
						});
					}
					_ => {}
				}
			}
		}
		_ => {}
	}
}

// ── Codex ──────────────────────────────────────────────────────────────────

fn codex_event(line: u64, value: &Value, ex: &mut Extractor) {
	let payload = value.get("payload").unwrap_or(&Value::Null);
	let loc = Loc {
		line,
		ordinal: value.get("ordinal").and_then(Value::as_i64),
		event_id: payload
			.get("call_id")
			.or_else(|| payload.get("id"))
			.and_then(Value::as_str)
			.map(str::to_owned),
		ts: timestamp_ms(value),
	};
	match value.get("type").and_then(Value::as_str) {
		Some("response_item") => match payload.get("type").and_then(Value::as_str) {
			Some("message") => {
				let blocks = payload.get("content").and_then(Value::as_array);
				let texts: Vec<&str> = blocks
					.map(|b| {
						b.iter().filter_map(|x| x.get("text").and_then(Value::as_str)).collect()
					})
					.unwrap_or_default();
				match payload.get("role").and_then(Value::as_str) {
					Some("user") => {
						// Codex's own context (the injected AGENTS.md, the environment
						// block) arrives as a user turn too.
						if texts
							.first()
							.is_some_and(|t| crate::agents::codex::is_injected_context(t))
						{
							return;
						}
						if let Some(text) = human_text(&texts.join("\n")) {
							ex.feed(Ev::Human { text, loc });
						}
					}
					Some("assistant") => {
						ex.feed(Ev::Assistant { summary: bounded(&texts.join("\n"), 400) });
					}
					_ => {}
				}
			}
			Some("function_call") | Some("custom_tool_call") => {
				ex.feed(Ev::ToolCall {
					call_id: payload
						.get("call_id")
						.and_then(Value::as_str)
						.unwrap_or_default()
						.to_string(),
					command: codex_command(payload).map(|c| bounded(&c, COMMAND_CHARS)),
					loc,
				});
			}
			Some("local_shell_call") => {
				ex.feed(Ev::ToolCall {
					call_id: payload
						.get("call_id")
						.or_else(|| payload.get("id"))
						.and_then(Value::as_str)
						.unwrap_or_default()
						.to_string(),
					command: command_from_value(payload.pointer("/action/command"))
						.map(|c| bounded(&c, COMMAND_CHARS)),
					loc,
				});
			}
			Some("function_call_output") | Some("custom_tool_call_output") => {
				let (failed, text) = codex_result(payload);
				ex.feed(Ev::ToolResult {
					call_id: payload
						.get("call_id")
						.and_then(Value::as_str)
						.unwrap_or_default()
						.to_string(),
					failed,
					text: bounded(&text, RESULT_TEXT_CHARS),
					loc,
				});
			}
			_ => {}
		},
		// The structured end of a command, where Codex writes one: the exit code is
		// stated rather than parsed out of text.
		Some("event_msg")
			if payload.get("type").and_then(Value::as_str) == Some("exec_command_end") =>
		{
			let Some(code) = payload.get("exit_code").and_then(Value::as_i64) else { return };
			let text = ["aggregated_output", "stderr", "stdout"]
				.iter()
				.find_map(|k| payload.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()))
				.unwrap_or_default();
			ex.feed(Ev::ToolResult {
				call_id: payload
					.get("call_id")
					.and_then(Value::as_str)
					.unwrap_or_default()
					.to_string(),
				failed: code != 0,
				text: bounded(text, RESULT_TEXT_CHARS),
				loc,
			});
		}
		_ => {}
	}
}

/// The command a Codex tool call runs, from its JSON `arguments`.
fn codex_command(payload: &Value) -> Option<String> {
	let parsed: Value = match payload.get("arguments") {
		Some(Value::String(text)) => serde_json::from_str(text).ok()?,
		Some(object @ Value::Object(_)) => object.clone(),
		_ => return None,
	};
	command_from_value(parsed.get("command").or_else(|| parsed.get("cmd")))
}

/// `"cargo test"`, or `["bash", "-lc", "cargo test"]`, as one command line.
fn command_from_value(value: Option<&Value>) -> Option<String> {
	match value? {
		Value::String(text) => Some(text.clone()),
		Value::Array(parts) => {
			let parts: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
			match parts.as_slice() {
				[] => None,
				[_, flag, rest @ ..] if matches!(*flag, "-lc" | "-c") && !rest.is_empty() => {
					Some(rest.join(" "))
				}
				all => Some(all.join(" ")),
			}
		}
		_ => None,
	}
}

/// Whether a Codex tool call failed, and its text.
///
/// Where the exit code is not stated, the call is **not** called failed: absence
/// of evidence is not a failure any more than it is a success.
fn codex_result(payload: &Value) -> (bool, String) {
	match payload.get("output") {
		Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
			Ok(Value::Object(object)) => {
				let body = object.get("output").and_then(Value::as_str).unwrap_or_default();
				let code =
					object.get("metadata").and_then(|m| m.get("exit_code")).and_then(Value::as_i64);
				(code.is_some_and(|c| c != 0), body.to_string())
			}
			_ => (exit_code_in_text(text).is_some_and(|c| c != 0), text.clone()),
		},
		Some(Value::Object(object)) => {
			let failed = object.get("success").and_then(Value::as_bool) == Some(false);
			let text = object.get("content").and_then(Value::as_str).unwrap_or_default();
			(failed, text.to_string())
		}
		Some(other @ Value::Array(_)) => (false, flatten_message_text(other)),
		_ => (false, String::new()),
	}
}

/// `Exit code: 2`, `exited with code 1`.
fn exit_code_in_text(text: &str) -> Option<i64> {
	let lower: String = text.chars().take(400).collect::<String>().to_lowercase();
	for marker in ["exit code:", "exit code ", "exited with code "] {
		if let Some(at) = lower.find(marker) {
			let digits: String = lower[at + marker.len()..]
				.trim_start()
				.chars()
				.take_while(|c| c.is_ascii_digit() || *c == '-')
				.collect();
			if let Ok(code) = digits.parse() {
				return Some(code);
			}
		}
	}
	None
}

// ── What is a person, and what is a correction ─────────────────────────────

/// The text of a message a human typed, or `None` when the record only looks
/// like one: a slash command, a shell escape, a caveat, an interrupt marker, a
/// compaction summary, or text that is nothing but injected context.
pub(crate) fn human_text(raw: &str) -> Option<String> {
	let text = raw.trim();
	if text.is_empty() {
		return None;
	}
	const MACHINERY: [&str; 7] = [
		"<command-name>",
		"<command-message>",
		"<bash-input>",
		"<bash-stdout>",
		"<local-command-stdout>",
		"<local-command-stderr>",
		"<local-command-caveat>",
	];
	if MACHINERY.iter().any(|m| text.contains(m))
		|| text.starts_with("[Request interrupted by user")
		|| text.starts_with("This session is being continued from a previous conversation")
	{
		return None;
	}
	let stripped = title_display(text)?;
	let stripped = stripped.trim();
	// Whatever still opens with a tag we did not strip is a harness message.
	if let Some(rest) = stripped.strip_prefix('<') {
		let name: String =
			rest.chars().take_while(|c| c.is_ascii_lowercase() || matches!(c, '-' | '_')).collect();
		if name.len() >= 3 && rest[name.len()..].starts_with('>') {
			return None;
		}
	}
	Some(stripped.chars().take(HUMAN_TEXT_CHARS).collect())
}

const NOT_CORRECTIONS: &[&str] = &[
	"no problem",
	"no worries",
	"no rush",
	"no need",
	"no issue",
	"no thanks",
	"never mind",
	"nevermind",
	"non c'è problema",
	"nessun problema",
	"no grazie",
];

/// A message that opens with one of these is correcting something.
const CORRECTION_STARTS: &[&str] = &[
	"no",
	"nope",
	"wrong",
	"stop",
	"don't",
	"dont",
	"do not",
	"never",
	"actually",
	"wait",
	"not that",
	"that's wrong",
	"that is wrong",
	"that's not",
	"that is not",
	"i said",
	"i told you",
	"i asked",
	"i meant",
	"please don't",
	"please do not",
	"why did you",
	"why would you",
	"sbagliato",
	"invece",
	"mai",
	"non fare",
	"non usare",
	"non devi",
	"non deve",
	"non va bene",
	"ti ho detto",
	"ti avevo detto",
	"perché hai",
	"perche hai",
];

/// ...and one of these anywhere near the top of a message says the same.
const CORRECTION_PHRASES: &[&str] = &[
	"you should",
	"you shouldn't",
	"you must",
	"you need to",
	"should not",
	"shouldn't",
	"must not",
	"mustn't",
	"don't use",
	"do not use",
	"don't add",
	"don't change",
	"don't touch",
	"don't modify",
	"don't create",
	"don't write",
	"instead of",
	"rather than",
	"that's wrong",
	"that is wrong",
	"this is wrong",
	"not what i",
	"i didn't ask",
	"i did not ask",
	"i told you",
	"i said",
	"always",
	"never",
	"from now on",
	"next time",
	"every time",
	"remember to",
	"stop using",
	"stop doing",
	"non devi",
	"non dovresti",
	"non usare",
	"non fare",
	"invece di",
	"usa invece",
	"è sbagliato",
	"e sbagliato",
	"sbagliato",
	"devi sempre",
	"sempre",
	"mai",
	"da ora in poi",
	"la prossima volta",
	"ogni volta",
	"ricordati",
	"ti ho detto",
	"non l'ho chiesto",
];

fn looks_like_correction(text: &str) -> bool {
	let lower = text.replace(['\u{2019}', '\u{2018}'], "'").to_lowercase();
	let trimmed = lower.trim();
	let length = trimmed.chars().count();
	// A very long message is a pasted plan or log, not a correction.
	if length == 0 || length > 2000 {
		return false;
	}
	let head: String = trimmed.chars().take(600).collect();
	if NOT_CORRECTIONS.iter().any(|p| starts_with_phrase(&head, p)) {
		return false;
	}
	CORRECTION_STARTS.iter().any(|p| starts_with_phrase(&head, p))
		|| CORRECTION_PHRASES.iter().any(|p| contains_phrase(&head, p))
}

fn starts_with_phrase(haystack: &str, phrase: &str) -> bool {
	haystack
		.strip_prefix(phrase)
		.is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_alphanumeric()))
}

fn contains_phrase(haystack: &str, phrase: &str) -> bool {
	let mut from = 0;
	while let Some(at) = haystack[from..].find(phrase) {
		let start = from + at;
		let end = start + phrase.len();
		let before = haystack[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
		let after = haystack[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
		if before && after {
			return true;
		}
		from = end;
	}
	false
}

// ── Which commands verify something ────────────────────────────────────────

const VERIFY_WORDS: &[&str] = &[
	"test",
	"tests",
	"build",
	"lint",
	"check",
	"typecheck",
	"type-check",
	"clippy",
	"verify",
	"ci",
	"e2e",
	"vet",
	"compile",
	"coverage",
];

/// Tools that are a check in themselves.
const DIRECT_TOOLS: &[&str] = &[
	"pytest",
	"jest",
	"vitest",
	"tsc",
	"eslint",
	"oxlint",
	"ruff",
	"mypy",
	"flake8",
	"pylint",
	"rspec",
	"tox",
	"phpunit",
	"golangci-lint",
	"shellcheck",
	"rubocop",
	"playwright",
	"cypress",
	"mocha",
	"unittest",
];

/// Tools that run a named task: `pnpm test`, `cargo clippy`, `make build`.
const RUNNERS: &[&str] = &[
	"pnpm", "npm", "yarn", "bun", "cargo", "go", "make", "mise", "just", "poetry", "uv", "mvn",
	"gradle", "gradlew", "dotnet", "mix", "rake", "bundle", "deno", "swift", "dart", "flutter",
	"python", "python3", "node", "npx", "turbo", "nx", "lerna", "bazel", "cmake", "ctest",
	"pipenv", "hatch",
];

const WRAPPERS: &[&str] = &["sudo", "time", "env", "nice", "command", "exec"];

/// `(tool, task)` when the command line runs a test, build or lint; the task is
/// what a re-run is matched on.
fn verification_class(command: &str) -> Option<String> {
	let lower = command.to_lowercase();
	for segment in lower.split([';', '|', '&', '\n']) {
		// A quoted string is a message or an argument, not a task name.
		let segment = segment.split(['"', '\'']).next().unwrap_or_default();
		let mut words = segment.split_whitespace().peekable();
		while let Some(word) = words.peek() {
			let is_assignment = word.contains('=') && !word.starts_with(['/', '.', '-']);
			if is_assignment || WRAPPERS.contains(word) {
				words.next();
			} else {
				break;
			}
		}
		let Some(first) = words.next() else { continue };
		let first = first.rsplit(['/', '\\']).next().unwrap_or(first);
		let first = first.trim_end_matches(".exe").trim_end_matches(".cmd");
		if DIRECT_TOOLS.contains(&first) {
			return Some(first.to_string());
		}
		if !RUNNERS.contains(&first) {
			continue;
		}
		for word in words {
			let task = word.split(':').next().unwrap_or(word);
			if VERIFY_WORDS.contains(&task) || DIRECT_TOOLS.contains(&word) {
				return Some(format!("{first} {word}"));
			}
		}
	}
	None
}

/// The last few lines of a failing command's output, where the cause is.
fn error_tail(text: &str) -> String {
	let clean = redact::strip_ansi(text);
	let lines: Vec<&str> = clean.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
	if lines.is_empty() {
		return "(no output recorded)".to_string();
	}
	let start = lines.len().saturating_sub(4);
	lines[start..].join(" | ")
}

// ── The rules ──────────────────────────────────────────────────────────────

fn normalise_for_fingerprint(text: &str) -> String {
	let lowered: String = text
		.chars()
		.flat_map(char::to_lowercase)
		.map(|c| if c.is_ascii_digit() { '#' } else { c })
		.collect();
	lowered.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(500).collect()
}

fn fingerprint(parts: &[&str]) -> String {
	let joined =
		parts.iter().map(|p| normalise_for_fingerprint(p)).collect::<Vec<_>>().join("\u{1f}");
	sha256_hex(joined.as_bytes())
}

struct HumanRecord {
	tokens: BTreeSet<String>,
	line: u64,
}

struct Pending {
	class: String,
	failure_fingerprint: String,
	calls_since: usize,
	retry: Option<(String, Loc)>,
}

#[derive(Default)]
struct Extractor {
	humans_seen: usize,
	humans: Vec<HumanRecord>,
	last_assistant: Option<String>,
	/// Verification calls awaiting their result: call id -> (command, class).
	calls: HashMap<String, (String, String)>,
	call_order: VecDeque<String>,
	pending: Option<Pending>,
	signals: Vec<RawSignal>,
	seen: HashSet<String>,
	counts: [usize; 4],
}

impl Extractor {
	fn feed(&mut self, event: Ev) {
		match event {
			Ev::Human { text, loc } => self.human(&text, &loc),
			Ev::Assistant { summary } => self.last_assistant = Some(summary),
			Ev::ToolCall { call_id, command, loc } => self.tool_call(call_id, command, loc),
			Ev::ToolResult { call_id, failed, text, loc } => {
				self.tool_result(&call_id, failed, &text, &loc)
			}
		}
	}

	fn finish(mut self) -> Vec<RawSignal> {
		// A retry whose result never arrived is reported as exactly that: nothing is
		// inferred from silence.
		if let Some(pending) = self.pending.take() {
			if let Some((_, loc)) = pending.retry {
				let fp = pending.failure_fingerprint;
				self.push(
					SignalType::RetryAfterFailure,
					format!(
						"A check was run again after failing ({}); no result was recorded for the re-run",
						pending.class
					),
					None,
					&loc,
					&["retry", &pending.class, "no_result", &fp],
				);
			}
		}
		self.signals
	}

	fn push(
		&mut self,
		signal_type: SignalType,
		excerpt: String,
		context: Option<String>,
		loc: &Loc,
		fingerprint_parts: &[&str],
	) {
		if excerpt.is_empty() {
			return;
		}
		let (slot, cap) = match signal_type {
			SignalType::UserCorrection => (0, CAP_CORRECTION),
			SignalType::ToolFailure => (1, CAP_FAILURE),
			SignalType::RetryAfterFailure => (2, CAP_RETRY),
			SignalType::RepeatedInstruction => (3, CAP_REPEAT),
		};
		let fp = fingerprint(fingerprint_parts);
		// Within a session the same observation is one observation.
		if self.counts[slot] >= cap || self.seen.contains(&fp) {
			return;
		}
		self.seen.insert(fp.clone());
		self.counts[slot] += 1;
		self.signals.push(RawSignal {
			signal_type,
			excerpt,
			context,
			locator: loc.locator(),
			fingerprint: fp,
			observed_at: loc.ts,
		});
	}

	fn human(&mut self, text: &str, loc: &Loc) {
		self.humans_seen += 1;
		// The opening message is the task. It can repeat later; it is not a
		// correction of anything.
		let first = self.humans_seen == 1;
		let normalised = normalise_for_fingerprint(text);
		if !first && looks_like_correction(text) {
			let context = self.last_assistant.as_deref().map(|s| redact::excerpt(s, CONTEXT_CHARS));
			self.push(
				SignalType::UserCorrection,
				redact::excerpt(text, EXCERPT_CHARS),
				context.filter(|c| !c.is_empty()),
				loc,
				&["user_correction", &normalised],
			);
		}
		let tokens = dedupe::tokens(text);
		if tokens.len() >= 3 {
			let earlier = self
				.humans
				.iter()
				.find(|h| dedupe::jaccard(&h.tokens, &tokens) >= 0.8)
				.map(|h| h.line);
			if let Some(line) = earlier {
				let key = tokens.iter().cloned().collect::<Vec<_>>().join(" ");
				self.push(
					SignalType::RepeatedInstruction,
					redact::excerpt(text, EXCERPT_CHARS),
					Some(format!("repeats what was said at line {line} of this session")),
					loc,
					&["repeated_instruction", &key],
				);
			}
			if self.humans.len() < MAX_HUMANS_TRACKED {
				self.humans.push(HumanRecord { tokens, line: loc.line });
			}
		}
	}

	fn tool_call(&mut self, call_id: String, command: Option<String>, loc: Loc) {
		if let Some(pending) = self.pending.as_mut() {
			pending.calls_since += 1;
		}
		if self.pending.as_ref().is_some_and(|p| p.calls_since > RETRY_WINDOW && p.retry.is_none())
		{
			self.pending = None;
		}
		let Some(command) = command else { return };
		let Some(class) = verification_class(&command) else { return };
		if let Some(pending) = self.pending.as_mut() {
			if pending.retry.is_none() && pending.class == class {
				pending.retry = Some((call_id.clone(), loc));
			}
		}
		self.calls.insert(call_id.clone(), (command, class));
		self.call_order.push_back(call_id);
		while self.call_order.len() > MAX_CALLS_TRACKED {
			if let Some(old) = self.call_order.pop_front() {
				self.calls.remove(&old);
			}
		}
	}

	fn tool_result(&mut self, call_id: &str, failed: bool, text: &str, loc: &Loc) {
		// Taken, not borrowed: Codex can report one call twice (its structured end
		// event, then the output item) and the second is the same observation.
		let Some((command, class)) = self.calls.remove(call_id) else { return };
		let was_retry = self
			.pending
			.as_ref()
			.and_then(|p| p.retry.as_ref())
			.is_some_and(|(retry_id, _)| retry_id == call_id);
		let earlier_failure = self.pending.as_ref().map(|p| p.failure_fingerprint.clone());
		let shown_command = redact::excerpt(&command, 120);

		if failed {
			let tail = error_tail(text);
			let fp_text = format!(
				"{} {}",
				normalise_for_fingerprint(&command).chars().take(80).collect::<String>(),
				normalise_for_fingerprint(&tail).chars().take(160).collect::<String>()
			);
			self.push(
				SignalType::ToolFailure,
				redact::excerpt(&tail, EXCERPT_CHARS),
				Some(format!("ran: {shown_command}")),
				loc,
				&["tool_failure", &fp_text],
			);
			if was_retry {
				let before = earlier_failure.unwrap_or_default();
				self.push(
					SignalType::RetryAfterFailure,
					format!("Ran `{shown_command}` again after a failure; the re-run failed too"),
					None,
					loc,
					&["retry", &class, "failed_again", &before],
				);
			}
			let failure_fingerprint = fingerprint(&["tool_failure", &fp_text]);
			self.pending =
				Some(Pending { class, failure_fingerprint, calls_since: 0, retry: None });
		} else {
			if was_retry {
				let before = earlier_failure.unwrap_or_default();
				self.push(
					SignalType::RetryAfterFailure,
					format!(
						"Ran `{shown_command}` again after a failure; the re-run reported success"
					),
					None,
					loc,
					&["retry", &class, "passed", &before],
				);
			}
			if self.pending.as_ref().is_some_and(|p| p.class == class) {
				self.pending = None;
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Write;

	const FIXTURES: &str =
		concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/fixtures/intelligence");

	fn fixture(name: &str) -> std::path::PathBuf {
		Path::new(FIXTURES).join(name)
	}

	fn of(signals: &[RawSignal], ty: SignalType) -> Vec<&RawSignal> {
		signals.iter().filter(|s| s.signal_type == ty).collect()
	}

	#[test]
	fn a_claude_correction_is_found_and_a_tool_result_is_not_a_person() {
		let signals = extract_session("claude", &fixture("claude-session-a.jsonl")).unwrap();
		let corrections = of(&signals, SignalType::UserCorrection);
		assert!(
			corrections.iter().any(|s| s.excerpt.contains("Don't use npm")),
			"{corrections:#?}"
		);
		// The tool result that mentions "never" and "don't" arrives as a `user`
		// record; it must not be read as a person speaking.
		assert!(
			signals.iter().all(|s| !s.excerpt.contains("never run this in production")),
			"{signals:#?}"
		);
		// The opening message says "always" and is the task, not a correction.
		assert!(corrections.iter().all(|s| !s.excerpt.contains("Implement the importer")));
	}

	#[test]
	fn a_correction_points_at_its_line_and_event() {
		let signals = extract_session("claude", &fixture("claude-session-a.jsonl")).unwrap();
		let hit = of(&signals, SignalType::UserCorrection)
			.into_iter()
			.find(|s| s.excerpt.contains("Don't use npm"))
			.unwrap();
		let text = std::fs::read_to_string(fixture("claude-session-a.jsonl")).unwrap();
		let line = text.lines().nth(hit.locator.line as usize - 1).unwrap();
		assert!(line.contains("Don't use npm"), "locator line {} is {line}", hit.locator.line);
		assert_eq!(hit.locator.event_id.as_deref(), Some("u-corr-1"));
		assert!(hit.observed_at.is_some());
	}

	#[test]
	fn a_failing_check_and_its_retry_are_recorded_without_inferring_success() {
		let signals = extract_session("claude", &fixture("claude-session-a.jsonl")).unwrap();
		let failures = of(&signals, SignalType::ToolFailure);
		assert_eq!(failures.len(), 1, "{failures:#?}");
		assert!(failures[0].excerpt.contains("1 failed"), "{}", failures[0].excerpt);
		assert!(failures[0].context.as_deref().unwrap().contains("pnpm test"));
		// ANSI colour is not text.
		assert!(!failures[0].excerpt.contains('\u{1b}'));
		let retries = of(&signals, SignalType::RetryAfterFailure);
		assert_eq!(retries.len(), 1, "{retries:#?}");
		assert!(retries[0].excerpt.contains("reported success"), "{}", retries[0].excerpt);
	}

	#[test]
	fn a_failure_in_a_command_that_is_not_a_check_is_not_a_signal() {
		let signals = extract_session("claude", &fixture("claude-session-a.jsonl")).unwrap();
		assert!(signals.iter().all(|s| !s.excerpt.contains("No such file")), "{signals:#?}");
	}

	#[test]
	fn the_same_instruction_given_twice_in_one_session_is_a_repeat() {
		let signals = extract_session("claude", &fixture("claude-session-a.jsonl")).unwrap();
		let repeats = of(&signals, SignalType::RepeatedInstruction);
		assert_eq!(repeats.len(), 1, "{repeats:#?}");
		assert!(repeats[0].context.as_deref().unwrap().starts_with("repeats what was said"));
	}

	#[test]
	fn sidechain_meta_and_command_records_are_not_people() {
		let signals = extract_session("claude", &fixture("claude-session-a.jsonl")).unwrap();
		for s in &signals {
			for noise in ["sidechain prompt", "local-command-caveat", "/clear", "interrupted"] {
				assert!(!s.excerpt.contains(noise), "{noise} leaked: {s:#?}");
			}
		}
	}

	#[test]
	fn a_codex_rollout_is_read_through_its_own_shapes() {
		let signals = extract_session("codex", &fixture("codex-rollout-a.jsonl")).unwrap();
		let corrections = of(&signals, SignalType::UserCorrection);
		assert!(
			corrections.iter().any(|s| s.excerpt.contains("Non usare npm")),
			"{corrections:#?}"
		);
		// The injected AGENTS.md and environment block are not a person.
		assert!(signals.iter().all(|s| !s.excerpt.contains("environment_context")));
		let failures = of(&signals, SignalType::ToolFailure);
		assert_eq!(failures.len(), 1, "{failures:#?}");
		assert!(failures[0].excerpt.contains("FAILED"), "{}", failures[0].excerpt);
		let retries = of(&signals, SignalType::RetryAfterFailure);
		assert_eq!(retries.len(), 1, "{retries:#?}");
		assert!(retries[0].excerpt.contains("reported success"));
		// Codex writes its own ordinal; it is carried through.
		assert!(failures[0].locator.ordinal.is_some());
	}

	#[test]
	fn a_codex_call_without_a_stated_exit_code_is_not_called_failed() {
		let mut file = tempfile::NamedTempFile::new().unwrap();
		for line in [
			r#"{"type":"response_item","ordinal":1,"payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"run the tests"}]}}"#,
			r#"{"type":"response_item","ordinal":2,"payload":{"type":"function_call","name":"shell","call_id":"c1","arguments":"{\"command\":[\"bash\",\"-lc\",\"cargo test\"]}"}}"#,
			r#"{"type":"response_item","ordinal":3,"payload":{"type":"function_call_output","call_id":"c1","output":"something happened"}}"#,
		] {
			writeln!(file, "{line}").unwrap();
		}
		let signals = extract_session("codex", file.path()).unwrap();
		assert!(signals.is_empty(), "{signals:#?}");
	}

	#[test]
	fn the_same_failure_twice_in_a_session_is_one_signal() {
		let mut file = tempfile::NamedTempFile::new().unwrap();
		writeln!(
			file,
			r#"{{"type":"user","uuid":"u0","message":{{"role":"user","content":"start the work"}}}}"#
		)
		.unwrap();
		for i in 0..3 {
			writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t{i}","name":"Bash","input":{{"command":"cargo test"}}}}]}}}}"#).unwrap();
			writeln!(file, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t{i}","is_error":true,"content":"test result: FAILED. 0 passed; 1 failed"}}]}}}}"#).unwrap();
		}
		let signals = extract_session("claude", file.path()).unwrap();
		assert_eq!(of(&signals, SignalType::ToolFailure).len(), 1, "{signals:#?}");
	}

	#[test]
	fn a_secret_in_a_transcript_never_reaches_a_signal() {
		let mut file = tempfile::NamedTempFile::new().unwrap();
		writeln!(
			file,
			r#"{{"type":"user","uuid":"u0","message":{{"role":"user","content":"start the work"}}}}"#
		)
		.unwrap();
		writeln!(file, r#"{{"type":"user","uuid":"u1","message":{{"role":"user","content":"No, don't hardcode the key. Use sk-ant-api03-abcdefghijklmnopqrstuvwxyz from the env, password=hunter2"}}}}"#).unwrap();
		let signals = extract_session("claude", file.path()).unwrap();
		assert_eq!(signals.len(), 1);
		assert!(!signals[0].excerpt.contains("abcdefghijklmnop"), "{}", signals[0].excerpt);
		assert!(!signals[0].excerpt.contains("hunter2"), "{}", signals[0].excerpt);
		assert!(signals[0].excerpt.contains("[REDACTED"));
	}

	#[test]
	fn a_truncated_last_line_and_garbage_are_skipped() {
		let mut file = tempfile::NamedTempFile::new().unwrap();
		writeln!(file, "not json at all").unwrap();
		writeln!(
			file,
			r#"{{"type":"user","uuid":"u0","message":{{"role":"user","content":"start"}}}}"#
		)
		.unwrap();
		writeln!(
			file,
			r#"{{"type":"user","uuid":"u1","message":{{"role":"user","content":"No, use pnpm instead"}}}}"#
		)
		.unwrap();
		write!(
			file,
			r#"{{"type":"user","uuid":"u2","message":{{"role":"user","content":"No, half writ"#
		)
		.unwrap();
		let signals = extract_session("claude", file.path()).unwrap();
		assert_eq!(signals.len(), 1, "{signals:#?}");
	}

	#[test]
	fn an_unknown_agent_yields_nothing_and_a_missing_file_is_an_error() {
		assert!(extract_session("gemini", &fixture("claude-session-a.jsonl")).unwrap().is_empty());
		assert!(extract_session("claude", &fixture("does-not-exist.jsonl")).is_err());
	}

	#[test]
	fn evidence_ids_are_stable_uuids_that_depend_on_project_session_and_fingerprint() {
		let a = evidence_id("p1", "s1", "fp");
		assert_eq!(a, evidence_id("p1", "s1", "fp"));
		assert_ne!(a, evidence_id("p2", "s1", "fp"));
		assert_ne!(a, evidence_id("p1", "s2", "fp"));
		assert_ne!(a, evidence_id("p1", "s1", "fp2"));
		let parsed = uuid::Uuid::parse_str(&a).unwrap();
		assert_eq!(parsed.get_version_num(), 5);
	}

	#[test]
	fn corrections_are_recognised_in_english_and_italian_and_not_in_courtesy() {
		for yes in [
			"No, that's the wrong file",
			"Don't use npm here",
			"Actually, put it in src/lib",
			"you should run the formatter first",
			"Always run pnpm lint before you finish",
			"Non usare npm, usa pnpm",
			"Invece usa il service layer",
			"Sbagliato: i test vanno in tests/",
			"è sbagliato, mai chiamare l'API dai componenti",
			"I told you to keep the old API",
		] {
			assert!(looks_like_correction(yes), "{yes}");
		}
		for no in [
			"No problem, carry on",
			"never mind, I found it",
			"Please add a unit test for the parser",
			"looks good, thanks",
			"Implement the importer for CSV files",
			"",
		] {
			assert!(!looks_like_correction(no), "{no}");
		}
		assert!(!looks_like_correction(&format!("No. {}", "x".repeat(3000))));
	}

	#[test]
	fn word_boundaries_stop_substring_matches() {
		assert!(!looks_like_correction("the mailbox is full"));
		assert!(!looks_like_correction("I have alwaysdata hosting"));
		assert!(!looks_like_correction("nonetheless, continue"));
	}

	#[test]
	fn only_test_build_and_lint_commands_are_checks() {
		for yes in [
			"pnpm test",
			"pnpm --filter @a/b test:unit",
			"cargo clippy --all-targets -- -D warnings",
			"cd app && npm run build",
			"npx vitest run",
			"go test ./...",
			"python -m pytest -x",
			"FOO=1 make test",
			"pytest",
			"./gradlew test",
		] {
			assert!(verification_class(yes).is_some(), "{yes}");
		}
		for no in [
			"git commit -m \"fix test\"",
			"ls -la",
			"pnpm install",
			"cat README.md",
			"echo build",
			"cargo run",
			"git status && git diff",
		] {
			assert!(verification_class(no).is_none(), "{no}: {:?}", verification_class(no));
		}
		assert_eq!(verification_class("pnpm test").as_deref(), Some("pnpm test"));
		assert_eq!(verification_class("sudo cargo clippy").as_deref(), Some("cargo clippy"));
	}

	#[test]
	fn human_text_rejects_what_only_looks_like_a_person() {
		assert!(human_text("<command-name>/clear</command-name>").is_none());
		assert!(human_text("<bash-input>git pull</bash-input>").is_none());
		assert!(human_text("[Request interrupted by user for tool use]").is_none());
		assert!(human_text("<system-reminder>be careful</system-reminder>").is_none());
		assert!(human_text("<task-notification>done</task-notification>").is_none());
		assert!(
			human_text("This session is being continued from a previous conversation").is_none()
		);
		assert_eq!(
			human_text("<system-reminder>x</system-reminder>No, use pnpm").as_deref(),
			Some("No, use pnpm")
		);
		assert_eq!(human_text("  plain words ").as_deref(), Some("plain words"));
	}

	#[test]
	fn a_long_head_and_tail_text_keeps_both_ends() {
		let text = format!("START{}END", "x".repeat(5000));
		let b = bounded(&text, 200);
		assert!(b.starts_with("START") && b.ends_with("END"), "{b}");
		assert!(b.chars().count() < 260);
	}
}
