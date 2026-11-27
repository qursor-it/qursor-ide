//! The terminal manager as the app builds it, read from the same database
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §5). Each callback is
//! the app's own from `lib.rs` setup, minus what still lives only in the app:
//! the usage limits tool and everything that resolves a server from the
//! address book (agent policy, server labels, remote files).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{params, Connection};
use tracing::warn;

use crate::agents;
use crate::attention::{AttentionNotifier, EventNotify, SETTLE};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::SettingKey;
use crate::services::event_sink::{CoreEvent, EventSink};
use crate::services::ide::ui_state::UiState;
use crate::services::terminal::{CanvasStore, RoutineStore, TerminalManager, WorktreeStore};
use crate::services::{canvas, profiles, routines, sessions, settings, shell_path};

/// What the server holds beside the manager.
pub struct Core {
	pub terminals: TerminalManager,
	pub db: Db,
	/// What clients have on screen: `ide.reportUi` and `terminal.visibility`.
	pub ui: Arc<UiState>,
	/// Told by `attention.focus` whether a client window is in front.
	pub attention: Arc<AttentionNotifier>,
}

pub fn build_manager(data_dir: &Path, sink: Arc<dyn EventSink>) -> anyhow::Result<Core> {
	let db = Db::open(data_dir)?;
	// A spawn needs the login shell's PATH; ask for it now, off this thread.
	shell_path::warm();
	let ui = Arc::new(UiState::default());
	let notify =
		Arc::new(EventNotify { db: db.clone(), events: sink.clone(), context: Default::default() });
	let attention = Arc::new(AttentionNotifier::new(notify.clone(), SETTLE));
	let attention_tap = attention.clone();
	let opencode_signals = data_dir.join("opencode");
	if let Err(error) = agents::opencode::prepare_signal_dir(&opencode_signals) {
		warn!(%error, "could not prepare the OpenCode session signal folder");
	}
	let mut terminals = TerminalManager::new(
		sink.clone(),
		crate::claude_dir(),
		ui.clone(),
		tokio::runtime::Handle::current(),
	)
	.with_opencode_signal_dir(opencode_signals)
	.with_status_tap(move |e| attention_tap.on_status(e))
	.with_usage_sink_dir(data_dir.join("usage").join("statusline"))
	.with_usage_statusline_enabled({
		let db = db.clone();
		Arc::new(move || setting(&db, SettingKey::UsageStatusLine).as_deref() != Some("off"))
	})
	.with_delegation_confirmation({
		let db = db.clone();
		Arc::new(move || {
			setting(&db, SettingKey::ConfirmAgentDelegations).as_deref() != Some("off")
		})
	})
	.with_user_binary({
		let db = db.clone();
		Arc::new(move |agent| settings::binary_override(&db, agent))
	})
	.with_agent_of({
		let db = db.clone();
		Arc::new(move |project_id, session_id| {
			profiles::agent_for_spawn(&db, project_id, session_id)
		})
	})
	.with_session_cwd({
		let db = db.clone();
		Arc::new(move |session_id| sessions::recorded_cwds(&db, session_id))
	})
	.with_session_key({
		let db = db.clone();
		Arc::new(move |session_id| sessions::recorded_key(&db, session_id))
	})
	.with_profile_dir({
		let db = db.clone();
		Arc::new(move |agent, project_id, session_id| {
			profiles::config_dir_for_spawn(&db, agent, project_id, session_id).or_else(|| {
				if agent != agents::OPENCODE {
					return None;
				}
				let descriptor = agents::descriptor(agent)?;
				let root = agents::ambient_dir(descriptor);
				profiles::ensure_default_for(&db, agent, &root)
					.ok()
					.map(|profile| PathBuf::from(profile.config_dir))
			})
		})
	})
	.with_worktree_store(WorktreeStore {
		get: {
			let db = db.clone();
			Arc::new(move |session_id| sessions::worktree(&db, session_id).map(PathBuf::from))
		},
		set: {
			let db = db.clone();
			Arc::new(move |session_id, path| {
				// Logged rather than propagated: the caller is a socket handler
				// answering an agent.
				if let Err(e) = sessions::set_worktree(
					&db,
					session_id,
					&path.to_string_lossy(),
					crate::epoch_ms(),
				) {
					warn!(error = %e, session_id, "could not record a session's worktree");
				}
			})
		},
	})
	.with_routine_store(RoutineStore {
		list: {
			let db = db.clone();
			Arc::new(move |project_id| {
				db.with(|conn| routines::list(conn, project_id, crate::epoch_ms()))
			})
		},
		create: {
			let (db, sink) = (db.clone(), sink.clone());
			Arc::new(move |input, author| {
				routines::create_and_announce(
					&db,
					sink.as_ref(),
					input,
					Some(author),
					crate::epoch_ms(),
				)
			})
		},
		update: {
			let (db, sink) = (db.clone(), sink.clone());
			Arc::new(move |id, patch, author| {
				routines::update_and_announce(
					&db,
					sink.as_ref(),
					id,
					patch,
					Some(author),
					crate::epoch_ms(),
				)
			})
		},
	})
	.with_canvas_store(CanvasStore {
		root: {
			let db = db.clone();
			Arc::new(move |project_id| db.with(|conn| project_path(conn, project_id)))
		},
		linked: {
			let db = db.clone();
			Arc::new(move |project_id, session_id| {
				db.with(|conn| canvas::canvas_for_session(conn, project_id, session_id))
			})
		},
	});
	// Every session a delegate runs under stays out of the lists for good (ADR-0126).
	{
		let db = db.clone();
		terminals.delegations.set_on_session(Arc::new(move |session_id| {
			if let Err(error) = db.with_mut(|conn| {
				conn.execute(
					"INSERT OR IGNORE INTO delegate_sessions(session_id) VALUES (?1)",
					[session_id],
				)?;
				Ok(())
			}) {
				warn!(%error, "could not record a delegate session");
			}
		}));
	}
	let run_sink = sink.clone();
	terminals.set_agent_run_cb(Arc::new(move |e| run_sink.emit(CoreEvent::AgentTerminalRun(e))));
	// The notifier holds a manager clone: a cycle that keeps the Drop backstop
	// from firing, so every exit path calls `kill_all` first.
	let context = terminals.clone();
	let _ = notify.context.set(Box::new(move |id| context.alert_context(id)));
	Ok(Core { terminals, db, ui, attention })
}

fn setting(db: &Db, key: SettingKey) -> Option<String> {
	db.with(|conn| settings::get(conn, key)).ok().flatten()
}

/// A project's folder, as the app's `commands::projects::project_path`.
pub fn project_path(conn: &Connection, id: &str) -> AppResult<PathBuf> {
	conn.query_row("SELECT real_path FROM projects WHERE id = ?1", params![id], |r| {
		r.get::<_, String>(0)
	})
	.map(PathBuf::from)
	.map_err(|_| AppError::NotFound(format!("project {id}")))
}
