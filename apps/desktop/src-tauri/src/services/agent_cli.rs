//! Locating an agent's CLI binary on disk — `claude`, `codex` (F30).
//!
//! Unix uses three-tier discovery — see specs/annex-A-cli-agent-patterns.md
//! § A.1 for the rationale. Windows uses native PATH/PATHEXT resolution and
//! common install locations. Both are parameterised on the
//! [`AgentDescriptor`]'s `binary_name`.
//!
//! Unix order of attempts:
//!   0. The user's override, when the caller passes one (F11).
//!   1. `which <name>` in the inherited process PATH.
//!   2. `$SHELL -lc 'command -v <name>'` (then /bin/zsh, /bin/bash) — handles
//!      macOS GUI launches that don't inherit a terminal PATH.
//!   3. Probe a list of common install locations.
//!
//! **The override arrives as a parameter, not as a database read.** This module
//! stays a pure function of its input — the caller resolves the setting and
//! hands a path — so it keeps no `Db` dependency and its tests keep working
//! without one. What matters is that *every* caller passes it: `check_cli`
//! reaching the finder on its own is how the settings page would come to report
//! "not installed" for the binary sessions are actually spawning from.
//!
//! Native Windows resolution uses the user PATH and PATHEXT, then common npm and Claude install locations.

#[cfg(windows)]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::io::Read;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[cfg(windows)]
use base64::Engine;
use serde::Serialize;
use tracing::{debug, warn};

use crate::agents::{self, AgentDescriptor};
use crate::error::{AppError, AppResult};

/// What the Agents section shows for one agent (F11, F30). The name still says
/// Claude because `@factorai/types` mirrors it under that name; it describes
/// any agent's binary.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCliStatus {
	pub installed: bool,
	pub binary_path: Option<String>,
	pub version: Option<String>,
}

/// Locate an agent's binary. Returns the first hit from any tier.
///
/// `override_path` is the user's setting (F11). When present it is the answer —
/// **no fallback to the probe**, because a typo that silently resolved to
/// whatever the tiers found would show a working version beside a path that
/// does not work, which is the opposite of validating before you depend on it.
pub fn find_agent_binary(
	agent: &AgentDescriptor,
	override_path: Option<&Path>,
) -> AppResult<PathBuf> {
	let name = agent.binary_name;
	if let Some(p) = override_path {
		#[cfg(windows)]
		if let Some(resolved) = windows_explicit_path(p) {
			debug!(path = %resolved.display(), name, "using the configured binary");
			return Ok(resolved);
		}
		#[cfg(not(windows))]
		if p.is_file() {
			debug!(?p, name, "using the configured binary");
			return Ok(p.to_path_buf());
		}
		return Err(AppError::NotFound(format!("no {name} binary at {}", p.display())));
	}
	if let Some(p) = find_on_path(name) {
		debug!(?p, name, "found via PATH");
		return Ok(p);
	}
	if let Some(p) = find_in_user_shell(name) {
		debug!(?p, name, "found via login shell");
		return Ok(p);
	}
	if let Some(p) = probe_known_candidates(name) {
		debug!(?p, name, "found via candidate probe");
		return Ok(p);
	}
	Err(AppError::NotFound(format!("{name} CLI not found")))
}

/// `find_agent_binary` for Claude — the callers that predate F30.
pub fn find_claude_binary(override_path: Option<&Path>) -> AppResult<PathBuf> {
	find_agent_binary(claude(), override_path)
}

fn claude() -> &'static AgentDescriptor {
	agents::descriptor(agents::CLAUDE).expect("claude is in the registry")
}

/// Check whether an agent is installed. Doesn't error — returns a status the
/// frontend can use to drive the Agents section's read-only row.
///
/// `override_path` is passed straight through to `find_agent_binary`, so what
/// this reports and what a session spawns are the same binary.
///
/// **`installed` means the binary resolved, not that `--version` answered.** A
/// resolved path with `version: None` is a real state — a wrapper script, a
/// broken install, a `--version` that hangs — and it is the caller's to
/// present. Folding it into `installed: false` would let a version probe veto a
/// binary that spawns sessions perfectly well.
pub fn check_agent_cli(agent: &AgentDescriptor, override_path: Option<&Path>) -> ClaudeCliStatus {
	match find_agent_binary(agent, override_path) {
		Ok(p) => {
			let version = version_for(&p);
			ClaudeCliStatus {
				installed: true,
				binary_path: Some(p.to_string_lossy().to_string()),
				version,
			}
		}
		Err(_) => ClaudeCliStatus { installed: false, binary_path: None, version: None },
	}
}

