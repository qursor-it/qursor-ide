//! Plan limits exposed to sessions over MCP (F50).

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::{now_ms, store::UsageStore, STALE_MS};
use crate::agents;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::{ModelLimit, UsageReading, UsageSource, UsageWindow};
use crate::services::{profiles, vault::Vault};

const GO_USAGE_URL: &str = "https://opencode.ai/inference/go/v1/usage";
const GO_CACHE_MS: i64 = 5 * 60 * 1000;

const GO_VAULT_ID: &str = "usage:opencode-go";
const GO_KEY_ENV: &str = "QURSOR_OPENCODE_GO_API_KEY";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GoKeySource {
	Saved,
	Environment,
}

fn valid_key(key: &str) -> bool {
	!key.is_empty() && key.len() <= 4096 && key.bytes().all(|b| b.is_ascii_graphic())
}

fn environment_key() -> Option<Zeroizing<String>> {
	std::env::var(GO_KEY_ENV).ok().map(Zeroizing::new).filter(|key| valid_key(key.trim()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GoStatus {
	NoKey,
	Available,
	RejectedKey,
	MissingEntitlement,
	Unavailable,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoKeyUpdate {
	pub source: Option<GoKeySource>,
	pub status: GoStatus,
}

pub struct GoUsage {
	vault: Arc<Vault>,
	cache: Mutex<Option<(i64, Result<UsageReading, GoStatus>)>>,
}

impl GoUsage {
	pub fn new(vault: Arc<Vault>) -> Self {
		Self { vault, cache: Mutex::default() }
	}

	pub fn key_source(&self) -> Option<GoKeySource> {
		if self.vault.has(GO_VAULT_ID) {
			Some(GoKeySource::Saved)
		} else {
			environment_key().map(|_| GoKeySource::Environment)
		}
	}

	pub fn set_key(&self, key: Option<&str>, store: &UsageStore) -> AppResult<Option<GoKeySource>> {
		let mut cache = self.cache.lock();
		if let Some(key) = key {
			let key = key.trim();
			if !valid_key(key) {
				return Err(AppError::InvalidInput("enter a valid OpenCode Go API key".into()));
			}
			self.vault.put(GO_VAULT_ID, key.as_bytes())?;
		} else {
			self.vault.remove(GO_VAULT_ID)?;
		}
		*cache = None;
		store.clear_go_except(None);
		Ok(self.key_source())
	}

	pub fn sync(&self, db: &Db, store: &UsageStore) -> GoStatus {
		let mut cache = self.cache.lock();
		let profile = match db.read(|conn| profiles::default_for(conn, agents::OPENCODE)) {
			Ok(Some(profile)) => profile,
			Ok(None) if self.key_source().is_none() => return GoStatus::NoKey,
			Ok(None) => {
				let descriptor =
					agents::descriptor(agents::OPENCODE).expect("OpenCode is in the registry");
				match profiles::ensure_default_for(
					db,
					agents::OPENCODE,
					&agents::ambient_dir(descriptor),
				) {
					Ok(profile) => profile,
					Err(_) => return GoStatus::Unavailable,
				}
			}
			Err(_) => return GoStatus::Unavailable,
		};
		let status = match self.read(&mut cache, now_ms()) {
			Ok(reading) => {
				store.record(agents::OPENCODE, &profile, reading);
				GoStatus::Available
			}
			Err(status) => status,
		};
		store.clear_go_except((status == GoStatus::Available).then_some(profile.id.as_str()));
		status
	}

	fn read(
		&self,
		cache: &mut Option<(i64, Result<UsageReading, GoStatus>)>,
		now: i64,
	) -> Result<UsageReading, GoStatus> {
		let stored = self.vault.get(GO_VAULT_ID).map_err(|_| GoStatus::Unavailable)?;
		let fallback = stored.is_none().then(environment_key).flatten();
		let key = stored
			.as_ref()
			.and_then(|v| std::str::from_utf8(v).ok())
			.or_else(|| fallback.as_ref().map(|key| key.trim()))
			.ok_or(GoStatus::NoKey)?;
		if let Some((observed, reading)) = cache.as_ref() {
			if now.saturating_sub(*observed) < GO_CACHE_MS {
				return reading.clone();
			}
		}
		let authorization = Zeroizing::new(format!("Bearer {key}"));
		let reading = match ureq::AgentBuilder::new()
			.redirects(0)
			.build()
			.get(GO_USAGE_URL)
			.set("Authorization", &authorization)
			.timeout(Duration::from_secs(5))
			.call()
		{
			Ok(response) if response.status() == 200 => response
				.into_string()
				.ok()
				.and_then(|body| serde_json::from_str(&body).ok())
				.and_then(|body| parse_go(&body, now))
				.ok_or(GoStatus::Unavailable),
			Err(ureq::Error::Status(401, _)) => Err(GoStatus::RejectedKey),
			Err(ureq::Error::Status(403, _)) => Err(GoStatus::MissingEntitlement),
			_ => Err(GoStatus::Unavailable),
		};
		*cache = Some((now, reading.clone()));
		reading
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
	if filter.is_none_or(|id| id == agents::OPENCODE) {
		go.sync(db, store);
	}
	let now = now_ms();
	let snapshot = store.observed_snapshot();
	let mut limits = Vec::new();
	for agent in agents::registry().iter().filter(|agent| filter.is_none_or(|id| id == agent.id)) {
		let profile = db.with(|conn| Ok(profiles::for_spawn(conn, agent.id, project, None)))?;
		let usage = profile.as_ref().and_then(|profile| {
			snapshot.iter().find(|row| row.profile_id == profile.id && row.agent == agent.id)
		});
		let (observed, source, windows) = if let Some(usage) = usage {
			let windows = usage
				.five_hour
				.iter()
				.map(|w| window("5h", w))
				.chain(usage.weekly.iter().map(|w| window("weekly", w)))
				.chain(usage.per_model.iter().map(|m| {
					let label =
						if usage.source == UsageSource::OpenCodeGo { "monthly" } else { &m.label };
					window(label, &m.window)
				}))
				.collect();
			(Some(usage.updated_at), json!(usage.source), windows)
		} else if agent.id == agents::OPENCODE && profile.as_ref().is_some_and(|p| p.is_default) {
			(None, json!(UsageSource::OpenCodeGo), Vec::new())
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

fn parse_go(value: &Value, observed_at: i64) -> Option<UsageReading> {
	let meter = |key: &str| {
		let entry = &value["usage"][key];
		let used_percent = entry["percent"].as_f64().filter(|p| (0.0..=100.0).contains(p))?;
		let resets_at = entry["resetsAt"]
			.as_str()
			.and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
			.map(|date| date.timestamp_millis());
		Some(UsageWindow { used_percent, resets_at })
	};
	let (five_hour, weekly, monthly) = (meter("rolling"), meter("weekly"), meter("monthly"));
	if five_hour.is_none() && weekly.is_none() && monthly.is_none() {
		return None;
	}
	Some(UsageReading {
		five_hour,
		weekly,
		per_model: monthly
			.into_iter()
			.map(|window| ModelLimit { label: "Monthly".into(), window })
			.collect(),
		source: UsageSource::OpenCodeGo,
		observed_at,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	struct TestKeychain;

	impl crate::services::vault::KeyStore for TestKeychain {
		fn load(&self) -> Result<Option<Vec<u8>>, String> {
			Ok(Some(vec![42; 32]))
		}

		fn save(&self, _: &[u8]) -> Result<(), String> {
			Ok(())
		}

		fn delete(&self) -> Result<(), String> {
			Ok(())
		}
	}

	#[test]
	fn the_key_is_encrypted_and_replacement_or_removal_invalidates_cached_limits() {
		let dir = tempfile::tempdir().expect("temporary profile");
		let path = dir.path().join("secrets.vault");
		let store = UsageStore::load(dir.path().join("usage.json"));
		let open = || Arc::new(Vault::open(path.clone(), Box::new(TestKeychain)).expect("vault"));
		let go = GoUsage::new(open());
		assert!(go.set_key(Some("invalid\r\nheader"), &store).is_err());
		assert_eq!(
			go.set_key(Some("  qa-opencode-token  "), &store).expect("save"),
			Some(GoKeySource::Saved)
		);
		assert_eq!(GoUsage::new(open()).key_source(), Some(GoKeySource::Saved));
		let disk = std::fs::read_to_string(&path).expect("saved vault");
		assert!(!disk.contains("qa-opencode-token"));

		*go.cache.lock() = Some((0, Err(GoStatus::Unavailable)));
		go.set_key(Some("qa-replacement"), &store).expect("replace");
		assert!(go.cache.lock().is_none());
		*go.cache.lock() = Some((0, Err(GoStatus::Unavailable)));
		go.set_key(None, &store).expect("remove");
		assert!(go.cache.lock().is_none());
		assert_ne!(GoUsage::new(open()).key_source(), Some(GoKeySource::Saved));
	}

	#[test]
	fn the_same_go_reading_reaches_the_status_bar_and_mcp() {
		let dir = tempfile::tempdir().expect("temporary directory");
		let db = Db::open(dir.path()).expect("app database");
		let store = UsageStore::load(dir.path().join("usage.json"));
		let go = GoUsage::new(Arc::new(
			Vault::open(dir.path().join("secrets.vault"), Box::new(TestKeychain)).expect("vault"),
		));
		go.set_key(Some("qa-opencode-token"), &store).expect("save");
		let at = now_ms();
		*go.cache.lock() = Some((
			at,
			Ok(parse_go(
				&json!({"usage": {
					"rolling": {"percent": 30}, "weekly": {"percent": 100}, "monthly": {"percent": 50}
				}}),
				at,
			)
			.expect("Go reading")),
		));
		assert_eq!(go.sync(&db, &store), GoStatus::Available);
		let entries = store.observed_snapshot();
		assert_eq!(entries.len(), 1);
		assert_eq!(entries[0].source, UsageSource::OpenCodeGo);
		assert_eq!(entries[0].per_model[0].label, "Monthly");

		let result = read(&db, &store, &go, "test-project", Some(agents::OPENCODE)).expect("MCP");
		let windows = result["limits"][0]["windows"].as_array().expect("windows");
		assert_eq!(windows.len(), 3);
		assert_eq!(windows[0]["remainingPercent"], 70.0);
		assert_eq!(windows[1]["remainingPercent"], 0.0);
		assert_eq!(windows[2]["label"], "monthly");
		assert_eq!(windows[2]["remainingPercent"], 50.0);

		assert!(UsageStore::load(dir.path().join("usage.json")).observed_snapshot().is_empty());
		go.set_key(None, &store).expect("remove");
		assert!(store.observed_snapshot().is_empty());
	}

	#[test]
	fn go_keeps_the_monthly_window_and_ignores_invalid_usage() {
		let result = parse_go(
			&json!({"usage": {
				"rolling": {"percent": 42, "resetsAt": "2026-11-24T12:00:00Z"},
				"weekly": {"percent": -1},
				"monthly": {"percent": 100}
			}}),
			7,
		)
		.expect("Go reading");
		let rolling = window("5h", &result.five_hour.expect("rolling"));
		assert_eq!(rolling["remainingPercent"], 58.0);
		assert!(rolling["resetsAt"].as_i64().is_some());
		assert!(result.weekly.is_none());
		assert_eq!(result.per_model[0].label, "Monthly");
		assert_eq!(result.per_model[0].window.used_percent, 100.0);
	}
}
