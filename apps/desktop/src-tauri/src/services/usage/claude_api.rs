//! Claude limits from Anthropic's usage endpoint (unofficial, opt-in).
//!
//! `GET /api/oauth/usage` with the profile's own OAuth access token, read from
//! `<config dir>/.credentials.json`. factorai never writes that file and never
//! refreshes the token: an expired one is reported and the profile skipped until
//! Claude Code itself renews it. The token appears in no log line, error string
//! or `Debug` output.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::models::{ModelLimit, UsageReading, UsageSource, UsageWindow};

const URL: &str = "https://api.anthropic.com/api/oauth/usage";
const TIMEOUT: Duration = Duration::from_secs(15);
/// Minimum gap between two requests for one profile.
const BASE_INTERVAL_S: u64 = 300;
const MAX_INTERVAL_S: u64 = 3600;

/// What `.credentials.json` yields.
#[derive(PartialEq, Eq)]
pub enum TokenState {
	Ok(String),
	/// `expiresAt` has passed. Never refreshed by factorai.
	Expired,
	/// No file, unreadable, or no access token in it.
	Missing,
}

// By hand: the derived `Debug` would print the token.
impl fmt::Debug for TokenState {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			TokenState::Ok(_) => f.write_str("Ok(<redacted>)"),
			TokenState::Expired => f.write_str("Expired"),
			TokenState::Missing => f.write_str("Missing"),
		}
	}
}

pub fn read_token(config_dir: &Path, now_ms: i64) -> TokenState {
	let Some(v) = std::fs::read(config_dir.join(".credentials.json"))
		.ok()
		.and_then(|b| serde_json::from_slice::<Value>(&b).ok())
	else {
		return TokenState::Missing;
	};
	let oauth = &v["claudeAiOauth"];
	let Some(token) = oauth["accessToken"].as_str().filter(|t| !t.is_empty()) else {
		return TokenState::Missing;
	};
	if oauth["expiresAt"].as_i64().is_some_and(|at| at <= now_ms) {
		return TokenState::Expired;
	}
	TokenState::Ok(token.to_string())
}

fn iso_ms(v: &Value) -> Option<i64> {
	chrono::DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|d| d.timestamp_millis())
}

fn window(v: &Value) -> Option<UsageWindow> {
	let used_percent = v.get("percent").or_else(|| v.get("utilization"))?.as_f64()?;
	Some(UsageWindow { used_percent, resets_at: v.get("resets_at").and_then(iso_ms) })
}

/// Both response shapes: the legacy `five_hour` / `seven_day` / `seven_day_*`
/// keys, and the `limits[]` array. Unknown keys and kinds are ignored.
pub fn parse_usage_response(v: &Value, observed_at: i64) -> UsageReading {
	let mut five_hour = v.get("five_hour").and_then(window);
	let mut weekly = v.get("seven_day").and_then(window);
	let mut per_model = Vec::new();
	for (key, label) in [("seven_day_opus", "Opus"), ("seven_day_sonnet", "Sonnet")] {
		if let Some(w) = v.get(key).and_then(window) {
			per_model.push(ModelLimit { label: label.into(), window: w });
		}
	}
	for limit in v.get("limits").and_then(Value::as_array).into_iter().flatten() {
		let Some(w) = window(limit) else { continue };
		match limit.get("kind").and_then(Value::as_str) {
			Some("session") => five_hour = five_hour.or(Some(w)),
			Some("weekly_all") => weekly = weekly.or(Some(w)),
			Some("weekly_scoped") => {
				let name = limit["scope"]["model"]["display_name"].as_str();
				if let Some(label) = name.filter(|n| !per_model.iter().any(|m| m.label == *n)) {
					per_model.push(ModelLimit { label: label.into(), window: w });
				}
			}
			_ => {}
		}
	}
	UsageReading { five_hour, weekly, per_model, source: UsageSource::ClaudeApi, observed_at }
}

/// Per-profile pacing: 5 minutes between requests; a 429 waits at least
/// `Retry-After`, doubling on consecutive 429s up to an hour; a 2xx resets.
#[derive(Default)]
pub struct Backoff {
	/// profile id → (next request not before ms, current 429 interval s)
	state: HashMap<String, (i64, u64)>,
}

impl Backoff {
	pub fn due(&self, profile_id: &str, now_ms: i64) -> bool {
		self.state.get(profile_id).is_none_or(|(next, _)| *next <= now_ms)
	}

