//! The terminal manager and what it needs, without Tauri
//! (docs/superpowers/specs/2026-11-26-local-core-design.md).

pub mod agents;
pub mod attention;
pub mod db;
pub mod error;
pub mod models;
pub mod protocol;
pub mod server;
pub mod services;

use std::path::PathBuf;

/// The Claude config directory this machine uses when nothing has been
/// configured here — the *ambient* one.
///
/// Two things read it, and both want the same answer: `ensure_default` seeds the
/// default profile from it (F25), and `TerminalManager` compares against it to
/// decide whether a spawn needs `CLAUDE_CONFIG_DIR` set at all.
///
/// **`CLAUDE_CONFIG_DIR` first, because it is the CLI's own variable.** If it is
/// exported in the environment factorai was launched from, that *is* this
/// machine's config directory, and reading `~/.claude` instead would have us
/// index one store while every session used another. `CLAUDE_HOME` stays honoured
/// after it — see specs/07-open-questions.md Q3, which this refines.
pub fn claude_dir() -> PathBuf {
	for var in ["CLAUDE_CONFIG_DIR", "CLAUDE_HOME"] {
		if let Some(env) = std::env::var_os(var) {
			if !env.is_empty() {
				return PathBuf::from(env);
			}
		}
	}
	dirs::home_dir().expect("no home dir").join(".claude")
}

/// Wall-clock milliseconds, for the two writes to `session_worktrees` that do
/// not come from the bridge — the setup callback below, and the human's picker
/// in `commands::sessions`.
///
/// A local copy rather than reaching into `services::terminal`'s private helper:
/// another caller is not a reason to widen that module's surface, and this is
/// three lines.
pub fn epoch_ms() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_millis() as i64)
		.unwrap_or(0)
}

/// Shared by every test that starts a process, here and in the app's tests.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
