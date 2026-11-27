//! Plan limits exposed to sessions over MCP (F50).

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::{now_ms, store::UsageStore, STALE_MS};
use crate::agents;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::{ModelLimit, ProviderUsage, UsageReading, UsageSource, UsageWindow};
use crate::services::{profiles, vault::Vault};

const GO_USAGE_URL: &str = "https://opencode.ai/inference/go/v1/usage";
const GO_CACHE_MS: i64 = 5 * 60 * 1000;

const GO_VAULT_ID: &str = "usage:opencode-go";
pub const GO_KEY_ENV: &str = "QURSOR_OPENCODE_GO_API_KEY";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GoKeySource {
	Saved,
	Environment,
}

fn valid_key(key: &str) -> bool {
	!key.is_empty() && key.len() <= 4096 && key.bytes().all(|b| b.is_ascii_graphic())
}

/// Taken out of the process environment so no child (git, an agent, a version
/// probe) inherits it. The app takes it before it spawns anything and hands it
/// to the core it starts, which takes it again for its own children.
pub fn take_environment_key() -> Option<Zeroizing<String>> {
	let key = std::env::var(GO_KEY_ENV).ok().map(Zeroizing::new);
	std::env::remove_var(GO_KEY_ENV);
	key.filter(|key| valid_key(key.trim()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GoStatus {
	NoKey,
	Available,
	RejectedKey,
	MissingEntitlement,
	Unavailable,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoKeyUpdate {
	pub source: Option<GoKeySource>,
	pub status: GoStatus,
}

/// What the core tells the app about the Go key: the key state, and the
/// reading it produced for the app's own usage store (design §5a: only the
/// core holds the vault the key is in).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoReport {
	pub source: Option<GoKeySource>,
	pub status: GoStatus,
	pub usage: Option<ProviderUsage>,
}

pub struct GoUsage {
	vault: Arc<Vault>,
	env_key: Option<Zeroizing<String>>,
	cache: Mutex<Option<(i64, Result<UsageReading, GoStatus>)>>,
}

impl GoUsage {
	pub fn new(vault: Arc<Vault>) -> Self {
		Self { vault, env_key: take_environment_key(), cache: Mutex::default() }
	}

	pub fn key_source(&self) -> Option<GoKeySource> {
		if self.vault.has(GO_VAULT_ID) {
			Some(GoKeySource::Saved)
		} else {
			self.env_key.as_ref().map(|_| GoKeySource::Environment)
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

	/// `sync`, then where the key comes from and the reading `store` now holds.
	pub fn report(&self, db: &Db, store: &UsageStore) -> GoReport {
		let status = self.sync(db, store);
		let usage =
			store.observed_snapshot().into_iter().find(|u| u.source == UsageSource::OpenCodeGo);
		GoReport { source: self.key_source(), status, usage }
	}

	pub fn sync(&self, db: &Db, store: &UsageStore) -> GoStatus {
		let mut cache = self.cache.lock();
		let profile = match db.read(|conn| profiles::default_for(conn, agents::OPENCODE)) {
			Ok(Some(profile)) => profile,
			// No profile means OpenCode was never seen here; nothing to attribute usage to.
			Ok(None) if self.key_source().is_none() => return GoStatus::NoKey,
			Ok(None) | Err(_) => return GoStatus::Unavailable,
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
		let fallback = stored.is_none().then_some(self.env_key.as_ref()).flatten();
		let key = stored
			.as_ref()
			.and_then(|v| std::str::from_utf8(v).ok())
			.or_else(|| fallback.map(|key| key.trim()))
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

	/// Two vault instances on one file each rewrite it whole from their own
	/// cache, so one writer's secret vanishes under the other's; the core is the
	/// only writer (design §5a). Saving the Go key keeps a server's password
	/// saved before it, and the other way round.
	#[test]
	fn the_go_key_and_server_secrets_share_one_vault() {
		let dir = tempfile::tempdir().expect("temporary directory");
		let path = dir.path().join("secrets.vault");
		let store = UsageStore::load(dir.path().join("usage.json"));
		let vault = Arc::new(Vault::open(path.clone(), Box::new(TestKeychain)).expect("vault"));
		let go = GoUsage::new(vault.clone());
		let password = crate::services::servers::secret_id("s1", "password");
		vault.put(&password, b"hunter2").expect("server secret");
		go.set_key(Some("qa-opencode-token"), &store).expect("go key");
		let other = crate::services::servers::secret_id("s2", "password");
		vault.put(&other, b"hunter3").expect("second server secret");
		let reopened = Vault::open(path, Box::new(TestKeychain)).expect("reopen");
		assert!(reopened.has(&password), "the Go key dropped a server secret saved before it");
		assert!(reopened.has(&other));
		assert!(reopened.has(GO_VAULT_ID), "a server secret dropped the Go key saved before it");
	}

	#[test]
	fn the_same_go_reading_reaches_the_status_bar_and_mcp() {
		let dir = tempfile::tempdir().expect("temporary directory");
		let db = Db::open(dir.path()).expect("app database");
		let store = UsageStore::load(dir.path().join("usage.json"));
		let go = GoUsage::new(Arc::new(
			Vault::open(dir.path().join("secrets.vault"), Box::new(TestKeychain)).expect("vault"),
		));
		profiles::ensure_default_for(&db, agents::OPENCODE, dir.path()).expect("profile");
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
