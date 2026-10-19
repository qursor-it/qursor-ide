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

use std::ffi::OsString;
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
/// `factorai --usage-statusline <sink dir>`: read stdin, run, print. Returns
/// whether `args` (argv without argv[0]) was a helper invocation; if it was,
/// the caller exits without starting the app.
pub fn helper_main(mut args: impl Iterator<Item = OsString>) -> bool {
	if args.next().as_deref() != Some(std::ffi::OsStr::new(HELPER_FLAG)) {
		return false;
	}
	// Nothing reaches the status line but the user's own output.
	std::panic::set_hook(Box::new(|_| {}));
	let sink_dir = args.next().map(PathBuf::from).unwrap_or_default();
	let _ = std::panic::catch_unwind(|| {
		let mut stdin = Vec::new();
		let _ = std::io::stdin().read_to_end(&mut stdin);
		let config_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
		let out = run_helper(&sink_dir, &stdin, config_dir.as_deref());
		let mut stdout = std::io::stdout().lock();
		let _ = stdout.write_all(&out);
		let _ = stdout.flush();
	});
	true
}

/// Forward `stdin`'s `rate_limits` to `<sink_dir>/<session_id>.json`, then run
/// the user's `statusLine` with the same `stdin` and return what it printed
/// (empty when there is none or it failed).
pub fn run_helper(sink_dir: &Path, stdin: &[u8], env_config_dir: Option<&Path>) -> Vec<u8> {
	let payload: Option<Value> = serde_json::from_slice(stdin).ok();
	let config_dir = env_config_dir
		.filter(|p| !p.as_os_str().is_empty())
		.map(Path::to_path_buf)
		.or_else(|| dirs::home_dir().map(|h| h.join(".claude")));
	let Some(config_dir) = config_dir else { return Vec::new() };
	if let Some(p) = &payload {
		write_sink(sink_dir, p, &config_dir);
	}
	let workspace = payload.as_ref().and_then(payload_cwd);
	let settings_cwd =
		workspace.clone().or_else(|| std::env::current_dir().ok()).unwrap_or_default();
	let Some(user) = user_statusline(&config_dir, &settings_cwd) else { return Vec::new() };
	let cwd = workspace.filter(|p| p.is_dir());
	run_user_command(&user.command, stdin, cwd.as_deref(), USER_COMMAND_TIMEOUT)
}

