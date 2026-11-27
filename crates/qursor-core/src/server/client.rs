//! One client connection: the handshake, its queue, its slots
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §3, §4).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::header::ORIGIN;
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, warn};

use super::Shared;
use crate::error::{AppError, AppResult};
use crate::protocol::frame::{self, Op};
use crate::protocol::messages::{ClientMsg, ServerMsg};
use crate::protocol::PROTOCOL_VERSION;
use crate::services::agent_tools::tokens_match;
use crate::services::ide::ui_state::UiState;
use crate::services::terminal::TerminalId;

/// Past this much waiting to be written, the connection is closed: the PTY
/// never waits for a slow client (§4).
pub(crate) const MAX_QUEUED: usize = 8 * 1024 * 1024;

/// Two of the client's 10 s ping windows with nothing arriving (§3).
const SILENCE: Duration = Duration::from_secs(20);

/// The Tauri webview, and the dev server in development. No `Origin` at all is
/// a client that is not a browser (§3).
const ORIGINS: [&str; 4] = [
	"tauri://localhost",
	"http://tauri.localhost",
	"https://tauri.localhost",
	"http://localhost:1420",
];

pub(crate) struct Client {
	tx: mpsc::UnboundedSender<Message>,
	queued: AtomicUsize,
	closed: AtomicBool,
	dropped: Notify,
	slots: Mutex<Slots>,
}

#[derive(Default)]
struct Slots {
	/// The last slot handed out; slots are never reused on one connection.
	last: u16,
	by_slot: HashMap<u16, TerminalId>,
	by_terminal: HashMap<TerminalId, u16>,
	visible: HashSet<TerminalId>,
}

impl Client {
	/// Queue a frame. Never blocks: past [`MAX_QUEUED`] the client is dropped.
	pub(crate) fn send(&self, msg: Message) {
		if self.closed.load(Ordering::Relaxed) {
			return;
		}
		let len = msg.len();
		if self.queued.fetch_add(len, Ordering::Relaxed) + len > MAX_QUEUED {
			warn!("a client fell 8 MiB behind; closing it");
			self.closed.store(true, Ordering::Relaxed);
			self.dropped.notify_one();
			return;
		}
		let _ = self.tx.send(msg);
	}

	pub(crate) fn reply(&self, msg: &ServerMsg) {
		match serde_json::to_string(msg) {
			Ok(text) => self.send(Message::Text(text.into())),
			Err(error) => warn!(%error, "could not encode a reply"),
		}
	}

	pub(crate) fn slot_of(&self, terminal_id: &str) -> Option<u16> {
		self.slots.lock().by_terminal.get(terminal_id).copied()
	}

	/// The slot this connection reads `terminal_id` on; the same one if it is
	/// already attached.
	pub(crate) fn attach(&self, terminal_id: &str) -> AppResult<u16> {
		let mut slots = self.slots.lock();
		if let Some(slot) = slots.by_terminal.get(terminal_id) {
			return Ok(*slot);
		}
		let slot = slots
			.last
			.checked_add(1)
			.ok_or_else(|| AppError::InvalidInput("this connection has used every slot".into()))?;
		slots.last = slot;
		slots.by_slot.insert(slot, terminal_id.to_string());
		slots.by_terminal.insert(terminal_id.to_string(), slot);
		Ok(slot)
	}

	pub(crate) fn detach(&self, slot: u16) {
		let mut slots = self.slots.lock();
		if let Some(id) = slots.by_slot.remove(&slot) {
			slots.by_terminal.remove(&id);
		}
	}

	fn terminal(&self, slot: u16) -> Option<TerminalId> {
		self.slots.lock().by_slot.get(&slot).cloned()
	}

	pub(crate) fn set_visible(&self, ui: &UiState, terminal_id: &str, visible: bool) {
		let mut slots = self.slots.lock();
		if visible && slots.visible.insert(terminal_id.to_string()) {
			ui.show(terminal_id);
		} else if !visible && slots.visible.remove(terminal_id) {
			ui.hide(terminal_id);
		}
	}
}

// `clippy::result_large_err`: the handshake callback's signature is
// tungstenite's, as in `services::ide::server`.
#[allow(clippy::result_large_err)]
fn check_origin(req: &Request, res: Response) -> Result<Response, ErrorResponse> {
	match req.headers().get(ORIGIN).map(|v| v.to_str()) {
		None => Ok(res),
		Some(Ok(origin)) if ORIGINS.contains(&origin) => Ok(res),
		_ => {
			warn!("rejected a connection from a foreign origin");
			let mut err = ErrorResponse::new(None);
			*err.status_mut() = StatusCode::FORBIDDEN;
			Err(err)
		}
	}
}