	pub fn on_result(
		&mut self,
		profile_id: &str,
		now_ms: i64,
		status: u16,
		retry_after_s: Option<u64>,
	) {
		let (wait_s, interval) = if status == 429 {
			let prev = self.state.get(profile_id).map(|(_, i)| *i).filter(|i| *i > 0);
			let interval = prev.map_or(BASE_INTERVAL_S, |i| (i * 2).min(MAX_INTERVAL_S));
			(retry_after_s.unwrap_or(0).max(interval).min(MAX_INTERVAL_S), interval)
		} else {
			// Success or another failure: the base pace, and the doubling restarts.
			(BASE_INTERVAL_S, 0)
		};
		self.state.insert(profile_id.to_string(), (now_ms + wait_s as i64 * 1000, interval));
	}
}

/// What one request came back with.
pub struct Fetched {
	/// HTTP status; `0` when the request never got a response.
	pub status: u16,
	pub retry_after_s: Option<u64>,
	pub body: Option<Value>,
}

/// One blocking request. Nothing here logs or returns the token.
pub fn fetch(token: &str) -> Fetched {
	let result = ureq::get(URL)
		.set("Authorization", &format!("Bearer {token}"))
		.set("anthropic-beta", "oauth-2025-04-20")
		.timeout(TIMEOUT)
		.call();
	let response = match result {
		Ok(r) => r,
		Err(ureq::Error::Status(_, r)) => r,
		Err(_) => return Fetched { status: 0, retry_after_s: None, body: None },
	};
	let status = response.status();
	let retry_after_s = response.header("retry-after").and_then(|h| h.trim().parse().ok());
	let body = (200..300)
		.contains(&status)
		.then(|| response.into_string().ok())
		.flatten()
		.and_then(|b| serde_json::from_str(&b).ok());
	Fetched { status, retry_after_s, body }
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;
	use crate::models::SettingKey;

	#[test]
	fn parses_legacy_shape_with_per_model() {
		let v = json!({
			"five_hour": {"utilization": 12.5, "resets_at": "2026-10-19T15:00:00+00:00"},
			"seven_day": {"utilization": 40.0, "resets_at": "2026-10-24T00:00:00Z"},
			"seven_day_opus": {"utilization": 70.0, "resets_at": null},
			"seven_day_sonnet": {"utilization": 5.0, "resets_at": "2026-10-24T00:00:00Z"},
		});
		let r = parse_usage_response(&v, 77);
		assert_eq!(r.source, UsageSource::ClaudeApi);
		assert_eq!(r.observed_at, 77);
		let five = r.five_hour.unwrap();
		assert_eq!(five.used_percent, 12.5);
		assert_eq!(five.resets_at, Some(1_792_422_000_000));
		assert_eq!(r.weekly.unwrap().used_percent, 40.0);
		let labels: Vec<_> = r.per_model.iter().map(|m| m.label.as_str()).collect();
		assert_eq!(labels, ["Opus", "Sonnet"]);
		assert_eq!(r.per_model[0].window.resets_at, None);
	}

	#[test]
	fn parses_limits_array_shape() {
		let v = json!({"limits": [
			{"kind": "session", "percent": 30.0, "resets_at": "2026-10-19T15:00:00Z"},
			{"kind": "weekly_all", "utilization": 50.0, "resets_at": "2026-10-24T00:00:00Z"},
			{"kind": "weekly_scoped", "percent": 80.0, "resets_at": "2026-10-24T00:00:00Z",
				"scope": {"model": {"display_name": "Opus"}}},
		]});
		let r = parse_usage_response(&v, 1);
		assert_eq!(r.five_hour.unwrap().used_percent, 30.0);
		assert_eq!(r.weekly.unwrap().used_percent, 50.0);
		assert_eq!(r.per_model.len(), 1);
		assert_eq!(r.per_model[0].label, "Opus");
		assert_eq!(r.per_model[0].window.used_percent, 80.0);
	}

	#[test]
	fn unknown_keys_are_ignored() {
		let v = json!({"mystery": {"utilization": 99.0}, "five_hour": {"utilization": 1.0},
			"limits": [{"kind": "something_new", "percent": 9.0}, {"nokind": true}]});
		let r = parse_usage_response(&v, 1);
		assert_eq!(r.five_hour.unwrap().used_percent, 1.0);
		assert!(r.weekly.is_none());
		assert!(r.per_model.is_empty());
	}

	fn creds(dir: &Path, expires_at: i64) {
		let body = json!({"claudeAiOauth": {"accessToken": "tok-secret", "expiresAt": expires_at}});
		std::fs::write(dir.join(".credentials.json"), body.to_string()).unwrap();
	}

	#[test]
	fn valid_token_is_returned_and_never_debug_printed() {
		let tmp = tempfile::tempdir().unwrap();
		creds(tmp.path(), 2_000);
		let state = read_token(tmp.path(), 1_000);
		assert_eq!(state, TokenState::Ok("tok-secret".into()));
		assert!(!format!("{state:?}").contains("tok-secret"));
	}

	#[test]
	fn expired_token_is_reported_not_refreshed() {
		let tmp = tempfile::tempdir().unwrap();
		creds(tmp.path(), 500);
		assert_eq!(read_token(tmp.path(), 1_000), TokenState::Expired);
	}

	#[test]
	fn missing_file_is_missing() {
		let tmp = tempfile::tempdir().unwrap();
		assert_eq!(read_token(tmp.path(), 1_000), TokenState::Missing);
		std::fs::write(tmp.path().join(".credentials.json"), "{}").unwrap();
		assert_eq!(read_token(tmp.path(), 1_000), TokenState::Missing);
	}

	#[test]
	fn backoff_honours_retry_after_and_caps_at_one_hour() {
		let mut b = Backoff::default();
		assert!(b.due("p", 0));
		b.on_result("p", 0, 429, Some(900));
		assert!(!b.due("p", 899_000));
		assert!(b.due("p", 900_000));
		// Retry-After below the base interval does not shorten it.
		let mut b = Backoff::default();
		b.on_result("p", 0, 429, Some(10));
		assert!(!b.due("p", 299_000));
		assert!(b.due("p", 300_000));
		// Consecutive 429s double: 300, 600, 1200, 2400, 3600, 3600.
		let mut b = Backoff::default();
		let mut now = 0;
		let mut waits = Vec::new();
		for _ in 0..6 {
			b.on_result("p", now, 429, None);
			let next = b.state["p"].0;
			waits.push((next - now) / 1000);
			now = next;
		}
		assert_eq!(waits, [300, 600, 1200, 2400, 3600, 3600]);
		b.on_result("p", now, 429, Some(86_400));
		assert_eq!((b.state["p"].0 - now) / 1000, 3600);
		// Profiles are independent.
		assert!(b.due("other", 0));
	}

	#[test]
	fn success_resets_backoff() {
		let mut b = Backoff::default();
		b.on_result("p", 0, 429, None);
		b.on_result("p", 300_000, 429, None);
		b.on_result("p", 900_000, 200, None);
		assert!(!b.due("p", 900_000 + 299_000));
		assert!(b.due("p", 900_000 + 300_000));
		b.on_result("p", 1_200_000, 429, None);
		assert_eq!((b.state["p"].0 - 1_200_000) / 1000, 300);
	}

	#[test]
	fn setting_key_columns_are_stable() {
		assert_eq!(SettingKey::UsageClaudeEndpoint.column(), "usage.claude_endpoint");
		assert_eq!(SettingKey::UsageTaskbarWidget.column(), "usage.taskbar_widget");
		assert_eq!(SettingKey::UsageStatusLine.column(), "usage.statusline");
	}

	/// Run by hand: `cargo test live_claude_usage_endpoint -- --ignored --nocapture`.
	/// Prints only percentages and reset times.
	#[test]
	#[ignore = "calls the real endpoint with this machine's credentials"]
	fn live_claude_usage_endpoint() {
		let dir = dirs::home_dir().unwrap().join(".claude");
		let now = crate::services::usage::now_ms();
		let TokenState::Ok(token) = read_token(&dir, now) else {
			println!("no usable token");
			return;
		};
		let f = fetch(&token);
		println!("status {} retry-after {:?}", f.status, f.retry_after_s);
		if let Some(body) = f.body {
			let r = parse_usage_response(&body, now);
			println!("five_hour {:?}", r.five_hour);
			println!("weekly {:?}", r.weekly);
			for m in r.per_model {
				println!("{} {:?}", m.label, m.window);
			}
		}
	}
}
