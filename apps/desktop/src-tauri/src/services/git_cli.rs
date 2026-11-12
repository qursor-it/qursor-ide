//! The one way the app runs the system `git` (ADR-0079, ADR-0089).
//!
//! Shared by the PR worktree checkout and Git mode's writes. Argv only, never a
//! shell; `GIT_TERMINAL_PROMPT=0` keeps a missing credential an error instead of
//! a hang on a prompt nobody can answer, while credential helpers keep working.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::services::gh::run::is_native;

/// Git's output is for a banner; the rest would only be memory.
const OUTPUT_CAP: usize = 64 * 1024;

static GIT: Mutex<Option<PathBuf>> = Mutex::new(None);

pub struct GitOutput {
	pub ok: bool,
	pub stdout: String,
	pub stderr: String,
}

fn not_installed() -> AppError {
	AppError::NotFound("git not found on PATH".into())
}

/// A positive answer is kept; a negative one never is, so installing git needs no
/// restart.
pub async fn git_binary() -> AppResult<PathBuf> {
	if let Some(found) = GIT.lock().clone() {
		return Ok(found);
	}
	let found = tokio::task::spawn_blocking(locate)
		.await
		.map_err(|e| AppError::Process(format!("background task failed: {e}")))?
		.ok_or_else(not_installed)?;
	*GIT.lock() = Some(found.clone());
	Ok(found)
}

fn locate() -> Option<PathBuf> {
	// A `git.cmd` shim would put the arguments through a shell (see `run::is_native`).
	let on_path = crate::services::agent_cli::find_binary_on_path("git").filter(|p| is_native(p));
	on_path.or_else(|| fallbacks().into_iter().find(|p| p.is_file()))
}

fn fallbacks() -> Vec<PathBuf> {
	let mut found: Vec<PathBuf> = Vec::new();
	#[cfg(windows)]
	{
		for var in ["ProgramFiles", "ProgramFiles(x86)"] {
			if let Some(base) = std::env::var_os(var) {
				found.push(PathBuf::from(base).join("Git").join("cmd").join("git.exe"));
			}
		}
	}
	#[cfg(not(windows))]
	{
		for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
			found.push(Path::new(dir).join("git"));
		}
	}
	found
}

pub fn command(git: &Path, args: &[String]) -> tokio::process::Command {
	let mut cmd = std::process::Command::new(git);
	cmd.args(args.iter().map(OsString::from));
	crate::services::child_env::changes_for_current_env().apply_to_command(&mut cmd);
	// Never block on a username prompt; credential helpers still answer.
	cmd.env("GIT_TERMINAL_PROMPT", "0");
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
	}
	let mut cmd = tokio::process::Command::from(cmd);
	// A timeout drops the future, which kills the child: no orphan git.
	cmd.kill_on_drop(true);
	cmd
}

pub async fn run_git(git: &Path, args: Vec<String>, timeout: Duration) -> AppResult<GitOutput> {
	run_git_stdin(git, args, None, timeout).await
}

/// `stdin` is written then closed, so `commit -F -` and `apply -` see end of input.
pub async fn run_git_stdin(
	git: &Path,
	args: Vec<String>,
	stdin: Option<String>,
	timeout: Duration,
) -> AppResult<GitOutput> {
	let mut cmd = command(git, &args);
	cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());
	let mut child = cmd
		.spawn()
		.map_err(|e| AppError::Process(format!("could not run {}: {e}", git.display())))?;
	if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
		// Concurrent with the read below, so a patch bigger than the pipe cannot deadlock.
		// A write error means git exited early; its exit status says why.
		tokio::spawn(async move {
			let _ = pipe.write_all(text.as_bytes()).await;
		});
	}
	match tokio::time::timeout(timeout, child.wait_with_output()).await {
		Ok(Ok(out)) => Ok(GitOutput {
			ok: out.status.success(),
			stdout: tail(&out.stdout),
			stderr: tail(&out.stderr),
		}),
		Ok(Err(e)) => Err(AppError::Process(format!("git did not finish: {e}"))),
		Err(_) => Err(AppError::Remote(format!(
			"git did not finish within {} seconds",
			timeout.as_secs()
		))),
	}
}

fn tail(bytes: &[u8]) -> String {
	let start = bytes.len().saturating_sub(OUTPUT_CAP);
	String::from_utf8_lossy(&bytes[start..]).into_owned()
}
