//! The handle factorai advertises itself with, at `~/.claude/ide/<port>.lock`
//! (F20, ADR-0017 §§ 1–2).
//!
//! **This is the only thing we ever write under `~/.claude/`**, and ADR-0017
//! amends ADR-0004 for exactly this file and nothing else. It is not session
//! data, not a transcript, not configuration Claude Code reads for its own
//! behaviour: it is a handle we create, own and delete. The rule ADR-0004 was
//! really about — never write anything the user or Claude Code would mistake
//! for their own data — is untouched.
//!
//! **The port lives in the filename**, which is why the file cannot be anywhere
//! else and why the directory is not configurable: the CLI enumerates a fixed
//! set of `.claude/ide` directories and parses the port off each entry's name.
//! `CLAUDE_CODE_SSE_PORT` selects among what it finds; it adds no search path.
//!
//! Field names are not guesses. They were read out of the shipped CLI
//! (**2.1.235**), whose parser destructures exactly `workspaceFolders`, `pid`,
//! `ideName`, `transport` (compared against the string `"ws"`),
//! `runningInWindows` and `authToken`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

/// What we call ourselves. Honest rather than a masquerade, and ADR-0017 § 5
/// records why: the CLI's `ideKind` table drives process detection and terminal
/// sniffing, neither of which applies to us, so there is nothing to gain by
/// claiming to be VS Code and a working editor's worth of semantics to get
/// wrong if it believed us.
pub const IDE_NAME: &str = "factorai";

/// The CLI reads this as `transport === "ws"` and otherwise builds an
/// `http://…/sse` URL, which we do not serve. It is a constant because there is
/// no second correct value.
const TRANSPORT: &str = "ws";

/// The directory the CLI looks in, under Claude's config dir.
pub fn dir(claude_dir: &Path) -> PathBuf {
	claude_dir.join("ide")
}

/// Where this port's handle lives.
pub fn path_for(claude_dir: &Path, port: u16) -> PathBuf {
	dir(claude_dir).join(format!("{port}.lock"))
}

/// The JSON body. `camelCase` to match the CLI's parser, like every other
/// cross-boundary type in this codebase — all of them hand-mirrored rather
/// than generated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lockfile {
	/// The project root this session runs in. The CLI marks a lockfile valid
	/// when the session's cwd is inside one of these, which is a second,
	/// independent reason for it to pick us besides the pinned port — worth
	/// having, because the port comparison happens between a parsed filename
	/// and an environment variable and we do not control that coercion.
	pub workspace_folders: Vec<String>,
	/// Ours, not the session's. The CLI uses it to tell a live editor from a
	/// crashed one, and it is what `sweep` matches on.
	pub pid: u32,
	pub ide_name: String,
	pub transport: String,
	/// Whether this session is running in the native Windows desktop build.
	pub running_in_windows: bool,
	pub auth_token: String,
}

impl Lockfile {
	/// A handle for one session, with a freshly minted token.
	pub fn new(workspace_root: &str) -> Self {
		Self {
			workspace_folders: vec![workspace_root.to_string()],
			pid: std::process::id(),
			ide_name: IDE_NAME.to_string(),
			transport: TRANSPORT.to_string(),
			running_in_windows: cfg!(windows),
			auth_token: new_token(),
		}
	}

	/// Is this one of ours? `sweep` will not delete a file that isn't.
	pub fn is_ours(&self) -> bool {
		self.ide_name == IDE_NAME
	}
}

/// A per-session bearer token.
///
/// A v4 UUID: 122 bits from the OS CSPRNG through `getrandom`, which is the
/// same source a hand-rolled token would reach for. It is not the real boundary
/// — see [`super::scope`] — but it should still cost more than a guess.
fn new_token() -> String {
	Uuid::new_v4().simple().to_string()
}

