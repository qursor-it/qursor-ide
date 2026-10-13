//! Finding `gh` and running it once (F31, ADR-0077).
//!
//! The model is `services::sops`: a one-shot child process whose stdout is the
//! answer. Three things differ. Every call is `async` with a hard timeout, since
//! these reach the network. At most [`MAX_CONCURRENT`] children run at once, so a
//! panel that opens a list, a detail and a page of blobs together cannot fork a
//! dozen `gh` processes. And the child is killed when its future is dropped, so a
//! navigation away from a slow request does not leave a process behind.
//!
//! Nothing here reads a token. `gh` finds its own credentials; the app only
//! ever sees what `gh` prints.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;

use crate::error::{AppError, AppResult};

/// Version probe and the login check: the panel's empty state waits on these.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(10);
/// A list, a detail or a page of changed files.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// One file's contents.
pub const BLOB_TIMEOUT: Duration = Duration::from_secs(30);

/// Concurrent `gh` children. Past this, a request waits for a permit.
const MAX_CONCURRENT: usize = 4;
static PERMITS: Semaphore = Semaphore::const_new(MAX_CONCURRENT);

/// How much of a failing `gh`'s stderr is kept. It is classified and shown in a
/// banner; the rest would only be memory.
const STDERR_CAP: u64 = 64 * 1024;

/// What a finished child left behind.
#[derive(Debug)]
pub struct Output {
	pub stdout: Vec<u8>,
	pub stderr: String,
	pub code: Option<i32>,
	/// Stdout hit the byte cap and the child was killed for it. The bytes are
	/// the first `cap + 1`, which is how a caller tells "exactly the cap" from
	/// "more than the cap".
	pub overflowed: bool,
}

impl Output {
	/// Success means exit 0, or a deliberate kill at the cap.
	pub fn succeeded(&self) -> bool {
		self.overflowed || self.code == Some(0)
	}
}

/// One invocation.
#[derive(Debug, Default)]
pub struct Call {
	pub args: Vec<String>,
	/// Piped to the child's stdin when present (a review body, from P3 on).
	/// Never an argument: argv is visible to every process on the machine.
	pub stdin: Option<Vec<u8>>,
	/// Stop reading and kill the child once stdout passes this many bytes.
	pub stdout_cap: Option<usize>,
	pub timeout: Option<Duration>,
}

/// A located `gh` and the version it reported.
#[derive(Debug, Clone)]
pub struct Binary {
	pub path: PathBuf,
	pub version: Option<String>,
}

/// A positive answer only. The probe spawns processes, and `gh` does not move
/// while the app runs; a negative answer is never kept, so installing `gh` and
/// pressing refresh works without a restart.
static BINARY: Mutex<Option<Binary>> = Mutex::new(None);

/// The `gh` to run, or `None` when there is none.
///
/// `refresh` re-probes even when a path is cached, so a `gh` that was upgraded
/// (or removed) while the app was open is noticed on the panel's refresh button.
pub fn binary(refresh: bool) -> Option<PathBuf> {
	if !refresh {
		if let Some(found) = BINARY.lock().as_ref() {
			return Some(found.path.clone());
		}
	}
	let path = locate()?;
	*BINARY.lock() = Some(Binary { path: path.clone(), version: None });
	Some(path)
}

/// The cached version, when it has been probed for the cached binary.
pub fn cached_version() -> Option<String> {
	BINARY.lock().as_ref().and_then(|b| b.version.clone())
}

pub fn remember_version(version: &str) {
	if let Some(found) = BINARY.lock().as_mut() {
		found.version = Some(version.to_string());
	}
}

fn locate() -> Option<PathBuf> {
	let on_path = crate::services::agent_cli::find_binary_on_path("gh").filter(|p| is_native(p));
	on_path.or_else(|| fallbacks().into_iter().find(|p| p.is_file() && is_native(p)))
}

/// A real executable, not a `.cmd` / `.bat` shim.
///
/// A batch wrapper goes through `cmd.exe` or PowerShell
/// (`agent_cli::launch_command`), and neither forwards the child's stdin the way
/// `CreateProcess` does — which the review submission in a later phase needs. It
/// would also put `gh`'s arguments through a shell. `gh.exe` is what the MSI,
/// winget and scoop all install, so refusing the shim costs nothing in practice.
pub(super) fn is_native(path: &Path) -> bool {
	!path.extension().is_some_and(|ext| {
		ext.to_string_lossy().eq_ignore_ascii_case("cmd")
			|| ext.to_string_lossy().eq_ignore_ascii_case("bat")
	})
}

/// Where `gh` lives when it is installed but its folder is not on the PATH the
/// app resolved — a GUI launch on macOS, or a winget install made after the
/// session that started the app.
fn fallbacks() -> Vec<PathBuf> {
	let mut found: Vec<PathBuf> = Vec::new();
	#[cfg(windows)]
	{
		let env_path = |var: &str, rest: &[&str]| {
			std::env::var_os(var)
				.map(|base| rest.iter().fold(PathBuf::from(base), |p, s| p.join(s)))
		};
		found.extend(env_path("ProgramFiles", &["GitHub CLI", "gh.exe"]));
		found.extend(env_path("LOCALAPPDATA", &["Microsoft", "WinGet", "Links", "gh.exe"]));
		found.extend(env_path("USERPROFILE", &["scoop", "shims", "gh.exe"]));
	}
	#[cfg(not(windows))]
	{
		for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
			found.push(Path::new(dir).join("gh"));
		}
	}
	found
}

