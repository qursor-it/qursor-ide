//! A session that wants you, while you are somewhere else (F32, ADR-0081).
//!
//! The decision is the core's (`qursor_core::attention`); this is the app's
//! display: the notification plugin and the main window's attention request.

use tracing::warn;

pub use qursor_core::attention::{
	alert_body, desktop_notifications_on, wants_you, AttentionNotifier, NotifySink, SETTLE,
};

use crate::services::terminal::{AlertContext, TerminalId, TerminalStatus};

/// The window came back: stop asking for attention.
pub fn clear_attention(app: &tauri::AppHandle) {
	use tauri::Manager;
	if let Some(window) = app.get_webview_window("main") {
		if let Err(err) = window.request_user_attention(None) {
			warn!(%err, "could not clear the attention request");
		}
	}
}

/// The real sink: the setting from SQLite, the project's name for a title, the
/// notification plugin, and the main window's attention request (F32).
pub struct TauriSink {
	app: tauri::AppHandle,
}

impl TauriSink {
	pub fn new(app: tauri::AppHandle) -> Self {
		Self { app }
	}

	fn main_window(&self) -> Option<tauri::WebviewWindow> {
		use tauri::Manager;
		self.app.get_webview_window("main")
	}
}

impl NotifySink for TauriSink {
	fn enabled(&self) -> bool {
		use tauri::Manager;
		let Some(state) = self.app.try_state::<crate::state::AppState>() else { return false };
		let value = state
			.db
			.with(|conn| {
				crate::services::settings::get(
					conn,
					crate::models::SettingKey::DesktopNotifications,
				)
			})
			.ok()
			.flatten();
		desktop_notifications_on(value.as_deref())
	}

	fn context(&self, id: &TerminalId) -> Option<AlertContext> {
		use tauri::Manager;
		let state = self.app.try_state::<crate::state::AppState>()?;
		state.terminals.alert_context(id)
	}

	fn send(&self, _id: &TerminalId, status: TerminalStatus, ctx: Option<&AlertContext>) {
		use rusqlite::OptionalExtension;
		use tauri::Manager;
		use tauri_plugin_notification::NotificationExt;

		let Some(state) = self.app.try_state::<crate::state::AppState>() else { return };
		let title = ctx
			.and_then(|c| {
				state
					.db
					.with(|conn| {
						Ok(conn
							.query_row(
								"SELECT display_name FROM projects WHERE id = ?1",
								[&c.project_id],
								|row| row.get::<_, String>(0),
							)
							.optional()?)
					})
					.ok()
					.flatten()
			})
			.unwrap_or_else(|| self.app.package_info().name.clone());
		let body = alert_body(status, ctx.and_then(|c| c.topic.as_deref()));

		if let Err(err) = self.app.notification().builder().title(title).body(body).show() {
			// No daemon on this Linux desktop, or notifications denied on macOS:
			// the dot and the attention request still carry the signal (F32).
			warn!(%err, "desktop notification not shown");
		}
		if let Some(window) = self.main_window() {
			if let Err(err) =
				window.request_user_attention(Some(tauri::UserAttentionType::Informational))
			{
				warn!(%err, "could not request user attention");
			}
		}
	}
}
