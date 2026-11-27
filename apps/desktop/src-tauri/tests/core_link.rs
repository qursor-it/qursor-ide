//! The app finds or starts its qursor-core
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §1).
//!
//! Runs the real binary: `cargo test --workspace` builds it beside this test;
//! `cargo test -p qursor-ide --test core_link` alone needs
//! `cargo build -p qursor-core` first.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use factorai_lib::core_link::CoreLink;
use futures_util::{SinkExt, StreamExt};
use qursor_core::protocol::frame::{self, Op};
use qursor_core::server::record::{self, Record};
use qursor_core::services::ide::lockfile::pid_is_alive;
use qursor_core::test_support::runtime;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

/// `target/<profile>/qursor-core`, beside `target/<profile>/deps/<this test>`.
fn core_binary() -> PathBuf {
	let exe = std::env::current_exe().unwrap();
	let profile = exe.parent().and_then(Path::parent).unwrap();
	let bin = profile.join(format!("qursor-core{}", std::env::consts::EXE_SUFFIX));
	assert!(bin.is_file(), "{} is missing: run `cargo build -p qursor-core` first", bin.display());
	bin
}

/// A data folder the app has already opened, so the database is migrated.
fn data_dir(root: &Path) -> PathBuf {
	std::fs::create_dir_all(root).unwrap();
	qursor_core::db::Db::open(root).unwrap();
	root.to_path_buf()
}

fn write_record(data_dir: &Path, pid: u32, port: u16) {
	let dir = record::core_dir(data_dir).unwrap();
	let rec = Record {
		pid,
		protocol_version: 1,
		app_version: "test".into(),
		port,
		secret: "not-the-secret".into(),
	};
	record::write(&dir, &rec).unwrap();
}

fn read_record(data_dir: &Path) -> Record {
	let bytes = std::fs::read(data_dir.join("core").join("core.json")).unwrap();
	serde_json::from_slice(&bytes).unwrap()
}

/// A process id nothing answers to any more.
fn dead_pid() -> u32 {
	let mut child = std::process::Command::new(std::env::current_exe().unwrap())
		.arg("--list")
		.stdout(std::process::Stdio::null())
		.spawn()
		.unwrap();
	let pid = child.id();
	child.wait().unwrap();
	pid
}

async fn ensure(dir: &Path) -> CoreLink {
	CoreLink::ensure(dir, "test", &core_binary()).await.expect("the core starts")
}

#[test]
fn stale_record_with_dead_pid_starts_new_core() {
	runtime().block_on(async {
		let tmp = tempfile::TempDir::new().unwrap();
		let dir = data_dir(tmp.path());
		let dead = dead_pid();
		write_record(&dir, dead, 1);
		let link = ensure(&dir).await;
		assert_ne!(link.pid(), dead);
		assert!(pid_is_alive(link.pid()));
		assert_eq!(read_record(&dir).pid, link.pid());
		link.shutdown().await.unwrap();
	});
}

#[test]
fn record_pointing_at_foreign_port_is_not_trusted() {
	runtime().block_on(async {
		let tmp = tempfile::TempDir::new().unwrap();
		let dir = data_dir(tmp.path());
		// Accepts connections and never says a word.
		let foreign = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
		let port = foreign.local_addr().unwrap().port();
		write_record(&dir, std::process::id(), port);
		let link = ensure(&dir).await;
		assert_ne!(link.pid(), std::process::id());
		assert!(!link.endpoint().url.contains(&format!(":{port}/")), "{}", link.endpoint().url);
		assert_eq!(read_record(&dir).pid, link.pid());
		link.shutdown().await.unwrap();
		drop(foreign);
	});
}

#[test]
fn data_dir_with_spaces_and_unicode() {
	runtime().block_on(async {
		let tmp = tempfile::TempDir::new().unwrap();
		let dir = data_dir(&tmp.path().join("Nome Cognome è"));
		let link = ensure(&dir).await;
		assert_eq!(read_record(&dir).pid, link.pid());
		link.shutdown().await.unwrap();
	});
}

