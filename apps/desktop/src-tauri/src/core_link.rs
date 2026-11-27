//! The app's link to its qursor-core: find the one serving this data folder or
//! start it, then stay connected as the `app` client
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §1-§3).
//!
//! The renderer talks to the core itself; this link is for what only the app
//! can do — the quit guard's counts, the shutdown on quit, window focus for the
//! attention notifier, and the live session ids the indexer must not reap.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use qursor_core::protocol::PROTOCOL_VERSION;
use qursor_core::server::record::{self, Record};
use qursor_core::services::ide::lockfile::pid_is_alive;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::AsyncBufReadExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::error::{AppError, AppResult};

/// A recorded core gets this long to answer before another is started.
const HANDSHAKE: Duration = Duration::from_secs(3);
/// From spawn to the `ready` line (design §1).
const READY: Duration = Duration::from_secs(10);
/// `core.shutdown` until the socket closes.
const SHUTDOWN: Duration = Duration::from_secs(3);
/// The core closes a client silent for two of these (§3).
const PING: Duration = Duration::from_secs(10);

type Ws = tokio_tungstenite::WebSocketStream<TcpStream>;

/// Where the renderer finds the core. Mirrored as `CoreEndpoint` in
/// `@factorai/types`; the secret never goes to storage or a log (§3).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreEndpoint {
	pub url: String,
	pub secret: String,
	pub host_id: String,
}

/// An `rpc_error`'s payload. `AppError` only serializes, so the app reads the
/// two fields it needs.
#[derive(Debug, Deserialize)]
struct WireError {
	kind: String,
	message: Option<String>,
}

impl std::fmt::Display for WireError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match &self.message {
			Some(message) => write!(f, "{}: {message}", self.kind),
			None => f.write_str(&self.kind),
		}
	}
}

impl std::error::Error for WireError {}

type Reply = Result<Value, anyhow::Error>;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Counts {
	live: usize,
	working: usize,
	session_ids: HashSet<String>,
}

struct Inner {
	endpoint: CoreEndpoint,
	pid: u32,
	out: mpsc::UnboundedSender<Message>,
	pending: Mutex<HashMap<u64, oneshot::Sender<Reply>>>,
	next_id: AtomicU64,
	/// The last `core.counts`, and the request that produced it: answers can
	/// arrive out of order, and an older one must not overwrite a newer one.
	counts: Mutex<(u64, Counts)>,
	closed: watch::Receiver<bool>,
}

#[derive(Clone)]
pub struct CoreLink(Arc<Inner>);

impl CoreLink {
	/// Use the core `core.json` names if it is alive and answers with our
	/// secret and protocol; otherwise forget that record and start one.
	pub async fn ensure(data_dir: &Path, app_version: &str, binary: &Path) -> anyhow::Result<Self> {
		let dir = record::core_dir(data_dir)?;
		if let Some(rec) = read_record(&dir) {
			if pid_is_alive(rec.pid) && rec.protocol_version == PROTOCOL_VERSION {
				match handshake(rec.port, &rec.secret, app_version).await {
					Ok((ws, host_id)) => return Ok(Self::run(ws, rec, host_id)),
					Err(error) => {
						warn!("the recorded core did not answer ({error:#}); starting one")
					}
				}
			}
			record::remove(&dir);
		}
		let (pid, port) = spawn(binary, data_dir).await?;
		let rec = read_record(&dir).context("qursor-core is ready but wrote no record")?;
		if rec.pid != pid || rec.port != port {
			bail!("qursor-core's record names another process");
		}
		let (ws, host_id) = handshake(port, &rec.secret, app_version).await?;
		info!(pid, port, "qursor-core started");
		Ok(Self::run(ws, rec, host_id))
	}

	pub fn endpoint(&self) -> CoreEndpoint {
		self.0.endpoint.clone()
	}

	pub fn pid(&self) -> u32 {
		self.0.pid
	}