pub(crate) async fn serve(stream: TcpStream, shared: Arc<Shared>) {
	let ws = match tokio::time::timeout(SILENCE, accept_hdr_async(stream, check_origin)).await {
		Ok(Ok(ws)) => ws,
		Ok(Err(error)) => return debug!(%error, "handshake failed"),
		Err(_) => return debug!("handshake never finished"),
	};
	let (mut sink, mut stream) = ws.split();

	// The first frame is a `hello` with the secret, or the connection ends.
	let Ok(Some(Ok(Message::Text(text)))) = tokio::time::timeout(SILENCE, stream.next()).await
	else {
		return debug!("a client sent no hello");
	};
	let Ok(ClientMsg::Hello { secret, .. }) = serde_json::from_str::<ClientMsg>(&text) else {
		return warn!("a client's first frame was not a hello");
	};
	if !tokens_match(&secret, &shared.secret) {
		return warn!("rejected a client with a wrong secret");
	}
	let info = ServerMsg::ServerInfo {
		host_id: shared.host_id.clone(),
		version: env!("CARGO_PKG_VERSION").into(),
		protocol_version: PROTOCOL_VERSION,
		features: vec![],
	};
	let Ok(info) = serde_json::to_string(&info) else { return };
	let (tx, mut rx) = mpsc::unbounded_channel();
	let client = Arc::new(Client {
		tx,
		queued: AtomicUsize::new(0),
		closed: AtomicBool::new(false),
		dropped: Notify::new(),
		slots: Mutex::default(),
	});
	// Joined before `server_info`: a core already shutting down answers nothing,
	// so no client is told it has terminals that are about to die.
	let Some(id) = shared.joined(client.clone()) else {
		return debug!("refused a client: the core is shutting down");
	};
	if sink.send(Message::Text(info.into())).await.is_err() {
		return shared.left(id);
	}
	let mut stop = shared.stop.subscribe();

	let write = async {
		while let Some(msg) = rx.recv().await {
			let len = msg.len();
			if sink.send(msg).await.is_err() {
				break;
			}
			client.queued.fetch_sub(len, Ordering::Relaxed);
		}
	};
	let read = async {
		loop {
			let msg = match tokio::time::timeout(SILENCE, stream.next()).await {
				Ok(Some(Ok(msg))) => msg,
				Ok(Some(Err(error))) => break debug!(%error, "client connection failed"),
				Ok(None) => break,
				Err(_) => break debug!("client silent for two ping windows; closing"),
			};
			match msg {
				Message::Text(text) => on_text(&shared, &client, &text),
				Message::Binary(bytes) => on_binary(&shared, &client, &bytes),
				Message::Close(_) => break,
				// tungstenite answers pings itself.
				_ => {}
			}
		}
	};
	tokio::select! {
		_ = write => {}
		_ = read => {}
		_ = client.dropped.notified() => {}
		_ = stop.wait_for(|stopped| *stopped) => {}
	}

	for terminal_id in std::mem::take(&mut client.slots.lock().visible) {
		shared.core.ui.hide(&terminal_id);
	}
	shared.left(id);
}

fn on_text(shared: &Arc<Shared>, client: &Arc<Client>, text: &str) {
	// The frame itself is never logged: a hello or a vault unlock carries a secret.
	match serde_json::from_str::<ClientMsg>(text) {
		Ok(ClientMsg::Ping) => client.reply(&ServerMsg::Pong),
		Ok(ClientMsg::Request { request_id, method, params }) => {
			let (shared, client) = (shared.clone(), client.clone());
			// Its own task: a spawn takes a while and the keys typed meanwhile
			// must not wait behind it.
			tokio::spawn(async move {
				let reply = match shared.rpc(&client, &method, params).await {
					Ok(result) => ServerMsg::Response { request_id, result },
					Err(error) => ServerMsg::RpcError { request_id, error },
				};
				client.reply(&reply);
			});
		}
		Ok(ClientMsg::Hello { .. }) => debug!("ignored a second hello"),
		Err(error) => debug!(%error, "ignored a frame that is not a client message"),
	}
}

fn on_binary(shared: &Shared, client: &Client, bytes: &[u8]) {
	let (op, slot, payload) = match frame::decode(bytes) {
		Ok(decoded) => decoded,
		Err(error) => return debug!(%error, "ignored a bad binary frame"),
	};
	let Some(id) = client.terminal(slot) else {
		return debug!(slot, "frame for a slot not attached");
	};
	let terminals = &shared.core.terminals;
	let result = match op {
		Op::Input => terminals.write(&id, payload),
		Op::Resize => {
			let cols = u16::from_be_bytes([payload[0], payload[1]]);
			let rows = u16::from_be_bytes([payload[2], payload[3]]);
			terminals.resize(&id, cols, rows)
		}
		Op::Output => return debug!("ignored an output frame from a client"),
	};
	if let Err(error) = result {
		debug!(%error, "terminal frame not applied");
	}
}