/// `check_agent_cli` for Claude — the callers that predate F30.
#[cfg(test)]
pub fn check_cli(override_path: Option<&Path>) -> ClaudeCliStatus {
	check_agent_cli(claude(), override_path)
}

pub fn find_binary_on_path(name: &str) -> Option<PathBuf> {
	#[cfg(windows)]
	{
		find_windows_executable(name, crate::services::shell_path::child_path())
	}
	#[cfg(not(windows))]
	{
		run_lookup("which", &[name])
	}
}

fn find_on_path(name: &str) -> Option<PathBuf> {
	find_binary_on_path(name)
}

#[cfg(not(windows))]
fn find_in_user_shell(name: &str) -> Option<PathBuf> {
	for shell in user_shell_candidates() {
		if !shell.exists() {
			continue;
		}
		if let Some(p) = ask_shell(&shell, name) {
			return Some(p);
		}
	}
	None
}

#[cfg(windows)]
fn find_in_user_shell(_name: &str) -> Option<PathBuf> {
	// The Windows shell-path probe already includes the user's PowerShell
	// profile additions. Re-running a shell lookup here would resolve against a
	// second, potentially different PATH.
	None
}

#[cfg(not(windows))]
fn user_shell_candidates() -> Vec<PathBuf> {
	let mut shells = Vec::new();
	if let Some(s) = std::env::var_os("SHELL") {
		if !s.is_empty() {
			shells.push(PathBuf::from(s));
		}
	}
	shells.push(PathBuf::from("/bin/zsh"));
	shells.push(PathBuf::from("/bin/bash"));
	shells
}

#[cfg(not(windows))]
fn ask_shell(shell: &Path, name: &str) -> Option<PathBuf> {
	let output = Command::new(shell).arg("-lc").arg(format!("command -v {name}")).output().ok()?;
	if !output.status.success() {
		return None;
	}
	let stdout = String::from_utf8_lossy(&output.stdout);
	first_existing(&stdout)
}

#[cfg(not(windows))]
fn run_lookup(cmd: &str, args: &[&str]) -> Option<PathBuf> {
	let output = Command::new(cmd).args(args).output().ok()?;
	if !output.status.success() {
		return None;
	}
	let stdout = String::from_utf8_lossy(&output.stdout);
	first_existing(&stdout)
}

#[cfg(not(windows))]
fn first_existing(stdout: &str) -> Option<PathBuf> {
	for line in stdout.lines() {
		let trimmed = line.trim();
		if trimmed.is_empty() {
			continue;
		}
		let p = PathBuf::from(trimmed);
		if p.exists() {
			return Some(p);
		}
	}
	None
}

fn probe_known_candidates(name: &str) -> Option<PathBuf> {
	candidate_paths(name).into_iter().find(|p| p.is_file())
}

/// Where an install lands when it is not on `PATH`. The same list for every
/// agent, plus the one place Claude's own installer uses; `~/.claude/local/`
/// holds only `claude`, and a `codex` there would be someone's mistake.
#[cfg(not(windows))]
fn candidate_paths(name: &str) -> Vec<PathBuf> {
	let mut out = Vec::new();
	if let Some(home) = dirs::home_dir() {
		out.extend([
			home.join(".local/bin").join(name),
			home.join(".claude/local").join(name),
			home.join(".local/share/mise/shims").join(name),
			home.join(".asdf/shims").join(name),
			home.join(".npm-global/bin").join(name),
			home.join(".npm/bin").join(name),
			home.join(".local/share/pnpm").join(name),
			home.join(".linuxbrew/bin").join(name),
		]);
		// nvm-managed installs: glob ~/.nvm/versions/node/*/bin/<name>
		if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
			let mut nvm: Vec<PathBuf> = entries
				.filter_map(Result::ok)
				.map(|e| e.path().join("bin").join(name))
				.filter(|p| p.exists())
				.collect();
			// Sort so the highest version (lexicographic) wins.
			nvm.sort();
			nvm.reverse();
			out.extend(nvm);
		}
	}
	out.extend([
		PathBuf::from("/opt/homebrew/bin").join(name),
		PathBuf::from("/usr/local/bin").join(name),
		PathBuf::from("/home/linuxbrew/.linuxbrew/bin").join(name),
	]);
	out
}

