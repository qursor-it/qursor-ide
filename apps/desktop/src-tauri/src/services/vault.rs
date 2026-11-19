//! The local vault for SSH secrets: passwords, private keys, passphrases
//! (F44, ADR-0104).
//!
//! **One encrypted file, one key.** Each entry is AES-256-GCM with its id as
//! associated data, so an entry cannot be moved under another server's id.
//! The key is either random and kept in the OS keychain (Windows Credential
//! Manager, macOS Keychain, Secret Service), or derived with Argon2id from a
//! master password the human types at unlock and that is never stored.
//!
//! **Nothing here returns a secret to the renderer.** The commands report
//! whether a server *has* a secret; only the SSH connect path reads one, into a
//! `Zeroizing` buffer that is wiped when the connection has authenticated.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, Generate, Key, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};

const KEYCHAIN_SERVICE: &str = "it.qursor.ide";
const KEYCHAIN_USER: &str = "ssh-vault-key";
/// Encrypted under the key so a typed master password can be checked without
/// trying it on a real secret.
const CHECK: &[u8] = b"qursor-vault-v1";
const NONCE_LEN: usize = 12;

/// Where the random key lives in `Os` mode. A trait only so tests can run
/// without a desktop keychain; the app uses [`OsKeychain`].
pub trait KeyStore: Send + Sync {
	fn load(&self) -> Result<Option<Vec<u8>>, String>;
	fn save(&self, key: &[u8]) -> Result<(), String>;
	fn delete(&self) -> Result<(), String>;
}

pub struct OsKeychain;

impl OsKeychain {
	fn entry() -> Result<keyring::Entry, String> {
		keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).map_err(|e| e.to_string())
	}
}

impl KeyStore for OsKeychain {
	fn load(&self) -> Result<Option<Vec<u8>>, String> {
		match Self::entry()?.get_secret() {
			Ok(k) => Ok(Some(k)),
			Err(keyring::Error::NoEntry) => Ok(None),
			Err(e) => Err(e.to_string()),
		}
	}
	fn save(&self, key: &[u8]) -> Result<(), String> {
		Self::entry()?.set_secret(key).map_err(|e| e.to_string())
	}
	fn delete(&self) -> Result<(), String> {
		match Self::entry()?.delete_credential() {
			Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
			Err(e) => Err(e.to_string()),
		}
	}
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum VaultMode {
	/// Random key in the OS keychain: unlocked whenever the OS user is.
	#[default]
	Keychain,
	/// Key derived from a master password; locked at every start.
	MasterPassword,
}

#[derive(Serialize, Deserialize, Default)]
struct VaultFile {
	version: u32,
	mode: VaultMode,
	/// Argon2id salt, base64, in `MasterPassword` mode.
	salt: Option<String>,
	/// [`CHECK`] encrypted under the current key.
	check: Option<String>,
	entries: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
	pub mode: VaultMode,
	pub locked: bool,
	/// Why the keychain could not be used, when it could not: the human is then
	/// offered a master password instead.
	pub keychain_error: Option<String>,
}

type KeyBytes = Zeroizing<[u8; 32]>;

pub struct Vault {
	path: PathBuf,
	store: Box<dyn KeyStore>,
	inner: Mutex<Inner>,
}

struct Inner {
	file: VaultFile,
	key: Option<KeyBytes>,
	keychain_error: Option<String>,
}

impl Vault {
	pub fn open(path: PathBuf, store: Box<dyn KeyStore>) -> AppResult<Self> {
		let file: VaultFile = match std::fs::read(&path) {
			Ok(bytes) => serde_json::from_slice(&bytes)
				.map_err(|e| AppError::Io(format!("the vault file is unreadable: {e}")))?,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => VaultFile::default(),
			Err(e) => return Err(e.into()),
		};
		let mut inner = Inner { file, key: None, keychain_error: None };
		if inner.file.mode == VaultMode::Keychain {
			match store.load() {
				Ok(Some(k)) => inner.key = Some(to_key(&k)?),
				Ok(None) => {}
				Err(e) => inner.keychain_error = Some(e),
			}
		}
		Ok(Self { path, store, inner: Mutex::new(inner) })
	}

