//! SSH connections for server sessions (F45, ADR-0105).
//!
//! **One library connection per server, many channels on it.** A second tab on
//! the same server is a second shell channel, not a second login, which is the
//! one thing the system `ssh.exe` cannot do on Windows (no `ControlMaster`).
//!
//! **Host keys are checked, never assumed.** The user's own
//! `~/.ssh/known_hosts` is read so a server they already trust is trusted here;
//! a key this app has been told to trust is written to its own file, never to
//! theirs. A changed key is refused outright — there is no "accept anyway".
//!
//! Secrets arrive in [`Auth`] already decrypted from the vault, live in
//! `Zeroizing` buffers, and are never logged or returned.

pub mod pool;
pub mod pty;
pub mod sftp;
pub mod tunnel;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use russh::client::{self, Handle};
use russh::keys::agent::client::AgentClient;
use russh::keys::agent::AgentIdentity;
use russh::keys::{self, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::Channel;
use zeroize::Zeroizing;

/// How a server is logged into. The secret-bearing variants are filled from
/// the vault at connect time and dropped (and zeroed) right after.
pub enum Auth {
	Password(Zeroizing<String>),
	/// A private key held in the vault, as OpenSSH/PEM text.
	Key {
		pem: Zeroizing<String>,
		passphrase: Option<Zeroizing<String>>,
	},
	/// A key file the user already has, e.g. `~/.ssh/id_ed25519`; not copied.
	KeyFile {
		path: PathBuf,
		passphrase: Option<Zeroizing<String>>,
	},
	/// The running SSH agent: `SSH_AUTH_SOCK`, or on Windows the OpenSSH
	/// agent's pipe and then Pageant.
	Agent,
}

pub struct Target {
	pub host: String,
	pub port: u16,
	pub user: String,
	pub auth: Auth,
}

#[derive(Debug, thiserror::Error)]
pub enum SshError {
	/// The server's key is in no known_hosts file. The human decides; the
	/// fingerprint is what they are shown and what they confirm.
	#[error("unknown host key {fingerprint}")]
	UnknownHost { fingerprint: String },
	#[error(
		"the host key of {host} has changed (now {fingerprint}); refusing to connect. If the server was reinstalled, remove its old entry from known_hosts"
	)]
	HostKeyChanged { host: String, fingerprint: String },
	#[error("authentication failed for {0}")]
	AuthFailed(String),
	#[error("{0}")]
	Other(String),
}

impl From<russh::Error> for SshError {
	fn from(e: russh::Error) -> Self {
		SshError::Other(e.to_string())
	}
}

impl From<SshError> for crate::error::AppError {
	fn from(e: SshError) -> Self {
		use crate::error::AppError;
		match e {
			SshError::UnknownHost { fingerprint } => AppError::HostKey(fingerprint),
			SshError::AuthFailed(_) => AppError::Unauthenticated(e.to_string()),
			other => AppError::Remote(other.to_string()),
		}
	}
}

/// Where host keys are looked up, and where a newly trusted one is written.
#[derive(Clone)]
pub struct KnownHosts {
	/// Read-only to us: the user's own files.
	pub read: Vec<PathBuf>,
	/// Read, and the only file a newly trusted key is written to.
	pub ours: PathBuf,
}

impl KnownHosts {
	/// The user's `~/.ssh/known_hosts`; ours is always read as well.
	pub fn for_user(ours: PathBuf) -> Self {
		let read =
			dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts")).into_iter().collect();
		Self { read, ours }
	}
}

enum Verdict {
	Unknown(String),
	Changed(String),
	Certificate,
}

pub struct Client {
	host: String,
	port: u16,
	known: KnownHosts,
	/// A fingerprint the human confirmed for this connect, if any.
	accept: Option<String>,
	verdict: Arc<Mutex<Option<Verdict>>>,
}

impl client::Handler for Client {
	type Error = russh::Error;

