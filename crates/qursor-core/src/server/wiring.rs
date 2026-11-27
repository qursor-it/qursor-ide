//! The terminal manager as the app builds it, read from the same database
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §5, §5a, §5b). Each
//! callback is the app's own from `lib.rs` setup.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::warn;

use crate::agents;
use crate::attention::{AttentionNotifier, EventNotify, SETTLE};
use crate::db::Db;
use crate::models::SettingKey;
use crate::services::agent_policy::{self, Confirmations};
use crate::services::event_sink::{CoreEvent, EventSink};
use crate::services::ide::ui_state::UiState;
use crate::services::ssh::pool::Pool;
use crate::services::ssh::sftp::RemoteFiles;
use crate::services::ssh::tunnel::Tunnels;
use crate::services::ssh::KnownHosts;
use crate::services::terminal::{
	CanvasStore, PolicyCb, RoutineStore, TerminalManager, WorktreeStore,
};
use crate::services::usage::mcp::{self, GoUsage};
use crate::services::usage::store::UsageStore;
use crate::services::vault::{self, Vault};
use crate::services::{
	canvas, profiles, routines, servers, sessions, settings, shell_path, workspace,
};

/// What the server holds beside the manager.
pub struct Core {
	pub terminals: TerminalManager,
	pub db: Db,
	/// What clients have on screen: `ide.reportUi` and `terminal.visibility`.
	pub ui: Arc<UiState>,
	/// Told by `attention.focus` whether a client window is in front.
	pub attention: Arc<AttentionNotifier>,
	/// The SSH secrets (ADR-0106): the core owns them (§5a).
	pub vault: Arc<Vault>,
	/// One login per server, carrying its tabs and SFTP.
	pub ssh: Arc<Pool>,
	/// SFTP on those logins, for agents and for a tab's tmux.
	pub files: Arc<RemoteFiles>,
	/// Agent commands waiting for the human (ADR-0110).
	pub confirmations: Arc<Confirmations>,
	/// Running local port forwards (F48).
	pub tunnels: Arc<Tunnels>,
}

pub fn build_manager(data_dir: &Path, sink: Arc<dyn EventSink>) -> anyhow::Result<Core> {
	// Only the app migrates (§5).
	let db = Db::open_existing(data_dir)?;
	// Before anything spawns a child: `GoUsage` takes the Go key out of the
	// environment so no child inherits it.
	let vault = Arc::new(vault::open_or_set_aside(data_dir.join("secrets.vault")));
	let go_usage = Arc::new(GoUsage::new(vault.clone()));
	// The app's poller writes it; the core only reads it (§5b).
	let usage = Arc::new(UsageStore::read_only(data_dir.join("usage.json")));
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
	.with_usage_limits({
		let db = db.clone();
		Arc::new(move |project, filter| mcp::read(&db, &usage, &go_usage, project, filter))
	})
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
			Arc::new(move |project_id| db.with(|conn| workspace::project_path(conn, project_id)))
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
	let ssh = Arc::new(Pool::new(
		KnownHosts::for_user(servers::known_hosts_path(data_dir)),
		tokio::runtime::Handle::current(),
	));
	let files = Arc::new(RemoteFiles::new(ssh.clone(), {
		let (db, vault) = (db.clone(), vault.clone());
		Arc::new(move |id| {
			let server = db.with(|c| servers::get(c, id))?;
			servers::target_for(&server, &vault)
		})
	}));
	terminals.set_remote_files(files.clone());
	let run_sink = sink.clone();
	terminals.set_agent_run_cb(Arc::new(move |e| run_sink.emit(CoreEvent::AgentTerminalRun(e))));
	let confirmations = Arc::new(Confirmations::default());
	terminals.set_agent_policy(agent_policy(db.clone(), confirmations.clone(), sink.clone()));
	terminals.set_server_label({
		let db = db.clone();
		Arc::new(move |id| {
			db.with(|c| servers::get(c, id))
				.map(|s| format!("{} ({}@{})", s.name, s.username, s.host))
				.unwrap_or_else(|_| id.to_string())
		})
	});
	// The notifier holds a manager clone: a cycle that keeps the Drop backstop
	// from firing, so every exit path calls `kill_all` first.
	let context = terminals.clone();
	let _ = notify.context.set(Box::new(move |id| context.alert_context(id)));
	Ok(Core {
		terminals,
		db,
		ui,
		attention,
		vault,
		ssh,
		files,
		confirmations,
		tunnels: Arc::default(),
	})
}