/// Write the handle for `port`, replacing anything already there.
///
/// Mode `0600` **before** the content lands: created with the permission rather
/// than chmod'ed after, so the token is never briefly world-readable. Any other
/// local process running as this user can still read it, which is precisely why
/// the token is not what the design leans on.
pub fn write(claude_dir: &Path, port: u16, lock: &Lockfile) -> AppResult<()> {
	let dir = dir(claude_dir);
	fs::create_dir_all(&dir)
		.map_err(|e| AppError::Io(format!("creating {}: {e}", dir.display())))?;

	let path = path_for(claude_dir, port);
	let body =
		serde_json::to_vec(lock).map_err(|e| AppError::Io(format!("serialising lockfile: {e}")))?;

	write_private(&path, &body)
		.map_err(|e| AppError::Io(format!("writing {}: {e}", path.display())))?;
	debug!(%port, path = %path.display(), "wrote ide lockfile");
	Ok(())
}

#[cfg(unix)]
fn write_private(path: &Path, body: &[u8]) -> io::Result<()> {
	use std::io::Write;
	use std::os::unix::fs::OpenOptionsExt;

	let mut file =
		fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
	// `mode` only applies when the file is *created*, so an existing one keeps
	// whatever it had. Re-assert it rather than trusting the history of a file
	// we are about to put a token in.
	file.set_permissions(fs::Permissions::from_mode(0o600))?;
	file.write_all(body)?;
	file.sync_all()
}

#[cfg(windows)]
fn write_private(path: &Path, body: &[u8]) -> io::Result<()> {
	use std::io::Write;

	let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
	set_owner_only_dacl(path)?;
	file.write_all(body)?;
	file.sync_all()
}

/// Keep the bearer token readable only by the file owner, the Windows analogue
/// of the Unix `0600` lockfile. The file is empty until this protected DACL is
/// installed, so a newly created lock never exposes the token through inherited
/// directory permissions.
#[cfg(windows)]
fn set_owner_only_dacl(path: &Path) -> io::Result<()> {
	use std::os::windows::ffi::OsStrExt;
	use std::ptr::null_mut;
	use windows_sys::Win32::Foundation::{LocalFree, HLOCAL};
	use windows_sys::Win32::Security::Authorization::{
		ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW,
		SDDL_REVISION_1, SE_FILE_OBJECT,
	};
	use windows_sys::Win32::Security::{
		GetSecurityDescriptorDacl, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
	};

	let descriptor_text: Vec<u16> = "D:P(A;;FA;;;OW)\0".encode_utf16().collect();
	let mut descriptor = null_mut();
	let mut descriptor_len = 0;
	let converted = unsafe {
		ConvertStringSecurityDescriptorToSecurityDescriptorW(
			descriptor_text.as_ptr(),
			SDDL_REVISION_1,
			&mut descriptor,
			&mut descriptor_len,
		)
	};
	if converted == 0 {
		return Err(io::Error::last_os_error());
	}

	let mut dacl_present = 0;
	let mut dacl_defaulted = 0;
	let mut dacl = null_mut();
	let got_dacl = unsafe {
		GetSecurityDescriptorDacl(descriptor, &mut dacl_present, &mut dacl, &mut dacl_defaulted)
	};
	if got_dacl == 0 || dacl_present == 0 || dacl.is_null() {
		unsafe {
			LocalFree(descriptor as HLOCAL);
		}
		return Err(io::Error::last_os_error());
	}

	let mut wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
	let result = unsafe {
		SetNamedSecurityInfoW(
			wide_path.as_mut_ptr(),
			SE_FILE_OBJECT,
			DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
			null_mut(),
			null_mut(),
			dacl,
			null_mut(),
		)
	};
	unsafe {
		LocalFree(descriptor as HLOCAL);
	}
	if result == 0 {
		Ok(())
	} else {
		Err(io::Error::from_raw_os_error(result as i32))
	}
}

/// Delete the handle for `port`. Missing is success: the caller is a teardown
/// path and the goal state is "no file", not "a file was removed".
pub fn remove(claude_dir: &Path, port: u16) {
	let path = path_for(claude_dir, port);
	match fs::remove_file(&path) {
		Ok(()) => debug!(%port, "removed ide lockfile"),
		Err(e) if e.kind() == io::ErrorKind::NotFound => {}
		Err(e) => warn!(%port, error = %e, "could not remove ide lockfile"),
	}
}

