//! Claude's limits, from the JSON Claude Code pipes to its `statusLine`
//! command. A factorai session gets a `statusLine` of ours (through
//! `--settings`) that runs this binary as a helper: it drops the payload's
//! `rate_limits` into a sink file for the poller, then runs the user's own
//! `statusLine` command with the same stdin and prints what it printed — so the
//! user's status line is unchanged.
//!
//! The helper runs on every status-line refresh, so it stays small: no Tauri,
//! no database, no logging. It never fails loudly — a broken status line must
//! not disturb the session.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use super::child_guard::KillOnDrop;
use crate::models::{UsageReading, UsageSource, UsageWindow};

/// The argv[1] that turns the factorai binary into the status-line helper.
pub const HELPER_FLAG: &str = "--usage-statusline";

/// How long the user's own status-line command may take.
const USER_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// The user's own `statusLine`, as their settings define it.
#[derive(Debug, Clone, PartialEq)]
pub struct UserStatusLine {
	pub command: String,
	/// The whole `statusLine` object, every field as written.
	pub settings: Value,
}

/// The helper's whole job when the binary was started as
/// `factorai --usage-statusline <sink dir> [<profile config dir>]`: read stdin,
/// run, print. Returns whether `args` (argv without argv[0]) was a helper
/// invocation; if it was, the caller exits without starting the app. The
/// two-argument form (sessions started by an older build) still works.
pub fn helper_main(mut args: impl Iterator<Item = OsString>) -> bool {
	if args.next().as_deref() != Some(std::ffi::OsStr::new(HELPER_FLAG)) {
		return false;
	}
	// Nothing reaches the status line but the user's own output.
	std::panic::set_hook(Box::new(|_| {}));
	let sink_dir = args.next().map(PathBuf::from).unwrap_or_default();
	let profile_dir = args.next().map(PathBuf::from);
	let _ = std::panic::catch_unwind(|| {
		let mut stdin = Vec::new();
		let _ = std::io::stdin().read_to_end(&mut stdin);
		let config_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
		let out = run_helper(&sink_dir, &stdin, profile_dir.as_deref(), config_dir.as_deref());
		let mut stdout = std::io::stdout().lock();
		let _ = stdout.write_all(&out);
		let _ = stdout.flush();
	});
	true
}

/// Forward `stdin`'s `rate_limits` to `<sink_dir>/<session_id>.json`, then run
/// the user's `statusLine` with the same `stdin` and return what it printed
/// (empty when there is none or it failed).
///
/// The sink names `profile_dir` — the profile's config directory as factorai
/// stores it, passed on the command line — so the poller matches it exactly
/// even when factorai and the session see different `CLAUDE_CONFIG_DIR`s.
/// Without one (the two-argument form) it names the directory the session
/// reads, `env_config_dir` or `~/.claude`, which is also where the user's own
/// settings are looked up.
pub fn run_helper(
	sink_dir: &Path,
	stdin: &[u8],
	profile_dir: Option<&Path>,
	env_config_dir: Option<&Path>,
) -> Vec<u8> {
	let payload: Option<Value> = serde_json::from_slice(stdin).ok();
	let non_empty = |p: &&Path| !p.as_os_str().is_empty();
	let config_dir = env_config_dir
		.filter(non_empty)
		.map(Path::to_path_buf)
		.or_else(|| dirs::home_dir().map(|h| h.join(".claude")));
	let Some(config_dir) = config_dir else { return Vec::new() };
	if let Some(p) = &payload {
		write_sink(sink_dir, p, profile_dir.filter(non_empty).unwrap_or(&config_dir));
	}
	let project = payload
		.as_ref()
		.and_then(project_dir)
		.or_else(|| std::env::current_dir().ok())
		.unwrap_or_default();
	let Some(user) = user_statusline(&config_dir, &project) else { return Vec::new() };
	// The user's command runs in the helper's own cwd: the one Claude Code chose.
	run_user_command(&user.command, stdin, USER_COMMAND_TIMEOUT)
}

/// Where Claude Code reads project settings from: `workspace.project_dir` (the
/// launch directory), else `workspace.current_dir`, else `cwd`.
fn project_dir(p: &Value) -> Option<PathBuf> {
	["/workspace/project_dir", "/workspace/current_dir", "/cwd"]
		.iter()
		.find_map(|ptr| p.pointer(ptr).and_then(Value::as_str).filter(|s| !s.is_empty()))
		.map(PathBuf::from)
}

/// A session id is used as a file name, so only a plain one is accepted.
fn safe_session_id(p: &Value) -> Option<&str> {
	let id = p.get("session_id")?.as_str()?;
	let plain = !id.is_empty()
		&& id.len() <= 128
		&& id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
	plain.then_some(id)
}