#[test]
fn a_core_that_answers_is_reused() {
	runtime().block_on(async {
		let tmp = tempfile::TempDir::new().unwrap();
		let dir = data_dir(tmp.path());
		let first = ensure(&dir).await;
		let second = ensure(&dir).await;
		assert_eq!(second.pid(), first.pid());
		assert_eq!(second.endpoint().host_id, first.endpoint().host_id);
		first.shutdown().await.unwrap();
	});
}

#[cfg(windows)]
const PRINT_PID: &str = "echo \"PID=$PID\"";
#[cfg(not(windows))]
const PRINT_PID: &str = "echo PID=$$";

#[test]
fn shutdown_kills_terminals() {
	runtime().block_on(async {
		let tmp = tempfile::TempDir::new().unwrap();
		let dir = data_dir(tmp.path());
		let link = ensure(&dir).await;
		let spawned = link
			.request(
				"shell.spawn",
				json!({"clientKey":"shell:t","projectId":"p1","cwd":tmp.path(),"cols":120,"rows":30}),
			)
			.await
			.unwrap();
		let terminal_id = spawned["id"].as_str().unwrap().to_string();
		let counts = link.request("core.counts", Value::Null).await.unwrap();
		assert_eq!(counts["live"], 1, "{counts}");

		// A second client reads the shell's pid off its screen.
		let endpoint = link.endpoint();
		let port = endpoint.url.rsplit(':').next().unwrap().trim_end_matches('/').parse::<u16>();
		let stream = tokio::net::TcpStream::connect(("127.0.0.1", port.unwrap())).await.unwrap();
		let (mut ws, _) =
			tokio_tungstenite::client_async(endpoint.url.as_str(), stream).await.unwrap();
		let hello = json!({"type":"hello","secret":endpoint.secret,"clientId":"t",
			"clientType":"cli","protocolVersion":1,"appVersion":"test","capabilities":[]});
		ws.send(Message::Text(hello.to_string().into())).await.unwrap();
		let attach = json!({"type":"request","requestId":1,"method":"terminal.attach",
			"params":{"terminalId":terminal_id}});
		ws.send(Message::Text(attach.to_string().into())).await.unwrap();
		let mut slot = None;
		let mut screen = String::new();
		let deadline = Instant::now() + Duration::from_secs(20);
		let pid = loop {
			assert!(Instant::now() < deadline, "no pid on screen: {screen:?}");
			let msg = tokio::time::timeout(Duration::from_secs(20), ws.next()).await;
			match msg.unwrap().unwrap().unwrap() {
				Message::Text(t) => {
					let v: Value = serde_json::from_str(&t).unwrap();
					if v["requestId"] == 1 {
						let s = v["result"]["slot"].as_u64().unwrap() as u16;
						slot = Some(s);
						let line = format!("{PRINT_PID}\r");
						let input = frame::encode(Op::Input, s, line.as_bytes());
						ws.send(Message::Binary(input.into())).await.unwrap();
					}
				}
				Message::Binary(b) => {
					let (op, s, payload) = frame::decode(&b).unwrap();
					if op == Op::Output && Some(s) == slot {
						screen.push_str(&String::from_utf8_lossy(payload));
						if let Some(pid) = pid_in(&screen) {
							break pid;
						}
					}
				}
				_ => {}
			}
		};
		assert!(pid_is_alive(pid));

		link.shutdown().await.unwrap();
		let deadline = Instant::now() + Duration::from_secs(2);
		while pid_is_alive(pid) {
			assert!(Instant::now() < deadline, "shell {pid} outlived the shutdown");
			tokio::time::sleep(Duration::from_millis(50)).await;
		}
	});
}

fn pid_in(out: &str) -> Option<u32> {
	out.match_indices("PID=").find_map(|(i, _)| {
		let digits: String = out[i + 4..].chars().take_while(char::is_ascii_digit).collect();
		digits.parse().ok()
	})
}