#[cfg(windows)]
fn candidate_paths(name: &str) -> Vec<PathBuf> {
	let mut bases = Vec::new();
	if let Some(home) = dirs::home_dir() {
		bases.extend([home.join(".claude/local").join(name), home.join(".local/bin").join(name)]);
	}
	if let Some(app_data) = std::env::var_os("APPDATA") {
		bases.push(PathBuf::from(app_data).join("npm").join(name));
	}
	let path = crate::services::shell_path::child_path();
	let extensions = windows_pathext();
	for dir in std::env::split_paths(path) {
		bases.push(dir.join(name));
	}
	bases.into_iter().flat_map(|base| windows_variants(&base, &extensions)).collect()
}

#[cfg(windows)]
fn windows_pathext() -> Vec<OsString> {
	let configured =
		std::env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
	let extensions: Vec<OsString> = configured
		.to_string_lossy()
		.split(';')
		.map(str::trim)
		.filter(|extension| extension.starts_with('.') && extension.len() > 1)
		.map(OsString::from)
		.collect();
	if extensions.is_empty() {
		[".COM", ".EXE", ".BAT", ".CMD"].into_iter().map(OsString::from).collect()
	} else {
		extensions
	}
}

#[cfg(windows)]
fn windows_variants(base: &Path, extensions: &[OsString]) -> Vec<PathBuf> {
	if base.extension().is_some() {
		return vec![base.to_path_buf()];
	}
	let mut variants = Vec::with_capacity(extensions.len());
	for extension in extensions {
		let mut path = base.as_os_str().to_os_string();
		path.push(extension);
		variants.push(PathBuf::from(path));
	}
	variants
}

#[cfg(windows)]
fn find_windows_executable(name: &str, path: &OsStr) -> Option<PathBuf> {
	find_windows_executable_with_extensions(name, path, &windows_pathext())
}

#[cfg(windows)]
fn find_windows_executable_with_extensions(
	name: &str,
	path: &OsStr,
	extensions: &[OsString],
) -> Option<PathBuf> {
	let requested = Path::new(name);
	if requested.components().count() > 1 || requested.is_absolute() {
		return windows_variants(requested, extensions)
			.into_iter()
			.find(|candidate| candidate.is_file());
	}
	std::env::split_paths(path)
		.flat_map(|dir| windows_variants(&dir.join(requested), extensions))
		.find(|candidate| candidate.is_file())
}

#[cfg(windows)]
fn windows_explicit_path(path: &Path) -> Option<PathBuf> {
	if path.components().count() == 1 {
		return find_windows_executable(
			&path.to_string_lossy(),
			crate::services::shell_path::child_path(),
		);
	}
	windows_variants(path, &windows_pathext()).into_iter().find(|candidate| candidate.is_file())
}

