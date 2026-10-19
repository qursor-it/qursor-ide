//! Tokens and estimated cost of one session, summed from its transcript.
//!
//! Transcripts can be tens of megabytes, so they are read line by line as raw
//! bytes (a line with invalid UTF-8 is tolerated) and only lines that pass a
//! substring pre-filter are parsed as JSON.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use super::prices::{price_for, Price};
use crate::models::SessionUsage;

fn contains(hay: &[u8], needle: &[u8]) -> bool {
	hay.windows(needle.len()).any(|w| w == needle)
}

/// Each line of the file, as raw bytes. An unreadable file reads as empty.
fn for_each_line(path: &Path, mut f: impl FnMut(&[u8])) {
	let Ok(file) = File::open(path) else { return };
	let mut reader = BufReader::new(file);
	let mut buf = Vec::new();
	loop {
		buf.clear();
		match reader.read_until(b'\n', &mut buf) {
			Ok(0) | Err(_) => break,
			Ok(_) => f(&buf),
		}
	}
}

fn tokens(v: &Value, key: &str) -> u64 {
	v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn cost(price: Price, input: u64, output: u64, read: u64, write: u64) -> f64 {
	(input as f64 * price.input
		+ output as f64 * price.output
		+ read as f64 * price.cache_read
		+ write as f64 * price.cache_write)
		/ 1_000_000.0
}

/// One response's model and `[input, output, cache read, cache write]` tokens.
type Entry = (Option<String>, [u64; 4]);

/// A Claude transcript. Claude writes one `assistant` line per content block of
/// a response, each carrying the same `message.id` and usage, so a response is
/// counted once (the last line seen for its id).
pub fn claude(path: &Path) -> SessionUsage {
	let mut by_id: HashMap<String, Entry> = HashMap::new();
	let mut anonymous: Vec<Entry> = Vec::new();
	for_each_line(path, |line| {
		if !contains(line, b"\"assistant\"") || !contains(line, b"\"usage\"") {
			return;
		}
		let Ok(v) = serde_json::from_slice::<Value>(line) else { return };
		if v.get("type").and_then(Value::as_str) != Some("assistant") {
			return;
		}
		let Some(message) = v.get("message") else { return };
		let Some(usage) = message.get("usage") else { return };
		let entry = (
			message.get("model").and_then(Value::as_str).map(str::to_owned),
			[
				tokens(usage, "input_tokens"),
				tokens(usage, "output_tokens"),
				tokens(usage, "cache_read_input_tokens"),
				tokens(usage, "cache_creation_input_tokens"),
			],
		);
		match message.get("id").and_then(Value::as_str) {
			Some(id) => {
				by_id.insert(id.to_owned(), entry);
			}
			None => anonymous.push(entry),
		}
	});

	let mut out = SessionUsage::default();
	let mut total_cost = 0.0;
	let mut unknown = false;
	for (model, [input, output, read, write]) in by_id.into_values().chain(anonymous) {
		out.input_tokens += input;
		out.output_tokens += output;
		out.cache_read_tokens += read;
		out.cache_write_tokens += write;
		match model.as_deref().and_then(price_for) {
			Some(price) => total_cost += cost(price, input, output, read, write),
			None => unknown = true,
		}
		if model.is_some() {
			out.model = model;
		}
	}
	out.total_tokens =
		out.input_tokens + out.output_tokens + out.cache_read_tokens + out.cache_write_tokens;
	out.cost_usd = (!unknown).then_some(total_cost);
	out
}

/// A Codex rollout: the last `token_count` event's running total. Codex's
/// `input_tokens` includes the cached part, which is split out and priced as a
/// cache read; reasoning tokens are part of `output_tokens`.
pub fn codex(path: &Path) -> SessionUsage {
	let mut model: Option<String> = None;
	let mut last: Option<Value> = None;
	for_each_line(path, |line| {
		let is_ctx = contains(line, b"\"turn_context\"");
		if !is_ctx && !contains(line, b"\"token_count\"") {
			return;
		}
		let Ok(v) = serde_json::from_slice::<Value>(line) else { return };
		let payload = v.get("payload").unwrap_or(&Value::Null);
		if is_ctx && v.get("type").and_then(Value::as_str) == Some("turn_context") {
			if let Some(m) = payload.get("model").and_then(Value::as_str) {
				model = Some(m.to_owned());
			}
		} else if payload.get("type").and_then(Value::as_str) == Some("token_count") {
			if let Some(total) = payload.get("info").and_then(|i| i.get("total_token_usage")) {
				last = Some(total.clone());
			}
		}
	});

	let mut out = SessionUsage { model, ..SessionUsage::default() };
	if let Some(total) = last {
		let input = tokens(&total, "input_tokens");
		let cached = tokens(&total, "cached_input_tokens").min(input);
		out.input_tokens = input - cached;
		out.cache_read_tokens = cached;
		out.output_tokens = tokens(&total, "output_tokens");
		out.reasoning_tokens = tokens(&total, "reasoning_output_tokens");
		out.total_tokens = input + out.output_tokens;
	}
	out.cost_usd = out
		.model
		.as_deref()
		.and_then(price_for)
		.map(|p| cost(p, out.input_tokens, out.output_tokens, out.cache_read_tokens, 0));
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/fixtures");

	fn claude_fixture() -> std::path::PathBuf {
		Path::new(FIXTURES).join("claude-usage/session.jsonl")
	}

	#[test]
	fn claude_sums_usage_once_per_message_id() {
		let u = claude(&claude_fixture());
		// msg_01A (3 lines) + msg_01B (2 lines), each counted once.
		assert_eq!(u.input_tokens, 14);
		assert_eq!(u.output_tokens, 300);
		assert_eq!(u.cache_write_tokens, 1000);
		assert_eq!(u.cache_read_tokens, 7000);
		assert_eq!(u.total_tokens, 14 + 300 + 1000 + 7000);
	}

	#[test]
	fn claude_prices_by_model() {
		let u = claude(&claude_fixture());
		// sonnet 4.5: 10*3 + 200*15 + 5000*0.30 + 1000*3.75
		// opus 4.8:   4*5  + 100*25 + 2000*0.50
		let expected = (30.0 + 3000.0 + 1500.0 + 3750.0 + 20.0 + 2500.0 + 1000.0) / 1_000_000.0;
		assert!((u.cost_usd.unwrap() - expected).abs() < 1e-12);
	}

	#[test]
	fn unknown_model_has_tokens_but_no_cost() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("s.jsonl");
		let mut data = b"\xff\xfe not utf-8\n".to_vec();
		data.extend_from_slice(
			br#"{"type":"assistant","message":{"id":"m1","model":"mystery-1","usage":{"input_tokens":7,"output_tokens":3}}}"#,
		);
		data.push(b'\n');
		std::fs::write(&path, data).unwrap();
		let u = claude(&path);
		assert_eq!((u.input_tokens, u.output_tokens, u.total_tokens), (7, 3, 10));
		assert_eq!(u.cost_usd, None);
	}

	#[test]
	fn codex_reads_last_total_token_usage() {
		let file =
			crate::agents::codex::rollouts(&Path::new(FIXTURES).join("codex")).pop().unwrap();
		let u = codex(&file);
		// input 15043 (11008 cached), output 5, reasoning 0.
		assert_eq!(u.input_tokens, 15043 - 11008);
		assert_eq!(u.cache_read_tokens, 11008);
		assert_eq!(u.output_tokens, 5);
		assert_eq!(u.reasoning_tokens, 0);
		assert_eq!(u.total_tokens, 15048);
		assert_eq!(u.model.as_deref(), Some("gpt-5.6-sol"));
		let expected = (4035.0 * 4.0 + 5.0 * 20.0 + 11008.0 * 0.40) / 1_000_000.0;
		assert!((u.cost_usd.unwrap() - expected).abs() < 1e-12);
	}
}
