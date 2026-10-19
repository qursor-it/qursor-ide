//! The latest rate-limit windows per (agent, profile), persisted to
//! `<data_dir>/usage.json` so the status bar has numbers before any source has
//! reported in this run.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tracing::warn;

use super::now_ms;
use crate::models::{Profile, ProviderUsage, UsageReading, UsageWindow};

type OnChange = Arc<dyn Fn(Vec<ProviderUsage>) + Send + Sync>;

pub struct UsageStore {
	path: PathBuf,
	entries: Mutex<HashMap<(String, String), ProviderUsage>>,
	on_change: Mutex<Option<OnChange>>,
	/// Serialises `persist`: two source threads share one temp file.
	persist_lock: Mutex<()>,
}

impl UsageStore {
	/// A missing or unreadable file is an empty store: usage is a cache of what
	/// the sources last said, and they will say it again.
	pub fn load(path: PathBuf) -> Self {
		let list: Vec<ProviderUsage> = std::fs::read(&path)
			.ok()
			.and_then(|b| serde_json::from_slice(&b).ok())
			.unwrap_or_default();
		let entries =
			list.into_iter().map(|u| ((u.agent.clone(), u.profile_id.clone()), u)).collect();
		Self {
			path,
			entries: Mutex::new(entries),
			on_change: Mutex::new(None),
			persist_lock: Mutex::new(()),
		}
	}

	pub fn set_on_change(&self, f: OnChange) {
		*self.on_change.lock() = Some(f);
	}

	/// Merge one reading in. Returns whether anything changed.
	pub fn record(&self, agent: &str, profile: &Profile, reading: UsageReading) -> bool {
		let key = (agent.to_string(), profile.id.clone());
		let list = {
			let mut map = self.entries.lock();
			let prev = map.get(&key);
			if prev.is_some_and(|p| reading.observed_at < p.updated_at) {
				return false;
			}
			let merged = ProviderUsage {
				agent: agent.to_string(),
				profile_id: profile.id.clone(),
				profile_name: profile.name.clone(),
				is_default: profile.is_default,
				five_hour: reading.five_hour.or_else(|| prev.and_then(|p| p.five_hour.clone())),
				weekly: reading.weekly.or_else(|| prev.and_then(|p| p.weekly.clone())),
				per_model: if reading.per_model.is_empty() {
					prev.map(|p| p.per_model.clone()).unwrap_or_default()
				} else {
					reading.per_model
				},
				source: reading.source,
				updated_at: reading.observed_at,
			};
			if prev == Some(&merged) {
				return false;
			}
			map.insert(key, merged);
			sorted(map.values().cloned().collect())
		};
		self.persist();
		// Cloned out so the callback runs with no lock held.
		let cb = self.on_change.lock().clone();
		if let Some(cb) = cb {
			cb(apply_resets(list, now_ms()));
		}
		true
	}

	/// Every entry, with a window whose reset time has passed shown as reset.
	pub fn snapshot(&self, now_ms: i64) -> Vec<ProviderUsage> {
		apply_resets(sorted(self.entries.lock().values().cloned().collect()), now_ms)
	}

	/// Temp file then rename, so a crash mid-write leaves the old file whole.
	/// The list is taken under the persist lock, so a slower writer can never
	/// land an older snapshot over a newer one.
	fn persist(&self) {
		let _guard = self.persist_lock.lock();
		let list = sorted(self.entries.lock().values().cloned().collect());
		let tmp = self.path.with_extension("json.tmp");
		let result = serde_json::to_vec(&list)
			.map_err(std::io::Error::other)
			.and_then(|b| std::fs::write(&tmp, b))
			.and_then(|_| std::fs::rename(&tmp, &self.path));
		if let Err(e) = result {
			warn!(error = %e, "could not persist usage");
		}
	}
}

fn sorted(mut list: Vec<ProviderUsage>) -> Vec<ProviderUsage> {
	list.sort_by(|a, b| (&a.agent, &a.profile_id).cmp(&(&b.agent, &b.profile_id)));
	list
}

fn reset_if_expired(w: &mut Option<UsageWindow>, now_ms: i64) {
	if let Some(w) = w {
		if w.resets_at.is_some_and(|r| r <= now_ms) {
			w.used_percent = 0.0;
			w.resets_at = None;
		}
	}
}

fn apply_resets(mut list: Vec<ProviderUsage>, now_ms: i64) -> Vec<ProviderUsage> {
	for u in &mut list {
		reset_if_expired(&mut u.five_hour, now_ms);
		reset_if_expired(&mut u.weekly, now_ms);
		for m in &mut u.per_model {
			let mut w = Some(m.window.clone());
			reset_if_expired(&mut w, now_ms);
			if let Some(w) = w {
				m.window = w;
			}
		}
	}
	list
}