/// Prepare a native process command. Batch wrappers cannot be passed directly
/// to `CreateProcessW`; on Windows they run through PowerShell with the complete
/// argv encoded as data, so prompts containing shell metacharacters remain one
/// argument. Other executables keep the direct argv path on every platform.
pub fn launch_command(binary: &Path, args: &[OsString]) -> AppResult<(OsString, Vec<OsString>)> {
	#[cfg(windows)]
	if is_batch_wrapper(binary) {
		let payload = serde_json::json!({
			"binary": binary.to_string_lossy(),
			"arguments": args.iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>(),
		});
		let payload = serde_json::to_vec(&payload).map_err(|error| {
			AppError::Process(format!("could not encode Windows CLI arguments: {error}"))
		})?;
		let encoded_payload = base64::engine::general_purpose::STANDARD.encode(payload);
		let script = format!(
			"$ErrorActionPreference = 'Stop'\n$payload = [System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String('{encoded_payload}')) | ConvertFrom-Json\n$binary = [string]$payload.binary\n$percentVariable = 'FACTORAI_LITERAL_PERCENT_' + [guid]::NewGuid().ToString('N')\n[Environment]::SetEnvironmentVariable($percentVariable, '%', 'Process')\n$percentPlaceholder = '%' + $percentVariable + '%'\n$arguments = @($payload.arguments | ForEach-Object {{ $_.Replace('%', $percentPlaceholder) }})\ntry {{ & $binary @arguments; exit $LASTEXITCODE }} catch {{ [Console]::Error.WriteLine($_); exit 1 }}"
		);
		let powershell = std::env::var_os("SystemRoot")
			.map(PathBuf::from)
			.map(|root| root.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
			.filter(|path| path.is_file())
			.unwrap_or_else(|| PathBuf::from("powershell.exe"));
		return Ok((
			powershell.into_os_string(),
			[
				OsString::from("-NoLogo"),
				OsString::from("-NoProfile"),
				OsString::from("-NonInteractive"),
				OsString::from("-Command"),
				OsString::from(script),
			]
			.into(),
		));
	}
	Ok((binary.as_os_str().to_os_string(), args.to_vec()))
}

#[cfg(windows)]
fn is_batch_wrapper(binary: &Path) -> bool {
	binary.extension().is_some_and(|extension| {
		let extension = extension.to_string_lossy();
		extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
	})
}

const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// Best-effort version lookup. Runs `<bin> --version` with a real 2-second
/// timeout, not `Command::output()`'s unbounded wait — a wrapper script or a
/// first-run self-check can hang the child on stdio it never had, and this is
/// reachable synchronously from a Tauri command, so it must not depend on the
/// binary behaving. Returns the first whitespace-separated token that looks
/// like a semver (e.g. "0.2.34" out of "claude 0.2.34 (build abc)", "0.155.1"
/// out of "codex-cli 0.155.1").
fn version_for(bin: &Path) -> Option<String> {
	let (program, args) = launch_command(bin, &[OsString::from("--version")]).ok()?;
	let mut command = Command::new(program);
	command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
	#[cfg(windows)]
	command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
	let mut child = command
		.spawn()
		.map_err(|e| warn!(error = %e, bin = %bin.display(), "--version failed"))
		.ok()?;
	let deadline = Instant::now() + VERSION_TIMEOUT;

	// Reading on this thread would be the hang the timeout exists to prevent —
	// see `shell_path::path_from_shell`, the same pattern.
	let mut stdout = child.stdout.take()?;
	let (tx, rx) = mpsc::channel();
	std::thread::spawn(move || {
		let mut buf = Vec::new();
		let _ = stdout.read_to_end(&mut buf);
		let _ = tx.send(buf);
	});

	let out = match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
		Ok(out) => out,
		Err(_) => {
			warn!(bin = %bin.display(), "--version did not answer in time");
			let _ = child.kill();
			let _ = child.wait();
			return None;
		}
	};
	// F30's version probe accepts a clean CLI exit: stdout may close before
	// shutdown finishes, so killing it here would discard valid output.
	loop {
		match child.try_wait() {
			Ok(Some(status)) if status.success() => break,
			Ok(Some(_)) | Err(_) => return None,
			Ok(None) if Instant::now() < deadline => {
				std::thread::sleep(Duration::from_millis(10));
			}
			Ok(None) => {
				warn!(bin = %bin.display(), "--version did not exit in time");
				let _ = child.kill();
				let _ = child.wait();
				return None;
			}
		}
	}

	let s = String::from_utf8_lossy(&out);
	for tok in s.split_whitespace() {
		if is_version_like(tok) {
			return Some(tok.to_string());
		}
	}
	None
}

fn is_version_like(s: &str) -> bool {
	let s = s.trim_start_matches('v');
	let mut parts = s.split('.');
	let a = parts.next();
	let b = parts.next();
	let c = parts.next();
	matches!((a, b, c), (Some(a), Some(b), Some(c)) if
		a.chars().all(|ch| ch.is_ascii_digit()) &&
		b.chars().all(|ch| ch.is_ascii_digit()) &&
		c.chars().all(|ch| ch.is_ascii_digit() || ch == '-' || ch == '+' || ch.is_ascii_alphabetic())
	)
}

#[cfg(all(test, unix))]
mod tests {
	use std::os::unix::fs::PermissionsExt;

	use super::*;

	fn fake_claude(dir: &Path, body: &str) -> PathBuf {
		let p = dir.join("claude");
		std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
		std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
		p
	}

	#[test]
	fn version_for_times_out_instead_of_hanging_forever() {
		// A `--version` that never returns — a wrapper script, a stalled
		// first-run self-check — must not block the caller past the timeout.
		let tmp = tempfile::TempDir::new().unwrap();
		let bin = fake_claude(tmp.path(), "sleep 30");
		let started = std::time::Instant::now();
		assert_eq!(version_for(&bin), None);
		assert!(started.elapsed() < Duration::from_secs(10));
	}

