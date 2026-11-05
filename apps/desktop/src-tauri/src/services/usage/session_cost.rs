//! Tokens and estimated cost of one session, summed from its transcript.
//!
//! Transcripts can be tens of megabytes, so they are read line by line as raw
//! bytes (a line with invalid UTF-8 is tolerated) and only lines that pass a
//! substring pre-filter are parsed as JSON.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use parking_lot::Mutex;
use serde_json::Value;

use super::prices::{price_for, Price};
use crate::models::SessionUsage;

fn contains(hay: &[u8], needle: &[u8]) -> bool {
	hay.windows(needle.len()).any(|w| w == needle)
}

fn tokens(v: &Value, key: &str) -> u64 {
	v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// `write_1h` is the part of `write` that went to the 1-hour cache, billed at
/// twice the input price rather than the 5-minute `cache_write` price.
fn cost(price: Price, input: u64, output: u64, read: u64, write: u64, write_1h: u64) -> f64 {
	let write_1h = write_1h.min(write);
	(input as f64 * price.input
		+ output as f64 * price.output
		+ read as f64 * price.cache_read
		+ (write - write_1h) as f64 * price.cache_write
		+ write_1h as f64 * price.input * 2.0)
		/ 1_000_000.0
}

/// One response's model and `[input, output, cache read, cache write, of which
/// 1-hour cache write]` tokens.
type Entry = (Option<String>, [u64; 5]);

/// What has been folded from a transcript's complete lines so far.
#[derive(Clone)]
enum Acc {
	/// Insertion order is kept (`entries`) so `model` is the last line's, not a
	/// hash order's.
	Claude {
		by_id: HashMap<String, usize>,
		entries: Vec<Entry>,
	},
	Codex {
		model: Option<String>,
		last: Option<Value>,
	},
}

/// Per transcript: the folded state and the byte offset just past its last
/// complete line. The status bar polls an open session's usage every few
/// seconds while the agent appends to a file that can be tens of megabytes, so
/// each poll parses only what was appended since the last one.
static CACHE: LazyLock<Mutex<HashMap<PathBuf, (u64, Acc)>>> =
	LazyLock::new(|| Mutex::new(HashMap::new()));

/// Enough for every session a user flips between; past it the cache is cleared
/// and the next poll of each pays one full read.
const CACHE_ENTRIES: usize = 64;

impl Acc {
	fn line(&mut self, line: &[u8]) {
		match self {
			Acc::Claude { by_id, entries } => claude_line(by_id, entries, line),
			Acc::Codex { model, last } => codex_line(model, last, line),
		}
	}

	fn usage(self) -> SessionUsage {
		match self {
			Acc::Claude { entries, .. } => claude_usage(entries),
			Acc::Codex { model, last } => codex_usage(model, last),
		}
	}
}

/// Folds the lines appended since the cached offset into the cached state and
/// returns the usage. A file shorter than the offset was rewritten and is read
/// from the start. A trailing line without its newline (one still being
/// written, or a file that ends without one) is counted in this result but not
/// in the cache, so it is folded exactly once when it completes.
fn read(path: &Path, empty: Acc) -> SessionUsage {
	let Ok(file) = File::open(path) else { return empty.usage() };
	let len = file.metadata().map_or(0, |m| m.len());
	let cached = CACHE.lock().remove(path);
	let fresh = empty.clone();
	let (mut offset, mut acc) = match cached {
		Some((offset, acc))
			if offset <= len && std::mem::discriminant(&acc) == std::mem::discriminant(&empty) =>
		{
			(offset, acc)
		}
		_ => (0, empty),
	};
	let mut reader = BufReader::new(file);
	if reader.seek(SeekFrom::Start(offset)).is_err() {
		(offset, acc) = (0, fresh);
		let _ = reader.rewind();
	}
	let mut buf = Vec::new();
	let mut tail = None;
	loop {
		buf.clear();
		match reader.read_until(b'\n', &mut buf) {
			Ok(0) | Err(_) => break,
			Ok(n) if buf.ends_with(b"\n") => {
				acc.line(&buf);
				offset += n as u64;
			}
			Ok(_) => {
				tail = Some(buf.clone());
				break;
			}
		}
	}
	let result = match tail {
		Some(line) => {
			let mut with_tail = acc.clone();
			with_tail.line(&line);
			with_tail
		}
		None => acc.clone(),
	};
	let mut cache = CACHE.lock();
	if cache.len() >= CACHE_ENTRIES {
		cache.clear();
	}
	cache.insert(path.to_path_buf(), (offset, acc));
	drop(cache);
	result.usage()
}

/// A Claude transcript. Claude writes one `assistant` line per content block of
/// a response, each carrying the same `message.id` and usage, so a response is
/// counted once (the last line seen for its id).
pub fn claude(path: &Path) -> SessionUsage {
	read(path, Acc::Claude { by_id: HashMap::new(), entries: Vec::new() })
}

fn claude_line(by_id: &mut HashMap<String, usize>, entries: &mut Vec<Entry>, line: &[u8]) {
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
			// The breakdown, when the transcript has one.
			usage.get("cache_creation").map_or(0, |c| tokens(c, "ephemeral_1h_input_tokens")),
		],
	);
	match message.get("id").and_then(Value::as_str) {
		Some(id) => match by_id.get(id) {
			Some(&i) => entries[i] = entry,
			None => {
				by_id.insert(id.to_owned(), entries.len());
				entries.push(entry);
			}
		},
		None => entries.push(entry),
	}
}