	async fn check_server_key(
		&mut self,
		server_key: &PublicKeyOrCertificate,
	) -> Result<bool, Self::Error> {
		// shortcut: host certificates are refused, upgrade when a user has a CA-signed host.
		let PublicKeyOrCertificate::PublicKey { key, .. } = server_key else {
			*self.verdict.lock() = Some(Verdict::Certificate);
			return Ok(false);
		};
		let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
		let mut pinned = false;
		for file in self.known.read.iter().chain(std::iter::once(&self.known.ours)) {
			match keys::check_known_hosts_path(&self.host, self.port, key, file) {
				Ok(true) => return Ok(true),
				Ok(false) => {}
				Err(keys::Error::KeyChanged { .. }) => pinned = true,
				// An unreadable file is a file with nothing in it for us.
				Err(_) => {}
			}
			// A key of *another type* is not a new host: an impostor offering only
			// RSA for a host pinned as ed25519 must not get a "trust?" prompt.
			pinned |= keys::known_hosts::known_host_keys_path(&self.host, self.port, file)
				.is_ok_and(|k| !k.is_empty());
		}
		if pinned {
			*self.verdict.lock() = Some(Verdict::Changed(fingerprint));
			return Ok(false);
		}
		if self.accept.as_deref() == Some(fingerprint.as_str()) {
			keys::known_hosts::learn_known_hosts_path(&self.host, self.port, key, &self.known.ours)
				.map_err(|e| russh::Error::IO(std::io::Error::other(e.to_string())))?;
			return Ok(true);
		}
		*self.verdict.lock() = Some(Verdict::Unknown(fingerprint));
		Ok(false)
	}
}

pub type Connection = Handle<Client>;

/// Open and authenticate a connection.
///
/// `accept` is the fingerprint the human confirmed after an
/// [`SshError::UnknownHost`]; it is trusted only if it is exactly the key the
/// server presents now.
pub async fn connect(
	target: &Target,
	known: &KnownHosts,
	accept: Option<String>,
) -> Result<Connection, SshError> {
	let config = Arc::new(client::Config {
		keepalive_interval: Some(Duration::from_secs(30)),
		..Default::default()
	});
	let verdict = Arc::new(Mutex::new(None));
	let handler = Client {
		host: target.host.clone(),
		port: target.port,
		known: known.clone(),
		accept,
		verdict: verdict.clone(),
	};
	let connecting = client::connect(config, (target.host.as_str(), target.port), handler);
	let mut handle = match tokio::time::timeout(Duration::from_secs(20), connecting).await {
		Ok(Ok(h)) => h,
		Ok(Err(e)) => {
			return Err(match verdict.lock().take() {
				Some(Verdict::Unknown(fingerprint)) => SshError::UnknownHost { fingerprint },
				Some(Verdict::Changed(fingerprint)) => {
					SshError::HostKeyChanged { host: target.host.clone(), fingerprint }
				}
				Some(Verdict::Certificate) => SshError::Other(format!(
					"{} presents a host certificate, which is not supported yet; use the system ssh login for it",
					target.host
				)),
				None => SshError::Other(format!("connecting to {}: {e}", target.host)),
			});
		}
		Err(_) => return Err(SshError::Other(format!("connecting to {} timed out", target.host))),
	};
	// A stuck agent or a slow PAM must not hang the tab forever.
	tokio::time::timeout(Duration::from_secs(30), authenticate(&mut handle, target))
		.await
		.map_err(|_| SshError::Other(format!("logging in to {} timed out", target.host)))??;
	Ok(handle)
}

async fn authenticate(h: &mut Connection, t: &Target) -> Result<(), SshError> {
	let who = format!("{}@{}", t.user, t.host);
	let ok = match &t.auth {
		Auth::Password(pw) => {
			if h.authenticate_password(&t.user, pw.as_str()).await?.success() {
				true
			} else {
				keyboard_interactive_with_password(h, &t.user, pw).await?
			}
		}
		Auth::Key { pem, passphrase } => {
			let key = keys::decode_secret_key(pem, passphrase.as_deref().map(String::as_str))
				.map_err(|e| SshError::Other(format!("reading the private key: {e}")))?;
			publickey(h, &t.user, key).await?
		}
		Auth::KeyFile { path, passphrase } => {
			let key = keys::load_secret_key(path, passphrase.as_deref().map(String::as_str))
				.map_err(|e| SshError::Other(format!("reading {}: {e}", path.display())))?;
			publickey(h, &t.user, key).await?
		}
		Auth::Agent => agent(h, &t.user).await?,
	};
	if ok {
		Ok(())
	} else {
		Err(SshError::AuthFailed(who))
	}
}

async fn publickey(
	h: &mut Connection,
	user: &str,
	key: keys::PrivateKey,
) -> Result<bool, SshError> {
	let hash = h.best_supported_rsa_hash().await?.flatten();
	Ok(h.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
		.await?
		.success())
}

