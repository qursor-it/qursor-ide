//! Rate-limit windows per profile, fed by the usage sources and read by the
//! status bar and the taskbar widget.

pub mod child_guard;
pub mod codex_app_server;
pub mod codex_log;
pub mod poller;
pub mod statusline;
pub mod store;

/// Wall-clock milliseconds.
pub fn now_ms() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_millis() as i64)
		.unwrap_or(0)
}