	/// One RPC; an `rpc_error` comes back as the error.
	pub async fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
		request(&self.0, method, params).await
	}

	/// `(live, working)`: every terminal, and the agents working now.
	pub fn counts(&self) -> (usize, usize) {
		let counts = self.0.counts.lock();
		(counts.1.live, counts.1.working)
	}

	/// Both ids of every live agent session (ADR-0062).
	pub fn live_session_ids(&self) -> HashSet<String> {
		self.0.counts.lock().1.session_ids.clone()
	}

	/// Kill every terminal and stop the core (`keepTerminals: false`, today's
	/// kill-on-quit), and wait for it to close the connection.
	pub async fn shutdown(&self) -> anyhow::Result<()> {
		let mut closed = self.0.closed.clone();
		tokio::time::timeout(SHUTDOWN, async {
			let asked = self.request("core.shutdown", json!({ "keepTerminals": false })).await;
			// A connection that closed under the request is the outcome wanted.
			if let Err(error) = asked {
				if !*closed.borrow() {
					return Err(error);
				}
			}
			let _ = closed.wait_for(|closed| *closed).await;
			Ok(())
		})
		.await
		.context("qursor-core did not stop within 3 s")?
	}

	fn run(ws: Ws, rec: Record, host_id: String) -> Self {
		let (mut sink, mut stream) = ws.split();
		let (out, mut outgoing) = mpsc::unbounded_channel::<Message>();
		let (closed_tx, closed) = watch::channel(false);
		let inner = Arc::new(Inner {
			endpoint: CoreEndpoint {
				url: format!("ws://127.0.0.1:{}/", rec.port),
				secret: rec.secret,
				host_id,
			},
			pid: rec.pid,
			out,
			pending: Mutex::default(),
			next_id: AtomicU64::new(1),
			counts: Mutex::default(),
			closed,
		});
		tokio::spawn(async move {
			while let Some(msg) = outgoing.recv().await {
				if sink.send(msg).await.is_err() {
					break;
				}
			}
		});
		let pinger = inner.clone();
		tokio::spawn(async move {
			let mut closed = pinger.closed.clone();
			loop {
				tokio::select! {
					_ = closed.wait_for(|closed| *closed) => return,
					_ = tokio::time::sleep(PING) => {
						let ping = Message::Text(json!({ "type": "ping" }).to_string().into());
						if pinger.out.send(ping).is_err() {
							return;
						}
					}
				}
			}
		});
		let reader = inner.clone();
		tokio::spawn(async move {
			while let Some(Ok(msg)) = stream.next().await {
				if let Message::Text(text) = msg {
					on_text(&reader, &text);
				}
			}
			let _ = closed_tx.send(true);
			for (_, call) in reader.pending.lock().drain() {
				let _ = call.send(Err(anyhow::anyhow!("the connection to qursor-core closed")));
			}
			warn!("the connection to qursor-core closed");
		});
		// A core we found may already have terminals.
		refresh_counts(&inner);
		Self(inner)
	}
}

async fn request(inner: &Inner, method: &str, params: Value) -> anyhow::Result<Value> {
	let id = inner.next_id.fetch_add(1, Ordering::Relaxed);
	let (tx, rx) = oneshot::channel();
	inner.pending.lock().insert(id, tx);
	let msg = json!({ "type": "request", "requestId": id, "method": method, "params": params });
	if inner.out.send(Message::Text(msg.to_string().into())).is_err() {
		inner.pending.lock().remove(&id);
		bail!("the connection to qursor-core is closed");
	}
	rx.await.context("the connection to qursor-core closed")?
}

