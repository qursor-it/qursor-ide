//! The usage widget in the Windows taskbar (ADR-0086): a native layered window
//! reparented into `Shell_TrayWnd`, falling back to a notification-area icon
//! when embedding fails. Windows only; elsewhere every call is a no-op.
//!
//! The widget runs on its own thread, whose input queue is coupled to
//! Explorer's once it is reparented — so if Explorer hangs, that thread hangs.
//! **Nothing here ever waits on it**: `update` and `stop` queue on a channel
//! and post a thread message, and the widget thread takes no lock that anyone
//! else waits on (the config lock is held only for a clone or a small write).

pub mod config;
pub mod geometry;
pub mod render;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod windows;

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::AppHandle;

use crate::models::ProviderUsage;
use crate::services::usage::Thresholds;
use config::{ConfigStore, Look, WidgetConfig};

pub use render::{load_system_font, render, render_icon, rows_from, WidgetRow};

/// What the widget thread is asked to do.
// Only the Windows widget thread reads the payload; elsewhere it is just queued.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum Msg {
	Update(Vec<ProviderUsage>),
	Thresholds(Thresholds),
	Config(WidgetConfig),
	Stop,
}

/// What the widget talks back to. No app (a test) means no click target and
/// no tray fallback.
#[derive(Clone, Default)]
pub(crate) struct Host {
	// Read only by the Windows widget thread; `start` fills it on every platform.
	#[cfg_attr(not(windows), allow(dead_code))]
	pub app: Option<AppHandle>,
	/// Where a drag or a resize saves the widget's new place.
	#[cfg_attr(not(windows), allow(dead_code))]
	pub store: Option<Arc<ConfigStore>>,
}

/// The running widget. Dropping it stops it.
pub struct WidgetHandle {
	tx: Sender<Msg>,
	#[cfg(windows)]
	shared: std::sync::Arc<windows::Shared>,
}

impl WidgetHandle {
	/// Start the widget thread. `None` off Windows, or if the thread could not start.
	pub fn start(
		app: AppHandle,
		store: Arc<ConfigStore>,
		initial: Vec<ProviderUsage>,
		thresholds: Thresholds,
	) -> Option<WidgetHandle> {
		let look = Look { config: store.get(), thresholds };
		Self::start_with(Host { app: Some(app), store: Some(store) }, initial, look)
	}

	pub(crate) fn start_with(
		host: Host,
		initial: Vec<ProviderUsage>,
		look: Look,
	) -> Option<WidgetHandle> {
		#[cfg(windows)]
		{
			let (tx, shared) = windows::spawn(host, initial, look)?;
			Some(WidgetHandle { tx, shared })
		}
		#[cfg(not(windows))]
		{
			let _ = (host, initial, look);
			None
		}
	}

	/// New numbers. Queues and posts; never waits.
	pub fn update(&self, usage: Vec<ProviderUsage>) {
		let _ = self.tx.send(Msg::Update(usage));
		self.wake();
	}

	/// New colour thresholds. Queues and posts; never waits.
	pub fn set_thresholds(&self, thresholds: Thresholds) {
		let _ = self.tx.send(Msg::Thresholds(thresholds));
		self.wake();
	}

	/// New config. Queues and posts; never waits.
	pub fn set_config(&self, config: WidgetConfig) {
		let _ = self.tx.send(Msg::Config(config));
		self.wake();
	}

	/// Tear the widget down. Queues and posts; never waits.
	pub fn stop(&self) {
		let _ = self.tx.send(Msg::Stop);
		self.wake();
	}

	fn wake(&self) {
		#[cfg(windows)]
		self.shared.wake();
	}

	/// A handle over a bare channel with no widget thread behind it.
	#[cfg(test)]
	fn over(tx: Sender<Msg>) -> WidgetHandle {
		WidgetHandle {
			tx,
			#[cfg(windows)]
			shared: std::sync::Arc::new(windows::Shared::new(0)),
		}
	}
}

impl Drop for WidgetHandle {
	fn drop(&mut self) {
		self.stop();
	}
}

/// Whether the `usageTaskbarWidget` setting asks for the widget: on unless `off`.
pub fn enabled(value: Option<&str>) -> bool {
	value != Some("off")
}

/// The widget as `AppState` holds it: started at boot, toggled by the setting,
/// stopped on quit. The lock guards only the handle and is never held across
/// anything that waits on the widget thread, which never takes it. The config
/// and thresholds live here, not in the handle, so a change made while the
/// widget is off is what the next start draws.
pub struct TaskbarWidget {
	handle: Mutex<Option<WidgetHandle>>,
	store: Arc<ConfigStore>,
	thresholds: Mutex<Thresholds>,
}

