// Prevents additional console window on Windows in release. DO NOT REMOVE.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
	// The status-line helper of a Claude session: no window, no app.
	if factorai_lib::run_usage_statusline_helper() {
		return;
	}
	factorai_lib::run()
}