/// The environment every `gh` child gets, beyond the shared child env.
///
/// Prompts, spinners and update notices are what `gh` does for a human at a
/// terminal; here they would be a hang (a prompt nobody can answer) or a network
/// call the user did not ask for (the update check).
pub const GH_ENV: &[(&str, &str)] = &[
	("GH_PROMPT_DISABLED", "1"),
	("GH_NO_UPDATE_NOTIFIER", "1"),
	("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1"),
	("GH_SPINNER_DISABLED", "1"),
	("NO_COLOR", "1"),
];

fn command(binary: &Path, args: &[String]) -> tokio::process::Command {
	let mut cmd = std::process::Command::new(binary);
	cmd.args(args.iter().map(OsString::from));
	// The same environment every other child gets: under an AppImage ours
	// carries a `LD_LIBRARY_PATH` into a squashfs mount.
	crate::services::child_env::changes_for_current_env().apply_to_command(&mut cmd);
	for (key, value) in GH_ENV {
		cmd.env(key, value);
	}
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
	}
	let mut cmd = tokio::process::Command::from(cmd);
	// The future being dropped (a timeout, or the caller going away) kills the
	// child. Kill-on-quit has the same reach: no orphan `gh` outlives the app.
	cmd.kill_on_drop(true);
	cmd
}

/// Run `gh` with `call` and collect what it printed.
///
/// An `Err` is only ever "could not run it at all" or "ran out of time"; `gh`
/// exiting non-zero is an `Ok(Output)` whose `code` and `stderr` the caller
/// hands to [`super::parse::classify`].
pub async fn run(binary: &Path, call: Call) -> AppResult<Output> {
	let timeout = call.timeout.unwrap_or(READ_TIMEOUT);
	// Waiting for a permit is not the child's time to spend.
	let _permit = PERMITS
		.acquire()
		.await
		.map_err(|_| AppError::Process("the gh process limiter was closed".into()))?;
	match tokio::time::timeout(timeout, run_inner(binary, call)).await {
		Ok(result) => result,
		Err(_) => {
			Err(AppError::Remote(format!("gh did not answer within {} seconds", timeout.as_secs())))
		}
	}
}

async fn run_inner(binary: &Path, call: Call) -> AppResult<Output> {
	let mut cmd = command(binary, &call.args);
	cmd.stdin(if call.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());
	let mut child = cmd
		.spawn()
		.map_err(|e| AppError::Process(format!("could not run {}: {e}", binary.display())))?;

	// Stdin goes in from its own task: a child that fills its stdout pipe before
	// reading all of stdin would otherwise deadlock against us.
	if let (Some(bytes), Some(mut stdin)) = (call.stdin, child.stdin.take()) {
		tokio::spawn(async move {
			let _ = stdin.write_all(&bytes).await;
			let _ = stdin.shutdown().await;
		});
	}

	let mut stderr_pipe = child.stderr.take();
	let stderr_task = tokio::spawn(async move {
		let mut buf = Vec::new();
		if let Some(pipe) = stderr_pipe.as_mut() {
			let _ = (&mut *pipe).take(STDERR_CAP).read_to_end(&mut buf).await;
			// Keep draining so a chatty child is never blocked on a full pipe.
			let _ = tokio::io::copy(pipe, &mut tokio::io::sink()).await;
		}
		buf
	});

	let mut stdout = Vec::new();
	let mut overflowed = false;
	if let Some(mut pipe) = child.stdout.take() {
		match call.stdout_cap {
			Some(cap) => {
				pipe.take(cap as u64 + 1)
					.read_to_end(&mut stdout)
					.await
					.map_err(|e| AppError::Process(format!("could not read gh's output: {e}")))?;
				overflowed = stdout.len() > cap;
			}
			None => {
				pipe.read_to_end(&mut stdout)
					.await
					.map_err(|e| AppError::Process(format!("could not read gh's output: {e}")))?;
			}
		}
	}
	if overflowed {
		let _ = child.kill().await;
	}
	let status =
		child.wait().await.map_err(|e| AppError::Process(format!("gh did not finish: {e}")))?;
	let stderr = stderr_task.await.unwrap_or_default();
	Ok(Output {
		stdout,
		stderr: String::from_utf8_lossy(&stderr).into_owned(),
		code: status.code(),
		overflowed,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn batch_wrappers_are_not_native() {
		assert!(!is_native(Path::new(r"C:\tools\gh.cmd")));
		assert!(!is_native(Path::new(r"C:\tools\GH.BAT")));
		assert!(is_native(Path::new(r"C:\Program Files\GitHub CLI\gh.exe")));
		assert!(is_native(Path::new("/usr/bin/gh")));
	}

	#[test]
	fn the_child_env_turns_off_everything_interactive() {
		let keys: Vec<&str> = GH_ENV.iter().map(|(k, _)| *k).collect();
		for needed in [
			"GH_PROMPT_DISABLED",
			"GH_NO_UPDATE_NOTIFIER",
			"GH_NO_EXTENSION_UPDATE_NOTIFIER",
			"GH_SPINNER_DISABLED",
			"NO_COLOR",
		] {
			assert!(keys.contains(&needed), "{needed}");
		}
	}

	/// An unknown binary is a `Process` error rather than a panic.
	#[tokio::test]
	async fn a_missing_binary_is_a_process_error() {
		let err = run(Path::new("definitely-not-a-real-gh"), Call::default()).await.unwrap_err();
		assert!(matches!(err, AppError::Process(_)), "{err:?}");
	}
}
