//! The core's server: every terminal, served over an authenticated loopback
//! WebSocket (docs/superpowers/specs/2026-11-26-local-core-design.md §1, §3, §4).
//!
//! In this plan the core lives as long as its clients: the last one gone, a
//! grace for a webview reload, then `kill_all` and out — today's kill-on-quit.

mod client;
pub mod record;
pub mod wiring;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};
use zeroize::Zeroizing;

use self::client::Client;
use crate::error::{AppError, AppResult};
use crate::protocol::frame::{self, Op};
use crate::protocol::messages::ServerMsg;
use crate::services::agent_policy::Answer;
use crate::services::event_sink::{CoreEvent, EventSink};
use crate::services::ide::protocol::Mention;
use crate::services::ide::ui_state::UiSnapshot;
use crate::services::routines::Runner;
use crate::services::terminal::{LinkMode, ShellSpawnOpts, SpawnOpts};
use crate::services::{servers, ssh};

/// How long the core waits for a client after the last one left: a webview
/// reload (ADR-0059) reconnects well inside it.
pub const GRACE: Duration = Duration::from_secs(10);

/// How often a `--parent-pid` is checked.
const PARENT_POLL: Duration = Duration::from_millis(500);

pub struct Config {
	pub data_dir: PathBuf,
	pub grace: Duration,
	/// The process whose death ends the core, like the last client leaving.
	pub parent_pid: Option<u32>,
}

pub struct Server {
	pub port: u16,
	pub secret: String,
	/// Fires once the terminals are dead and the server has stopped.
	pub exited: oneshot::Receiver<()>,
	shared: Arc<Shared>,
}

impl Server {
	/// Authenticated clients connected now.
	pub fn client_count(&self) -> usize {
		self.shared.clients.lock().len()
	}
}

pub(crate) struct Shared {
	core: wiring::Core,
	clients: Arc<Clients>,
	secret: String,
	host_id: String,
	grace: Duration,
	/// Records a routine's fire when its session's PTY exists (ADR-0026 § 7).
	routines: Arc<Runner>,
	stop: watch::Sender<bool>,
	exited: Mutex<Option<oneshot::Sender<()>>>,
	/// Set before `kill_all`; written and read under the `clients` lock, so a
	/// join and the grace's last look cannot interleave.
	stopping: AtomicBool,
	/// Bumped by every client that joins, so a grace timer knows it was overtaken.
	joins: AtomicU64,
	next_client: AtomicU64,
}

type Clients = Mutex<HashMap<u64, Arc<Client>>>;

/// The manager's events, out to the clients (§4).
struct WsSink(Arc<Clients>);

impl EventSink for WsSink {
	fn emit(&self, event: CoreEvent) {
		let clients: Vec<Arc<Client>> = self.0.lock().values().cloned().collect();
		match event {
			// Terminal bytes are binary frames, and only for the clients attached.
			CoreEvent::TerminalData { terminal_id, bytes } => {
				for client in clients {
					if let Some(slot) = client.slot_of(&terminal_id) {
						client
							.send(Message::Binary(frame::encode(Op::Output, slot, &bytes).into()));
					}
				}
			}
			event => {
				let text = match serde_json::to_string(&ServerMsg::Event(event)) {
					Ok(text) => tokio_tungstenite::tungstenite::Utf8Bytes::from(text),
					Err(error) => return warn!(%error, "could not encode an event"),
				};
				for client in clients {
					client.send(Message::Text(text.clone()));
				}
			}
		}
	}
}

/// Bind on loopback and serve. Must run inside a tokio runtime, which the
/// manager keeps for its bridges.
pub async fn start(config: Config) -> anyhow::Result<Server> {
	let dir = record::core_dir(&config.data_dir)?;
	let host_id = record::host_id(&dir)?;
	let secret = record::new_secret()?;
	let clients = Arc::new(Clients::default());
	let sink: Arc<dyn EventSink> = Arc::new(WsSink(clients.clone()));
	let core = wiring::build_manager(&config.data_dir, sink.clone())?;
	let live = core.terminals.clone();
	let routines =
		Arc::new(Runner::new(core.db.clone(), sink, Arc::new(move || live.live_session_ids())));
	let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
	let port = listener.local_addr()?.port();
	let (exited_tx, exited) = oneshot::channel();
	let shared = Arc::new(Shared {
		core,
		clients,
		secret: secret.clone(),
		host_id,
		grace: config.grace,
		routines,
		stop: watch::channel(false).0,
		exited: Mutex::new(Some(exited_tx)),
		stopping: AtomicBool::new(false),
		joins: AtomicU64::new(0),
		next_client: AtomicU64::new(1),
	});
	tokio::spawn(accept(listener, shared.clone()));
	// A core nobody ever connects to leaves too.
	shared.arm_grace();
	if let Some(pid) = config.parent_pid {
		tokio::spawn(watch_parent(pid, shared.clone()));
	}
	info!(port, "qursor-core listening");
	Ok(Server { port, secret, exited, shared })
}