fn on_text(inner: &Arc<Inner>, text: &str) {
	let Ok(msg) = serde_json::from_str::<Value>(text) else {
		return warn!("qursor-core sent a frame that is not JSON");
	};
	match msg["type"].as_str() {
		Some("response" | "rpc_error") => {
			let Some(id) = msg["requestId"].as_u64() else { return };
			let Some(call) = inner.pending.lock().remove(&id) else { return };
			let reply = if msg["type"] == "response" {
				Ok(msg["result"].clone())
			} else {
				Err(match serde_json::from_value::<WireError>(msg["error"].clone()) {
					Ok(error) => error.into(),
					Err(error) => error.into(),
				})
			};
			let _ = call.send(reply);
		}
		// What changes the counts or a session's ids.
		Some("event")
			if matches!(
				msg["event"].as_str(),
				Some("terminalStatus" | "terminalExit" | "sessionAdopted")
			) =>
		{
			refresh_counts(inner)
		}
		_ => {}
	}
}

fn refresh_counts(inner: &Arc<Inner>) {
	let inner = inner.clone();
	tokio::spawn(async move {
		let seq = inner.next_id.load(Ordering::Relaxed);
		match request(&inner, "core.counts", Value::Null).await {
			Ok(value) => match serde_json::from_value::<Counts>(value) {
				Ok(counts) => {
					let mut held = inner.counts.lock();
					if seq >= held.0 {
						*held = (seq, counts);
					}
				}
				Err(error) => warn!(%error, "qursor-core's counts did not parse"),
			},
			Err(error) => warn!("could not read qursor-core's counts: {error:#}"),
		}
	});
}

fn read_record(dir: &Path) -> Option<Record> {
	let bytes = std::fs::read(dir.join("core.json")).ok()?;
	match serde_json::from_slice(&bytes) {
		Ok(rec) => Some(rec),
		Err(error) => {
			warn!(%error, "core.json does not parse");
			None
		}
	}
}

/// Connect, `hello`, and wait for a `server_info` speaking our protocol.
async fn handshake(port: u16, secret: &str, app_version: &str) -> anyhow::Result<(Ws, String)> {
	tokio::time::timeout(HANDSHAKE, async {
		let stream = TcpStream::connect(("127.0.0.1", port)).await?;
		let url = format!("ws://127.0.0.1:{port}/");
		let (mut ws, _) = tokio_tungstenite::client_async(url.as_str(), stream).await?;
		let hello = json!({
			"type": "hello",
			"secret": secret,
			"clientId": format!("app-{}", std::process::id()),
			"clientType": "app",
			"protocolVersion": PROTOCOL_VERSION,
			"appVersion": app_version,
			"capabilities": [],
		});
		ws.send(Message::Text(hello.to_string().into())).await?;
		let Some(Ok(Message::Text(text))) = ws.next().await else {
			bail!("closed before server_info (wrong secret?)");
		};
		let info: Value = serde_json::from_str(&text)?;
		if info["type"] != "server_info" {
			bail!("the first frame was not server_info");
		}
		if info["protocolVersion"] != PROTOCOL_VERSION {
			bail!("speaks protocol {}, not {PROTOCOL_VERSION}", info["protocolVersion"]);
		}
		let host_id = info["hostId"].as_str().context("server_info without hostId")?.to_string();
		Ok((ws, host_id))
	})
	.await
	.context("no server_info within 3 s")?
}