/// What an agent may do on each server (ADR-0110): decided per command from the
/// address book, asked of the human when the policy says so, and written to the
/// local audit log. Never left unset: without it every command runs.
pub fn agent_policy(
	db: Db,
	confirmations: Arc<Confirmations>,
	sink: Arc<dyn EventSink>,
) -> PolicyCb {
	Arc::new(move |req| {
		agent_policy::decide(&db, &confirmations, req, |ev| {
			sink.emit(CoreEvent::AgentTerminalConfirm(ev.clone()));
		})
	})
}

fn setting(db: &Db, key: SettingKey) -> Option<String> {
	db.with(|conn| settings::get(conn, key)).ok().flatten()
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use parking_lot::Mutex;

	use super::*;
	use crate::services::agent_policy::Answer;
	use crate::services::terminal::PolicyRequest;

	#[derive(Default)]
	struct Asked(Mutex<Vec<String>>);
	impl EventSink for Asked {
		fn emit(&self, event: CoreEvent) {
			if let CoreEvent::AgentTerminalConfirm(ev) = event {
				self.0.lock().push(ev.id);
			}
		}
	}

	fn setup(policy: &str) -> (tempfile::TempDir, Arc<Asked>, Arc<Confirmations>, PolicyCb) {
		let dir = tempfile::TempDir::new().unwrap();
		let db = Db::open(dir.path()).unwrap();
		db.with(|c| {
			c.execute(
				"INSERT INTO servers(id, name, host, username, agent_policy, auth, created_at)
				 VALUES ('s1', 'web', 'h', 'u', ?1, 'agent', 0)",
				[policy],
			)?;
			Ok(())
		})
		.unwrap();
		let asked = Arc::new(Asked::default());
		let confirmations = Arc::new(Confirmations::default());
		let cb = agent_policy(db, confirmations.clone(), asked.clone());
		(dir, asked, confirmations, cb)
	}

	fn req(server_id: &str, command: &str) -> PolicyRequest {
		PolicyRequest {
			server_id: server_id.into(),
			terminal_id: "t1".into(),
			session_id: "sess".into(),
			command: command.into(),
			keys: false,
			editable: true,
		}
	}

	#[test]
	fn a_server_not_in_the_address_book_runs_nothing() {
		let (_dir, _asked, _c, cb) = setup("free");
		assert!(cb(&req("gone", "ls")).is_err());
	}

	#[test]
	fn readonly_refuses_a_command_that_acts() {
		let (_dir, _asked, _c, cb) = setup("readonly");
		assert!(cb(&req("s1", "rm -rf /")).is_err());
		assert_eq!(cb(&req("s1", "ls -la")), Ok(None));
	}

	#[test]
	fn confirm_waits_for_the_human_and_runs_nothing_meanwhile() {
		let (_dir, asked, confirmations, cb) = setup("confirm");
		let call = std::thread::spawn(move || cb(&req("s1", "systemctl restart nginx")));
		std::thread::sleep(Duration::from_millis(300));
		assert!(!call.is_finished(), "a command needing confirmation ran unanswered");
		let id = asked.0.lock().first().cloned().expect("the confirmation was announced");
		assert!(confirmations.answer(&id, Answer::Refuse));
		assert!(call.join().unwrap().is_err());
	}
}
