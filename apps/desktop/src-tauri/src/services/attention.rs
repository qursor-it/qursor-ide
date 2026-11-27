//! A session that wants you, while you are somewhere else (F32, ADR-0081).
//!
//! The decision is the core's (`qursor_core::attention`, which also reads the
//! `notifications.desktop` setting); this is the app's display: the
//! notification plugin and the main window's attention request.

use tauri::Manager;
use tracing::warn;

pub use qursor_core::attention::AttentionNotice;

/// The window came back: stop asking for attention.
pub fn clear_attention(app: &tauri::AppHandle) {
	if let Some(window) = app.get_webview_window("main") {
		if let Err(err) = window.request_user_attention(None) {
			warn!(%err, "could not clear the attention request");
		}
	}
}

/// Show what the core decided: a desktop notification and the window's
/// attention request.
pub fn show(app: &tauri::AppHandle, notice: &AttentionNotice) {
	use tauri_plugin_notification::NotificationExt;

	if let Err(err) = app.notification().builder().title(&notice.title).body(&notice.body).show() {
		// No daemon on this Linux desktop, or notifications denied on macOS:
		// the dot and the attention request still carry the signal (F32).
		warn!(%err, "desktop notification not shown");
	}
	if let Some(window) = app.get_webview_window("main") {
		if let Err(err) =
			window.request_user_attention(Some(tauri::UserAttentionType::Informational))
		{
			warn!(%err, "could not request user attention");
		}
	}
}
