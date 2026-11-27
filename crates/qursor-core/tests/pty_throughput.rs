//! The PTY budgets of specs/10-performance.md P9, measured through qursor-core:
//! the workloads of `baseline_throughput` and `baseline_echo` in
//! `services/terminal.rs`, timed on the Output frames a WebSocket client
//! receives. Run alternately with the baseline:
//! `cargo test -p qursor-core --release --test pty_throughput -- --ignored --nocapture --test-threads=1`.
//!
//! The wire spawns no arbitrary argv, so the workload is typed into a footer
//! shell (`shell.spawn`): the same child writes the same bytes into the same
//! PTY, with the shell idle above it.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::stream::SplitSink;
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
type Stamps = Arc<Mutex<Vec<(Instant, usize)>>>;
type ExitStamp = Arc<Mutex<Option<(Instant, Option<i32>)>>>;

/// A visible footer shell on a core of its own, its Output frames and its exit
/// stamped on arrival by a reader task, as the baseline's sink stamps them.
struct Shell {
	tx: SplitSink<Ws, Message>,
	slot: u16,
	data: Stamps,
	exit: ExitStamp,
	pinged: Instant,
	server: Server,
	_dir: tempfile::TempDir,
}

async fn request(ws: &mut Ws, id: u64, method: &str, params: Value) -> Value {
	let msg = json!({"type":"request","requestId":id,"method":method,"params":params});
	ws.send(Message::Text(msg.to_string().into())).await.unwrap();
	loop {
		match tokio::time::timeout(Duration::from_secs(10), ws.next()).await {
			Ok(Some(Ok(Message::Text(t)))) => {
				let v: Value = serde_json::from_str(&t).unwrap();
				if v["requestId"] == id {
					assert_eq!(v["type"], "response", "{v}");
					return v["result"].clone();
				}
			}
			Ok(Some(Ok(_))) => continue,
			other => panic!("no reply to {method}: {other:?}"),
		}
	}
}

impl Shell {
	async fn start() -> Shell {
		let dir = tempfile::TempDir::new().unwrap();
		// The app creates and migrates the database; the core only opens it.
		qursor_core::db::Db::open(dir.path()).unwrap();
		let config = Config {
			data_dir: dir.path().to_path_buf(),
			grace: Duration::from_secs(600),
			parent_pid: None,
		};
		let server = server::start(config).await.expect("server starts");
		let stream = TcpStream::connect(("127.0.0.1", server.port)).await.unwrap();
		let req = format!("ws://127.0.0.1:{}/", server.port).into_client_request().unwrap();
		let mut ws = tokio_tungstenite::client_async(req, stream).await.expect("handshake").0;
		let hello = json!({"type":"hello","secret":server.secret,"clientId":"t",
			"clientType":"cli","protocolVersion":1,"appVersion":"test","capabilities":[]});
		ws.send(Message::Text(hello.to_string().into())).await.unwrap();
		let opts = json!({"clientKey":"shell:t","projectId":"p1","cwd":dir.path(),
			"cols":80,"rows":24});
		let r = request(&mut ws, 1, "shell.spawn", opts).await;
		let (id, slot) =
			(r["id"].as_str().unwrap().to_string(), r["slot"].as_u64().unwrap() as u16);
		// Measured on screen, the window today's renderer watches in.
		request(&mut ws, 2, "terminal.visibility", json!({"terminalId":id,"visible":true})).await;

		let (tx, mut rx) = ws.split();
		let data: Stamps = Arc::default();
		let exit: ExitStamp = Arc::default();
		let (dc, ec) = (data.clone(), exit.clone());
		tokio::spawn(async move {
			while let Some(Ok(msg)) = rx.next().await {
				let at = Instant::now();
				match msg {
					Message::Binary(b) => {
						let (op, s, payload) = frame::decode(&b).unwrap();
						if op == Op::Output && s == slot {
							dc.lock().unwrap().push((at, payload.len()));
						}
					}
					Message::Text(t) => {
						let v: Value = serde_json::from_str(&t).unwrap();
						if v["event"] == "terminalExit" && v["payload"]["id"] == id.as_str() {
							let code = v["payload"]["code"].as_i64().map(|c| c as i32);
							*ec.lock().unwrap() = Some((at, code));
						}
					}
					_ => {}
				}
			}
		});
		let mut shell = Shell { tx, slot, data, exit, pinged: Instant::now(), server, _dir: dir };
		// The shell's banner and prompt, before anything is timed.
		shell.quiet(Duration::from_millis(500)).await;
		shell
	}

	async fn send(&mut self, bytes: &[u8]) {
		let frame = frame::encode(Op::Input, self.slot, bytes);
		self.tx.send(Message::Binary(frame.into())).await.unwrap();
	}