#[cfg(test)]
mod tests {
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::sync::Arc;

	use super::*;
	use crate::models::{Profile, UsageReading, UsageSource, UsageWindow};

	fn profile() -> Profile {
		Profile {
			id: "p1".into(),
			agent: "claude".into(),
			name: "Main".into(),
			config_dir: "/tmp/x".into(),
			is_default: true,
			is_app_default: true,
			missing: false,
			created_at: 0,
		}
	}

	fn win(p: f64, r: Option<i64>) -> UsageWindow {
		UsageWindow { used_percent: p, resets_at: r }
	}

	fn reading(five: Option<UsageWindow>, week: Option<UsageWindow>, at: i64) -> UsageReading {
		UsageReading {
			five_hour: five,
			weekly: week,
			per_model: vec![],
			source: UsageSource::StatusLine,
			observed_at: at,
		}
	}

	fn store() -> (tempfile::TempDir, std::path::PathBuf, UsageStore) {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("usage.json");
		let s = UsageStore::load(path.clone());
		(dir, path, s)
	}

	#[test]
	fn record_then_snapshot_round_trips() {
		let (_d, _p, s) = store();
		let r = reading(Some(win(62.0, Some(9_000))), Some(win(31.0, Some(99_000))), 100);
		assert!(s.record("claude", &profile(), r));
		let snap = s.snapshot(1_000);
		assert_eq!(snap.len(), 1);
		assert_eq!(snap[0].five_hour.as_ref().unwrap().used_percent, 62.0);
		assert_eq!(snap[0].weekly.as_ref().unwrap().used_percent, 31.0);
		assert_eq!(snap[0].source, UsageSource::StatusLine);
		assert_eq!(snap[0].updated_at, 100);
		assert_eq!(snap[0].profile_name, "Main");
	}

	#[test]
	fn older_reading_is_dropped() {
		let (_d, _p, s) = store();
		assert!(s.record("claude", &profile(), reading(Some(win(50.0, None)), None, 200)));
		assert!(!s.record("claude", &profile(), reading(Some(win(10.0, None)), None, 100)));
		let snap = s.snapshot(0);
		assert_eq!(snap[0].five_hour.as_ref().unwrap().used_percent, 50.0);
		assert_eq!(snap[0].updated_at, 200);
	}

	#[test]
	fn missing_window_keeps_stored_one() {
		let (_d, _p, s) = store();
		s.record("claude", &profile(), reading(Some(win(62.0, None)), Some(win(31.0, None)), 100));
		s.record("claude", &profile(), reading(Some(win(70.0, None)), None, 200));
		let snap = s.snapshot(0);
		assert_eq!(snap[0].five_hour.as_ref().unwrap().used_percent, 70.0);
		assert_eq!(snap[0].weekly.as_ref().unwrap().used_percent, 31.0);
	}

	#[test]
	fn expired_window_reads_as_reset() {
		let (_d, _p, s) = store();
		s.record("claude", &profile(), reading(Some(win(80.0, Some(999))), None, 100));
		let snap = s.snapshot(1_000);
		let w = snap[0].five_hour.as_ref().unwrap();
		assert_eq!(w.used_percent, 0.0);
		assert_eq!(w.resets_at, None);
	}

	#[test]
	fn persists_and_reloads() {
		let (_d, path, s) = store();
		s.record("claude", &profile(), reading(Some(win(62.0, Some(9_000))), None, 100));
		let before = s.snapshot(0);
		drop(s);
		let again = UsageStore::load(path);
		assert_eq!(again.snapshot(0), before);
	}

	#[test]
	fn corrupt_file_loads_empty() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("usage.json");
		std::fs::write(&path, "{nope").unwrap();
		assert!(UsageStore::load(path).snapshot(0).is_empty());
	}

	#[test]
	fn change_callback_fires_only_on_change() {
		let (_d, _p, s) = store();
		let n = Arc::new(AtomicUsize::new(0));
		let n2 = n.clone();
		s.set_on_change(Arc::new(move |_| {
			n2.fetch_add(1, Ordering::SeqCst);
		}));
		let r = reading(Some(win(62.0, None)), None, 100);
		s.record("claude", &profile(), r.clone());
		assert_eq!(n.load(Ordering::SeqCst), 1);
		s.record("claude", &profile(), r);
		assert_eq!(n.load(Ordering::SeqCst), 1);
		s.record("claude", &profile(), reading(Some(win(63.0, None)), None, 200));
		assert_eq!(n.load(Ordering::SeqCst), 2);
	}
}
