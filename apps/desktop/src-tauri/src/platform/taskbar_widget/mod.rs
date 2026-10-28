//! The usage widget in the Windows taskbar (ADR-0086): a native layered window
//! reparented into `Shell_TrayWnd`, falling back to a notification-area icon
//! when embedding fails. Windows only; elsewhere every call is a no-op.
//!
//! The widget runs on its own thread, whose input queue is coupled to
//! Explorer's once it is reparented — so if Explorer hangs, that thread hangs.
//! **Nothing here ever waits on it**: `update` and `stop` queue on a channel
//! and post a thread message, and the widget thread takes no lock that anyone
//! else waits on.

pub mod render;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod windows;

use std::sync::mpsc::Sender;

use parking_lot::Mutex;
use tauri::AppHandle;

use crate::models::ProviderUsage;

pub use render::{load_system_font, render, render_icon, rows_from, WidgetRow};

/// What the widget thread is asked to do.
// Only the Windows widget thread reads the payload; elsewhere it is just queued.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum Msg {
	Update(Vec<ProviderUsage>),
	Stop,
}

/// What the widget talks back to. No app (a test) means no click target and
/// no tray fallback.
#[derive(Clone, Default)]
pub(crate) struct Host {
	// Read only by the Windows widget thread; `start` fills it on every platform.
	#[cfg_attr(not(windows), allow(dead_code))]
	pub app: Option<AppHandle>,
}

/// The running widget. Dropping it stops it.
pub struct WidgetHandle {
	tx: Sender<Msg>,
	#[cfg(windows)]
	shared: std::sync::Arc<windows::Shared>,
}

impl WidgetHandle {
	/// Start the widget thread. `None` off Windows, or if the thread could not start.
	pub fn start(app: AppHandle, initial: Vec<ProviderUsage>) -> Option<WidgetHandle> {
		Self::start_with(Host { app: Some(app) }, initial)
	}

	pub(crate) fn start_with(host: Host, initial: Vec<ProviderUsage>) -> Option<WidgetHandle> {
		#[cfg(windows)]
		{
			let (tx, shared) = windows::spawn(host, initial)?;
			Some(WidgetHandle { tx, shared })
		}
		#[cfg(not(windows))]
		{
			let _ = (host, initial);
			None
		}
	}

	/// New numbers. Queues and posts; never waits.
	pub fn update(&self, usage: Vec<ProviderUsage>) {
		let _ = self.tx.send(Msg::Update(usage));
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
/// anything that waits on the widget thread, which never takes it.
#[derive(Default)]
pub struct TaskbarWidget(Mutex<Option<WidgetHandle>>);

impl TaskbarWidget {
	/// Start or stop to match the setting. `initial` is asked for only on a start.
	pub fn set_enabled(
		&self,
		app: &AppHandle,
		on: bool,
		initial: impl FnOnce() -> Vec<ProviderUsage>,
	) {
		let mut slot = self.0.lock();
		if on && slot.is_none() {
			*slot = WidgetHandle::start(app.clone(), initial());
		} else if !on {
			// Dropping the handle stops the widget.
			slot.take();
		}
	}

	pub fn update(&self, usage: Vec<ProviderUsage>) {
		if let Some(h) = self.0.lock().as_ref() {
			h.update(usage);
		}
	}

	pub fn stop(&self) {
		self.0.lock().take();
	}
}

/// Bring factorai's main window to the front: what a click on the widget or the
/// tray icon means.
#[cfg(windows)]
pub(crate) fn focus_main(app: &AppHandle) {
	use tauri::Manager;
	if let Some(window) = app.get_webview_window("main") {
		let _ = window.unminimize();
		let _ = window.show();
		let _ = window.set_focus();
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
			handle.stop();
		}
		assert!(started.elapsed() < Duration::from_secs(1));
		#[cfg(not(windows))]
		assert!(WidgetHandle::start_with(Host::default(), Vec::new()).is_none());
	}

	#[test]
	fn on_unless_set_off() {
		assert!(enabled(None));
		assert!(enabled(Some("on")));
		assert!(!enabled(Some("off")));
	}
}