	pub fn status(&self) -> VaultStatus {
		let i = self.inner.lock();
		let locked = match i.file.mode {
			VaultMode::MasterPassword => i.key.is_none(),
			// A keychain vault that has never stored anything has no key yet and
			// is not locked: the first `put` creates one.
			VaultMode::Keychain => i.key.is_none() && i.keychain_error.is_some(),
		};
		VaultStatus { mode: i.file.mode, locked, keychain_error: i.keychain_error.clone() }
	}

	pub fn unlock(&self, password: &str) -> AppResult<()> {
		let mut i = self.inner.lock();
		if i.file.mode != VaultMode::MasterPassword {
			return Ok(());
		}
		let salt =
			i.file.salt.as_deref().ok_or_else(|| AppError::Io("vault has no salt".into()))?;
		let key = derive(password, &B64.decode(salt).map_err(|e| AppError::Io(e.to_string()))?)?;
		let check = i.file.check.clone().unwrap_or_default();
		if open(&key, "", &check).ok().as_deref().map(|v| &v[..]) != Some(CHECK) {
			return Err(AppError::Unauthenticated("wrong master password".into()));
		}
		i.key = Some(key);
		Ok(())
	}

	pub fn lock(&self) {
		let mut i = self.inner.lock();
		if i.file.mode == VaultMode::MasterPassword {
			i.key = None;
		}
	}

	pub fn has(&self, id: &str) -> bool {
		self.inner.lock().file.entries.contains_key(id)
	}

	pub fn get(&self, id: &str) -> AppResult<Option<Zeroizing<Vec<u8>>>> {
		let i = self.inner.lock();
		let Some(sealed) = i.file.entries.get(id) else { return Ok(None) };
		let key = i.key.as_ref().ok_or_else(locked)?;
		open(key, id, sealed).map(Some)
	}

	pub fn put(&self, id: &str, secret: &[u8]) -> AppResult<()> {
		let mut i = self.inner.lock();
		if i.key.is_none() && i.file.mode == VaultMode::Keychain && i.keychain_error.is_none() {
			let key = new_key();
			self.store.save(&key[..]).map_err(|e| {
				AppError::Io(format!(
					"the OS keychain refused the vault key: {e}. Set a master password instead."
				))
			})?;
			i.file.check = Some(seal(&key, "", CHECK)?);
			i.key = Some(key);
		}
		let key = i.key.as_ref().ok_or_else(locked)?;
		let sealed = seal(key, id, secret)?;
		i.file.entries.insert(id.to_string(), sealed);
		write(&self.path, &i.file)
	}

	pub fn remove(&self, id: &str) -> AppResult<()> {
		let mut i = self.inner.lock();
		if i.file.entries.remove(id).is_some() {
			write(&self.path, &i.file)?;
		}
		Ok(())
	}

	/// Remove every entry whose id starts with `prefix` — a deleted server's.
	pub fn remove_prefix(&self, prefix: &str) -> AppResult<()> {
		let mut i = self.inner.lock();
		let before = i.file.entries.len();
		i.file.entries.retain(|k, _| !k.starts_with(prefix));
		if i.file.entries.len() != before {
			write(&self.path, &i.file)?;
		}
		Ok(())
	}