	#[test]
	fn version_for_reads_a_well_behaved_binary() {
		let tmp = tempfile::TempDir::new().unwrap();
		let bin = fake_claude(tmp.path(), "printf 'claude 1.2.3 (build abc)\\n'");
		assert_eq!(version_for(&bin), Some("1.2.3".to_string()));
	}

	#[test]
	fn version_for_waits_for_clean_exit_after_stdout_closes() {
		let tmp = tempfile::TempDir::new().unwrap();
		let bin = fake_claude(tmp.path(), "printf 'codex-cli 0.155.1\\n'; exec 1>&-; sleep 0.1");
		assert_eq!(version_for(&bin), Some("0.155.1".to_string()));
	}

	#[test]
	fn version_like_accepts_semver() {
		assert!(is_version_like("0.2.34"));
		assert!(is_version_like("v1.0.0"));
		assert!(is_version_like("1.2.3-alpha"));
	}

	#[test]
	fn version_like_rejects_non_versions() {
		assert!(!is_version_like("claude"));
		assert!(!is_version_like("1.2"));
		assert!(!is_version_like(""));
		assert!(!is_version_like("abc.def.ghi"));
	}

	#[test]
	fn candidate_paths_includes_known_locations() {
		let paths = candidate_paths("claude");
		// At minimum the absolute paths show up regardless of HOME.
		assert!(paths.iter().any(|p| p == &PathBuf::from("/opt/homebrew/bin/claude")));
		assert!(paths.iter().any(|p| p == &PathBuf::from("/usr/local/bin/claude")));
	}

	#[test]
	fn candidate_paths_are_named_after_the_agent() {
		let paths = candidate_paths("codex");
		assert!(paths.iter().any(|p| p == &PathBuf::from("/usr/local/bin/codex")));
		assert!(paths.iter().all(|p| p.file_name().unwrap() == "codex"));
	}

	#[test]
	fn a_codex_version_line_parses() {
		let tmp = tempfile::TempDir::new().unwrap();
		let bin = fake_claude(tmp.path(), "printf 'codex-cli 0.155.1\\n'");
		assert_eq!(version_for(&bin), Some("0.155.1".to_string()));
	}

	#[test]
	fn first_existing_picks_first_real_path() {
		let tmp = tempfile::TempDir::new().unwrap();
		let real = tmp.path().join("real");
		std::fs::write(&real, "").unwrap();
		let stdout = format!("/no/such/path\n{}\n/also/missing\n", real.display());
		assert_eq!(first_existing(&stdout), Some(real));
	}

	#[test]
	fn first_existing_returns_none_when_all_missing() {
		assert_eq!(first_existing("/a\n/b\n"), None);
	}

	#[test]
	fn override_wins_over_the_probe() {
		let tmp = tempfile::TempDir::new().unwrap();
		let fake = tmp.path().join("claude");
		std::fs::write(&fake, "").unwrap();
		// Whatever the three tiers would have found on this machine, the
		// configured path is the answer — that is what makes the settings page
		// and the spawn path agree about one binary (F11).
		assert_eq!(find_claude_binary(Some(&fake)).unwrap(), fake);
	}

	#[test]
	fn a_missing_override_does_not_fall_back_to_the_probe() {
		let tmp = tempfile::TempDir::new().unwrap();
		let nowhere = tmp.path().join("no-such-claude");
		// The alternative — probing anyway — would report a working version
		// beside a path that does not work, which is the one thing validating
		// before you depend on it exists to prevent.
		assert!(find_claude_binary(Some(&nowhere)).is_err());
		assert!(!check_cli(Some(&nowhere)).installed);
	}

	#[test]
	fn a_directory_is_not_a_binary() {
		let tmp = tempfile::TempDir::new().unwrap();
		// `exists()` would accept this; a path you cannot exec is not an install.
		assert!(find_claude_binary(Some(tmp.path())).is_err());
	}

	#[test]
	fn no_override_still_probes() {
		// Whether `claude` is installed on the machine running the tests is not
		// this test's business — that the absent override reaches the tiers
		// rather than short-circuiting is. Either outcome proves it ran them;
		// what it must not do is fail the way a missing override does.
		let status = check_cli(None);
		assert_eq!(status.installed, status.binary_path.is_some());
	}
}

#[cfg(all(test, windows))]
mod windows_tests {
	use super::*;
	use std::io::Write;