/// Temp file then rename, so the poller never reads half a file. Nothing is
/// written without `rate_limits`: an API-key session, or one before its first
/// response, must leave the stored snapshot alone.
fn write_sink(sink_dir: &Path, payload: &Value, config_dir: &Path) {
	let Some(rate_limits) = payload.get("rate_limits").filter(|v| v.is_object()) else { return };
	let Some(id) = safe_session_id(payload) else { return };
	let body = json!({
		"configDir": config_dir.to_string_lossy(),
		"observedAt": super::now_ms(),
		"rateLimits": rate_limits,
	});
	if std::fs::create_dir_all(sink_dir).is_err() {
		return;
	}
	// Per process: two refreshes of one session may overlap. The rename keeps
	// the temp file's mtime, which is why the poller reads with `SINK_SLACK`.
	let tmp = sink_dir.join(format!("{id}.{}.tmp", std::process::id()));
	let written = std::fs::write(&tmp, body.to_string())
		.and_then(|_| std::fs::rename(&tmp, sink_dir.join(format!("{id}.json"))));
	if written.is_err() {
		let _ = std::fs::remove_file(&tmp);
	}
}

/// The user's `statusLine`, with Claude Code's precedence:
/// `<cwd>/.claude/settings.local.json` > `<cwd>/.claude/settings.json` >
/// `<config_dir>/settings.json`. Our own helper is never an answer.
pub fn user_statusline(config_dir: &Path, cwd: &Path) -> Option<UserStatusLine> {
	settings_files(config_dir, cwd).find_map(|v| {
		let line = v.get("statusLine").filter(|l| l.is_object())?;
		let command = line.get("command")?.as_str()?.trim();
		if command.is_empty() || command.contains(HELPER_FLAG) {
			return None;
		}
		Some(UserStatusLine { command: command.to_string(), settings: line.clone() })
	})
}

/// The settings files Claude Code merges, highest precedence first, parsed;
/// missing or unreadable ones are skipped.
fn settings_files(config_dir: &Path, cwd: &Path) -> impl Iterator<Item = Value> {
	let project = cwd.join(".claude");
	[
		project.join("settings.local.json"),
		project.join("settings.json"),
		config_dir.join("settings.json"),
	]
	.into_iter()
	.filter_map(|f| serde_json::from_slice(&std::fs::read(f).ok()?).ok())
}

/// A sink file's config directory and reading.
pub fn parse_sink(v: &Value) -> Option<(PathBuf, UsageReading)> {
	let config_dir = v.get("configDir")?.as_str()?;
	let observed_at = v.get("observedAt")?.as_i64()?;
	let limits = v.get("rateLimits")?;
	let window = |key: &str| {
		let w = limits.get(key)?;
		let used_percent = w.get("used_percentage")?.as_f64()?;
		let resets_at =
			w.get("resets_at").and_then(Value::as_f64).map(|s| (s * 1000.0).round() as i64);
		Some(UsageWindow { used_percent, resets_at })
	};
	let five_hour = window("five_hour");
	let weekly = window("seven_day");
	if five_hour.is_none() && weekly.is_none() {
		return None;
	}
	Some((
		PathBuf::from(config_dir),
		UsageReading {
			five_hour,
			weekly,
			per_model: Vec::new(),
			source: UsageSource::StatusLine,
			observed_at,
		},
	))
}

/// The shell Claude Code runs a status-line command in, which decides how our
/// command has to be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusShell {
	/// `sh` / Git Bash.
	Posix,
	/// Windows with no Git Bash: Claude Code falls back to PowerShell.
	PowerShell,
}