/// `workspace.current_dir`, else `cwd`.
fn payload_cwd(p: &Value) -> Option<PathBuf> {
	p.pointer("/workspace/current_dir")
		.and_then(Value::as_str)
		.or_else(|| p.get("cwd").and_then(Value::as_str))
		.filter(|s| !s.is_empty())
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
	// Per process: two refreshes of one session may overlap.
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
	let project = cwd.join(".claude");
	let files = [
		project.join("settings.local.json"),
		project.join("settings.json"),
		config_dir.join("settings.json"),
	];
	files.iter().find_map(|f| {
		let v: Value = serde_json::from_slice(&std::fs::read(f).ok()?).ok()?;
		let line = v.get("statusLine").filter(|l| l.is_object())?;
		let command = line.get("command")?.as_str()?.trim();
		if command.is_empty() || command.contains(HELPER_FLAG) {
			return None;
		}
		Some(UserStatusLine { command: command.to_string(), settings: line.clone() })
	})
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

/// The inline `--settings` JSON for a factorai Claude session: the user's own
/// `statusLine` object with every field kept but `command`, which runs the
/// helper (and the helper runs theirs).
pub fn settings_arg(exe: &Path, sink_dir: &Path, user: Option<&Value>) -> String {
	let mut line: Map<String, Value> = user.and_then(Value::as_object).cloned().unwrap_or_default();
	line.insert("type".into(), "command".into());
	line.insert(
		"command".into(),
		format!("{} {HELPER_FLAG} {}", quote_path(exe), quote_path(sink_dir)).into(),
	);
	json!({ "statusLine": line }).to_string()
}

/// Double-quoted for the shell Claude Code runs the command in. On Windows the
/// separators become `/`, which Git Bash and `cmd` both accept and neither
/// treats as an escape.
fn quote_path(p: &Path) -> String {
	let s = p.to_string_lossy();
	#[cfg(windows)]
	let s = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
	#[cfg(not(windows))]
	let s = s.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`");
	format!("\"{s}\"")
}

/// The shell Claude Code itself runs a status line in: Git Bash on Windows
/// (`cmd` when there is none), `sh` elsewhere.
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
			None => cmd_command(command),
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

/// `cmd /S /C "<command>"`: `/S` strips exactly the outer quotes, so the
/// command's own quotes reach `cmd` untouched.
#[cfg(windows)]
fn cmd_command(command: &str) -> Command {
	use std::os::windows::process::CommandExt;
	let mut cmd = Command::new("cmd");
	cmd.raw_arg(format!("/D /S /C \"{command}\""));
	cmd
}

/// Run the user's command with `stdin` piped in and return its stdout. Past
/// `timeout` the command and everything it started are killed and whatever it
/// printed so far is returned.
fn run_user_command(command: &str, stdin: &[u8], cwd: Option<&Path>, timeout: Duration) -> Vec<u8> {
	let mut cmd = shell_command(command);
	cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
	if let Some(dir) = cwd {
		cmd.current_dir(dir);
	}
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
	while let Ok(chunk) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
		out.extend_from_slice(&chunk);
	}
	out
}

/// Git Bash, found the way Claude Code finds it: `CLAUDE_CODE_GIT_BASH_PATH`,
/// then `<git root>/bin/bash.exe` beside a `git.exe` on `PATH`, then the
/// default install.
#[cfg(windows)]
fn git_bash() -> Option<PathBuf> {
	if let Some(p) = std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH").map(PathBuf::from) {
		if p.is_file() {
			return Some(p);
		}
	}
	let on_path = std::env::var_os("PATH").and_then(|path| {
		std::env::split_paths(&path).map(|d| d.join("git.exe")).find(|p| p.is_file())
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

		run_helper(&sink, &payload("abc-123", &cwd, Some(limits())), Some(&config));

		let written: Value =
			serde_json::from_slice(&std::fs::read(sink.join("abc-123.json")).unwrap()).unwrap();
		assert_eq!(written["configDir"], config.to_string_lossy().as_ref());
		assert_eq!(written["rateLimits"], limits());
		assert!(written["observedAt"].as_i64().unwrap() > 0);
		// No temp file left behind.
		assert_eq!(std::fs::read_dir(&sink).unwrap().count(), 1);
	}

	#[test]
	fn payload_without_rate_limits_is_ignored() {
		let tmp = tempfile::tempdir().unwrap();
		let sink = tmp.path().join("sink");
		let config = tmp.path().join("config");
		let cwd = tmp.path().join("work");
		std::fs::create_dir_all(&cwd).unwrap();
		write_settings(&config, json!({"type": "command", "command": "echo hi"}));

		let out = run_helper(&sink, &payload("abc", &cwd, None), Some(&config));

		assert!(!sink.join("abc.json").exists());
		assert_eq!(String::from_utf8_lossy(&out).trim_end(), "hi");
	}

	#[test]
	fn helper_runs_the_users_statusline_and_prints_its_output() {
		let tmp = tempfile::tempdir().unwrap();
		let config = tmp.path().join("config");
		let cwd = tmp.path().join("work");
		std::fs::create_dir_all(&cwd).unwrap();
		write_settings(&config, json!({"type": "command", "command": "echo hi", "padding": 0}));

		let out = run_helper(
			&tmp.path().join("sink"),
			&payload("s", &cwd, Some(limits())),
			Some(&config),
		);

		#[cfg(unix)]
		assert_eq!(out, b"hi\n");
		// Git Bash prints `hi\n`, the `cmd` fallback `hi\r\n`.
		#[cfg(windows)]
		assert_eq!(String::from_utf8_lossy(&out).trim_end(), "hi");
	}

	#[test]
	fn no_user_statusline_prints_nothing() {
		let tmp = tempfile::tempdir().unwrap();
		let cwd = tmp.path().join("work");
		std::fs::create_dir_all(&cwd).unwrap();
		let out = run_helper(
			&tmp.path().join("sink"),
			&payload("s", &cwd, Some(limits())),
			Some(&tmp.path().join("config")),
		);
		assert!(out.is_empty());
	}

	#[test]
	fn garbage_stdin_prints_nothing_and_writes_nothing() {
		let tmp = tempfile::tempdir().unwrap();
		let sink = tmp.path().join("sink");
		let out = run_helper(&sink, b"{not json", Some(&tmp.path().join("config")));
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
			Some(&tmp.path().join("config")),
		);
		assert!(!tmp.path().join("escape.json").exists());
		assert!(!sink.exists() || std::fs::read_dir(&sink).unwrap().count() == 0);
	}

	#[test]
	fn the_users_command_gets_the_same_stdin() {
		#[cfg(windows)]
		if git_bash().is_none() {
			return;
		}
		let input = b"{\"session_id\":\"x\"}";
		let out = run_user_command("cat", input, None, Duration::from_secs(5));
		assert_eq!(out, input);
	}

	#[cfg(windows)]
	#[test]
	fn the_cmd_fallback_keeps_the_commands_own_quotes() {
		let out = cmd_command("echo \"a b\"").stdout(Stdio::piped()).output().unwrap().stdout;
		assert_eq!(String::from_utf8_lossy(&out).trim_end(), "\"a b\"");
	}

	#[test]
	fn a_hung_user_command_is_cut_off() {
		let started = Instant::now();
		#[cfg(unix)]
		let command = "sleep 30";
		#[cfg(windows)]
		let command = if git_bash().is_some() { "sleep 30" } else { "ping -n 30 127.0.0.1 >NUL" };
		let out = run_user_command(command, b"", None, Duration::from_secs(1));
		assert!(out.is_empty());
		assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
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
		let v: Value = serde_json::from_str(&settings_arg(exe, sink, None)).unwrap();
		let line = v["statusLine"].as_object().unwrap();
		assert_eq!(line.len(), 2, "{line:?}");
		assert_eq!(line["type"], "command");
		let command = line["command"].as_str().unwrap();
		#[cfg(windows)]
		assert_eq!(
			command,
			"\"C:/Users/Fyruz DEV/factorai/factorai.exe\" --usage-statusline \
			 \"C:/Users/Fyruz DEV/AppData/Roaming/factorai/usage/statusline\""
		);
		#[cfg(unix)]
		assert_eq!(
			command,
			format!("\"{}\" --usage-statusline \"{}\"", exe.display(), sink.display())
		);
	}

	#[test]
	fn settings_arg_keeps_every_user_field_but_the_command() {
		let user =
			json!({"type": "command", "command": "mine", "padding": 1, "refreshInterval": 5});
		let v: Value = serde_json::from_str(&settings_arg(
			Path::new("/opt/factorai"),
			Path::new("/data/sink"),
			Some(&user),
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