/// Servers that only offer `keyboard-interactive` (PAM) still ask for the
/// password; answer every prompt of the first round with it. A real second
/// factor needs a human and is not answered.
async fn keyboard_interactive_with_password(
	h: &mut Connection,
	user: &str,
	pw: &Zeroizing<String>,
) -> Result<bool, SshError> {
	use russh::client::KeyboardInteractiveAuthResponse as R;
	let reply = h.authenticate_keyboard_interactive_start(user, None::<String>).await?;
	let R::InfoRequest { prompts, .. } = reply else { return Ok(matches!(reply, R::Success)) };
	// The API takes owned strings; they live only for this call.
	let answers = prompts.iter().map(|_| pw.to_string()).collect();
	// Only the first round: a second one is a second factor, and answering it
	// with the password would count towards a lockout.
	Ok(matches!(h.authenticate_keyboard_interactive_respond(answers).await?, R::Success))
}

async fn agent(h: &mut Connection, user: &str) -> Result<bool, SshError> {
	#[cfg(unix)]
	{
		let client = AgentClient::connect_env()
			.await
			.map_err(|e| SshError::Other(format!("no SSH agent (SSH_AUTH_SOCK): {e}")))?;
		agent_with(h, user, client).await
	}
	#[cfg(windows)]
	{
		if let Ok(client) = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
			if agent_with(h, user, client).await? {
				return Ok(true);
			}
		}
		let client = AgentClient::connect_pageant()
			.await
			.map_err(|e| SshError::Other(format!("no SSH agent (OpenSSH or Pageant): {e}")))?;
		agent_with(h, user, client).await
	}
}

async fn agent_with<R>(
	h: &mut Connection,
	user: &str,
	mut client: AgentClient<R>,
) -> Result<bool, SshError>
where
	R: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
	let ids = client
		.request_identities()
		.await
		.map_err(|e| SshError::Other(format!("SSH agent: {e}")))?;
	let hash = h.best_supported_rsa_hash().await?.flatten();
	for id in ids {
		let AgentIdentity::PublicKey { key, .. } = id else { continue };
		let res = h
			.authenticate_publickey_with(user, key, hash, &mut client)
			.await
			.map_err(|e| SshError::Other(format!("SSH agent: {e}")))?;
		if res.success() {
			return Ok(true);
		}
	}
	Ok(false)
}

/// A login shell with a PTY, started in `dir` when one is given, inside the
/// tmux session `tmux` when that is given and the server has tmux (F49).
///
/// `cd` failing (a folder since removed) still opens the shell, in the home
/// directory, rather than closing the tab the human just opened.
pub async fn open_shell(
	conn: &Connection,
	cols: u16,
	rows: u16,
	dir: Option<&str>,
	tmux: Option<&str>,
) -> Result<Channel<client::Msg>, SshError> {
	let ch = conn.channel_open_session().await?;
	ch.request_pty(true, "xterm-256color", u32::from(cols), u32::from(rows), 0, 0, &[]).await?;
	match start_in(dir, tmux) {
		Some(cmd) => ch.exec(true, cmd).await?,
		None => ch.request_shell(true).await?,
	}
	Ok(ch)
}

/// The remote command that opens the tab's shell, if a plain login shell is
/// not enough. sshd runs it through the user's own shell, which may be fish or
/// csh; wrapping it in `/bin/sh` keeps the redirection and `${…}` meaning what
/// they say. A server without tmux gets a plain login shell.
pub fn start_in(dir: Option<&str>, tmux: Option<&str>) -> Option<String> {
	let dir = dir.filter(|d| !d.is_empty());
	if dir.is_none() && tmux.is_none() {
		return None;
	}
	let mut inner = String::new();
	if let Some(d) = dir {
		inner.push_str(&format!("cd {} 2>/dev/null; ", quote(d)));
	}
	if let Some(name) = tmux {
		inner.push_str(&format!(
			"if command -v tmux >/dev/null 2>&1; then exec tmux new-session -A -s {}; fi; ",
			quote(name)
		));
	}
	inner.push_str("exec \"${SHELL:-/bin/sh}\" -l");
	Some(format!("exec /bin/sh -c {}", quote(&inner)))
}

/// The tmux session a persistent tab lives in (F49): stable across restarts,
/// because the tab key is.
pub fn tmux_name(client_key: &str) -> String {
	let id: String = client_key.chars().filter(char::is_ascii_alphanumeric).collect();
	format!("qursor-{}", &id[id.len().saturating_sub(12)..])
}

