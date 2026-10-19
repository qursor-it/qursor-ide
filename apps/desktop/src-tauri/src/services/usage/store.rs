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
	/// Everyone told about a change: the renderer's event and the taskbar widget.
	on_change: Mutex<Vec<OnChange>>,
	/// Serialises writing and notifying: two source threads share one temp
	/// file, and subscribers must hear changes in the order they were made.
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
			on_change: Mutex::new(Vec::new()),
			persist_lock: Mutex::new(()),
		}
	}

	pub fn add_on_change(&self, f: OnChange) {
		self.on_change.lock().push(f);
	}

	/// Merge one reading in. Returns whether anything changed.
	pub fn record(&self, agent: &str, profile: &Profile, reading: UsageReading) -> bool {
		let key = (agent.to_string(), profile.id.clone());
		{
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
		}
		self.persist_and_notify();
		true
	}

	/// Bring the stored profile names and default flags in line with
	/// `profiles`, and drop entries whose profile is gone, so a rename or a new
	/// default shows at once rather than at that profile's next reading. An
	/// empty list (a failed profile read) changes nothing. Returns whether
	/// anything changed; subscribers hear only a change.
	pub fn sync_profiles(&self, profiles: &[Profile]) -> bool {
		if profiles.is_empty() {
			return false;
		}
		let changed = {
			let mut map = self.entries.lock();
			let before = map.len();
			map.retain(|(_, id), _| profiles.iter().any(|p| &p.id == id));
			let mut changed = map.len() != before;
			for u in map.values_mut() {
				let Some(p) = profiles.iter().find(|p| p.id == u.profile_id) else { continue };
				if u.profile_name != p.name || u.is_default != p.is_default {
					u.profile_name = p.name.clone();
					u.is_default = p.is_default;
					changed = true;
				}
			}
			changed
		};
		if changed {
			self.persist_and_notify();
		}
		changed
	}

	/// Every entry, with a window whose reset time has passed shown as reset.
	pub fn snapshot(&self, now_ms: i64) -> Vec<ProviderUsage> {
		apply_resets(sorted(self.entries.lock().values().cloned().collect()), now_ms)
	}

	/// Write the store, then tell every subscriber. Both happen under the
	/// persist lock with the list taken inside it, so a slower writer can never
	/// land, or announce, an older snapshot after a newer one. The callbacks
	/// only post or emit and never call back into the store.
	fn persist_and_notify(&self) {
		let _guard = self.persist_lock.lock();
		let list = sorted(self.entries.lock().values().cloned().collect());
		self.write(&list);
		let subscribers = self.on_change.lock().clone();
		let list = apply_resets(list, now_ms());
		for cb in subscribers {
			cb(list.clone());
		}
	}

	/// Temp file then rename, so a crash mid-write leaves the old file whole.
	fn write(&self, list: &[ProviderUsage]) {
		let tmp = self.path.with_extension("json.tmp");
		let result = serde_json::to_vec(list)
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

/// A window whose reset time has passed reads as reset. Public so the taskbar
/// widget, which keeps a list between changes, can age it on its own tick.
pub fn apply_resets(mut list: Vec<ProviderUsage>, now_ms: i64) -> Vec<ProviderUsage> {
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
		s.add_on_change(Arc::new(move |_| {
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

	/// The renderer and the taskbar widget both listen.
	#[test]
	fn every_subscriber_hears_a_change() {
		let (_d, _p, s) = store();
		let n = Arc::new(AtomicUsize::new(0));
		for _ in 0..2 {
			let n = n.clone();
			s.add_on_change(Arc::new(move |list| {
				assert_eq!(list.len(), 1);
				n.fetch_add(1, Ordering::SeqCst);
			}));
		}
		s.record("claude", &profile(), reading(Some(win(62.0, None)), None, 100));
		assert_eq!(n.load(Ordering::SeqCst), 2);
	}

	/// Two writer threads: subscribers must never hear an older snapshot after
	/// a newer one, or the bar would sit on stale numbers until the next change.
	#[test]
	fn subscribers_hear_snapshots_in_order() {
		let (_d, _p, s) = store();
		let s = Arc::new(s);
		let heard: Arc<parking_lot::Mutex<Vec<Vec<ProviderUsage>>>> = Arc::default();
		{
			let heard = heard.clone();
			s.add_on_change(Arc::new(move |list| heard.lock().push(list)));
		}
		let writers: Vec<_> = ["a", "b"]
			.into_iter()
			.map(|id| {
				let s = s.clone();
				std::thread::spawn(move || {
					let p = Profile { id: id.into(), ..profile() };
					for at in 1..=200 {
						s.record("codex", &p, reading(Some(win(at as f64 / 4.0, None)), None, at));
					}
				})
			})
			.collect();
		for w in writers {
			w.join().unwrap();
		}
		let heard = heard.lock();
		assert_eq!(heard.len(), 400);
		let at = |list: &[ProviderUsage], id: &str| {
			list.iter().find(|u| u.profile_id == id).map_or(0, |u| u.updated_at)
		};
		for pair in heard.windows(2) {
			for id in ["a", "b"] {
				assert!(
					at(&pair[0], id) <= at(&pair[1], id),
					"an older snapshot came after a newer one"
				);
			}
		}
		assert_eq!(heard.last().map(|l| (at(l, "a"), at(l, "b"))), Some((200, 200)));
	}

	fn named(id: &str, name: &str, is_default: bool) -> Profile {
		Profile { id: id.into(), name: name.into(), is_default, ..profile() }
	}

	#[test]
	fn sync_profiles_follows_renames_and_the_default() {
		let (_d, path, s) = store();
		let n = Arc::new(AtomicUsize::new(0));
		let n2 = n.clone();
		s.add_on_change(Arc::new(move |_| {
			n2.fetch_add(1, Ordering::SeqCst);
		}));
		s.record("claude", &named("p1", "Main", true), reading(Some(win(1.0, None)), None, 1));
		s.record("claude", &named("p2", "Work", false), reading(Some(win(2.0, None)), None, 1));
		assert_eq!(n.load(Ordering::SeqCst), 2);

		// Nothing differs: no emit.
		assert!(!s.sync_profiles(&[named("p1", "Main", true), named("p2", "Work", false)]));
		assert_eq!(n.load(Ordering::SeqCst), 2);

		// Renamed, and the default moved.
		assert!(s.sync_profiles(&[named("p1", "Home", false), named("p2", "Work", true)]));
		assert_eq!(n.load(Ordering::SeqCst), 3);
		let snap = s.snapshot(0);
		let get = |id: &str| snap.iter().find(|u| u.profile_id == id).unwrap().clone();
		assert_eq!((get("p1").profile_name.as_str(), get("p1").is_default), ("Home", false));
		assert_eq!((get("p2").profile_name.as_str(), get("p2").is_default), ("Work", true));
		// The readings themselves are untouched, and the change is persisted.
		assert_eq!(get("p1").five_hour.unwrap().used_percent, 1.0);
		assert_eq!(UsageStore::load(path).snapshot(0), snap);
	}

	#[test]
	fn sync_profiles_drops_deleted_profiles_but_not_on_an_empty_list() {
		let (_d, _p, s) = store();
		s.record("claude", &named("p1", "Main", true), reading(Some(win(1.0, None)), None, 1));
		s.record("claude", &named("p2", "Work", false), reading(Some(win(2.0, None)), None, 1));

		// A failed profile read is an empty list: keep everything.
		assert!(!s.sync_profiles(&[]));
		assert_eq!(s.snapshot(0).len(), 2);

		assert!(s.sync_profiles(&[named("p1", "Main", true)]));
		let snap = s.snapshot(0);
		assert_eq!(snap.len(), 1);
		assert_eq!(snap[0].profile_id, "p1");
	}
}
