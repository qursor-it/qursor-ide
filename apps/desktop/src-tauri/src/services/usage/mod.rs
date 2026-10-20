//! Rate-limit windows per profile, fed by the usage sources and read by the
//! status bar and the taskbar widget.

pub mod child_guard;
pub mod claude_api;
pub mod codex_app_server;
pub mod codex_log;
pub mod poller;
pub mod prices;
pub mod session_cost;
pub mod statusline;
pub mod store;

/// Bar colour thresholds, percent used: amber from `WARN`, red from `CRITICAL`.
pub const WARN: f64 = 70.0;
pub const CRITICAL: f64 = 90.0;
/// A reading older than this (15 min) is shown dimmed.
pub const STALE_MS: i64 = 15 * 60 * 1000;

/// Wall-clock milliseconds.
pub fn now_ms() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_millis() as i64)
		.unwrap_or(0)
}