/// Read and parse one lockfile. `None` for anything unreadable or malformed —
/// another editor's file we cannot understand is not our problem and must not
/// stop the sweep.
pub fn read(path: &Path) -> Option<Lockfile> {
	let body = fs::read(path).ok()?;
	serde_json::from_slice(&body).ok()
}

/// Delete our own handles whose process is gone.
///
/// Necessary because a `SIGKILL` leaves a file behind, which is ADR-0005's
/// orphan problem on a new surface — the difference being that a stale lockfile
/// is inert rather than dangerous, since the CLI TCP-probes the port before
/// trusting it. So this degrades rather than breaks, and is still worth doing:
/// the CLI auto-connects only when exactly one candidate matches, and our own
/// litter is the easiest way to stop being that one.
///
/// **Only ours, and only dead ones.** A file from a running VS Code, or one
/// belonging to a second factorai that is very much alive, is left alone.
/// `is_alive` is injected so the decision is testable without spawning
/// processes.
pub fn sweep(claude_dir: &Path, is_alive: impl Fn(u32) -> bool) -> usize {
	let dir = dir(claude_dir);
	let Ok(entries) = fs::read_dir(&dir) else { return 0 };

	let mut removed = 0;
	for entry in entries.flatten() {
		let path = entry.path();
		if path.extension().and_then(|e| e.to_str()) != Some("lock") {
			continue;
		}
		let Some(lock) = read(&path) else { continue };
		if !lock.is_ours() || is_alive(lock.pid) {
			continue;
		}
		if fs::remove_file(&path).is_ok() {
			removed += 1;
			debug!(path = %path.display(), pid = lock.pid, "swept stale ide lockfile");
		}
	}
	if removed > 0 {
		debug!(removed, "swept stale ide lockfiles");
	}
	removed
}

