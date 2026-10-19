//! The notification-area icon the widget falls back to when it cannot live in
//! the taskbar (ADR-0084).
//!
//! Tauri's tray icon is not thread-safe (an `Rc` inside), so the widget thread
//! never holds one: every build, update and removal is a closure posted with
//! `run_on_main_thread`, which also means the widget thread never waits on it.

use fontdue::Font;
use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;
use tracing::warn;

use super::render::sane;
use super::{focus_main, render_icon, WidgetRow};

const ID: &str = "usage-widget";
const ICON_PX: u32 = 32;

pub(super) struct TrayFallback {
	app: AppHandle,
}

impl TrayFallback {
	pub(super) fn start(app: AppHandle) -> Self {
		let handle = app.clone();
		let _ = app.run_on_main_thread(move || {
			let mut builder = TrayIconBuilder::with_id(ID)
				.tooltip(tooltip(&[]))
				.on_tray_icon_event(|tray, event| {
					if let TrayIconEvent::Click {
						button: MouseButton::Left,
						button_state: MouseButtonState::Up,
						..
					} = event
					{
						focus_main(tray.app_handle());
					}
				});
			if let Some(icon) = handle.default_window_icon() {
				builder = builder.icon(icon.clone());
			}
			if let Err(e) = builder.build(&handle) {
				warn!(error = %e, "could not add the usage tray icon");
			}
		});
		Self { app }
	}

	/// New numbers or theme. Without a font the app icon stays.
	pub(super) fn show(&self, rows: &[WidgetRow], font: Option<&Font>, light: bool) {
		let text = tooltip(rows);
		let icon = font.map(|f| {
			let pm = render_icon(critical(rows), f, ICON_PX, light);
			// The tray wants straight alpha; tiny-skia stores premultiplied.
			let rgba: Vec<u8> = pm
				.pixels()
				.iter()
				.flat_map(|p| {
					let c = p.demultiply();
					[c.red(), c.green(), c.blue(), c.alpha()]
				})
				.collect();
			(rgba, pm.width(), pm.height())
		});
		let app = self.app.clone();
		let _ = self.app.run_on_main_thread(move || {
			let Some(tray) = app.tray_by_id(ID) else { return };
			if let Some((rgba, w, h)) = icon {
				let _ = tray.set_icon(Some(Image::new_owned(rgba, w, h)));
			}
			let _ = tray.set_tooltip(Some(text));
		});
	}
}

impl Drop for TrayFallback {
	fn drop(&mut self) {
		let app = self.app.clone();
		let _ = self.app.run_on_main_thread(move || {
			app.remove_tray_by_id(ID);
		});
	}
}

/// The highest percentage on show, which is what the icon draws.
pub(super) fn critical(rows: &[WidgetRow]) -> Option<f64> {
	rows.iter().flat_map(|r| [r.five_hour, r.weekly]).flatten().map(sane).max_by(f64::total_cmp)
}

/// `Claude 5h 62% · wk 31%`, one line per agent.
pub(super) fn tooltip(rows: &[WidgetRow]) -> String {
	if rows.is_empty() {
		return "No usage data yet".into();
	}
	let pct = |p: Option<f64>| match p.map(sane) {
		Some(p) => format!("{}%", p.round() as u32),
		None => "--".into(),
	};
	rows.iter()
		.map(|r| {
			let name = if r.letter == 'C' { "Claude" } else { "Codex" };
			format!("{name} 5h {} · wk {}", pct(r.five_hour), pct(r.weekly))
		})
		.collect::<Vec<_>>()
		.join("\n")
}

#[cfg(test)]
mod tests {
	use super::*;

	fn row(letter: char, five: Option<f64>, week: Option<f64>) -> WidgetRow {
		WidgetRow { letter, five_hour: five, weekly: week, stale: false }
	}

	#[test]
	fn tooltip_lists_both_windows_per_agent() {
		let rows = [row('C', Some(62.0), Some(31.4)), row('X', Some(18.0), Some(16.0))];
		assert_eq!(tooltip(&rows), "Claude 5h 62% · wk 31%\nCodex 5h 18% · wk 16%");
	}

	#[test]
	fn tooltip_marks_missing_numbers() {
		assert_eq!(tooltip(&[row('X', None, Some(5.0))]), "Codex 5h -- · wk 5%");
		assert_eq!(tooltip(&[]), "No usage data yet");
		// NaN reads as 0, as the widget's own `sane` has it.
		assert_eq!(tooltip(&[row('C', Some(f64::NAN), None)]), "Claude 5h 0% · wk --");
	}

	#[test]
	fn critical_is_the_highest_number_anywhere() {
		let rows = [row('C', Some(62.0), Some(31.0)), row('X', Some(18.0), Some(91.0))];
		assert_eq!(critical(&rows), Some(91.0));
		assert_eq!(critical(&[row('C', None, None)]), None);
		assert_eq!(critical(&[row('C', Some(f64::NAN), Some(4.0))]), Some(4.0));
	}
}