fn claude_usage(entries: Vec<Entry>) -> SessionUsage {
	let mut out = SessionUsage::default();
	let mut total_cost = 0.0;
	let mut unknown = false;
	for (model, [input, output, read, write, write_1h]) in entries {
		out.input_tokens += input;
		out.output_tokens += output;
		out.cache_read_tokens += read;
		out.cache_write_tokens += write;
		match model.as_deref().and_then(price_for) {
			Some(price) => total_cost += cost(price, input, output, read, write, write_1h),
			// Synthetic and zero-usage lines (API errors, interrupts) cost nothing.
			None if model.as_deref() == Some("<synthetic>")
				|| input + output + read + write == 0 => {}
			None => unknown = true,
		}
		if model.is_some() && model.as_deref() != Some("<synthetic>") {
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
	read(path, Acc::Codex { model: None, last: None })
}

fn codex_line(model: &mut Option<String>, last: &mut Option<Value>, line: &[u8]) {
	let is_ctx = contains(line, b"\"turn_context\"");
	if !is_ctx && !contains(line, b"\"token_count\"") {
		return;
	}
	let Ok(v) = serde_json::from_slice::<Value>(line) else { return };
	let payload = v.get("payload").unwrap_or(&Value::Null);
	if is_ctx && v.get("type").and_then(Value::as_str) == Some("turn_context") {
		if let Some(m) = payload.get("model").and_then(Value::as_str) {
			*model = Some(m.to_owned());
		}
	} else if payload.get("type").and_then(Value::as_str) == Some("token_count") {
		if let Some(total) = payload.get("info").and_then(|i| i.get("total_token_usage")) {
			*last = Some(total.clone());
		}
	}
}

fn codex_usage(model: Option<String>, last: Option<Value>) -> SessionUsage {
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
		.map(|p| cost(p, out.input_tokens, out.output_tokens, out.cache_read_tokens, 0, 0));
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
	fn one_hour_cache_writes_are_priced_at_twice_input() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("s.jsonl");
		std::fs::write(
			&path,
			r#"{"type":"assistant","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":1000,"cache_creation":{"ephemeral_5m_input_tokens":400,"ephemeral_1h_input_tokens":600}}}}"#,
		)
		.unwrap();
		let u = claude(&path);
		assert_eq!(u.cache_write_tokens, 1000);
		// sonnet 4.5: 400 * 3.75 (5-minute write) + 600 * 3 * 2 (1-hour write).
		let expected = (400.0 * 3.75 + 600.0 * 6.0) / 1_000_000.0;
		assert!((u.cost_usd.unwrap() - expected).abs() < 1e-12, "{:?}", u.cost_usd);
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
	fn synthetic_zero_usage_lines_do_not_poison_the_cost() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("s.jsonl");
		let lines = [
			r#"{"type":"assistant","message":{"id":"a","model":"claude-opus-4-8","usage":{"input_tokens":1,"output_tokens":1}}}"#,
			r#"{"type":"assistant","message":{"model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}}}"#,
			r#"{"type":"assistant","message":{"id":"b","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":1,"output_tokens":1}}}"#,
			r#"{"type":"assistant","message":{"model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}}}"#,
		];
		std::fs::write(
			&path,
			lines.join(
				"
",
			),
		)
		.unwrap();
		let u = claude(&path);
		assert!(u.cost_usd.is_some());
		// The last real model, deterministically.
		assert_eq!(u.model.as_deref(), Some("claude-sonnet-4-5-20250929"));
		assert_eq!(claude(&path).model, u.model);
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

	#[test]
	fn appended_lines_are_counted_once_across_reads() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("s.jsonl");
		let line = |id: &str, out: u64| {
			format!(
				r#"{{"type":"assistant","message":{{"id":"{id}","model":"claude-opus-4-8","usage":{{"input_tokens":1,"output_tokens":{out}}}}}}}"#
			)
		};
		// The second line is still being written: no newline yet.
		std::fs::write(&path, format!("{}\n{}", line("a", 1), line("b", 2))).unwrap();
		assert_eq!(claude(&path).output_tokens, 3);
		// It completes, then a later block of the same response replaces it.
		std::fs::write(&path, format!("{}\n{}\n{}\n", line("a", 1), line("b", 2), line("b", 5)))
			.unwrap();
		assert_eq!(claude(&path).output_tokens, 6);
		assert_eq!(claude(&path).output_tokens, 6);
		// A rewrite shorter than the cached offset starts over.
		std::fs::write(&path, format!("{}\n", line("c", 4))).unwrap();
		assert_eq!(claude(&path).output_tokens, 4);
	}
}
