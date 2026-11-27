//! The core's WebSocket server, in-process on a random port
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §1, §3, §4).

use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use qursor_core::protocol::frame::{self, Op};
use qursor_core::server::{self, Config, Server};
use qursor_core::test_support::{runtime, spawn_lock};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

type Ws = WebSocketStream<TcpStream>;

/// Long enough that no test but the grace ones sees the core leave.
const NO_GRACE: Duration = Duration::from_secs(600);

async fn start(grace: Duration) -> (Server, tempfile::TempDir) {
	let dir = tempfile::TempDir::new().unwrap();
	let server =
		server::start(Config { data_dir: dir.path().to_path_buf(), grace, parent_pid: None })
			.await
			.expect("server starts");
	(server, dir)
}

async fn connect(port: u16) -> Ws {
	let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
	let req = format!("ws://127.0.0.1:{port}/").into_client_request().unwrap();
	tokio_tungstenite::client_async(req, stream).await.expect("handshake").0
}

fn hello(secret: &str) -> Message {
	Message::Text(
		json!({"type":"hello","secret":secret,"clientId":"t","clientType":"cli",
			"protocolVersion":1,"appVersion":"test","capabilities":[]})
		.to_string()
		.into(),
	)
}

/// A client past the handshake.
async fn client(server: &Server) -> Ws {
	let mut ws = connect(server.port).await;
	ws.send(hello(&server.secret)).await.unwrap();
	let info = next_json(&mut ws).await;
	assert_eq!(info["type"], "server_info", "{info}");
	ws
}

async fn next_json(ws: &mut Ws) -> Value {
	loop {
		match tokio::time::timeout(Duration::from_secs(10), ws.next()).await {
			Ok(Some(Ok(Message::Text(t)))) => return serde_json::from_str(&t).unwrap(),
			Ok(Some(Ok(_))) => continue,
			other => panic!("no JSON frame: {other:?}"),
		}
	}
}

async fn request(ws: &mut Ws, id: u64, method: &str, params: Value) -> Value {
	let msg = json!({"type":"request","requestId":id,"method":method,"params":params});
	ws.send(Message::Text(msg.to_string().into())).await.unwrap();
	loop {
		let v = next_json(ws).await;
		if v["requestId"] == id {
			assert_eq!(v["type"], "response", "{v}");
			return v["result"].clone();
		}
	}
}

async fn spawn_shell(ws: &mut Ws, cwd: &std::path::Path) -> (String, u16) {
	let opts = json!({"clientKey":"shell:t","projectId":"p1","cwd":cwd,"cols":120,"rows":30});
	let r = request(ws, 1, "shell.spawn", opts).await;
	(r["id"].as_str().unwrap().to_string(), r["slot"].as_u64().unwrap() as u16)
}

async fn type_line(ws: &mut Ws, slot: u16, line: &str) {
	let bytes = frame::encode(Op::Input, slot, format!("{line}\r").as_bytes());
	ws.send(Message::Binary(bytes.into())).await.unwrap();
}

/// Output on `slot` until `pred` matches what has arrived, or `limit` passes.
async fn read_until(
	ws: &mut Ws,
	slot: u16,
	limit: Duration,
	pred: impl Fn(&str) -> bool,
) -> String {
	let deadline = Instant::now() + limit;
	let mut seen = String::new();
	while Instant::now() < deadline {
		let left = deadline - Instant::now();
		if let Ok(Some(Ok(Message::Binary(b)))) = tokio::time::timeout(left, ws.next()).await {
			let (op, s, payload) = frame::decode(&b).unwrap();
			if op == Op::Output && s == slot {
				seen.push_str(&String::from_utf8_lossy(payload));
				if pred(&seen) {
					return seen;
				}
			}
		}
	}
	panic!("never matched; output was {seen:?}");
}

/// Closed by the server: a close frame, an error or the end of the stream, and
/// never a `server_info`.
async fn assert_closed_without_info(ws: &mut Ws) {
	loop {
		match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
			Ok(Some(Ok(Message::Text(t)))) => panic!("unexpected text {t}"),
			Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) | Ok(None) => return,
			Ok(Some(Ok(_))) => continue,
			Err(_) => panic!("connection still open"),
		}
	}
}

async fn shutdown(ws: &mut Ws, server: Server) {
	request(ws, 99, "core.shutdown", json!({"keepTerminals":false})).await;
	tokio::time::timeout(Duration::from_secs(5), server.exited).await.unwrap().unwrap();
}

#[cfg(windows)]
const PRINT_PID: &str = "echo \"PID=$PID\"";
#[cfg(not(windows))]
const PRINT_PID: &str = "echo PID=$$";

fn pid_in(out: &str) -> Option<u32> {
	out.match_indices("PID=").find_map(|(i, _)| {
		let digits: String = out[i + 4..].chars().take_while(char::is_ascii_digit).collect();
		digits.parse().ok()
	})
}

#[test]
fn rejects_wrong_secret() {
	runtime().block_on(async {
		let (server, _dir) = start(NO_GRACE).await;
		let mut ws = connect(server.port).await;
		ws.send(hello("not-the-secret")).await.unwrap();
		assert_closed_without_info(&mut ws).await;
	});
}

