pub mod commands;
pub mod core_link;
pub mod platform;
pub mod services;
pub mod state;
pub mod tauri_sink;

pub use qursor_core::{agents, db, error, models};
pub(crate) use qursor_core::{claude_dir, epoch_ms};

use std::sync::Arc;

use serde_json::{json, Value};
use tauri::{Emitter, Manager, WindowEvent};
use tracing::{info, warn};

use crate::core_link::{CoreCell, CoreLink};
use crate::db::Db;
use crate::services::event_sink::EventSink;
use crate::services::indexer::{spawn_initial_scan, Indexer};
use crate::services::watcher;
use crate::state::AppState;
use crate::tauri_sink::TauriSink;

/// `factorai --usage-statusline <sink dir>`: the status line a factorai Claude
/// session runs (`services::usage::statusline`). Returns whether this process
/// was that helper — it has then done its work and must exit before any Tauri
/// code runs.
pub fn run_usage_statusline_helper() -> bool {
	services::usage::statusline::helper_main(std::env::args_os().skip(1))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
	// Held until the event loop ends. `setup()` and the synchronous commands run
	// on this thread, and core code they call may look for a current runtime
	// rather than Tauri's own handle.
	let _runtime = tauri::async_runtime::handle().inner().enter();
	tracing_subscriber::fmt()
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,factorai_lib=debug")),
		)
		.init();

	let builder = tauri::Builder::default();

	// The app menu, macOS only and for one reason: Tauri's default menu binds
	// `Cmd+W` to Close Window, and AppKit consumes that before the webview sees
	// it — so F28's tab close could not exist. Ours moves Close Window to
	// `Cmd+Shift+W` and keeps `Cmd+Q` on the same `CloseRequested` path as the
	// close button (ADR-0046).
	#[cfg(target_os = "macos")]
	let builder = builder
		.menu(services::menu::build)
		.on_menu_event(|app, event| services::menu::on_menu_event(app, event.id().as_ref()));

	builder
		.plugin(tauri_plugin_shell::init())
		.plugin(tauri_plugin_notification::init())
		// Copying needs a route the webview can't provide: WebKitGTK implements
		// neither `ClipboardItem` nor, since 2.52, a `writeText` it will allow, so
		// images hand RGBA to this and text goes through its `write_text` (F7).
		.plugin(tauri_plugin_clipboard_manager::init())
		.plugin(tauri_plugin_dialog::init())
		.plugin(tauri_plugin_fs::init())
		.plugin(tauri_plugin_process::init())
		.plugin(tauri_plugin_updater::Builder::new().build())
		.setup(|app| {
			let data_dir = app.path().app_data_dir().expect("failed to resolve app_data_dir");
			let db = Db::open(&data_dir).expect("failed to open db");

			let cd = claude_dir();
			info!(?cd, ?data_dir, "factorai booting");

			// **The default profile has to exist before anything resolves one**
			// (F25, ADR-0036). `CLAUDE_HOME` — or `$HOME/.claude` — *seeds* it the
			// first time and is not consulted again: the row is authoritative from
			// then on, so a stale export in a shell profile can never outrank what
			// Settings shows (specs/07-open-questions.md Q3).
			let default_profile = services::profiles::ensure_default(&db, &cd)
				.expect("failed to ensure the default claude profile");
			info!(
				id = %default_profile.id,
				config_dir = %default_profile.config_dir,
				"default claude profile"
			);

			// A previous OpenCode install can already have history before binary probing (ADR-0122).
			let opencode =
				agents::descriptor(agents::OPENCODE).expect("OpenCode is in the registry");
			let opencode_root = agents::ambient_dir(opencode);
			if agents::opencode::database_path(&opencode_root).is_file() {
				if let Err(error) =
					services::profiles::ensure_default_for(&db, opencode.id, &opencode_root)
				{
					warn!(%error, "could not seed the OpenCode profile");
				}
			}

			// Before anything spawns a child: the OpenCode Go key leaves the
			// environment so no child inherits it, and goes to the core, which
			// owns the vault it is kept in (docs/superpowers/specs/
			// 2026-11-26-local-core-design.md §5a).
			let go_key = services::usage::mcp::take_environment_key();

			// Ask the user's login shell what their PATH is, on its own thread.
			// A GUI process has never run an rc file, so ours has no Homebrew and
			// no version-manager shims in it, and a session handed that PATH
			// cannot run a hook, an stdio MCP server or a statusline command. Off
			// the main thread because the answer costs a shell startup and the
			// window must not wait on `~/.zshrc`. See `services::shell_path`.
			services::shell_path::warm();

			// Anything we left behind last time. A SIGKILL leaves an IDE lockfile
			// pointing at a port nothing is listening on, which is ADR-0005's
			// orphan problem on a surface where it is inert rather than dangerous
			// — the CLI probes before it trusts one. It still matters: the CLI
			// auto-connects only when exactly one candidate matches, and our own
			// litter is the easiest way to stop being that one. Only our entries,
			// and only those whose process is gone (ADR-0017).
			// Every profile, not just the boot directory: a session's lockfile is
			// written into the config directory *that session* runs under (F25), so
			// our litter is spread across as many directories as there are profiles.
			let swept: usize = services::profiles::config_dirs(&db)
				.iter()
				.map(|dir| {
					services::ide::lockfile::sweep(dir, services::ide::lockfile::pid_is_alive)
				})
				.sum();
			if swept > 0 {
				info!(swept, "removed stale ide lockfiles from a previous run");
			}

			let statusline_dir = data_dir.join("usage").join("statusline");
			let usage =
				Arc::new(services::usage::store::UsageStore::load(data_dir.join("usage.json")));
			let sink: Arc<dyn EventSink> = Arc::new(TauriSink::new(app.handle().clone()));

			// Every terminal lives in qursor-core (docs/superpowers/specs/
			// 2026-11-26-local-core-design.md §1). Found or started off the main
			// thread: the window does not wait for it, and `core_endpoint` does.
			// The database is migrated above, which the core needs before it starts.
			let core = Arc::new(CoreCell::new(
				data_dir.clone(),
				app.package_info().version.to_string(),
				go_key,
			));
			{
				let (core, app) = (core.clone(), app.handle().clone());
				tauri::async_runtime::spawn(async move {
					let Ok(link) = core.link().await else { return };
					// The window may have taken focus before the link was up.
					let focused = app
						.get_webview_window("main")
						.and_then(|w| w.is_focused().ok())
						.unwrap_or(false);
					tell_focus(&link, focused);
				});
			}
			let live = core.clone();
			let reap = core.clone();
			// Project Intelligence (F33). Asks the terminals which sessions are
			// still running, so an analysis never reads one mid-turn.
			let intelligence_live = core.clone();
			let intelligence = services::intelligence::IntelligenceService::for_app(
				db.clone(),
				app.handle().clone(),
				Arc::new(move || intelligence_live.live_session_ids()),
			);
			let indexer = Arc::new(
				Indexer::for_app(db.clone(), app.handle().clone())
					.with_live_ids(Arc::new(move || live.live_session_ids()))
					// A footer shell whose own directory has gone (ADR-0032). The
					// scan is the trigger because it is already the pass that stats
					// the filesystem — see `ReapShellsCb`.
					.with_shell_reap(Arc::new(move || {
						let Some(link) = reap.ready() else { return };
						tauri::async_runtime::spawn(async move {
							if let Err(error) =
								link.request("shell.reapMissingCwd", Value::Null).await
							{
								warn!("could not reap footer shells: {error:#}");
							}
						});
					})),
			);

			// The routine scheduler (F22, ADR-0026). It decides *when*; the
			// renderer performs the spawn, so this only ever writes rows and
			// emits `routine:fire`. `live` is asked for rather than held — the
			// cap and the overlap skip need the live set, and owning the
			// terminal manager would make the runner the wrong kind of object.
			let routine_live = core.clone();
			let routines = Arc::new(services::routines::Runner::new(
				db.clone(),
				sink.clone(),
				Arc::new(move || routine_live.live_session_ids()),
			));

			// The watcher's roots are one per profile and the set changes while the
			// app runs, so the control handle lives in `AppState`: a profile write
			// asks for a reconcile rather than the watcher polling the table (F25).
			let watch = Arc::new(watcher::Control::default());

			// The renderer runs in a process of its own and can die without taking
			// ours with it, which leaves a window that is on screen, accepting
			// clicks and completely dead (ADR-0059). The handler that reloads it
			// is installed here, and what it records is drained by the renderer on
			// its next boot — so both ends need the same handle.
			let webview_health = Arc::new(services::webview_health::Health::default());
			if let Some(window) = app.get_webview_window("main") {
				services::webview_health::install(&window, webview_health.clone());
			}

			// On Windows the window starts hidden and the renderer shows it on its
			// first commit (PERF-22, `specs/10-performance.md`). This is the floor
			// for a boot that never runs a line of script — a chunk that fails to
			// load — which would otherwise leave the app running with no window.
			// A second `show` of a visible window changes nothing.
			#[cfg(windows)]
			if let Some(window) = app.get_webview_window("main") {
				std::thread::spawn(move || {
					std::thread::sleep(std::time::Duration::from_secs(5));
					let _ = window.show();
				});
			}

			// The usage snapshot persists beside the database and tells the
			// renderer whenever a source changes it.
			{
				let handle = app.handle().clone();
				usage.add_on_change(Arc::new(move |list| {
					let _ = handle.emit("usage:changed", list);
				}));
			}
			// The taskbar widget (ADR-0086) hears the same changes. `update` only
			// posts to the widget thread, so a hung Explorer cannot hold a source.
			let taskbar_widget = Arc::new(platform::taskbar_widget::TaskbarWidget::new(
				data_dir.join("taskbar_widget.json"),
				services::settings::usage_thresholds(&db),
			));
			{
				let widget = taskbar_widget.clone();
				usage.add_on_change(Arc::new(move |list| widget.update(list)));
			}
			let widget_setting =
				db.with(|c| services::settings::get(c, models::SettingKey::UsageTaskbarWidget));

			let usage_db = db.clone();
			let usage_poll_store = usage.clone();
			app.manage(AppState {
				db,
				indexer: indexer.clone(),
				core: core.clone(),
				claude_dir: cd,
				data_dir,
				routines: routines.clone(),
				watch: watch.clone(),
				file_watch: Arc::new(services::file_watch::FileWatch::new()),
				canvas_watch: Arc::new(services::file_watch::FileWatch::new()),
				media: Arc::new(std::sync::OnceLock::new()),
				webview_health,
				intelligence,
				usage: usage.clone(),
				taskbar_widget: taskbar_widget.clone(),
			});
			// On unless the setting says `off`; a no-op off Windows.
			taskbar_widget.set_enabled(
				app.handle(),
				platform::taskbar_widget::enabled(widget_setting.ok().flatten().as_deref()),
				|| usage.snapshot(services::usage::now_ms()),
			);

			services::usage::poller::spawn(
				app.handle().clone(),
				usage_poll_store,
				usage_db,
				statusline_dir,
				core.clone(),
			);
			spawn_initial_scan(indexer.clone());
			watcher::spawn(indexer, watch.clone());
			// Ticking starts with the window, because a routine is scheduled work
			// in a project you can see — and the first tick is what catches up
			// what was missed while the app was closed.
			routines.start();

			// A dev build says so in its title, because that is the only part of
			// it the window switcher, the dock and `wmctrl -l` can see — and the
			// release factorai runs beside it all day with live Claude sessions
			// in it. The header shows the same marker (components/layout/
			// DevBadge.tsx) for when the window itself is in front of you.
			#[cfg(debug_assertions)]
			if let Some(window) = app.get_webview_window("main") {
				let _ = window.set_title("Qursor IDE DEV");
			}

			// Devtools is opt-in via FACTORAI_DEVTOOLS=1 so screenshot-driven
			// QA captures aren't cluttered by the inspector pane.
			#[cfg(debug_assertions)]
			if std::env::var("FACTORAI_DEVTOOLS").as_deref() == Ok("1") {
				if let Some(window) = app.get_webview_window("main") {
					window.open_devtools();
				}
			}
			Ok(())
		})
		.on_window_event(|window, event| {
			if let WindowEvent::Focused(focused) = event {
				// The core's notifier only notifies a window that is not in front,
				// and cannot see the window itself (F32).
				if let Some(link) = window.try_state::<AppState>().and_then(|s| s.core.ready()) {
					tell_focus(&link, *focused);
				}
				if *focused {
					services::attention::clear_attention(window.app_handle());
				}
			}
			if let WindowEvent::CloseRequested { api, .. } = event {
				if let Some(state) = window.try_state::<AppState>() {
					// An analysis is a child process too (ADR-0005, ADR-0084): it ends
					// with the window whether or not a session is asking to be kept.
					state.intelligence.cancel_all();
					let link = state.core.ready();
					// Asked now, not the last counts heard: a terminal that has not
					// changed status since it started has sent no event.
					if let Some(link) = &link {
						let asked = tauri::async_runtime::block_on(async {
							tokio::time::timeout(COUNTS_AT_CLOSE, link.refresh()).await
						});
						match asked {
							Ok(Ok(())) => {}
							Ok(Err(error)) => warn!("quit guard: no fresh counts: {error:#}"),
							Err(_) => warn!("quit guard: qursor-core did not answer in time"),
						}
					}
					// **What is working decides whether we ask; what is live decides
					// what dies** (ADR-0020). Both go to the renderer so the dialog
					// can say the second thing rather than imply it.
					let (live, working) = link.as_ref().map(CoreLink::counts).unwrap_or((0, 0));
					if working > 0 {
						api.prevent_close();
						// Emit via the AppHandle, NOT `window.emit`: in Tauri v2 the
						// `Window` from `on_window_event` emits to window-level
						// listeners, but the frontend's `listen()` is registered on
						// the webview, so `window.emit` never reaches it. The app
						// handle broadcasts to the webview. Without this the quit
						// confirm dialog never appeared and the close button did
						// nothing while sessions were live.
						let _ = window.app_handle().emit(
							"app:quit-requested",
							json!({ "liveCount": live, "workingCount": working }),
						);
					} else if link.is_some() {
						// Nothing is working, so nothing is asked — but kill-on-quit
						// is still non-optional (ADR-0005): the core stops with every
						// terminal it holds, footer shells included.
						//
						// Synchronous on purpose, and the close proceeds when it
						// returns: the core's shutdown waits out the terminals' grace
						// before it closes the connection, which a process already
						// gone could not. The core would also follow us out through
						// `--parent-pid`, after that grace.
						tauri::async_runtime::block_on(state.core.shutdown());
					}
					// The close goes ahead: the widget goes with it.
					if working == 0 {
						state.taskbar_widget.stop();
					}
				}
			}
		})
		.invoke_handler(tauri::generate_handler![
			commands::core::core_endpoint,
			commands::core::attention_show,
			commands::core::core_shutdown,
			commands::servers::create_server_folder,
			commands::servers::rename_server_folder,
			commands::servers::delete_server_folder,
			commands::servers::fix_server_file,
			commands::servers::ssh_config_hosts,
			commands::servers::server_agent_project,
			commands::servers::agent_audit,
			commands::profiles::list_profiles,
			commands::profiles::create_profile,
			commands::profiles::rename_profile,
			commands::profiles::set_default_profile,
			commands::profiles::set_app_default_profile,
			commands::profiles::delete_profile,
			commands::profiles::set_project_profile,
			commands::profiles::suggest_profile_dir,
			commands::projects::list_projects,
			commands::projects::add_project,
			commands::projects::remove_project,
			commands::projects::list_import_candidates,
			commands::projects::resolve_project_path,
			commands::sidebar::list_sidebar,
			commands::sidebar::reorder_sidebar,
			commands::sidebar::create_group,
			commands::sidebar::rename_group,
			commands::sidebar::remove_group,
			commands::routines::list_routines,
			commands::routines::create_routine,
			commands::routines::update_routine,
			commands::routines::delete_routine,
			commands::routines::set_routine_enabled,
			commands::routines::run_routine_now,
			commands::routines::routine_pending_fires,
			commands::sessions::list_sessions,
			commands::sessions::get_session_tail,
			commands::sessions::session_transcript_path,
			commands::sessions::list_running_subagents,
			commands::sessions::search_sessions,
			commands::sessions::set_session_pinned,
			commands::sessions::delete_session,
			commands::files::list_dir,
			commands::files::read_file,
			commands::files::write_file,
			commands::files::read_image,
			commands::files::probe_media,
			commands::files::read_pdf,
			commands::files::path_kinds,
			commands::files::reveal_in_file_manager,
			commands::files::watch_file,
			commands::files::unwatch_file,
			commands::canvas::list_canvases,
			commands::canvas::create_canvas,
			commands::canvas::read_canvas,
			commands::canvas::write_canvas,
			commands::canvas::watch_canvas,
			commands::canvas::unwatch_canvas,
			commands::canvas::get_canvas_session,
			commands::canvas::set_canvas_session,
			commands::canvas::list_canvas_origins,
			commands::sops::sops_status,
			commands::sops::sops_decrypt,
			commands::sops::sops_encrypt,
			commands::git::git_status,
			commands::git::git_blob,
			commands::git::git_graph,
			commands::git::git_commit,
			commands::git::git_blob_at,
			commands::git::git_op,
			commands::git::git_check_proposed,
			commands::git::git_run_proposed,
			commands::git::git_ask_agent,
			commands::git::git_ask_cancel,
			commands::git::git_refs,
			commands::git::git_repo_state,
			commands::git::git_file_patch,
			commands::gh::gh_status,
			commands::gh::gh_pr_list,
			commands::gh::gh_pr_detail,
			commands::gh::gh_pr_files,
			commands::gh::gh_pr_blob,
			commands::gh::gh_pr_comments,
			commands::gh::gh_pr_submit_review,
			commands::gh::gh_pr_checkout_plan,
			commands::gh::gh_pr_checkout,
			commands::git::git_worktrees,
			commands::sessions::clear_session_worktree,
			commands::sessions::set_session_worktree,
			commands::settings::get_setting,
			commands::updates::check_update,
			commands::settings::set_setting,
			commands::settings::check_claude_cli,
			commands::settings::validate_claude_binary,
			commands::settings::check_agent_cli,
			commands::settings::validate_agent_binary,
			commands::intelligence::get_intelligence_status,
			commands::intelligence::preview_analysis,
			commands::intelligence::start_analysis,
			commands::intelligence::cancel_analysis,
			commands::intelligence::get_intelligence_run,
			commands::intelligence::latest_intelligence_run,
			commands::intelligence::list_improvements,
			commands::intelligence::dismiss_improvement,
			commands::intelligence::mark_follow_up,
			commands::intelligence::prepare_instruction_change,
			commands::intelligence::record_implementation,
			commands::intelligence::record_verification,
			commands::intelligence::mark_retained,
			commands::intelligence::prepare_revert,
			commands::intelligence::record_revert,
			commands::terminal::shell_name,
			commands::terminal::app_quit_confirmed,
			commands::webview::webview_crash_notice,
			commands::usage::get_usage,
			commands::usage::open_code_go_key_source,
			commands::usage::set_open_code_go_key,
			commands::usage::get_taskbar_widget_config,
			commands::usage::set_taskbar_widget_config,
			commands::usage::session_usage,
			commands::usage::session_plan,
		])
		.run(tauri::generate_context!())
		.expect("error while running factorai");
}

/// How long the quit guard waits for fresh counts before it uses the last ones.
const COUNTS_AT_CLOSE: std::time::Duration = std::time::Duration::from_secs(1);

/// Tell the core whether the window is in front; best effort, like an emit.
fn tell_focus(link: &CoreLink, focused: bool) {
	let link = link.clone();
	tauri::async_runtime::spawn(async move {
		if let Err(error) = link.request("attention.focus", json!({ "focused": focused })).await {
			warn!("could not tell qursor-core about focus: {error:#}");
		}
	});
}

#[cfg(test)]
pub(crate) use qursor_core::test_support;