/// The shell a Claude session spawned with this config dir and project will
/// run its status line in: Git Bash on Windows when Claude Code would find it,
/// else PowerShell; `sh` elsewhere.
///
/// Judged against the session's environment, not factorai's: a
/// `CLAUDE_CODE_GIT_BASH_PATH` in the settings' `env` (which Claude Code
/// applies to itself) wins over the process variable, and `path` is the `PATH`
/// the session is given (`shell_path::child_path()`).
pub fn status_shell_for(config_dir: &Path, project: &Path, path: Option<&OsStr>) -> StatusShell {
	#[cfg(windows)]
	{
		let explicit = settings_git_bash_path(config_dir, project)
			.map(OsString::from)
			.or_else(|| std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH"));
		match git_bash_in(explicit.as_deref(), path) {
			Some(_) => StatusShell::Posix,
			None => StatusShell::PowerShell,
		}
	}
	#[cfg(not(windows))]
	{
		let _ = (config_dir, project, path);
		StatusShell::Posix
	}
}

/// `env.CLAUDE_CODE_GIT_BASH_PATH` from the highest-precedence settings file
/// that sets it.
#[cfg(windows)]
fn settings_git_bash_path(config_dir: &Path, project: &Path) -> Option<String> {
	settings_files(config_dir, project).find_map(|v| {
		v.pointer("/env/CLAUDE_CODE_GIT_BASH_PATH")
			.and_then(Value::as_str)
			.filter(|s| !s.trim().is_empty())
			.map(str::to_string)
	})
}

/// The inline `--settings` JSON for a factorai Claude session: the user's own
/// `statusLine` object with every field kept but `command`, which runs the
/// helper (and the helper runs theirs), written for `shell`. `config_dir` is
/// the profile's config directory, which the helper writes into each sink file
/// so the poller can tell whose reading it is.
pub fn settings_arg(
	exe: &Path,
	sink_dir: &Path,
	config_dir: &Path,
	user: Option<&Value>,
	shell: StatusShell,
) -> String {
	let mut line: Map<String, Value> = user.and_then(Value::as_object).cloned().unwrap_or_default();
	line.insert("type".into(), "command".into());
	line.insert("command".into(), helper_command(exe, sink_dir, config_dir, shell).into());
	json!({ "statusLine": line }).to_string()
}

/// Single-quoted in both shells, so nothing in a path (`$`, a backtick, a
/// space) is expanded. PowerShell needs `&` to run a quoted path.
fn helper_command(exe: &Path, sink_dir: &Path, config_dir: &Path, shell: StatusShell) -> String {
	let (exe, sink, config) = (path_text(exe), path_text(sink_dir), path_text(config_dir));
	match shell {
		StatusShell::Posix => {
			format!("{} {HELPER_FLAG} {} {}", sh_quote(&exe), sh_quote(&sink), sh_quote(&config))
		}
		StatusShell::PowerShell => {
			format!("& {} {HELPER_FLAG} {} {}", ps_quote(&exe), ps_quote(&sink), ps_quote(&config))
		}
	}
}

/// On Windows the separators become `/`, which Git Bash and PowerShell both
/// accept and neither treats as an escape.
fn path_text(p: &Path) -> String {
	let s = p.to_string_lossy();
	#[cfg(windows)]
	let s = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
	s.to_string()
}

/// POSIX single quotes: a `'` inside closes, escapes and reopens.
fn sh_quote(s: &str) -> String {
	format!("'{}'", s.replace('\'', r"'\''"))
}

/// PowerShell single quotes: a quote inside is doubled. PowerShell also takes
/// the typographic single quotes as delimiters, so those are doubled too.
fn ps_quote(s: &str) -> String {
	let mut out = String::with_capacity(s.len() + 2);
	out.push('\'');
	for c in s.chars() {
		if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
			out.push(c);
		}
		out.push(c);
	}
	out.push('\'');
	out
}

/// The shell Claude Code itself runs a status line in: Git Bash on Windows
/// (PowerShell when there is none), `sh` elsewhere.
fn shell_command(command: &str) -> Command {
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		let mut cmd = match git_bash() {
			Some(bash) => {
				let mut cmd = Command::new(bash);
				cmd.arg("-c").arg(command);
				cmd
			}
			None => powershell_command(command),
		};
		cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
		cmd
	}
	#[cfg(not(windows))]
	{
		use std::os::unix::process::CommandExt;
		let mut cmd = Command::new("sh");
		cmd.arg("-c").arg(command);
		// Its own group, so the guard can kill the whole tree.
		cmd.process_group(0);
		cmd
	}
}

#[cfg(windows)]
fn powershell_command(command: &str) -> Command {
	let mut cmd = Command::new("powershell");
	cmd.args(["-NoProfile", "-NonInteractive", "-Command", command]);
	cmd
}

/// After the command exits, how long a background job it left holding stdout
/// may keep us waiting for the rest of the output.
const OUTPUT_GRACE: Duration = Duration::from_millis(200);

/// Run the user's command with `stdin` piped in and return its stdout — only
/// when it exits with status 0 within `timeout`, as Claude Code shows it.
/// Anything else is an empty line. Past `timeout` the command and everything
/// it started are killed; a command that finished is left alone, background
/// jobs included.
fn run_user_command(command: &str, stdin: &[u8], timeout: Duration) -> Vec<u8> {
	run_in(shell_command(command), stdin, timeout)
}