/// Start the core detached — not the app's child, so a crash of the app does
/// not take it along (§1) — and read its port off the `ready` line.
async fn spawn(binary: &Path, data_dir: &Path) -> anyhow::Result<(u32, u16)> {
	let mut cmd = tokio::process::Command::new(binary);
	cmd.arg("--data-dir")
		.arg(data_dir)
		.arg("--parent-pid")
		.arg(std::process::id().to_string())
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		// The core logs to `core/core.log`; nobody reads a detached stderr.
		.stderr(Stdio::null());
	let mut child = detached_spawn(&mut cmd)
		.with_context(|| format!("could not start {}", binary.display()))?;
	let pid = child.id().context("qursor-core exited at once")?;
	let stdout = child.stdout.take().context("qursor-core's stdout")?;
	let mut lines = tokio::io::BufReader::new(stdout).lines();
	let line = match tokio::time::timeout(READY, lines.next_line()).await {
		Ok(line) => line?,
		Err(_) => {
			let _ = child.start_kill();
			bail!("qursor-core did not report ready within 10 s");
		}
	};
	let Some(line) = line else {
		let status = child.wait().await?;
		bail!("qursor-core exited before it was ready: {}", exit_reason(status.code()));
	};
	#[derive(Deserialize)]
	struct Ready {
		ready: bool,
		port: u16,
	}
	let ready: Ready = serde_json::from_str(&line)
		.with_context(|| format!("qursor-core's ready line is not JSON: {line:?}"))?;
	if !ready.ready {
		bail!("qursor-core said it is not ready");
	}
	Ok((pid, ready.port))
}

/// The binary's exit codes (its header in `qursor-core.rs`).
fn exit_reason(code: Option<i32>) -> String {
	match code {
		Some(2) => "it did not understand its arguments".into(),
		Some(3) => "another qursor-core holds this data folder and did not answer".into(),
		Some(4) => "the database is missing or not migrated".into(),
		Some(code) => format!("exit code {code}; see core/core.log"),
		None => "killed by a signal".into(),
	}
}

#[cfg(windows)]
fn detached_spawn(cmd: &mut tokio::process::Command) -> std::io::Result<tokio::process::Child> {
	use windows_sys::Win32::System::Threading::{
		CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS,
	};
	let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
	cmd.creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB);
	match cmd.spawn() {
		// A job that does not allow breakaway (a terminal or an IDE that runs
		// us in one) refuses the flag; the core then shares our job, which
		// plan 1's kill-on-quit is fine with.
		Err(e) if e.raw_os_error() == Some(5) => {
			cmd.creation_flags(flags);
			cmd.spawn()
		}
		other => other,
	}
}

#[cfg(unix)]
fn detached_spawn(cmd: &mut tokio::process::Command) -> std::io::Result<tokio::process::Child> {
	cmd.process_group(0);
	cmd.spawn()
}

/// The core binary Tauri bundles beside the app (`bundle.externalBin`), and
/// cargo builds beside it in development.
pub fn sidecar() -> anyhow::Result<PathBuf> {
	let exe = std::env::current_exe()?;
	let dir = exe.parent().context("the app's executable has no folder")?;
	Ok(dir.join(format!("qursor-core{}", std::env::consts::EXE_SUFFIX)))
}

/// The link once it exists. Setup starts it off the main thread; whoever asks
/// first waits for it, and a failure is kept so every caller sees the reason.
pub struct CoreCell {
	link: tokio::sync::OnceCell<Result<CoreLink, String>>,
	data_dir: PathBuf,
	app_version: String,
}

impl CoreCell {
	pub fn new(data_dir: PathBuf, app_version: String) -> Self {
		Self { link: tokio::sync::OnceCell::new(), data_dir, app_version }
	}

	pub async fn link(&self) -> AppResult<&CoreLink> {
		self.link
			.get_or_init(|| async {
				let binary = sidecar().map_err(|e| format!("{e:#}"))?;
				CoreLink::ensure(&self.data_dir, &self.app_version, &binary).await.map_err(|e| {
					warn!("qursor-core is not available: {e:#}");
					format!("qursor-core is not available: {e:#}")
				})
			})
			.await
			.as_ref()
			.map_err(|e| AppError::Process(e.clone()))
	}

	/// The link if it is up, without waiting.
	pub fn ready(&self) -> Option<&CoreLink> {
		self.link.get().and_then(|r| r.as_ref().ok())
	}

	/// No link yet means no core, and no core has no live sessions.
	pub fn live_session_ids(&self) -> HashSet<String> {
		self.ready().map(CoreLink::live_session_ids).unwrap_or_default()
	}
}
