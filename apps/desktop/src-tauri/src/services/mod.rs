pub mod agent_policy;
pub mod attention;
pub mod file_watch;
pub mod gh;
pub mod git_agent;
pub mod git_cli;
pub mod git_ops;
pub mod git_proposed;
pub mod indexer;
pub mod intelligence;
pub mod media_server;
// macOS only: the menu exists to give up `Cmd+W` (ADR-0046). Nothing to build
// on Linux, where the same actions are renderer hotkeys.
#[cfg(target_os = "macos")]
pub mod menu;
pub mod profiles;
pub mod reveal;
pub mod search;
pub mod server_files;
pub mod servers;
pub mod session_plan;
pub mod sessions;
pub mod updates;
pub mod usage;
pub mod watcher;
pub mod webview_health;
pub mod wsl;

pub use qursor_core::services::{
	agent_cli, agent_tools, canvas, canvas_tools, child_env, event_sink, files, git, hash, ide,
	jsonl, mcp_wire, osc_title, routines, settings, shell_path, sops, ssh, term_capture, terminal,
	vault,
};