/// POSIX single-quoting: the only quoting that needs no escaping rules inside.
pub fn quote(s: &str) -> String {
	format!("'{}'", s.replace('\'', r"'\''"))
}

/// A throwaway `sshd` on loopback for tests: its own host key, client key and
/// config in a temp dir, logging in as whoever runs the tests. Never touches
/// system accounts or the system's sshd configuration.
#[cfg(all(test, unix))]
pub(crate) mod test_sshd {
	use std::net::{TcpListener, TcpStream};
	use std::path::PathBuf;
	use std::process::{Child, Command, Stdio};
	use std::time::{Duration, Instant};

	pub struct Sshd {
		child: Child,
		pub port: u16,
		pub dir: tempfile::TempDir,
		pub key: PathBuf,
		pub user: String,
	}

	impl Drop for Sshd {
		fn drop(&mut self) {
			let _ = self.child.kill();
			let _ = self.child.wait();
		}
	}

	fn keygen(path: &std::path::Path, kind: &str) {
		let ok = Command::new("ssh-keygen")
			.args(["-q", "-t", kind, "-N", "", "-f"])
			.arg(path)
			.status()
			.expect("ssh-keygen is installed (openssh-client)")
			.success();
		assert!(ok, "ssh-keygen failed");
	}

	pub fn public_key_line(path: &std::path::Path) -> String {
		let pub_path = path.with_extension("pub");
		let text = std::fs::read_to_string(pub_path).expect("public key");
		text.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
	}

	pub fn new_key(dir: &std::path::Path, name: &str) -> PathBuf {
		new_key_of(dir, name, "ed25519")
	}

	pub fn new_key_of(dir: &std::path::Path, name: &str, kind: &str) -> PathBuf {
		let p = dir.join(name);
		keygen(&p, kind);
		p
	}

	pub fn start() -> Sshd {
		let sshd = ["/usr/sbin/sshd", "/usr/bin/sshd"]
			.into_iter()
			.map(PathBuf::from)
			.find(|p| p.exists())
			.expect("the SSH tests need an sshd binary (openssh-server)");
		let dir = tempfile::tempdir().expect("tempdir");
		let host = new_key(dir.path(), "host");
		let key = new_key(dir.path(), "id");
		std::fs::copy(key.with_extension("pub"), dir.path().join("authorized_keys"))
			.expect("authorized_keys");
		let port =
			TcpListener::bind("127.0.0.1:0").expect("free port").local_addr().expect("addr").port();
		let config = dir.path().join("sshd_config");
		std::fs::write(
			&config,
			format!(
				"ListenAddress 127.0.0.1\nPort {port}\nHostKey {}\nAuthorizedKeysFile {}\nPidFile none\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPermitRootLogin prohibit-password\nStrictModes no\nUsePAM no\nSubsystem sftp internal-sftp\n",
				host.display(),
				dir.path().join("authorized_keys").display()
			),
		)
		.expect("sshd_config");
		// Privilege separation wants this when sshd runs as root.
		let _ = std::fs::create_dir_all("/run/sshd");
		let child = Command::new(sshd)
			.args(["-D", "-e", "-f"])
			.arg(&config)
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.expect("start sshd");
		let deadline = Instant::now() + Duration::from_secs(10);
		while TcpStream::connect(("127.0.0.1", port)).is_err() {
			assert!(Instant::now() < deadline, "sshd did not start listening");
			std::thread::sleep(Duration::from_millis(50));
		}
		let user = String::from_utf8(Command::new("id").arg("-un").output().expect("id").stdout)
			.expect("utf8")
			.trim()
			.to_string();
		Sshd { child, port, dir, key, user }
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(unix)]
	fn target(d: &test_sshd::Sshd) -> (Target, KnownHosts) {
		(
			Target {
				host: "127.0.0.1".into(),
				port: d.port,
				user: d.user.clone(),
				auth: Auth::KeyFile { path: d.key.clone(), passphrase: None },
			},
			KnownHosts { read: vec![], ours: d.dir.path().join("known_hosts") },
		)
	}