#[test]
fn rejects_non_hello_first_frame() {
	runtime().block_on(async {
		let (server, _dir) = start(NO_GRACE).await;
		let mut ws = connect(server.port).await;
		ws.send(Message::Text(r#"{"type":"ping"}"#.into())).await.unwrap();
		assert_closed_without_info(&mut ws).await;
	});
}

#[test]
fn rejects_a_foreign_origin() {
	runtime().block_on(async {
		let (server, _dir) = start(NO_GRACE).await;
		let stream = TcpStream::connect(("127.0.0.1", server.port)).await.unwrap();
		let mut req = format!("ws://127.0.0.1:{}/", server.port).into_client_request().unwrap();
		req.headers_mut().insert("Origin", "https://evil.example".parse().unwrap());
		assert!(tokio_tungstenite::client_async(req, stream).await.is_err());
	});
}

#[test]
fn spawn_echo_roundtrip() {
	let _spawn = spawn_lock();
	runtime().block_on(async {
		let (server, dir) = start(NO_GRACE).await;
		let mut ws = client(&server).await;
		let (_id, slot) = spawn_shell(&mut ws, dir.path()).await;
		type_line(&mut ws, slot, "echo qx").await;
		read_until(&mut ws, slot, Duration::from_secs(5), |s| s.contains("qx")).await;
		shutdown(&mut ws, server).await;
	});
}

#[test]
fn output_only_to_attached() {
	let _spawn = spawn_lock();
	runtime().block_on(async {
		let (server, dir) = start(NO_GRACE).await;
		let mut first = client(&server).await;
		let mut second = client(&server).await;
		let (_id, slot) = spawn_shell(&mut first, dir.path()).await;
		type_line(&mut first, slot, "echo qx").await;
		read_until(&mut first, slot, Duration::from_secs(5), |s| s.contains("qx")).await;
		let deadline = Instant::now() + Duration::from_millis(500);
		while Instant::now() < deadline {
			if let Ok(Some(Ok(m))) =
				tokio::time::timeout(deadline - Instant::now(), second.next()).await
			{
				assert!(!m.is_binary(), "the unattached client got terminal bytes");
			}
		}
		shutdown(&mut first, server).await;
	});
}

#[test]
fn last_client_grace_then_kill() {
	let _spawn = spawn_lock();
	runtime().block_on(async {
		let (server, dir) = start(Duration::from_millis(200)).await;
		let mut ws = client(&server).await;
		let (_id, slot) = spawn_shell(&mut ws, dir.path()).await;
		type_line(&mut ws, slot, PRINT_PID).await;
		let out = read_until(&mut ws, slot, Duration::from_secs(10), |s| pid_in(s).is_some()).await;
		let pid = pid_in(&out).unwrap();
		ws.close(None).await.unwrap();
		drop(ws);
		// The brief's 400 ms is 200 ms of grace plus `kill_all`'s own 500 ms
		// grace, which it cannot hold; a bound well clear of both instead.
		tokio::time::timeout(Duration::from_secs(3), server.exited)
			.await
			.expect("core left after the grace")
			.unwrap();
		assert!(!qursor_core::services::ide::lockfile::pid_is_alive(pid), "shell {pid} survived");
	});
}

#[test]
fn reconnect_within_grace_keeps_terminals() {
	let _spawn = spawn_lock();
	runtime().block_on(async {
		let (server, dir) = start(Duration::from_millis(1500)).await;
		let mut ws = client(&server).await;
		let (id, _slot) = spawn_shell(&mut ws, dir.path()).await;
		ws.close(None).await.unwrap();
		drop(ws);
		tokio::time::sleep(Duration::from_millis(300)).await;
		let mut ws = client(&server).await;
		// Past the first grace: a reconnect cancels it rather than racing it.
		tokio::time::sleep(Duration::from_millis(1500)).await;
		let list = request(&mut ws, 2, "terminal.list", Value::Null).await;
		assert!(list.as_array().unwrap().iter().any(|t| t["id"] == id.as_str()), "{list}");
		shutdown(&mut ws, server).await;
	});
}

#[test]
fn parent_death_shuts_down() {
	runtime().block_on(async {
		#[cfg(windows)]
		let mut parent = std::process::Command::new("cmd")
			.args(["/c", "ping -n 30 127.0.0.1 >NUL"])
			.spawn()
			.unwrap();
		#[cfg(not(windows))]
		let mut parent = std::process::Command::new("sleep").arg("30").spawn().unwrap();
		let dir = tempfile::TempDir::new().unwrap();
		let server = server::start(Config {
			data_dir: dir.path().to_path_buf(),
			grace: NO_GRACE,
			parent_pid: Some(parent.id()),
		})
		.await
		.unwrap();
		parent.kill().unwrap();
		parent.wait().unwrap();
		tokio::time::timeout(Duration::from_secs(5), server.exited).await.unwrap().unwrap();
	});
}

#[test]
fn slow_client_closed_over_8mib() {
	let _spawn = spawn_lock();
	runtime().block_on(async {
		let (server, dir) = start(NO_GRACE).await;
		let mut watcher = client(&server).await;
		let mut slow = client(&server).await;
		let (id, slot) = spawn_shell(&mut slow, dir.path()).await;
		#[cfg(windows)]
		let flood = "while($true){'y' * 512}";
		#[cfg(not(windows))]
		let flood = "yes";
		type_line(&mut slow, slot, flood).await;
		// From here `slow` is never read. Inside the 20 s silence rule, so the
		// queue is what closes it.
		let deadline = Instant::now() + Duration::from_secs(15);
		while server.client_count() > 1 {
			assert!(Instant::now() < deadline, "the slow client was never dropped");
			tokio::time::sleep(Duration::from_millis(100)).await;
		}
		let list = request(&mut watcher, 3, "terminal.list", Value::Null).await;
		assert!(list.as_array().unwrap().iter().any(|t| t["id"] == id.as_str()), "{list}");
		drop(slow);
		shutdown(&mut watcher, server).await;
	});
}