	/// Switch to a master password (`Some`) or back to the keychain (`None`),
	/// re-encrypting every entry under the new key. The vault must be unlocked.
	pub fn set_master_password(&self, password: Option<&str>) -> AppResult<()> {
		let mut i = self.inner.lock();
		let plain: Vec<(String, Zeroizing<Vec<u8>>)> = match &i.key {
			Some(k) => i
				.file
				.entries
				.iter()
				.map(|(id, sealed)| open(k, id, sealed).map(|v| (id.clone(), v)))
				.collect::<AppResult<_>>()?,
			None if i.file.entries.is_empty() => Vec::new(),
			None => return Err(locked()),
		};
		let (key, mode, salt) = match password {
			Some(pw) if pw.chars().count() < 8 => {
				return Err(AppError::InvalidInput(
					"a master password needs at least 8 characters".into(),
				))
			}
			Some(pw) => {
				let salt = new_key();
				(derive(pw, &salt[..16])?, VaultMode::MasterPassword, Some(B64.encode(&salt[..16])))
			}
			None => {
				let key = new_key();
				self.store.save(&key[..]).map_err(|e| {
					AppError::Io(format!("the OS keychain refused the vault key: {e}"))
				})?;
				(key, VaultMode::Keychain, None)
			}
		};
		let mut entries = BTreeMap::new();
		for (id, v) in &plain {
			entries.insert(id.clone(), seal(&key, id, v)?);
		}
		let file =
			VaultFile { version: 1, mode, salt, check: Some(seal(&key, "", CHECK)?), entries };
		write(&self.path, &file)?;
		if mode == VaultMode::MasterPassword {
			// The keychain key must not outlive the switch: it would decrypt nothing,
			// but it is still a secret lying around.
			let _ = self.store.delete();
		}
		i.file = file;
		i.key = Some(key);
		i.keychain_error = None;
		Ok(())
	}
}

fn locked() -> AppError {
	AppError::Unauthenticated("the vault is locked".into())
}

fn new_key() -> KeyBytes {
	let k = Key::<Aes256Gcm>::generate();
	let mut out = Zeroizing::new([0u8; 32]);
	out.copy_from_slice(k.as_slice());
	out
}

fn to_key(bytes: &[u8]) -> AppResult<KeyBytes> {
	let arr: [u8; 32] = bytes
		.try_into()
		.map_err(|_| AppError::Io("the stored vault key has the wrong size".into()))?;
	Ok(Zeroizing::new(arr))
}

fn derive(password: &str, salt: &[u8]) -> AppResult<KeyBytes> {
	let mut out = Zeroizing::new([0u8; 32]);
	argon2::Argon2::default()
		.hash_password_into(password.as_bytes(), salt, &mut out[..])
		.map_err(|e| AppError::Io(format!("deriving the vault key: {e}")))?;
	Ok(out)
}

fn cipher(key: &KeyBytes) -> AppResult<Aes256Gcm> {
	Aes256Gcm::new_from_slice(&key[..]).map_err(|e| AppError::Io(e.to_string()))
}

fn seal(key: &KeyBytes, id: &str, plain: &[u8]) -> AppResult<String> {
	let nonce = Nonce::generate();
	let ct = cipher(key)?
		.encrypt(&nonce, Payload { msg: plain, aad: id.as_bytes() })
		.map_err(|_| AppError::Io("encryption failed".into()))?;
	let mut out = nonce.to_vec();
	out.extend_from_slice(&ct);
	Ok(B64.encode(out))
}

fn open(key: &KeyBytes, id: &str, sealed: &str) -> AppResult<Zeroizing<Vec<u8>>> {
	let raw = B64.decode(sealed).map_err(|e| AppError::Io(e.to_string()))?;
	if raw.len() < NONCE_LEN {
		return Err(AppError::Io("a vault entry is truncated".into()));
	}
	let (n, ct) = raw.split_at(NONCE_LEN);
	let nonce = Nonce::try_from(n).map_err(|_| AppError::Io("bad nonce".into()))?;
	cipher(key)?
		.decrypt(&nonce, Payload { msg: ct, aad: id.as_bytes() })
		.map(Zeroizing::new)
		.map_err(|_| AppError::Unauthenticated("a vault entry could not be decrypted".into()))
}

/// Atomic, and owner-only on Unix: a half-written vault would lose every secret.
fn write(path: &Path, file: &VaultFile) -> AppResult<()> {
	let dir = path.parent().ok_or_else(|| AppError::Io("vault path has no folder".into()))?;
	std::fs::create_dir_all(dir)?;
	let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
	std::io::Write::write_all(&mut tmp, &serde_json::to_vec_pretty(file)?)?;
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o600))?;
	}
	tmp.persist(path).map_err(|e| AppError::Io(e.to_string()))?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::Arc;

	#[derive(Default, Clone)]
	struct MemStore(Arc<Mutex<Option<Vec<u8>>>>);
	impl KeyStore for MemStore {
		fn load(&self) -> Result<Option<Vec<u8>>, String> {
			Ok(self.0.lock().clone())
		}
		fn save(&self, key: &[u8]) -> Result<(), String> {
			*self.0.lock() = Some(key.to_vec());
			Ok(())
		}
		fn delete(&self) -> Result<(), String> {
			*self.0.lock() = None;
			Ok(())
		}
	}

	struct NoKeychain;
	impl KeyStore for NoKeychain {
		fn load(&self) -> Result<Option<Vec<u8>>, String> {
			Err("no secret service".into())
		}
		fn save(&self, _: &[u8]) -> Result<(), String> {
			Err("no secret service".into())
		}
		fn delete(&self) -> Result<(), String> {
			Ok(())
		}
	}

	#[test]
	fn a_secret_round_trips_and_never_lands_in_the_file_in_clear() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("secrets.vault");
		let store = MemStore::default();
		let v = Vault::open(path.clone(), Box::new(store.clone())).unwrap();
		v.put("server:1:password", b"hunter2-very-secret").unwrap();
		let on_disk = std::fs::read_to_string(&path).unwrap();
		assert!(!on_disk.contains("hunter2"), "plaintext on disk");
		// A fresh process finds the key in the keychain again.
		let v2 = Vault::open(path, Box::new(store)).unwrap();
		assert_eq!(&v2.get("server:1:password").unwrap().unwrap()[..], b"hunter2-very-secret");
	}

	#[test]
	fn an_entry_cannot_be_moved_under_another_id() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("secrets.vault");
		let store = MemStore::default();
		let v = Vault::open(path.clone(), Box::new(store.clone())).unwrap();
		v.put("a", b"secret-a").unwrap();
		let mut file: VaultFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
		let sealed = file.entries.remove("a").unwrap();
		file.entries.insert("b".into(), sealed);
		std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
		let v = Vault::open(path, Box::new(store)).unwrap();
		assert!(v.get("b").is_err());
	}

	#[test]
	fn a_master_password_locks_the_vault_until_it_is_typed() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("secrets.vault");
		let store = MemStore::default();
		let v = Vault::open(path.clone(), Box::new(store.clone())).unwrap();
		v.put("k", b"private key").unwrap();
		v.set_master_password(Some("correct horse")).unwrap();
		assert!(store.0.lock().is_none(), "the keychain key is removed");

		let v = Vault::open(path, Box::new(store)).unwrap();
		assert!(v.status().locked);
		assert!(v.get("k").is_err());
		assert!(v.unlock("wrong password").is_err());
		v.unlock("correct horse").unwrap();
		assert_eq!(&v.get("k").unwrap().unwrap()[..], b"private key");
	}

	#[test]
	fn without_a_keychain_the_vault_says_so_and_a_master_password_still_works() {
		let dir = tempfile::tempdir().unwrap();
		let v = Vault::open(dir.path().join("secrets.vault"), Box::new(NoKeychain)).unwrap();
		assert!(v.status().keychain_error.is_some());
		assert!(v.put("k", b"x").is_err(), "nothing is stored without a key");
		v.set_master_password(Some("long enough")).unwrap();
		v.put("k", b"x").unwrap();
		assert_eq!(&v.get("k").unwrap().unwrap()[..], b"x");
	}
}