/// `run_user_command` for an already-built shell command.
fn run_in(mut cmd: Command, stdin: &[u8], timeout: Duration) -> Vec<u8> {
	cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
	let Ok(child) = cmd.spawn() else { return Vec::new() };
	let mut guard = KillOnDrop::new(child);
	let (Some(mut input), Some(mut output)) = (guard.child.stdin.take(), guard.child.stdout.take())
	else {
		return Vec::new();
	};
	let bytes = stdin.to_vec();
	// Its own thread: a command that never reads must not block us on a full pipe.
	let _ = std::thread::Builder::new().name("statusline-stdin".into()).spawn(move || {
		let _ = input.write_all(&bytes);
	});
	let (tx, rx) = mpsc::channel::<Vec<u8>>();
	let reader = std::thread::Builder::new().name("statusline-stdout".into()).spawn(move || {
		let mut buf = [0u8; 8192];
		while let Ok(n) = output.read(&mut buf) {
			if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
				break;
			}
		}
	});
	if reader.is_err() {
		return Vec::new();
	}
	let deadline = Instant::now() + timeout;
	let mut out = Vec::new();
	let mut eof = false;
	let status = loop {
		match guard.child.try_wait() {
			Ok(Some(status)) => break status,
			Ok(None) => {}
			Err(_) => return Vec::new(),
		}
		if Instant::now() >= deadline {
			// The guard kills the whole tree.
			return Vec::new();
		}
		if eof {
			std::thread::sleep(Duration::from_millis(10));
			continue;
		}
		match rx.recv_timeout(Duration::from_millis(10)) {
			Ok(chunk) => out.extend_from_slice(&chunk),
			Err(mpsc::RecvTimeoutError::Disconnected) => eof = true,
			Err(mpsc::RecvTimeoutError::Timeout) => {}
		}
	};
	guard.release();
	if !status.success() {
		return Vec::new();
	}
	let grace = (Instant::now() + OUTPUT_GRACE).min(deadline);
	while !eof {
		match rx.recv_timeout(grace.saturating_duration_since(Instant::now())) {
			Ok(chunk) => out.extend_from_slice(&chunk),
			Err(_) => eof = true,
		}
	}
	out
}

/// Git Bash as this process sees it. The helper runs inside Claude Code's own
/// environment, so its variables are already the session's.
#[cfg(windows)]
fn git_bash() -> Option<PathBuf> {
	git_bash_in(
		std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH").as_deref(),
		std::env::var_os("PATH").as_deref(),
	)
}

/// Git Bash, found the way Claude Code finds it: `explicit`
/// (`CLAUDE_CODE_GIT_BASH_PATH`), then `<git root>/bin/bash.exe` beside a
/// `git.exe` on `path`, then the default install.
#[cfg(windows)]
fn git_bash_in(explicit: Option<&OsStr>, path: Option<&OsStr>) -> Option<PathBuf> {
	if let Some(p) = explicit.map(PathBuf::from) {
		if p.is_file() {
			return Some(p);
		}
	}
	let on_path = path.and_then(|path| {
		std::env::split_paths(path).map(|d| d.join("git.exe")).find(|p| p.is_file())
	});
	if let Some(git) = on_path {
		// `<root>/cmd/git.exe`, `<root>/bin/git.exe`, `<root>/mingw64/bin/git.exe`.
		for root in git.ancestors().skip(2).take(3) {
			let bash = root.join("bin").join("bash.exe");
			if bash.is_file() {
				return Some(bash);
			}
		}
	}
	let default = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
	default.is_file().then_some(default)
}

/// Whether two config-directory strings name the same directory: trailing
/// separators ignored, and on Windows case and `\` vs `/` too.
pub fn same_dir(a: &str, b: &str) -> bool {
	normalise_dir(a) == normalise_dir(b)
}