	#[test]
	fn path_resolution_honors_pathext_order_for_cmd_and_exe_files() {
		let dir = tempfile::TempDir::new().unwrap();
		let cmd = dir.path().join("claude.CMD");
		let exe = dir.path().join("claude.EXE");
		std::fs::write(&cmd, "@echo off\r\n").unwrap();
		std::fs::write(&exe, "fixture").unwrap();
		let extensions = [OsString::from(".CMD"), OsString::from(".EXE"), OsString::from(".BAT")];
		assert_eq!(
			find_windows_executable_with_extensions(
				"claude",
				&OsString::from(dir.path()),
				&extensions,
			),
			Some(cmd)
		);
	}

	#[test]
	fn batch_launch_arguments_are_encoded_as_data() {
		let dir = tempfile::TempDir::new().unwrap();
		let batch = dir.path().join("fake cli.cmd");
		let args = [
			OsString::from("prompt %PATH% with spaces & pipes | redirects < >"),
			OsString::from("100% `value` !PATH!"),
			OsString::from("quote \" and apostrophe '"),
			OsString::from("Zoë 東京"),
			OsString::from("first prompt line\nsecond prompt line"),
		];
		let (program, launched) = launch_command(&batch, &args).unwrap();
		assert!(program.to_string_lossy().to_ascii_lowercase().ends_with("powershell.exe"));
		assert_eq!(launched[0], "-NoLogo");
		assert_eq!(launched[1], "-NoProfile");
		assert_eq!(launched[2], "-NonInteractive");
		assert_eq!(launched[3], "-Command");
		let script = launched[4].to_string_lossy();
		let payload_start = script.find("FromBase64String('").unwrap() + "FromBase64String('".len();
		let payload_end = script[payload_start..].find("')").unwrap() + payload_start;
		let payload = base64::engine::general_purpose::STANDARD
			.decode(&script[payload_start..payload_end])
			.unwrap();
		let payload: serde_json::Value = serde_json::from_slice(&payload).unwrap();
		assert_eq!(payload["binary"], batch.to_string_lossy().as_ref());
		assert_eq!(
			payload["arguments"],
			serde_json::json!(args
				.iter()
				.map(|a| a.to_string_lossy().to_string())
				.collect::<Vec<_>>())
		);
		assert!(args.iter().all(|arg| !script.contains(arg.to_string_lossy().as_ref())));
	}

	#[test]
	fn a_fake_batch_cli_receives_arguments_as_separate_values() {
		let _spawn = crate::test_support::spawn_lock();
		let dir = tempfile::TempDir::new().unwrap();
		let batch = dir.path().join("fake cli.cmd");
		let capture = dir.path().join("argv.json");
		let mut file = std::fs::File::create(&batch).unwrap();
		writeln!(
			file,
			"@echo off\r\nsetlocal DisableDelayedExpansion\r\nset \"FACTORAI_TEST_ARG_1=%~1\"\r\nset \"FACTORAI_TEST_ARG_2=%~2\"\r\nset \"FACTORAI_TEST_ARG_3=%~3\"\r\nset \"FACTORAI_TEST_CAPTURE=%~dp0argv.json\"\r\npowershell.exe -NoLogo -NoProfile -NonInteractive -Command \"$values = @($env:FACTORAI_TEST_ARG_1, $env:FACTORAI_TEST_ARG_2, $env:FACTORAI_TEST_ARG_3); $json = ConvertTo-Json -InputObject $values -Compress; [System.IO.File]::WriteAllText($env:FACTORAI_TEST_CAPTURE, $json, [System.Text.UTF8Encoding]::new($false))\""
		)
		.unwrap();
		drop(file);
		let args = [
			OsString::from("first value & pipe | <redirect> ü"),
			OsString::from("literal %PATH% and !PATH!"),
			OsString::from("quoted \"prompt\" 東京 🐈"),
		];
		let (program, launched) = launch_command(&batch, &args).unwrap();
		let output = Command::new(program).args(launched).output().unwrap();
		assert!(
			output.status.success(),
			"batch launch failed: stdout={} stderr={}",
			String::from_utf8_lossy(&output.stdout),
			String::from_utf8_lossy(&output.stderr)
		);
		assert!(
			capture.is_file(),
			"fake batch CLI did not write its argv: stdout={} stderr={}",
			String::from_utf8_lossy(&output.stdout),
			String::from_utf8_lossy(&output.stderr)
		);
		let actual: Vec<String> = serde_json::from_slice(&std::fs::read(capture).unwrap()).unwrap();
		assert_eq!(
			actual,
			args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>()
		);
	}
}
