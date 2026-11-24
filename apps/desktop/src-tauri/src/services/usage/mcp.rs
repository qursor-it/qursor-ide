//! Plan limits exposed to sessions over MCP (F50).

use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::{now_ms, store::UsageStore, STALE_MS};
use crate::agents;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::UsageWindow;
use crate::services::profiles;

const GO_USAGE_URL: &str = "https://opencode.ai/inference/go/v1/usage";
const GO_CACHE_MS: i64 = 5 * 60 * 1000;

#[derive(Default)]
pub struct GoUsage(Mutex<Option<(i64, Vec<Value>)>>);

impl GoUsage {
	fn read(&self, now: i64) -> (Option<i64>, Vec<Value>) {
		let key =
			std::env::var("QURSOR_OPENCODE_GO_API_KEY").ok().filter(|key| !key.trim().is_empty());
		let Some(key) = key else { return (None, Vec::new()) };
		let mut cache = self.0.lock();
		if let Some((observed, windows)) = cache.as_ref() {
			if now.saturating_sub(*observed) < GO_CACHE_MS {
				return ((!windows.is_empty()).then_some(*observed), windows.clone());
			}
		}
		let reading = ureq::AgentBuilder::new()
			.redirects(0)
			.build()
			.get(GO_USAGE_URL)
			.set("Authorization", &format!("Bearer {key}"))
			.timeout(Duration::from_secs(5))
			.call()
			.ok()
			.filter(|response| response.status() == 200)
			.and_then(|response| response.into_string().ok())
			.and_then(|body| serde_json::from_str(&body).ok());
		let windows = reading.as_ref().map(parse_go).unwrap_or_default();
		*cache = Some((now, windows.clone()));
		((!windows.is_empty()).then_some(now), windows)
	}
}

pub fn read(
	db: &Db,
	store: &UsageStore,
	go: &GoUsage,
	project: &str,
	filter: Option<&str>,
) -> AppResult<Value> {
	if let Some(id) = filter {
		if agents::descriptor(id).is_none() {
			return Err(AppError::InvalidInput(format!("unknown agent {id}")));
		}
	}
	let now = now_ms();
	let snapshot = store.observed_snapshot();
	let mut limits = Vec::new();
	for agent in agents::registry().iter().filter(|agent| filter.is_none_or(|id| id == agent.id)) {
		let profile = db.with(|conn| Ok(profiles::for_spawn(conn, agent.id, project, None)))?;
		let usage = profile.as_ref().and_then(|profile| {
			snapshot.iter().find(|row| row.profile_id == profile.id && row.agent == agent.id)
		});
		let (observed, source, windows) =
			if agent.id == agents::OPENCODE && profile.as_ref().is_some_and(|p| p.is_default) {
				let (observed, windows) = go.read(now);
				(observed, json!("openCodeGo"), windows)
			} else if let Some(usage) = usage {
				let windows = usage
					.five_hour
					.iter()
					.map(|w| window("5h", w))
					.chain(usage.weekly.iter().map(|w| window("weekly", w)))
					.chain(usage.per_model.iter().map(|m| window(&m.label, &m.window)))
					.collect();
				(Some(usage.updated_at), json!(usage.source), windows)
			} else {
				(None, Value::Null, Vec::new())
			};
		let expired_window =
			windows.iter().any(|value| value["resetsAt"].as_i64().is_some_and(|t| t <= now));
		let status = match observed {
			None => "unavailable",
			Some(t) if expired_window || now.saturating_sub(t) >= STALE_MS => "stale",
			Some(_) => "available",
		};
		limits.push(json!({
			"agent": agent.id,
			"profile": profile.map(|p| p.name),
			"source": source,
			"status": status,
			"observedAt": observed,
			"windows": windows,
		}));
	}
	Ok(json!({ "limits": limits }))
}

fn window(label: &str, window: &UsageWindow) -> Value {
	json!({
		"label": label,
		"remainingPercent": (100.0 - window.used_percent).clamp(0.0, 100.0),
		"resetsAt": window.resets_at,
	})
}

fn parse_go(value: &Value) -> Vec<Value> {
	[("rolling", "5h"), ("weekly", "weekly"), ("monthly", "monthly")]
		.into_iter()
		.filter_map(|(key, label)| {
			let entry = &value["usage"][key];
			let used_percent = entry["percent"].as_f64().filter(|p| (0.0..=100.0).contains(p))?;
			let resets_at = entry["resetsAt"]
				.as_str()
				.and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
				.map(|date| date.timestamp_millis());
			Some(window(label, &UsageWindow { used_percent, resets_at }))
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn go_keeps_the_monthly_window_and_ignores_invalid_usage() {
		let result = parse_go(&json!({"usage": {
			"rolling": {"percent": 42, "resetsAt": "2026-11-24T12:00:00Z"},
			"weekly": {"percent": -1},
			"monthly": {"percent": 100}
		}}));
		assert_eq!(result.len(), 2);
		assert_eq!(result[0]["remainingPercent"], 58.0);
		assert!(result[0]["resetsAt"].as_i64().is_some());
		assert_eq!(result[1]["label"], "monthly");
		assert_eq!(result[1]["remainingPercent"], 0.0);
	}
}
