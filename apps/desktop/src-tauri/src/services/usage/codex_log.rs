//! Codex writes its rate limits into every rollout as a `token_count` event
//! (`payload.rate_limits`). Reading the newest one costs no quota.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::now_ms;
use crate::agents::codex::{rollouts, Line};
use crate::models::{UsageReading, UsageSource, UsageWindow};

/// How many of the most recently written rollouts are searched.
const RECENT_ROLLOUTS: usize = 20;
/// Rollout names embed their start time, so the newest files are among the
/// last names; only these are stat-ed (a margin over `RECENT_ROLLOUTS`, since a
/// resumed old session can be written to later than a newer-named one).
const CANDIDATES: usize = 40;
/// First tail read per rollout; widened x4 until a reading is found.
const TAIL: u64 = 256 * 1024;

/// The newest `token_count` with non-null `rate_limits` across the most
/// recently modified rollouts.
pub fn latest_reading(codex_dir: &Path) -> Option<UsageReading> {
	let all = rollouts(codex_dir);
	let skip = all.len().saturating_sub(CANDIDATES);
	let mut files: Vec<(SystemTime, PathBuf)> = all
		.into_iter()
		.skip(skip)
		.filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
		.collect();
	files.sort_by_key(|f| std::cmp::Reverse(f.0));
	files
		.into_iter()
		.take(RECENT_ROLLOUTS)
		.filter_map(|(_, p)| last_reading(&p))
		.max_by_key(|r| r.observed_at)
}

/// The last `token_count` in one rollout that carries rate limits. Reads the
/// tail of the file, widening only when the tail holds none.
fn last_reading(path: &Path) -> Option<UsageReading> {
	let mut file = File::open(path).ok()?;
	let len = file.metadata().ok()?.len();
	let mut window = TAIL;
	loop {
		let start = len.saturating_sub(window);
		file.seek(SeekFrom::Start(start)).ok()?;
		let mut buf = Vec::new();
		(&mut file).take(len - start).read_to_end(&mut buf).ok()?;
		let mut lines = buf.split(|b| *b == b'\n');
		// A tail read begins mid-line; that fragment is not a line.
		if start > 0 {
			lines.next();
		}
		if let Some(r) = lines.rev().find_map(reading_of_line) {
			return Some(r);
		}
		if start == 0 {
			return None;
		}
		window = window.saturating_mul(4);
	}
}

/// The reading in one raw rollout line, if it is a `token_count` with limits.
fn reading_of_line(bytes: &[u8]) -> Option<UsageReading> {
	// Cheap pre-filter: most lines are not token counts.
	if !bytes.windows(b"token_count".len()).any(|w| w == b"token_count") {
		return None;
	}
	let line: Line = serde_json::from_slice(bytes).ok()?;
	if line.kind != "event_msg"
		|| line.payload.get("type").and_then(Value::as_str) != Some("token_count")
	{
		return None;
	}
	let observed_at = line
		.timestamp
		.as_deref()
		.and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
		.map(|t| t.timestamp_millis())
		.unwrap_or_else(now_ms);
	parse_rate_limits(line.payload.get("rate_limits")?, observed_at)
}

