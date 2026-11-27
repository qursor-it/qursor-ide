use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::core_link::CoreCell;
use crate::db::Db;
use crate::services::file_watch::FileWatch;
use crate::services::indexer::Indexer;
use crate::services::media_server::MediaServer;
use crate::services::routines::Runner as RoutineRunner;
use crate::services::webview_health::Health;

/// Application state shared across Tauri commands. Constructed in
/// `setup()`.
pub struct AppState {
	pub db: Db,
	pub indexer: Arc<Indexer>,
	/// The link to qursor-core, which owns every terminal
	/// (docs/superpowers/specs/2026-11-26-local-core-design.md §1).
	pub core: Arc<CoreCell>,
	pub claude_dir: PathBuf,
	pub data_dir: PathBuf,
	/// The routine scheduler (F22). Held so `run_routine_now` fires through the
	/// same path the tick does, rather than growing a second one.
	pub routines: Arc<RoutineRunner>,
	/// The filesystem watcher's roots, one per profile (F25). Held so a profile
	/// write can ask for a reconcile — a new profile's transcripts are noticed by
	/// something watching its directory, and nothing else can arrange that.
	pub watch: Arc<crate::services::watcher::Control>,
	/// The watch on whatever file the viewer has open (F7). One at a time, and
	/// the renderer owns its lifetime — it watches on open and releases on
	/// close, so an app with no viewer open holds no watch at all.
	pub file_watch: Arc<FileWatch>,
	/// The watch on the open canvas (F44, ADR-0103): a second instance, so the
	/// viewer's watch above is left alone.
	pub canvas_watch: Arc<FileWatch>,
	/// The loopback server the viewer's media files are fetched from (F7,
	/// ADR-0057). Started on the first `probe_media` and not before: an app that
	/// never opens a video never opens a socket.
	pub media: Arc<OnceLock<MediaServer>>,
	/// What the window's own web process has done to us (F7, ADR-0059). Filled
	/// by the crash handler installed in `setup()` and drained by the renderer on
	/// boot, which is the only pair of things that ever touch it.
	pub webview_health: Arc<Health>,
	/// Project Intelligence (F33, ADR-0084): analyses of a project's sessions and
	/// the candidates they produce. Holds the analysis processes it started so
	/// quitting the app ends them.
	pub intelligence: crate::services::intelligence::IntelligenceService,
	/// The latest rate-limit windows per profile, fed by the usage sources.
	pub usage: Arc<crate::services::usage::store::UsageStore>,
	/// The usage widget in the Windows taskbar (ADR-0086). Held so the setting
	/// can start and stop it and the quit path can take it down.
	pub taskbar_widget: Arc<crate::platform::taskbar_widget::TaskbarWidget>,
}