fn normalise_dir(p: &str) -> String {
	let s = p.trim();
	#[cfg(windows)]
	let s = s.strip_prefix(r"\\?\").unwrap_or(s).replace('\\', "/").to_lowercase();
	s.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
	use std::time::Instant;

	use serde_json::json;

	use super::*;
	use crate::models::UsageSource;

	fn payload(session: &str, cwd: &Path, rate_limits: Option<Value>) -> Vec<u8> {
		let mut v = json!({
			"session_id": session,
			"cwd": cwd,
			"workspace": {"current_dir": cwd, "project_dir": cwd},
			"model": {"id": "claude-opus", "display_name": "Opus"},
		});
		if let Some(rl) = rate_limits {
			v["rate_limits"] = rl;
		}
		serde_json::to_vec(&v).unwrap()
	}

	fn limits() -> Value {
		json!({
			"five_hour": {"used_percentage": 42.5, "resets_at": 1_790_000_000},
			"seven_day": {"used_percentage": 7, "resets_at": 1_790_600_000},
		})
	}

	fn write_settings(dir: &Path, statusline: Value) {
		std::fs::create_dir_all(dir).unwrap();
		std::fs::write(dir.join("settings.json"), json!({"statusLine": statusline}).to_string())
			.unwrap();
	}

	#[test]
	fn sink_is_written_when_rate_limits_present() {
		let tmp = tempfile::tempdir().unwrap();
		let sink = tmp.path().join("sink");
		let config = tmp.path().join("config");
		let cwd = tmp.path().join("work");
		std::fs::create_dir_all(&cwd).unwrap();

		run_helper(&sink, &payload("abc-123", &cwd, Some(limits())), None, Some(&config));

		let written: Value =
			serde_json::from_slice(&std::fs::read(sink.join("abc-123.json")).unwrap()).unwrap();
		assert_eq!(written["configDir"], config.to_string_lossy().as_ref());
		assert_eq!(written["rateLimits"], limits());
		assert!(written["observedAt"].as_i64().unwrap() > 0);
		// No temp file left behind.
		assert_eq!(std::fs::read_dir(&sink).unwrap().count(), 1);
	}

	/// factorai's own view of the profile's directory names the reading, even
	/// when the session reads another one (an inherited `CLAUDE_CONFIG_DIR`);
	/// the user's settings still come from the directory the session reads.
	#[test]
	fn an_explicit_profile_dir_names_the_sink() {
		let _spawn = crate::test_support::spawn_lock();
		let tmp = tempfile::tempdir().unwrap();
		let sink = tmp.path().join("sink");
		let (profile, session) = (tmp.path().join("profile"), tmp.path().join("session"));
		let cwd = tmp.path().join("work");
		std::fs::create_dir_all(&cwd).unwrap();
		write_settings(&session, json!({"type": "command", "command": "echo session"}));

		let out = run_helper(
			&sink,
			&payload("abc", &cwd, Some(limits())),
			Some(&profile),
			Some(&session),
		);

		let written: Value =
			serde_json::from_slice(&std::fs::read(sink.join("abc.json")).unwrap()).unwrap();
		assert_eq!(written["configDir"], profile.to_string_lossy().as_ref());
		assert_eq!(String::from_utf8_lossy(&out).trim_end(), "session");
		// An empty argument is no argument.
		run_helper(
			&sink,
			&payload("abd", &cwd, Some(limits())),
			Some(Path::new("")),
			Some(&session),
		);
		let written: Value =
			serde_json::from_slice(&std::fs::read(sink.join("abd.json")).unwrap()).unwrap();
		assert_eq!(written["configDir"], session.to_string_lossy().as_ref());
	}

	#[test]
	fn payload_without_rate_limits_is_ignored() {
		let _spawn = crate::test_support::spawn_lock();
		let tmp = tempfile::tempdir().unwrap();
		let sink = tmp.path().join("sink");
		let config = tmp.path().join("config");
		let cwd = tmp.path().join("work");
		std::fs::create_dir_all(&cwd).unwrap();
		write_settings(&config, json!({"type": "command", "command": "echo hi"}));

		let out = run_helper(&sink, &payload("abc", &cwd, None), None, Some(&config));

		assert!(!sink.join("abc.json").exists());
		assert_eq!(String::from_utf8_lossy(&out).trim_end(), "hi");
	}

	#[test]
	fn helper_runs_the_users_statusline_and_prints_its_output() {
		let _spawn = crate::test_support::spawn_lock();
		let tmp = tempfile::tempdir().unwrap();
		let config = tmp.path().join("config");
		let cwd = tmp.path().join("work");
		std::fs::create_dir_all(&cwd).unwrap();
		write_settings(&config, json!({"type": "command", "command": "echo hi", "padding": 0}));

		let out = run_helper(
			&tmp.path().join("sink"),
			&payload("s", &cwd, Some(limits())),
			None,
			Some(&config),
		);

		#[cfg(unix)]
		assert_eq!(out, b"hi\n");
		// Git Bash prints `hi\n`, the PowerShell fallback `hi\r\n`.
		#[cfg(windows)]
		assert_eq!(String::from_utf8_lossy(&out).trim_end(), "hi");
	}

	#[test]
	fn project_settings_come_from_the_project_dir_not_the_current_dir() {
		let _spawn = crate::test_support::spawn_lock();
		let tmp = tempfile::tempdir().unwrap();
		let config = tmp.path().join("config");
		let (project, current) = (tmp.path().join("project"), tmp.path().join("project/sub"));
		write_settings(&project.join(".claude"), json!({"command": "echo project"}));
		write_settings(&current.join(".claude"), json!({"command": "echo current"}));
		let input = json!({"session_id": "s", "cwd": current,
			"workspace": {"current_dir": current, "project_dir": project}});

		let out = run_helper(
			&tmp.path().join("sink"),
			&serde_json::to_vec(&input).unwrap(),
			None,
			Some(&config),
		);

		assert_eq!(String::from_utf8_lossy(&out).trim_end(), "project");
	}

	#[test]
	fn no_user_statusline_prints_nothing() {
		let tmp = tempfile::tempdir().unwrap();
		let cwd = tmp.path().join("work");
		std::fs::create_dir_all(&cwd).unwrap();
		let out = run_helper(
			&tmp.path().join("sink"),
			&payload("s", &cwd, Some(limits())),
			None,
			Some(&tmp.path().join("config")),
		);
		assert!(out.is_empty());
	}

	#[test]
	fn garbage_stdin_prints_nothing_and_writes_nothing() {
		let tmp = tempfile::tempdir().unwrap();
		let sink = tmp.path().join("sink");
		let out = run_helper(&sink, b"{not json", None, Some(&tmp.path().join("config")));
		assert!(out.is_empty());
		assert!(!sink.exists());
	}

	#[test]
	fn an_unsafe_session_id_writes_no_sink() {
		let tmp = tempfile::tempdir().unwrap();
		let sink = tmp.path().join("sink");
		let cwd = tmp.path().join("work");
		std::fs::create_dir_all(&cwd).unwrap();
		run_helper(
			&sink,
			&payload("../escape", &cwd, Some(limits())),
			None,
			Some(&tmp.path().join("config")),
		);
		assert!(!tmp.path().join("escape.json").exists());
		assert!(!sink.exists() || std::fs::read_dir(&sink).unwrap().count() == 0);
	}

	/// The POSIX shell tests need `sh`, or Git Bash on Windows.
	fn posix() -> bool {
		#[cfg(windows)]
		return git_bash().is_some();
		#[cfg(not(windows))]
		true
	}

	#[cfg(windows)]
	#[test]
	fn settings_env_git_bash_path_wins_over_the_process_env() {
		let tmp = tempfile::tempdir().unwrap();
		let (config, project) = (tmp.path().join("config"), tmp.path().join("project"));
		let (user_bash, local_bash) =
			(tmp.path().join("user-bash.exe"), tmp.path().join("local-bash.exe"));
		std::fs::write(&user_bash, "").unwrap();
		std::fs::write(&local_bash, "").unwrap();
		std::fs::create_dir_all(&config).unwrap();
		std::fs::write(
			config.join("settings.json"),
			json!({"env": {"CLAUDE_CODE_GIT_BASH_PATH": user_bash}}).to_string(),
		)
		.unwrap();
		assert_eq!(settings_git_bash_path(&config, &project).as_deref(), user_bash.to_str());

		std::fs::create_dir_all(project.join(".claude")).unwrap();
		std::fs::write(
			project.join(".claude").join("settings.local.json"),
			json!({"env": {"CLAUDE_CODE_GIT_BASH_PATH": local_bash}}).to_string(),
		)
		.unwrap();
		assert_eq!(settings_git_bash_path(&config, &project).as_deref(), local_bash.to_str());

		// No Git anywhere on the given PATH, and the process variable (if any)
		// is beaten by the settings: the settings' bash makes it Posix.
		let empty = tmp.path().join("empty-path");
		std::fs::create_dir_all(&empty).unwrap();
		assert_eq!(
			status_shell_for(&config, &project, Some(empty.as_os_str())),
			StatusShell::Posix
		);
	}

	#[cfg(windows)]
	#[test]
	fn the_git_bash_lookup_uses_the_given_path() {
		let tmp = tempfile::tempdir().unwrap();
		let root = tmp.path().join("Git");
		std::fs::create_dir_all(root.join("cmd")).unwrap();
		std::fs::create_dir_all(root.join("bin")).unwrap();
		std::fs::write(root.join("cmd").join("git.exe"), "").unwrap();
		std::fs::write(root.join("bin").join("bash.exe"), "").unwrap();
		let path = std::env::join_paths([tmp.path().join("nothing"), root.join("cmd")]).unwrap();

		assert_eq!(git_bash_in(None, Some(&path)), Some(root.join("bin").join("bash.exe")));
		// An explicit path that is not a file falls through to the lookup.
		let missing = tmp.path().join("missing.exe");
		assert_eq!(
			git_bash_in(Some(missing.as_os_str()), Some(&path)),
			Some(root.join("bin").join("bash.exe"))
		);
	}

	#[test]
	fn the_users_command_gets_the_same_stdin() {
		let _spawn = crate::test_support::spawn_lock();
		if !posix() {
			return;
		}
		let input = b"{\"session_id\":\"x\"}";
		let out = run_user_command("cat", input, Duration::from_secs(5));
		assert_eq!(out, input);
	}

	#[test]
	fn the_users_command_runs_in_the_helpers_own_cwd() {
		let _spawn = crate::test_support::spawn_lock();
		if !posix() {
			return;
		}
		let here = std::env::current_dir().unwrap();
		let out = run_user_command("basename \"$PWD\"", b"", Duration::from_secs(5));
		assert_eq!(
			String::from_utf8_lossy(&out).trim_end(),
			here.file_name().unwrap().to_string_lossy()
		);
	}

	#[test]
	fn a_non_zero_exit_blanks_the_line() {
		let _spawn = crate::test_support::spawn_lock();
		if !posix() {
			return;
		}
		assert!(run_user_command("echo hi; exit 1", b"", Duration::from_secs(5)).is_empty());
	}

	#[test]
	fn a_background_job_neither_blanks_nor_holds_the_line() {
		let _spawn = crate::test_support::spawn_lock();
		if !posix() {
			return;
		}
		let started = Instant::now();
		let out = run_user_command("sleep 10 & echo hi", b"", Duration::from_secs(9));
		assert_eq!(String::from_utf8_lossy(&out).trim_end(), "hi");
		// Well short of the sleep: the line was not held for the background job.
		assert!(started.elapsed() < Duration::from_secs(8), "{:?}", started.elapsed());
	}

	#[test]
	fn a_hung_user_command_is_cut_off() {
		let _spawn = crate::test_support::spawn_lock();
		if !posix() {
			return;
		}
		let started = Instant::now();
		let out = run_user_command("echo partial; sleep 30", b"", Duration::from_secs(1));
		assert!(out.is_empty(), "a timeout blanks the line");
		assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
	}

	#[cfg(windows)]
	#[test]
	fn the_powershell_fallback_runs_quotes_and_gets_stdin() {
		let _spawn = crate::test_support::spawn_lock();
		let out = run_in(
			powershell_command("$s = ($input | Out-String).Trim(); Write-Output \"got: $s\""),
			b"{\"a\":1}",
			Duration::from_secs(20),
		);
		assert_eq!(String::from_utf8_lossy(&out).trim_end(), "got: {\"a\":1}");
	}

	#[cfg(windows)]
	#[test]
	fn the_powershell_fallback_blanks_on_failure_and_timeout() {
		let _spawn = crate::test_support::spawn_lock();
		let out =
			run_in(powershell_command("Write-Output hi; exit 3"), b"", Duration::from_secs(20));
		assert!(out.is_empty());
		let started = Instant::now();
		let out = run_in(
			powershell_command("Write-Output hi; Start-Sleep 30"),
			b"",
			Duration::from_secs(3),
		);
		assert!(out.is_empty());
		assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
	}

	/// A directory whose name has a space and a single quote, holding `name`.
	fn awkward_dir(tmp: &Path) -> PathBuf {
		let dir = tmp.join("it's a dir");
		std::fs::create_dir_all(&dir).unwrap();
		dir
	}

	#[test]
	fn the_posix_form_survives_the_shell() {
		let _spawn = crate::test_support::spawn_lock();
		#[cfg(unix)]
		let shell = PathBuf::from("sh");
		#[cfg(windows)]
		let Some(shell) = git_bash() else {
			return;
		};
		let tmp = tempfile::tempdir().unwrap();
		let dir = awkward_dir(tmp.path());
		let exe = dir.join("args.sh");
		std::fs::write(&exe, "#!/bin/sh\nprintf '%s|%s|%s' \"$1\" \"$2\" \"$3\"\n").unwrap();
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
		}
		let sink = dir.join("sink $HOME `x`");
		let config = dir.join(".claude $(x)");
		let command = helper_command(&exe, &sink, &config, StatusShell::Posix);
		let out = Command::new(shell).arg("-c").arg(&command).output().unwrap();
		assert_eq!(
			String::from_utf8_lossy(&out.stdout),
			format!("{HELPER_FLAG}|{}|{}", path_text(&sink), path_text(&config)),
			"{command}"
		);
	}

	#[cfg(windows)]
	#[test]
	fn the_powershell_form_survives_powershell() {
		let _spawn = crate::test_support::spawn_lock();
		let tmp = tempfile::tempdir().unwrap();
		let dir = awkward_dir(tmp.path());
		let exe = dir.join("args.cmd");
		std::fs::write(&exe, "@echo off\r\necho [%~1] [%~2] [%~3]\r\n").unwrap();
		let sink = dir.join("sink $HOME");
		let config = dir.join(".claude $(x)");
		let command = helper_command(&exe, &sink, &config, StatusShell::PowerShell);
		let out = Command::new("powershell")
			.args(["-NoProfile", "-NonInteractive", "-Command", &command])
			.output()
			.unwrap();
		assert_eq!(
			String::from_utf8_lossy(&out.stdout).trim_end(),
			format!("[{HELPER_FLAG}] [{}] [{}]", path_text(&sink), path_text(&config)),
			"{command}"
		);
	}

	#[test]
	fn quotes_escape_an_embedded_single_quote_per_shell() {
		assert_eq!(sh_quote("it's"), r"'it'\''s'");
		assert_eq!(ps_quote("it's"), "'it''s'");
		assert_eq!(ps_quote("it\u{2019}s"), "'it\u{2019}\u{2019}s'");
	}

	#[test]
	fn project_local_settings_win_over_user_settings() {
		let tmp = tempfile::tempdir().unwrap();
		let config = tmp.path().join("config");
		let cwd = tmp.path().join("work");
		write_settings(&config, json!({"type": "command", "command": "user"}));
		assert_eq!(user_statusline(&config, &cwd).unwrap().command, "user");

		write_settings(&cwd.join(".claude"), json!({"type": "command", "command": "project"}));
		assert_eq!(user_statusline(&config, &cwd).unwrap().command, "project");

		std::fs::write(
			cwd.join(".claude").join("settings.local.json"),
			json!({"statusLine": {"type": "command", "command": "local", "padding": 2}})
				.to_string(),
		)
		.unwrap();
		let local = user_statusline(&config, &cwd).unwrap();
		assert_eq!(local.command, "local");
		assert_eq!(local.settings["padding"], 2);
	}

	#[test]
	fn our_own_helper_is_never_the_users_statusline() {
		let tmp = tempfile::tempdir().unwrap();
		let config = tmp.path().join("config");
		write_settings(
			&config,
			json!({"type": "command", "command": format!("factorai {HELPER_FLAG} x")}),
		);
		assert!(user_statusline(&config, &tmp.path().join("work")).is_none());
	}

	#[test]
	fn parse_sink_converts_seconds_to_ms_and_maps_windows() {
		let v =
			json!({"configDir": "C:/Users/me/.claude", "observedAt": 1234, "rateLimits": limits()});
		let (dir, r) = parse_sink(&v).unwrap();
		assert_eq!(dir, PathBuf::from("C:/Users/me/.claude"));
		assert_eq!(r.source, UsageSource::StatusLine);
		assert_eq!(r.observed_at, 1234);
		let five = r.five_hour.unwrap();
		assert_eq!(five.used_percent, 42.5);
		assert_eq!(five.resets_at, Some(1_790_000_000_000));
		let week = r.weekly.unwrap();
		assert_eq!(week.used_percent, 7.0);
		assert_eq!(week.resets_at, Some(1_790_600_000_000));
		assert!(r.per_model.is_empty());
	}

	#[test]
	fn parse_sink_takes_a_lone_window_and_refuses_none() {
		let one = json!({"configDir": "/c", "observedAt": 1,
			"rateLimits": {"seven_day": {"used_percentage": 3.0}}});
		let (_, r) = parse_sink(&one).unwrap();
		assert!(r.five_hour.is_none());
		assert_eq!(r.weekly.unwrap().resets_at, None);
		let none = json!({"configDir": "/c", "observedAt": 1, "rateLimits": {}});
		assert!(parse_sink(&none).is_none());
	}

	#[test]
	fn settings_arg_is_valid_json_with_quoted_paths() {
		let exe = Path::new(r"C:\Users\Fyruz DEV\factorai\factorai.exe");
		let sink = Path::new(r"C:\Users\Fyruz DEV\AppData\Roaming\factorai\usage\statusline");
		let config = Path::new(r"C:\Users\Fyruz DEV\.claude-work");
		let command = |shell| {
			let v: Value =
				serde_json::from_str(&settings_arg(exe, sink, config, None, shell)).unwrap();
			let line = v["statusLine"].as_object().unwrap().clone();
			assert_eq!(line.len(), 2, "{line:?}");
			assert_eq!(line["type"], "command");
			line["command"].as_str().unwrap().to_string()
		};
		#[cfg(windows)]
		let (exe_text, sink_text, config_text) = (
			"C:/Users/Fyruz DEV/factorai/factorai.exe",
			"C:/Users/Fyruz DEV/AppData/Roaming/factorai/usage/statusline",
			"C:/Users/Fyruz DEV/.claude-work",
		);
		#[cfg(unix)]
		let (exe_text, sink_text, config_text) =
			(exe.to_str().unwrap(), sink.to_str().unwrap(), config.to_str().unwrap());
		assert_eq!(
			command(StatusShell::Posix),
			format!("'{exe_text}' --usage-statusline '{sink_text}' '{config_text}'")
		);
		assert_eq!(
			command(StatusShell::PowerShell),
			format!("& '{exe_text}' --usage-statusline '{sink_text}' '{config_text}'")
		);
	}

	#[test]
	fn settings_arg_keeps_every_user_field_but_the_command() {
		let user =
			json!({"type": "command", "command": "mine", "padding": 1, "refreshInterval": 5});
		let v: Value = serde_json::from_str(&settings_arg(
			Path::new("/opt/factorai"),
			Path::new("/data/sink"),
			Path::new("/home/me/.claude"),
			Some(&user),
			StatusShell::Posix,
		))
		.unwrap();
		let line = &v["statusLine"];
		assert_eq!(line["padding"], 1);
		assert_eq!(line["refreshInterval"], 5);
		assert_eq!(line["type"], "command");
		assert!(line["command"].as_str().unwrap().contains(HELPER_FLAG));
		assert!(!line["command"].as_str().unwrap().contains("mine"));
	}

	#[test]
	fn same_dir_ignores_trailing_slashes_and_on_windows_case_and_separators() {
		assert!(same_dir("/home/me/.claude/", "/home/me/.claude"));
		assert!(!same_dir("/home/me/.claude", "/home/me/.claude-work"));
		#[cfg(windows)]
		assert!(same_dir(r"C:\Users\Me\.claude", "c:/users/me/.claude/"));
	}
}
