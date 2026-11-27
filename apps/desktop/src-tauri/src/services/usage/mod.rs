//! Rate-limit windows per profile, fed by the usage sources and read by the
//! status bar and the taskbar widget.

pub mod claude_api;
pub mod codex_app_server;
pub mod codex_log;
pub mod mcp;
pub mod poller;
pub mod prices;
pub mod session_cost;
pub mod store;

pub use qursor_core::services::usage::{child_guard, now_ms, statusline, Thresholds};

/// A reading older than this (15 min) is shown dimmed.
pub const STALE_MS: i64 = 15 * 60 * 1000;
