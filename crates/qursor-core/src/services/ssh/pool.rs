//! One live connection per saved server, shared by every tab on it (ADR-0105).

use std::future::Future;
use std::sync::Arc;

use dashmap::DashMap;

use super::{connect, Connection, KnownHosts, Target};
use crate::error::AppResult;

pub struct Pool {
	conns: DashMap<String, Arc<Connection>>,
	/// One connect at a time per server, so two tabs opened together share one
	/// login instead of racing two.
	connecting: DashMap<String, Arc<tokio::sync::Mutex<()>>>,
	known: KnownHosts,
	/// `disconnect` is called from plain threads (the reaper), which have no
	/// runtime to spawn on.
	rt: tokio::runtime::Handle,
}

impl Pool {
	pub fn new(known: KnownHosts, rt: tokio::runtime::Handle) -> Self {
		Self { conns: DashMap::new(), connecting: DashMap::new(), known, rt }
	}

	pub fn is_connected(&self, server_id: &str) -> bool {
		self.conns.get(server_id).is_some_and(|c| !c.is_closed())
	}

	/// The server's connection, opening it if there is none or it dropped.
	/// `target` is only built — and its secrets only decrypted — when a new
	/// login is actually needed.
	pub async fn connection<F, Fut>(
		&self,
		server_id: &str,
		target: F,
		accept: Option<String>,
	) -> AppResult<Arc<Connection>>
	where
		F: FnOnce() -> Fut,
		Fut: Future<Output = AppResult<Target>>,
	{
		let gate = self.connecting.entry(server_id.to_string()).or_default().clone();
		let _held = gate.lock().await;
		if let Some(c) = self.conns.get(server_id).map(|c| c.clone()) {
			if !c.is_closed() {
				return Ok(c);
			}
		}
		let t = target().await?;
		let conn = Arc::new(connect(&t, &self.known, accept).await?);
		drop(t);
		self.conns.insert(server_id.to_string(), conn.clone());
		Ok(conn)
	}

	/// Drop the server's connection. Its shell channels close with it, which the
	/// terminals report as an exit.
	pub fn disconnect(&self, server_id: &str) {
		if let Some((_, conn)) = self.conns.remove(server_id) {
			self.rt.spawn(async move {
				let _ = conn.disconnect(russh::Disconnect::ByApplication, "", "en").await;
			});
		}
	}

	/// Drop every connection no tab uses any more. Called on a timer, so the last
	/// tab closing ends the login without each close path having to remember.
	pub fn reap(&self, in_use: impl Fn(&str) -> bool) {
		let idle: Vec<String> =
			self.conns.iter().map(|e| e.key().clone()).filter(|id| !in_use(id)).collect();
		for id in idle {
			self.disconnect(&id);
		}
	}

	pub fn disconnect_all(&self) {
		let ids: Vec<String> = self.conns.iter().map(|e| e.key().clone()).collect();
		for id in ids {
			self.disconnect(&id);
		}
	}
}