impl TaskbarWidget {
	/// `config_path` is the `taskbar_widget.json` that holds the user's layout.
	pub fn new(config_path: PathBuf, thresholds: Thresholds) -> Self {
		Self {
			handle: Mutex::new(None),
			store: Arc::new(ConfigStore::load(config_path)),
			thresholds: Mutex::new(thresholds),
		}
	}

	/// Start or stop to match the setting. `initial` is asked for only on a start.
	pub fn set_enabled(
		&self,
		app: &AppHandle,
		on: bool,
		initial: impl FnOnce() -> Vec<ProviderUsage>,
	) {
		let mut slot = self.handle.lock();
		if on && slot.is_none() {
			let thresholds = *self.thresholds.lock();
			*slot = WidgetHandle::start(app.clone(), self.store.clone(), initial(), thresholds);
		} else if !on {
			// Dropping the handle stops the widget.
			slot.take();
		}
	}

	pub fn update(&self, usage: Vec<ProviderUsage>) {
		if let Some(h) = self.handle.lock().as_ref() {
			h.update(usage);
		}
	}

	pub fn set_thresholds(&self, thresholds: Thresholds) {
		*self.thresholds.lock() = thresholds;
		if let Some(h) = self.handle.lock().as_ref() {
			h.set_thresholds(thresholds);
		}
	}

	pub fn config(&self) -> WidgetConfig {
		self.store.get()
	}

	/// Keep and save the config, and redraw the running widget with it.
	pub fn set_config(&self, config: WidgetConfig) {
		let config = self.store.set(config);
		if let Some(h) = self.handle.lock().as_ref() {
			h.set_config(config);
		}
	}

	pub fn stop(&self) {
		self.handle.lock().take();
	}
}

/// Event the widget emits for a click set to `details`; the status bar opens
/// its usage popover on it.
#[cfg(windows)]
pub(crate) const OPEN_DETAILS_EVENT: &str = "usage:open-details";

/// Bring factorai's main window to the front: what a click on the tray icon
/// means, and the widget's default click.
#[cfg(windows)]
pub(crate) fn focus_main(app: &AppHandle) {
	use tauri::Manager;
	if let Some(window) = app.get_webview_window("main") {
		let _ = window.unminimize();
		let _ = window.show();
		let _ = window.set_focus();
	}
}

/// What a click on the widget does, as the config says.
#[cfg(windows)]
pub(crate) fn click(app: &AppHandle, action: config::Click) {
	use tauri::Emitter;
	match action {
		config::Click::None => {}
		config::Click::Focus => focus_main(app),
		// A hidden status bar is not listening, so this is then just a focus.
		config::Click::Details => {
			focus_main(app);
			let _ = app.emit(OPEN_DETAILS_EVENT, ());
		}
	}
}

#[cfg(test)]
mod tests {
	use std::time::{Duration, Instant};

	use super::*;

	/// Nothing outside the widget thread may wait on it: `update` and `stop` only
	/// queue, even when nobody is draining.
	#[test]
	fn widget_handle_only_posts() {
		let (tx, _rx) = std::sync::mpsc::channel();
		let handle = WidgetHandle::over(tx);
		let started = Instant::now();
		for _ in 0..1000 {
			handle.update(Vec::new());
			handle.set_config(WidgetConfig::default());
			handle.set_thresholds(Thresholds::default());
			handle.stop();
		}
		assert!(started.elapsed() < Duration::from_secs(1));
		#[cfg(not(windows))]
		assert!(WidgetHandle::start_with(Host::default(), Vec::new(), Look::default()).is_none());
	}

	#[test]
	fn on_unless_set_off() {
		assert!(enabled(None));
		assert!(enabled(Some("on")));
		assert!(!enabled(Some("off")));
	}

	/// A config set while the widget is off is saved, and read back.
	#[test]
	fn config_is_kept_while_off() {
		let dir = tempfile::TempDir::new().unwrap();
		let widget =
			TaskbarWidget::new(dir.path().join("taskbar_widget.json"), Thresholds::default());
		assert_eq!(widget.config(), WidgetConfig::default());
		widget.set_config(WidgetConfig { width: 300.0, ..WidgetConfig::default() });
		assert_eq!(widget.config().width, 300.0);
		assert!(dir.path().join("taskbar_widget.json").exists());
	}
}