async fn accept(listener: TcpListener, shared: Arc<Shared>) {
	let mut stop = shared.stop.subscribe();
	loop {
		tokio::select! {
			_ = stop.wait_for(|stopped| *stopped) => return,
			accepted = listener.accept() => match accepted {
				Ok((stream, _)) => {
					tokio::spawn(client::serve(stream, shared.clone()));
				}
				Err(error) => warn!(%error, "accept failed"),
			},
		}
	}
}

async fn watch_parent(pid: u32, shared: Arc<Shared>) {
	while crate::services::ide::lockfile::pid_is_alive(pid) {
		tokio::time::sleep(PARENT_POLL).await;
	}
	info!(pid, "parent process gone; shutting down");
	shared.shutdown();
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalParams {
	terminal_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectParams {
	project_id: String,
}

fn parse<T: DeserializeOwned>(params: Value) -> AppResult<T> {
	serde_json::from_value(params).map_err(|e| AppError::InvalidInput(e.to_string()))
}

fn to_value(value: impl serde::Serialize) -> AppResult<Value> {
	serde_json::to_value(value).map_err(|e| AppError::Process(e.to_string()))
}

/// Off the runtime's threads, as the app's `commands::off_main`: a spawn runs
/// `which`, binds sockets and forks.
async fn blocking<T: Send + 'static>(
	f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
	tokio::task::spawn_blocking(f).await.map_err(|e| AppError::Process(e.to_string()))?
}

impl Shared {
	/// `None` once the core is stopping.
	fn joined(&self, client: Arc<Client>) -> Option<u64> {
		let mut clients = self.clients.lock();
		if self.stopping.load(Ordering::SeqCst) {
			return None;
		}
		let id = self.next_client.fetch_add(1, Ordering::Relaxed);
		self.joins.fetch_add(1, Ordering::SeqCst);
		clients.insert(id, client);
		Some(id)
	}

	fn left(self: &Arc<Self>, id: u64) {
		let mut clients = self.clients.lock();
		clients.remove(&id);
		if clients.is_empty() {
			drop(clients);
			self.arm_grace();
		}
	}

	/// Shut down after the grace unless a client joins first.
	fn arm_grace(self: &Arc<Self>) {
		let joins = self.joins.load(Ordering::SeqCst);
		let me = self.clone();
		tokio::spawn(async move {
			tokio::time::sleep(me.grace).await;
			{
				let clients = me.clients.lock();
				if me.joins.load(Ordering::SeqCst) != joins || !clients.is_empty() {
					return;
				}
				me.stopping.store(true, Ordering::SeqCst);
			}
			info!("no client came back; shutting down");
			me.shutdown();
		});
	}

	/// Kill every terminal, stop serving, signal `exited`. Once.
	fn shutdown(self: &Arc<Self>) {
		// Under the lock, so it orders with `joined`.
		let clients = self.clients.lock();
		self.stopping.store(true, Ordering::SeqCst);
		drop(clients);
		let Some(exited) = self.exited.lock().take() else { return };
		let me = self.clone();
		tokio::spawn(async move {
			let terminals = me.core.terminals.clone();
			if let Err(error) = tokio::task::spawn_blocking(move || terminals.kill_all()).await {
				warn!(%error, "kill_all failed");
			}
			me.core.ssh.disconnect_all();
			me.stop.send_replace(true);
			let _ = exited.send(());
		});
	}

	/// A shell tab on a server, as the app's `server_open_shell`. An unknown host
	/// key comes back as `AppError::HostKey(fingerprint)`; the client asks the
	/// human and calls again with `acceptFingerprint`.
	async fn open_shell(
		&self,
		server_id: String,
		client_key: String,
		cols: u16,
		rows: u16,
		accept_fingerprint: Option<String>,
	) -> AppResult<String> {
		let terminals = &self.core.terminals;
		let server = self.core.db.with(|c| servers::get(c, &server_id))?;
		let label = server.start_dir.clone().unwrap_or_else(|| "~".into());
		if server.auth == "system" {
			let argv = servers::system_argv(&server);
			return terminals.spawn_ssh_command(&server.id, client_key, argv, cols, rows);
		}
		let vault = self.core.vault.clone();
		let for_target = server.clone();
		let conn = self
			.core
			.ssh
			.connection(
				&server.id,
				|| async move { servers::target_for(&for_target, &vault) },
				accept_fingerprint,
			)
			.await?;
		// Persistent only where the server has tmux (F49).
		let tmux = if server.persistent && self.core.files.has_tmux(&server.id).await {
			Some(ssh::tmux_name(&client_key))
		} else {
			None
		};
		let channel =
			ssh::open_shell(&conn, cols, rows, server.start_dir.as_deref(), tmux.as_deref())
				.await?;
		let (master, child) = ssh::pty::bridge(channel, cols, rows);
		let id = terminals.attach_ssh(&server.id, client_key, &label, master, child)?;
		if let Some(name) = tmux {
			terminals.set_tmux(&id, name);
		}
		Ok(id)
	}

	/// End a persistent tab's tmux session (F49), as the app's `server_end_tab`.
	async fn end_tab(&self, server_id: String, client_key: String) -> AppResult<()> {
		let server = self.core.db.with(|c| servers::get(c, &server_id))?;
		if !server.persistent || server.auth == "system" {
			return Ok(());
		}
		let name = ssh::tmux_name(&client_key);
		self.core
			.files
			.with(&server_id, |conn, _| async move {
				let ch = conn
					.channel_open_session()
					.await
					.map_err(|e| AppError::Remote(e.to_string()))?;
				// Best effort: a session that is already gone is the outcome wanted.
				let _ = ch
					.exec(true, format!("tmux kill-session -t {} 2>/dev/null", ssh::quote(&name)))
					.await;
				Ok(())
			})
			.await
	}

	/// The RPC methods (§3): the app's terminal commands, by their wire names.
	async fn rpc(
		self: &Arc<Self>,
		client: &Arc<Client>,
		method: &str,
		params: Value,
	) -> AppResult<Value> {
		let terminals = &self.core.terminals;
		match method {
			"terminal.spawn" => {
				let opts: SpawnOpts = parse(params)?;
				let (terminals, routines) = (terminals.clone(), self.routines.clone());
				let (id, agent) = blocking(move || {
					let session_id = opts.session_id.clone();
					let delegated = terminals
						.delegations
						.begin(&opts, |parent| terminals.can_start_delegate(parent))?;
					let result = terminals.spawn(opts);
					if delegated {
						terminals.delegations.started(&session_id, result.is_ok());
					}
					let id = result?;
					let agent = terminals.agent_of_terminal(&id);
					// The PTY exists: this is where a routine's fire becomes a run.
					routines.mark_started(&session_id, crate::epoch_ms());
					Ok((id, agent))
				})
				.await?;
				let slot = client.attach(&id)?;
				Ok(json!({ "id": id, "agent": agent, "slot": slot }))
			}
			"shell.spawn" => {
				let opts: ShellSpawnOpts = parse(params)?;
				let terminals = terminals.clone();
				let id = blocking(move || terminals.spawn_shell(opts)).await?;
				let slot = client.attach(&id)?;
				Ok(json!({ "id": id, "slot": slot }))
			}
			"terminal.attach" => {
				let TerminalParams { terminal_id } = parse(params)?;
				if !terminals.list().iter().any(|t| t.id == terminal_id) {
					return Err(AppError::NotFound(format!("terminal {terminal_id}")));
				}
				Ok(json!({ "slot": client.attach(&terminal_id)? }))
			}
			"terminal.detach" => {
				#[derive(Deserialize)]
				struct P {
					slot: u16,
				}
				let P { slot } = parse(params)?;
				client.detach(slot);
				Ok(Value::Null)
			}
			"terminal.kill" => {
				#[derive(Deserialize)]
				struct P {
					id: String,
				}
				let P { id } = parse(params)?;
				terminals.kill(&id)?;
				Ok(Value::Null)
			}
			"terminal.list" => to_value(terminals.list()),
			"terminal.visibility" => {
				#[derive(Deserialize)]
				#[serde(rename_all = "camelCase")]
				struct P {
					terminal_id: String,
					visible: bool,
				}
				let P { terminal_id, visible } = parse(params)?;
				client.set_visible(&self.core.ui, &terminal_id, visible);
				Ok(Value::Null)
			}
			"shell.killForProject" => {
				let ProjectParams { project_id } = parse(params)?;
				terminals.kill_shells_for_project(&project_id);
				Ok(Value::Null)
			}
			"session.start" => {
				#[derive(Deserialize)]
				#[serde(rename_all = "camelCase")]
				struct P {
					project_id: String,
					agent: Option<String>,
					fresh: Option<bool>,
				}
				let P { project_id, agent, fresh } = parse(params)?;
				let (terminals, db) = (terminals.clone(), self.core.db.clone());
				let id = blocking(move || {
					let folder = db
						.with(|conn| crate::services::workspace::project_path(conn, &project_id))?;
					terminals.next_session_id(
						&project_id,
						&folder,
						agent.as_deref(),
						fresh.unwrap_or(false),
					)
				})
				.await?;
				Ok(Value::String(id))
			}
			"delegations.list" => {
				let ProjectParams { project_id } = parse(params)?;
				to_value(terminals.delegations.list(&project_id))
			}
			"delegations.resolve" => {
				#[derive(Deserialize)]
				struct P {
					id: String,
					approve: bool,
				}
				let P { id, approve } = parse(params)?;
				let start = terminals
					.delegations
					.decide(&id, approve, |parent| terminals.can_start_delegate(parent))?;
				to_value(start)
			}
			"ide.reportUi" => {
				let snapshot: UiSnapshot = parse(params)?;
				self.core.ui.set(snapshot);
				Ok(Value::Null)
			}
			"ide.resync" => {
				terminals.resync_ide_status();
				Ok(Value::Null)
			}
			"ide.mention" => {
				#[derive(Deserialize)]
				#[serde(rename_all = "camelCase")]
				struct P {
					session_id: String,
					mentions: Vec<Mention>,
				}
				let P { session_id, mentions } = parse(params)?;
				let terminals = terminals.clone();
				blocking(move || terminals.mention(&session_id, &mentions)).await?;
				Ok(Value::Null)
			}
			"server.openShell" => {
				#[derive(Deserialize)]
				#[serde(rename_all = "camelCase")]
				struct P {
					server_id: String,
					client_key: String,
					cols: u16,
					rows: u16,
					accept_fingerprint: Option<String>,
				}
				let p: P = parse(params)?;
				let id = self
					.open_shell(p.server_id, p.client_key, p.cols, p.rows, p.accept_fingerprint)
					.await?;
				let slot = client.attach(&id)?;
				Ok(json!({ "id": id, "slot": slot }))
			}
			"server.endTab" => {
				#[derive(Deserialize)]
				#[serde(rename_all = "camelCase")]
				struct P {
					server_id: String,
					client_key: String,
				}
				let P { server_id, client_key } = parse(params)?;
				self.end_tab(server_id, client_key).await?;
				Ok(Value::Null)
			}
			"agentTerminal.link" => {
				#[derive(Deserialize)]
				#[serde(rename_all = "camelCase")]
				struct P {
					terminal_id: String,
					session_id: String,
					mode: LinkMode,
				}
				let P { terminal_id, session_id, mode } = parse(params)?;
				terminals.link_agent(&terminal_id, &session_id, mode)?;
				Ok(Value::Null)
			}
			"agentTerminal.unlink" => {
				let TerminalParams { terminal_id } = parse(params)?;
				terminals.unlink_agent(&terminal_id);
				Ok(Value::Null)
			}
			"agentTerminal.answer" => {
				#[derive(Deserialize)]
				struct P {
					id: String,
					run: bool,
					command: Option<String>,
				}
				let P { id, run, command } = parse(params)?;
				let answer = match (run, command.filter(|c| !c.trim().is_empty())) {
					(false, _) => Answer::Refuse,
					(true, Some(c)) => Answer::RunEdited(c),
					(true, None) => Answer::Run,
				};
				if !self.core.confirmations.answer(&id, answer) {
					return Err(AppError::NotFound(
						"that confirmation is no longer waiting".into(),
					));
				}
				Ok(Value::Null)
			}
			// The password crosses once, on the authenticated socket, and is
			// never logged (§5a); no answer carries a secret.
			"vault.status" => to_value(self.core.vault.status()),
			"vault.unlock" => {
				#[derive(Deserialize)]
				struct P {
					password: String,
				}
				let password = Zeroizing::new(parse::<P>(params)?.password);
				self.core.vault.unlock(&password)?;
				to_value(self.core.vault.status())
			}
			"vault.lock" => {
				self.core.vault.lock();
				to_value(self.core.vault.status())
			}
			"vault.setMasterPassword" => {
				#[derive(Deserialize)]
				struct P {
					password: Option<String>,
				}
				let password = parse::<P>(params)?.password.map(Zeroizing::new);
				self.core.vault.set_master_password(password.as_deref().map(String::as_str))?;
				to_value(self.core.vault.status())
			}
			"attention.focus" => {
				#[derive(Deserialize)]
				struct P {
					focused: bool,
				}
				let P { focused } = parse(params)?;
				self.core.attention.set_focused(focused);
				Ok(Value::Null)
			}
			"core.counts" => {
				Ok(json!({ "live": terminals.live_count(), "working": terminals.working_count() }))
			}
			"core.shutdown" => {
				#[derive(Deserialize)]
				#[serde(rename_all = "camelCase")]
				struct P {
					keep_terminals: bool,
				}
				let P { keep_terminals } = parse(params)?;
				if keep_terminals {
					return Err(AppError::InvalidInput(
						"keeping terminals is not supported yet".into(),
					));
				}
				self.shutdown();
				Ok(Value::Null)
			}
			other => Err(AppError::NotFound(format!("method {other}"))),
		}
	}
}