	/// The real client's ping, without which a long run reads as a dead client.
	async fn keepalive(&mut self) {
		if self.pinged.elapsed() >= Duration::from_secs(5) {
			self.tx.send(Message::Text(r#"{"type":"ping"}"#.into())).await.unwrap();
			self.pinged = Instant::now();
		}
	}

	/// Until something has arrived and nothing more has for `idle`.
	async fn quiet(&mut self, idle: Duration) {
		let start = Instant::now();
		loop {
			tokio::time::sleep(Duration::from_millis(50)).await;
			if self.data.lock().unwrap().last().is_some_and(|(t, _)| t.elapsed() >= idle) {
				return;
			}
			assert!(start.elapsed() < Duration::from_secs(15), "the shell never settled");
		}
	}

	async fn shutdown(mut self) {
		let msg = json!({"type":"request","requestId":99,"method":"core.shutdown",
			"params":{"keepTerminals":false}});
		self.tx.send(Message::Text(msg.to_string().into())).await.unwrap();
		tokio::time::timeout(Duration::from_secs(5), self.server.exited).await.unwrap().unwrap();
	}
}

#[test]
#[ignore = "PTY budget, see specs/10-performance.md P9"]
fn core_throughput() {
	const SIZE: usize = 50 * 1024 * 1024;
	let _spawn = spawn_lock();
	runtime().block_on(async {
		let dir = tempfile::TempDir::new().unwrap();
		let file = dir.path().join("50mb.txt");
		let line = "0123456789abcdef".repeat(4) + "\n";
		let lines = SIZE / line.len();
		std::fs::write(&file, line.repeat(lines)).unwrap();
		let path = file.to_string_lossy().into_owned();
		// The shell leaves with the child's code, so the exit event carries it.
		#[cfg(windows)]
		let cmd = format!("cmd /c type \"{path}\"; exit $LASTEXITCODE\r");
		#[cfg(not(windows))]
		let cmd = format!("exec /bin/cat '{path}'\r");

		let mut sh = Shell::start().await;
		let before = sh.data.lock().unwrap().len();
		let start = Instant::now();
		sh.send(cmd.as_bytes()).await;
		let (exited, code) = loop {
			if let Some(e) = *sh.exit.lock().unwrap() {
				break e;
			}
			assert!(start.elapsed() < Duration::from_secs(120), "the child never exited");
			sh.keepalive().await;
			tokio::time::sleep(Duration::from_millis(5)).await;
		};
		assert_eq!(code, Some(0), "the child failed, the number would be meaningless");
		// The exit event is not ordered after the final flush: the clock stops
		// at the last byte, and the quiet wait below is not counted.
		// POSIX: the tty adds a CR to every line. The command line's own echo
		// arrives too, so the count is a floor rather than exact.
		#[cfg(not(windows))]
		let expected = Some(lines * (line.len() + 1));
		#[cfg(windows)]
		let expected: Option<usize> = None;
		let quiet_since = Instant::now();
		let (last, received) = loop {
			tokio::time::sleep(Duration::from_millis(100)).await;
			let (last, received) = {
				let d = sh.data.lock().unwrap();
				let received: usize = d[before..].iter().map(|(_, n)| n).sum();
				(d.last().map_or(exited, |(t, _)| *t), received)
			};
			let complete = expected.is_none_or(|n| received >= n);
			if last.elapsed() >= Duration::from_millis(100) && complete {
				break (last, received);
			}
			assert!(
				quiet_since.elapsed() < Duration::from_secs(30),
				"output never completed: {received}"
			);
			sh.keepalive().await;
		};
		assert!(received > 0, "nothing arrived");
		println!("bytes_received={received}");
		println!("throughput_50mb_ms={}", exited.max(last).duration_since(start).as_millis());
		sh.shutdown().await;
	});
}

#[test]
#[ignore = "PTY budget, see specs/10-performance.md P9"]
fn core_echo() {
	const SAMPLES: usize = 400;
	let _spawn = spawn_lock();
	runtime().block_on(async {
		let mut sh = Shell::start().await;
		// The baseline's children, started from the shell.
		#[cfg(windows)]
		sh.send(b"cmd /q\r").await;
		#[cfg(not(windows))]
		sh.send(b"exec /bin/cat\r").await;
		sh.quiet(Duration::from_millis(500)).await;
		let mut lat = Vec::new();
		for i in 0..SAMPLES {
			// Let the previous echo drain. The jitter keeps the probe from locking
			// onto one phase of the flusher's tick, which would hide added delay.
			tokio::time::sleep(Duration::from_millis(100 + (i as u64 * 7919) % 32)).await;
			let seen = sh.data.lock().unwrap().len();
			let t0 = Instant::now();
			sh.send(b"a").await;
			// The reader stamps each frame as it lands, so how often this looks
			// does not enter the figure.
			let at = loop {
				if let Some((t, _)) = sh.data.lock().unwrap().get(seen) {
					break *t;
				}
				assert!(t0.elapsed() < Duration::from_secs(5), "no echo");
				tokio::time::sleep(Duration::from_millis(1)).await;
			};
			assert!(at >= t0, "an echo stamped before its write is a stale event");
			lat.push(at.duration_since(t0).as_micros());
		}
		lat.sort_unstable();
		println!("echo_p50_us={}", lat[lat.len() / 2]);
		println!("echo_p95_us={}", lat[lat.len() * 95 / 100]);
		sh.shutdown().await;
	});
}