/// A `rate_limits` object (`primary` / `secondary`, each with `used_percent`,
/// `window_minutes`, `resets_at` in Unix seconds). Windows are classified by
/// their length, not their slot: Codex puts the weekly window in `primary`
/// when there is no 5-hour one.
pub fn parse_rate_limits(v: &Value, observed_at: i64) -> Option<UsageReading> {
	let mut five_hour = None;
	let mut weekly = None;
	for slot in ["primary", "secondary"] {
		let Some(w) = v.get(slot).filter(|w| w.is_object()) else { continue };
		let Some(used_percent) = w.get("used_percent").and_then(Value::as_f64) else { continue };
		let Some(minutes) = w.get("window_minutes").and_then(Value::as_i64) else { continue };
		let window = UsageWindow {
			used_percent,
			resets_at: w.get("resets_at").and_then(Value::as_i64).map(|s| s * 1000),
		};
		if minutes <= 360 {
			five_hour = five_hour.or(Some(window));
		} else if minutes >= 1440 {
			weekly = weekly.or(Some(window));
		}
	}
	if five_hour.is_none() && weekly.is_none() {
		return None;
	}
	Some(UsageReading {
		five_hour,
		weekly,
		per_model: Vec::new(),
		source: UsageSource::CodexLog,
		observed_at,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/fixtures/codex");

	/// A Codex home holding `fixture` as its only rollout.
	fn home_with(fixture: &Path) -> tempfile::TempDir {
		let home = tempfile::tempdir().unwrap();
		let day = home.path().join("sessions/2026/09/22");
		std::fs::create_dir_all(&day).unwrap();
		std::fs::copy(
			fixture,
			day.join("rollout-2026-09-22T16-11-31-01a0c975-21fa-7e51-ae6f-cb1045c10d51.jsonl"),
		)
		.unwrap();
		home
	}

	fn limits_home() -> tempfile::TempDir {
		home_with(&Path::new(FIXTURES).join("limits/rollout-weekly-in-primary.jsonl"))
	}

	#[test]
	fn reads_both_windows_from_the_repo_fixture() {
		// The repo fixture's last non-null rate_limits: primary is a 10080-minute
		// (weekly) window at 0.0%, resetting at 1790690583 s; no 5-hour window.
		let file = rollouts(Path::new(FIXTURES)).pop().unwrap();
		let home = home_with(&file);
		let r = latest_reading(home.path()).unwrap();
		assert_eq!(r.source, UsageSource::CodexLog);
		assert_eq!(r.five_hour, None);
		assert_eq!(
			r.weekly,
			Some(UsageWindow { used_percent: 0.0, resets_at: Some(1_790_690_583_000) })
		);
	}

	#[test]
	fn classifies_by_window_minutes_not_slot() {
		let r = latest_reading(limits_home().path()).unwrap();
		assert_eq!(r.weekly.unwrap().used_percent, 55.5);
		assert_eq!(r.five_hour, None);
	}

	#[test]
	fn newest_line_wins() {
		let r = latest_reading(limits_home().path()).unwrap();
		assert_eq!(r.weekly.as_ref().unwrap().used_percent, 55.5);
		assert_ne!(
			r.weekly,
			Some(UsageWindow { used_percent: 40.0, resets_at: Some(1_790_600_000_000) })
		);
	}

	#[test]
	fn null_rate_limits_is_none() {
		let v = serde_json::json!(null);
		assert_eq!(parse_rate_limits(&v, 1), None);
		let home = tempfile::tempdir().unwrap();
		let day = home.path().join("sessions/2026/09/22");
		std::fs::create_dir_all(&day).unwrap();
		std::fs::write(
			day.join("rollout-2026-09-22T16-11-31-01a0c975-21fa-7e51-ae6f-cb1045c10d51.jsonl"),
			"{\"timestamp\":\"2026-09-22T14:11:39.137Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":null}}\n",
		)
		.unwrap();
		assert_eq!(latest_reading(home.path()), None);
	}

	#[test]
	fn invalid_utf8_line_does_not_hide_later_readings() {
		let home = tempfile::tempdir().unwrap();
		let day = home.path().join("sessions/2026/09/22");
		std::fs::create_dir_all(&day).unwrap();
		let mut bytes = vec![0xff, 0xfe, b'\n'];
		bytes.extend_from_slice(b"{\"timestamp\":\"2026-09-22T14:11:39.137Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":9.0,\"window_minutes\":300,\"resets_at\":1790000000}}}}
");
		std::fs::write(
			day.join("rollout-2026-09-22T16-11-31-01a0c975-21fa-7e51-ae6f-cb1045c10d51.jsonl"),
			bytes,
		)
		.unwrap();
		let r = latest_reading(home.path()).unwrap();
		assert_eq!(r.five_hour.unwrap().used_percent, 9.0);
	}

	#[test]
	fn reading_before_a_large_tail_is_still_found() {
		let home = tempfile::tempdir().unwrap();
		let day = home.path().join("sessions/2026/09/22");
		std::fs::create_dir_all(&day).unwrap();
		let mut text = String::from("{\"timestamp\":\"2026-09-22T14:11:39.137Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":3.0,\"window_minutes\":300,\"resets_at\":1790000000}}}}
");
		for _ in 0..4 {
			text.push_str(&format!(
				"{{\"type\":\"response_item\",\"payload\":\"{}\"}}
",
				"x".repeat(200_000)
			));
		}
		std::fs::write(
			day.join("rollout-2026-09-22T16-11-31-01a0c975-21fa-7e51-ae6f-cb1045c10d51.jsonl"),
			text,
		)
		.unwrap();
		let r = latest_reading(home.path()).unwrap();
		assert_eq!(r.five_hour.unwrap().used_percent, 3.0);
	}

	#[test]
	fn resets_at_seconds_become_ms() {
		let v = serde_json::json!({"primary":{"used_percent":10.0,"window_minutes":300,"resets_at":1790000000}});
		let r = parse_rate_limits(&v, 7).unwrap();
		assert_eq!(
			r.five_hour,
			Some(UsageWindow { used_percent: 10.0, resets_at: Some(1_790_000_000_000) })
		);
		assert_eq!(r.observed_at, 7);
	}
}