	/// The whole M1 path against a real sshd: an unknown key is refused until the
	/// human confirms its fingerprint, is remembered afterwards, and a shell
	/// channel bridged to a PTY carries input, output and the exit status.
	#[cfg(unix)]
	#[test]
	fn a_shell_over_ssh_round_trips_after_the_host_key_is_confirmed() {
		use std::io::{Read, Write};
		let d = test_sshd::start();
		let (t, known) = target(&d);
		crate::test_support::block_on(async {
			let fingerprint = match connect(&t, &known, None).await {
				Err(SshError::UnknownHost { fingerprint }) => fingerprint,
				Err(e) => panic!("expected an unknown host, got {e}"),
				Ok(_) => panic!("an unknown host key was trusted"),
			};
			assert!(fingerprint.starts_with("SHA256:"), "{fingerprint}");
			let wrong = connect(&t, &known, Some("SHA256:not-it".into())).await;
			assert!(
				matches!(wrong, Err(SshError::UnknownHost { .. })),
				"only the confirmed key is trusted"
			);
			connect(&t, &known, Some(fingerprint)).await.expect("trusted after confirmation");
			let conn = connect(&t, &known, None).await.expect("remembered in our known_hosts");

			let ch = open_shell(&conn, 80, 24, Some("/tmp"), None).await.expect("shell");
			let (master, mut child) = pty::bridge(ch, 80, 24);
			let mut w = master.take_writer().expect("writer");
			let mut r = master.try_clone_reader().expect("reader");
			let reading = std::thread::spawn(move || {
				let mut out = Vec::new();
				let _ = r.read_to_end(&mut out);
				String::from_utf8_lossy(&out).into_owned()
			});
			w.write_all(b"echo \"cwd=$(pwd)\"; exit 7\n").expect("write");
			let status = tokio::task::spawn_blocking(move || child.wait())
				.await
				.expect("join")
				.expect("wait");
			assert_eq!(status.exit_code(), 7);
			let out = reading.join().expect("reader thread");
			assert!(out.contains("cwd=/tmp"), "{out}");
		});
	}

	#[cfg(unix)]
	#[test]
	fn a_changed_host_key_is_refused_even_when_a_fingerprint_is_offered() {
		let d = test_sshd::start();
		let (t, known) = target(&d);
		let other = test_sshd::new_key(d.dir.path(), "impostor");
		std::fs::write(
			&known.ours,
			format!("[127.0.0.1]:{} {}\n", d.port, test_sshd::public_key_line(&other)),
		)
		.expect("known_hosts");
		let res =
			crate::test_support::block_on(connect(&t, &known, Some("SHA256:anything".into())));
		assert!(
			matches!(res, Err(SshError::HostKeyChanged { .. })),
			"a changed key must never connect"
		);
	}

	/// A host pinned with a key of another type is not a "new host": the impostor
	/// offering a different algorithm gets a refusal, not a trust prompt.
	#[cfg(unix)]
	#[test]
	fn a_host_pinned_with_another_key_type_is_refused_not_offered() {
		let d = test_sshd::start();
		let (t, known) = target(&d);
		let rsa = test_sshd::new_key_of(d.dir.path(), "pinned_rsa", "rsa");
		std::fs::write(
			&known.ours,
			format!("[127.0.0.1]:{} {}\n", d.port, test_sshd::public_key_line(&rsa)),
		)
		.expect("known_hosts");
		let res = crate::test_support::block_on(connect(&t, &known, None));
		assert!(matches!(res, Err(SshError::HostKeyChanged { .. })), "got {:?}", res.err());
	}

	/// The reaper is a plain thread with no runtime: dropping a login from there
	/// must reach the pool's own runtime rather than panic (and abort the app).
	#[cfg(unix)]
	#[test]
	fn the_reaper_thread_can_drop_a_connection_without_a_runtime() {
		let d = test_sshd::start();
		let (t, known) = target(&d);
		let pool = Arc::new(pool::Pool::new(known.clone(), crate::test_support::handle()));
		crate::test_support::block_on(async {
			let Err(SshError::UnknownHost { fingerprint }) = connect(&t, &known, None).await else {
				panic!("expected an unknown host");
			};
			let (target, _) = target(&d);
			pool.connection("s", || async move { Ok(target) }, Some(fingerprint))
				.await
				.expect("connect");
		});
		let reaper = pool.clone();
		std::thread::spawn(move || reaper.reap(|_| false)).join().expect("reap must not panic");
		assert!(!pool.is_connected("s"));
	}

	#[test]
	fn quote_survives_single_quotes_and_spaces() {
		assert_eq!(quote("/srv/my app"), "'/srv/my app'");
		assert_eq!(quote("it's"), r"'it'\''s'");
	}
}