/// Is a pid live? Signal 0 checks for existence without delivering anything.
///
/// It cannot distinguish "gone" from "alive but owned by someone else", and
/// that asymmetry is the safe way round: a pid we cannot signal reads as alive,
/// so the sweep leaves the file rather than deleting a stranger's.
#[cfg(unix)]
pub fn pid_is_alive(pid: u32) -> bool {
	// SAFETY: `kill` with signal 0 performs error checking only — it delivers no
	// signal and touches no memory of ours. The cast is to the platform's own
	// pid type, which is what `std::process::id` widened from.
	unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Probe a process by opening a limited-information handle. Failure due to
/// access restrictions is treated as alive, so a sweep never removes a lock
/// that may belong to another user's still-running session.
#[cfg(windows)]
pub fn pid_is_alive(pid: u32) -> bool {
	use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER};
	use windows_sys::Win32::System::Threading::{
		GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
	};

	let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
	if process.is_null() {
		// ERROR_INVALID_PARAMETER means the process id does not exist. Other
		// errors (notably access denied) are deliberately kept as "alive".
		return unsafe { GetLastError() } != ERROR_INVALID_PARAMETER;
	}

	let mut exit_code = 0;
	let read_code = unsafe { GetExitCodeProcess(process, &mut exit_code) } != 0;
	unsafe {
		CloseHandle(process);
	}
	// If Windows let us open the process but could not report its state, keep
	// the lock. The sweep must only remove a handle when it knows the owner died.
	!read_code || exit_code == 259 // STILL_ACTIVE
}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::tempdir;

	fn claude_dir() -> tempfile::TempDir {
		tempdir().unwrap()
	}

	#[test]
	fn the_port_is_the_filename_because_that_is_where_the_cli_reads_it() {
		let dir = claude_dir();
		assert_eq!(path_for(dir.path(), 51234).file_name().unwrap(), "51234.lock");
	}

	#[test]
	fn the_json_keys_are_the_ones_the_cli_parses() {
		// Read out of CLI 2.1.235, which destructures exactly these. A rename
		// here is a silent failure to be detected as an editor at all.
		let lock = Lockfile::new("/home/u/proj");
		let json: serde_json::Value = serde_json::to_value(&lock).unwrap();

		for key in
			["workspaceFolders", "pid", "ideName", "transport", "runningInWindows", "authToken"]
		{
			assert!(json.get(key).is_some(), "missing key {key}");
		}
		assert_eq!(json["transport"], "ws");
		assert_eq!(json["ideName"], IDE_NAME);
		assert_eq!(json["runningInWindows"], cfg!(windows));
		assert_eq!(json["workspaceFolders"], serde_json::json!(["/home/u/proj"]));
	}

	#[test]
	fn every_session_gets_its_own_token() {
		assert_ne!(Lockfile::new("/p").auth_token, Lockfile::new("/p").auth_token);
	}

	#[test]
	#[cfg(unix)]
	fn writing_creates_the_directory_and_a_private_file() {
		let dir = claude_dir();
		let lock = Lockfile::new("/home/u/proj");
		write(dir.path(), 51234, &lock).unwrap();

		let path = path_for(dir.path(), 51234);
		let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
		assert_eq!(mode, 0o600, "the file holds a token; nobody else may read it");
		assert_eq!(read(&path).unwrap(), lock);
	}

	#[test]
	#[cfg(unix)]
	fn rewriting_re_asserts_the_mode_of_a_file_that_already_existed() {
		let dir = claude_dir();
		fs::create_dir_all(super::dir(dir.path())).unwrap();
		let path = path_for(dir.path(), 51234);
		fs::write(&path, b"{}").unwrap();
		fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

		write(dir.path(), 51234, &Lockfile::new("/p")).unwrap();

		let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
		assert_eq!(mode, 0o600, "an inherited mode is not a reason to leak a token");
	}

	#[test]
	fn removing_a_handle_that_is_not_there_is_success() {
		let dir = claude_dir();
		remove(dir.path(), 51234);
		write(dir.path(), 51234, &Lockfile::new("/p")).unwrap();
		remove(dir.path(), 51234);
		assert!(!path_for(dir.path(), 51234).exists());
	}

	#[test]
	fn sweep_takes_our_dead_handles_and_nothing_else() {
		let dir = claude_dir();

		let mut ours_dead = Lockfile::new("/p");
		ours_dead.pid = 111;
		write(dir.path(), 1111, &ours_dead).unwrap();

		let mut ours_alive = Lockfile::new("/p");
		ours_alive.pid = 222;
		write(dir.path(), 2222, &ours_alive).unwrap();

		let mut theirs_dead = Lockfile::new("/p");
		theirs_dead.pid = 333;
		theirs_dead.ide_name = "vscode".into();
		write(dir.path(), 3333, &theirs_dead).unwrap();

		let removed = sweep(dir.path(), |pid| pid == 222);

		assert_eq!(removed, 1);
		assert!(!path_for(dir.path(), 1111).exists(), "our crashed session's handle goes");
		assert!(path_for(dir.path(), 2222).exists(), "a live one stays");
		assert!(
			path_for(dir.path(), 3333).exists(),
			"someone else's editor is not ours to tidy up, dead or not"
		);
	}

	#[test]
	fn sweep_steps_over_junk_rather_than_giving_up() {
		let dir = claude_dir();
		fs::create_dir_all(super::dir(dir.path())).unwrap();
		fs::write(super::dir(dir.path()).join("garbage.lock"), b"not json").unwrap();
		fs::write(super::dir(dir.path()).join("notes.txt"), b"ignored").unwrap();

		let mut ours = Lockfile::new("/p");
		ours.pid = 111;
		write(dir.path(), 1111, &ours).unwrap();

		assert_eq!(sweep(dir.path(), |_| false), 1);
		assert!(super::dir(dir.path()).join("garbage.lock").exists());
		assert!(super::dir(dir.path()).join("notes.txt").exists());
	}

	#[test]
	fn sweep_of_a_directory_that_was_never_created_is_zero() {
		let dir = claude_dir();
		assert_eq!(sweep(dir.path(), |_| false), 0);
	}

	#[test]
	fn our_own_process_reads_as_alive() {
		assert!(pid_is_alive(std::process::id()));
		// A pid well outside the range assigned to ordinary local processes.
		assert!(!pid_is_alive(0x7FFF_FFFF));
	}
}
