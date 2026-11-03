//! The channel this install reports to the UI (F14, ADR-0064) and the update
//! feed it may poll. The feed is only ever the one the build configures
//! (ADR-0087); there is no per-channel endpoint any more.

use crate::db::Db;
use crate::models::SettingKey;
use crate::services::settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
	Stable,
	Alpha,
}

impl Channel {
	/// A stored value as a channel. **Anything unknown is stable**: a value no
	/// release ever wrote must not put somebody on the fast lane.
	pub fn parse(value: Option<&str>) -> Channel {
		match value {
			Some("alpha") => Channel::Alpha,
			_ => Channel::Stable,
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			Channel::Stable => "stable",
			Channel::Alpha => "alpha",
		}
	}
}

/// The channel the setting names, stable when unset or unreadable. A database
/// error here is not worth failing an update check over: the check falls back
/// to the channel every install started on.
pub fn channel(db: &Db) -> Channel {
	let value = db.with(|conn| settings::get(conn, SettingKey::UpdateChannel)).ok().flatten();
	Channel::parse(value.as_deref())
}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::TempDir;

	#[test]
	fn unset_and_unknown_values_are_stable() {
		assert_eq!(Channel::parse(None), Channel::Stable);
		assert_eq!(Channel::parse(Some("stable")), Channel::Stable);
		assert_eq!(Channel::parse(Some("nightly")), Channel::Stable);
		assert_eq!(Channel::parse(Some("")), Channel::Stable);
		assert_eq!(Channel::parse(Some("alpha")), Channel::Alpha);
	}

	#[test]
	fn the_setting_picks_the_channel() {
		let tmp = TempDir::new().expect("tempdir");
		let db = Db::open(&tmp.path().join("data")).expect("open db");
		assert_eq!(channel(&db), Channel::Stable);
		db.with(|conn| settings::set(conn, SettingKey::UpdateChannel, Some("alpha"))).expect("set");
		assert_eq!(channel(&db), Channel::Alpha);
		db.with(|conn| settings::set(conn, SettingKey::UpdateChannel, None)).expect("clear");
		assert_eq!(channel(&db), Channel::Stable);
	}
}

/// A build polls only the signed feed its own config names (ADR-0087). The
/// shipped Windows build names this fork's feed; every other build names none,
/// so it never polls, and the upstream feed must never replace this fork.
pub fn configured_endpoints(
	config: Option<&serde_json::Value>,
) -> crate::error::AppResult<Vec<tauri::Url>> {
	use crate::error::AppError;
	let not_configured = || {
		AppError::InvalidInput(
        "Automatic updates are not configured for this build. Install a newer release manually, or rebuild from source.".into(),
    )
	};
	let config = config.ok_or_else(not_configured)?;
	if config.get("pubkey").and_then(|v| v.as_str()).is_none_or(|key| key.trim().is_empty()) {
		return Err(not_configured());
	}
	let configured =
		config.get("endpoints").and_then(|v| v.as_array()).ok_or_else(not_configured)?;
	if configured.is_empty() {
		return Err(not_configured());
	}
	configured
		.iter()
		.map(|value| {
			let raw = value.as_str().ok_or_else(not_configured)?;
			let url = tauri::Url::parse(raw)
				.map_err(|e| AppError::InvalidInput(format!("update endpoint: {e}")))?;
			if url.scheme() != "https" {
				return Err(AppError::InvalidInput("Update feeds must use HTTPS.".into()));
			}
			if url.host_str() == Some("github.com")
				&& url.path().to_ascii_lowercase().starts_with("/nightbr/factorai/")
			{
				return Err(AppError::InvalidInput(
					"Configure this fork's own native Windows update feed.".into(),
				));
			}
			Ok(url)
		})
		.collect()
}

#[cfg(test)]
mod windows_tests {
	use super::configured_endpoints;
	use serde_json::json;

	#[test]
	fn a_local_build_cannot_check_the_upstream_feed() {
		assert!(configured_endpoints(None).is_err());
		assert!(configured_endpoints(Some(&json!({"pubkey": "", "endpoints": []}))).is_err());
		assert!(configured_endpoints(Some(
			&json!({"pubkey": "synthetic", "endpoints": ["https://github.com/Nightbr/factorai/releases/latest/download/latest.json"]})
		))
		.is_err());
	}

	#[test]
	fn explicit_https_fork_feed_is_used_without_channel_fallback() {
		let config = json!({"pubkey": "synthetic", "endpoints": ["https://example.test/windows/latest.json"]});
		let endpoints = configured_endpoints(Some(&config)).expect("synthetic fork configuration");
		assert_eq!(endpoints[0].as_str(), "https://example.test/windows/latest.json");
		assert!(configured_endpoints(Some(
			&json!({"pubkey": "synthetic", "endpoints": ["http://example.test/latest.json"]})
		))
		.is_err());
	}
}
