//! Local port forwards over a server's connection (F47): `127.0.0.1:<port>`
//! here, carried to `<host>:<port>` as the server sees it — the `ssh -L` the
//! human would type, as a switch.
//!
//! **Loopback only.** The listener binds `127.0.0.1`, so nothing else on the
//! network reaches the server through this computer.
//!
//! A running tunnel keeps its server's login alive (the reaper asks
//! [`Tunnels::uses`]); stopping it, deleting the server or quitting the app ends
//! it.

use std::future::Future;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::net::TcpListener;

use super::Connection;
use crate::error::{AppError, AppResult};

struct Running {
	server_id: String,
	task: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Default)]
pub struct Tunnels {
	running: DashMap<String, Running>,
}

impl Tunnels {
	pub fn is_running(&self, tunnel_id: &str) -> bool {
		self.running.contains_key(tunnel_id)
	}

	/// Whether any running tunnel rides `server_id`'s connection.
	pub fn uses(&self, server_id: &str) -> bool {
		self.running.iter().any(|r| r.value().server_id == server_id)
	}

	/// Bind the local port, then forward every connection to it. `conn` is
	/// asked for on each accepted connection, so a dropped login comes back.
	pub async fn start<F, Fut>(
		&self,
		tunnel_id: &str,
		server_id: &str,
		local_port: u16,
		dest_host: String,
		dest_port: u16,
		conn: F,
	) -> AppResult<()>
	where
		F: Fn() -> Fut + Send + Sync + 'static,
		Fut: Future<Output = AppResult<Arc<Connection>>> + Send,
	{
		if self.is_running(tunnel_id) {
			return Ok(());
		}
		let listener = TcpListener::bind(("127.0.0.1", local_port))
			.await
			.map_err(|e| AppError::Io(format!("cannot listen on 127.0.0.1:{local_port}: {e}")))?;
		let conn = Arc::new(conn);
		let task = tauri::async_runtime::spawn(async move {
			while let Ok((mut socket, peer)) = listener.accept().await {
				let (conn, host) = (conn.clone(), dest_host.clone());
				tauri::async_runtime::spawn(async move {
					let Ok(c) = conn().await else { return };
					let Ok(ch) = c
						.channel_open_direct_tcpip(
							host,
							u32::from(dest_port),
							peer.ip().to_string(),
							u32::from(peer.port()),
						)
						.await
					else {
						return;
					};
					let mut remote = ch.into_stream();
					let _ = tokio::io::copy_bidirectional(&mut socket, &mut remote).await;
				});
			}
		});
		self.running
			.insert(tunnel_id.to_string(), Running { server_id: server_id.to_string(), task });
		Ok(())
	}

	pub fn stop(&self, tunnel_id: &str) {
		if let Some((_, r)) = self.running.remove(tunnel_id) {
			r.task.abort();
		}
	}

	pub fn stop_server(&self, server_id: &str) {
		let ids: Vec<String> = self
			.running
			.iter()
			.filter(|r| r.value().server_id == server_id)
			.map(|r| r.key().clone())
			.collect();
		for id in ids {
			self.stop(&id);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::services::ssh::{connect, test_sshd, Auth, KnownHosts, SshError, Target};
	use tokio::io::{AsyncReadExt, AsyncWriteExt};

	#[test]
	fn a_local_port_reaches_a_service_behind_the_server() {
		let d = test_sshd::start();
		let known = KnownHosts { read: vec![], ours: d.dir.path().join("known_hosts") };
		let target = Target {
			host: "127.0.0.1".into(),
			port: d.port,
			user: d.user.clone(),
			auth: Auth::KeyFile { path: d.key.clone(), passphrase: None },
		};
		tauri::async_runtime::block_on(async {
			let fp = match connect(&target, &known, None).await {
				Err(SshError::UnknownHost { fingerprint }) => fingerprint,
				_ => panic!("expected an unknown host"),
			};
			let conn = Arc::new(connect(&target, &known, Some(fp)).await.expect("connect"));
			// The "service behind the server": an echo listener the server can reach.
			let service = TcpListener::bind("127.0.0.1:0").await.expect("bind");
			let service_port = service.local_addr().expect("addr").port();
			tokio::spawn(async move {
				while let Ok((mut s, _)) = service.accept().await {
					tokio::spawn(async move {
						let mut buf = [0u8; 64];
						let n = s.read(&mut buf).await.unwrap_or(0);
						let _ = s.write_all(&buf[..n]).await;
					});
				}
			});
			let local = std::net::TcpListener::bind("127.0.0.1:0")
				.expect("free")
				.local_addr()
				.expect("a")
				.port();
			let tunnels = Tunnels::default();
			let c = conn.clone();
			tunnels
				.start("t1", "s1", local, "127.0.0.1".into(), service_port, move || {
					let c = c.clone();
					async move { Ok(c) }
				})
				.await
				.expect("start");
			assert!(tunnels.uses("s1"));
			let mut client =
				tokio::net::TcpStream::connect(("127.0.0.1", local)).await.expect("dial");
			client.write_all(b"through-the-tunnel").await.expect("write");
			let mut got = vec![0u8; 18];
			client.read_exact(&mut got).await.expect("read");
			assert_eq!(&got, b"through-the-tunnel");
			tunnels.stop("t1");
			assert!(!tunnels.uses("s1"));
			tokio::time::sleep(std::time::Duration::from_millis(100)).await;
			assert!(
				tokio::net::TcpStream::connect(("127.0.0.1", local)).await.is_err(),
				"a stopped tunnel no longer listens"
			);
		});
	}
}
