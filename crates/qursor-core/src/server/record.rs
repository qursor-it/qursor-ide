//! What a client reads to find the core, and the lock that keeps one core per
//! data folder (docs/superpowers/specs/2026-11-26-local-core-design.md §1).

use std::fs::{File, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

const RECORD: &str = "core.json";
const LOCK: &str = "core.lock";
const HOST_ID: &str = "host-id";

/// `Debug` by hand: the secret never reaches a log.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
	pub pid: u32,
	pub protocol_version: u32,
	pub app_version: String,
	pub port: u16,
	pub secret: String,
}

impl std::fmt::Debug for Record {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Record")
			.field("pid", &self.pid)
			.field("protocol_version", &self.protocol_version)
			.field("app_version", &self.app_version)
			.field("port", &self.port)
			.field("secret", &"<redacted>")
			.finish()
	}
}

/// `<data_dir>/core`, created.
pub fn core_dir(data_dir: &Path) -> io::Result<PathBuf> {
	let dir = data_dir.join("core");
	std::fs::create_dir_all(&dir)?;
	Ok(dir)
}

/// Written whole or not at all: a temp file beside it, renamed over it. The temp
/// file is created owner-only (0600 on Unix), and the rename keeps the mode.
pub fn write(dir: &Path, record: &Record) -> io::Result<()> {
	let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
	tmp.write_all(&serde_json::to_vec(record)?)?;
	tmp.as_file().sync_all()?;
	tmp.persist(dir.join(RECORD)).map_err(|e| e.error)?;
	Ok(())
}

/// Gone already is fine. One that will not go (permissions, a scanner holding
/// it) stays stale until a client's pid check sees through it, so it is logged.
pub fn remove(dir: &Path) {
	match std::fs::remove_file(dir.join(RECORD)) {
		Ok(()) => {}
		Err(e) if e.kind() == io::ErrorKind::NotFound => {}
		Err(error) => warn!(%error, "could not remove core.json"),
	}
}

/// The single-instance lock, held for as long as the returned file lives.
/// `None` when another process holds it.
pub fn lock(dir: &Path) -> io::Result<Option<File>> {
	let file = File::options().create(true).truncate(false).write(true).open(dir.join(LOCK))?;
	match file.try_lock() {
		Ok(()) => Ok(Some(file)),
		Err(TryLockError::WouldBlock) => Ok(None),
		Err(TryLockError::Error(e)) => Err(e),
	}
}

/// This host's id: minted on the first start, the same on every start after.
pub fn host_id(dir: &Path) -> io::Result<String> {
	let path = dir.join(HOST_ID);
	match std::fs::read_to_string(&path) {
		Ok(id) if !id.trim().is_empty() => Ok(id.trim().to_string()),
		Ok(_) => mint_host_id(&path),
		Err(e) if e.kind() == io::ErrorKind::NotFound => mint_host_id(&path),
		Err(e) => Err(e),
	}
}

fn mint_host_id(path: &Path) -> io::Result<String> {
	let id = uuid::Uuid::new_v4().to_string();
	std::fs::write(path, &id)?;
	Ok(id)
}

/// 32 bytes from the OS CSPRNG, hex.
pub fn new_secret() -> io::Result<String> {
	let mut bytes = [0u8; 32];
	getrandom::fill(&mut bytes).map_err(io::Error::other)?;
	Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn record() -> Record {
		Record {
			pid: 1,
			protocol_version: 1,
			app_version: "0.58.0".into(),
			port: 4242,
			secret: "ab".into(),
		}
	}

	#[test]
	fn write_then_read_back() {
		let dir = tempfile::TempDir::new().unwrap();
		write(dir.path(), &record()).unwrap();
		write(dir.path(), &record()).unwrap();
		let back: Record =
			serde_json::from_slice(&std::fs::read(dir.path().join(RECORD)).unwrap()).unwrap();
		assert_eq!(back, record());
		remove(dir.path());
		assert!(!dir.path().join(RECORD).exists());
	}

	#[cfg(unix)]
	#[test]
	fn record_is_owner_only() {
		use std::os::unix::fs::PermissionsExt;
		let dir = tempfile::TempDir::new().unwrap();
		write(dir.path(), &record()).unwrap();
		let mode = std::fs::metadata(dir.path().join(RECORD)).unwrap().permissions().mode();
		assert_eq!(mode & 0o777, 0o600);
	}

	#[test]
	fn second_instance_exits_3() {
		let dir = tempfile::TempDir::new().unwrap();
		let first = lock(dir.path()).unwrap();
		assert!(first.is_some());
		assert!(lock(dir.path()).unwrap().is_none(), "a second holder got the lock");
		drop(first);
		assert!(lock(dir.path()).unwrap().is_some());
	}

	#[test]
	fn debug_never_shows_the_secret() {
		let r = Record { secret: "s3cr3t".into(), ..record() };
		assert!(!format!("{r:?}").contains("s3cr3t"));
	}

	#[test]
	fn host_id_is_minted_once() {
		let dir = tempfile::TempDir::new().unwrap();
		let id = host_id(dir.path()).unwrap();
		assert_eq!(host_id(dir.path()).unwrap(), id);
	}

	#[test]
	fn secret_is_32_bytes_of_hex() {
		let (a, b) = (new_secret().unwrap(), new_secret().unwrap());
		assert_eq!(a.len(), 64);
		assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
		assert_ne!(a, b);
	}
}
