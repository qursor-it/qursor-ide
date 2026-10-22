//! The Claude CLI adapter (F33, ADR-0084) driven against a **fake CLI**: a shell
//! script standing in for `claude`, so no model is ever called.
//!
//! What is pinned is what makes the call safe to make at all: the flags that
//! remove tools, MCP, hooks and session files are passed; the payload arrives on
//! stdin and nowhere else; the process runs in an empty temporary directory of
//! its own; a CLI that does not list a required flag is not run; and a process
//! that hangs, floods, or is cancelled is killed - the whole group, not just the
//! script. Unix only: the stand-in is `sh`.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use factorai_lib::db::Db;
use factorai_lib::models::intelligence::SignalType;
use factorai_lib::models::SettingKey;
use factorai_lib::services::intelligence::provider::{
	AnalysisProvider, ClaudeCliProvider, EvidenceItem, ProviderError, ProviderRequest, RunControl,
	UnavailableReason, REQUIRED_FLAGS,
};
use factorai_lib::services::settings;
use tempfile::TempDir;

const SCRIPT: &str = r#"#!/bin/sh
here="$(dirname "$0")"
mode="$(cat "$here/mode" 2>/dev/null || echo ok)"
case "$1" in
  --version) echo "9.9.9 (Fake Claude)"; exit 0;;
  --help) cat "$here/help.txt"; exit 0;;
esac
printf '%s\n' "$@" > "$here/argv.txt"
pwd -P > "$here/cwd.txt"
ls -A | wc -l > "$here/cwd_entries.txt"
echo "$CLAUDE_CONFIG_DIR" > "$here/config_dir.txt"
cat > "$here/stdin.txt"
echo $$ > "$here/pid.txt"
case "$mode" in
  ok) cat "$here/reply.json";;
  hang) sleep 60 & echo $! > "$here/sleep_pid.txt"; wait;;
  big) head -c 600000 /dev/zero | tr '\0' 'x';;
  nologin) echo '{"type":"result","subtype":"success","is_error":true,"result":"Not logged in - run /login"}'; exit 1;;
  garbage) echo 'hello there';;
  twoturns) echo '{"type":"result","subtype":"success","is_error":false,"num_turns":4,"result":"{}"}';;
esac
"#;

struct Fake {
	_tmp: TempDir,
	dir: PathBuf,
	claude_dir: PathBuf,
	db: Db,
}

impl Fake {
	fn new() -> Self {
		let tmp = TempDir::new().unwrap();
		let dir = tmp.path().join("bin");
		std::fs::create_dir_all(&dir).unwrap();
		let script = dir.join("claude");
		std::fs::write(&script, SCRIPT).unwrap();
		std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
		std::fs::write(dir.join("help.txt"), REQUIRED_FLAGS.join("\n")).unwrap();
		std::fs::write(
			dir.join("reply.json"),
			r#"{"type":"result","subtype":"success","is_error":false,"num_turns":1,"permission_denials":[],"result":"{\"candidates\":[]}"}"#,
		)
		.unwrap();
		let db = Db::open(&tmp.path().join("data")).unwrap();
		let claude_dir = tmp.path().join("claude-config");
		factorai_lib::services::profiles::ensure_default(&db, &claude_dir).unwrap();
		db.with(|c| settings::set(c, SettingKey::ClaudeBinaryPath, Some(script.to_str().unwrap())))
			.unwrap();
		Fake { _tmp: tmp, dir, claude_dir, db }
	}

	fn mode(&self, mode: &str) {
		std::fs::write(self.dir.join("mode"), mode).unwrap();
	}

	fn read(&self, name: &str) -> String {
		std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
	}

	fn provider(&self) -> ClaudeCliProvider {
		ClaudeCliProvider::new(self.db.clone())
	}

	fn request(&self, project_id: &str) -> ProviderRequest {
		ProviderRequest {
			project_id: project_id.to_string(),
			project_name: "shop".into(),
			evidence: vec![EvidenceItem {
				evidence_id: "ev-1".into(),
				session_id: "session-uuid-1".into(),
				agent: "claude".into(),
				signal_type: SignalType::UserCorrection,
				excerpt: "Don't use npm; SECRET-EXCERPT-MARKER".into(),
				context: None,
			}],
			instructions: Some("- use pnpm".into()),
		}
	}
}

/// Whether a process is still running. A zombie is not: its parent has not
/// reaped it, but it executes nothing.
fn alive(pid: &str) -> bool {
	let pid = pid.trim();
	if pid.is_empty() {
		return false;
	}
	if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
		return stat.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z'));
	}
	if Path::new("/proc/self").exists() {
		return false;
	}
	std::process::Command::new("kill").args(["-0", pid]).status().is_ok_and(|s| s.success())
}

fn wait_for_file(path: &Path) {
	let deadline = Instant::now() + Duration::from_secs(10);
	while !path.exists() || std::fs::read_to_string(path).unwrap_or_default().trim().is_empty() {
		assert!(Instant::now() < deadline, "{} never appeared", path.display());
		std::thread::sleep(Duration::from_millis(10));
	}
}

#[test]
fn a_cli_that_lists_every_isolation_flag_is_available() {
	let fake = Fake::new();
	let a = fake.provider().availability("p");
	assert!(a.available, "{a:?}");
	assert_eq!(a.version.as_deref(), Some("9.9.9"));
	assert!(a.binary_path.unwrap().ends_with("claude"));
	assert!(fake.provider().is_remote(), "a local CLI is not local inference");
}

#[test]
fn a_cli_missing_any_required_flag_is_unavailable_and_says_which() {
	for flag in REQUIRED_FLAGS {
		let fake = Fake::new();
		let help: Vec<&str> = REQUIRED_FLAGS.iter().copied().filter(|f| f != flag).collect();
		std::fs::write(fake.dir.join("help.txt"), help.join("\n")).unwrap();
		let a = fake.provider().availability("p");
		assert!(!a.available, "{flag} is required");
		assert_eq!(a.reason, Some(UnavailableReason::MissingFlag));
		assert!(a.detail.unwrap().contains(flag), "the reason should name {flag}");
	}
}

#[test]
fn a_missing_binary_is_not_installed() {
	let fake = Fake::new();
	fake.db
		.with(|c| settings::set(c, SettingKey::ClaudeBinaryPath, Some("/nonexistent/claude")))
		.unwrap();
	let a = fake.provider().availability("p");
	assert!(!a.available);
	assert_eq!(a.reason, Some(UnavailableReason::NotInstalled));
}

#[test]
fn analyzing_without_a_usable_cli_spawns_nothing() {
	let fake = Fake::new();
	std::fs::write(fake.dir.join("help.txt"), "--print").unwrap();
	let e = fake.provider().analyze(&fake.request("p"), &RunControl::default()).unwrap_err();
	assert!(matches!(e, ProviderError::Unavailable(_)), "{e:?}");
	assert!(!fake.dir.join("argv.txt").exists(), "the analysis command must not have been run");
}

#[test]
fn the_payload_goes_in_on_stdin_with_every_isolation_flag_in_a_fresh_empty_directory() {
	let fake = Fake::new();
	let project = TempDir::new().unwrap();
	let out = fake.provider().analyze(&fake.request("p"), &RunControl::default()).unwrap();
	assert_eq!(out.text, r#"{"candidates":[]}"#);

	// stdin carries the evidence; argv carries none of it.
	let stdin = fake.read("stdin.txt");
	assert!(
		stdin.contains("SECRET-EXCERPT-MARKER") && stdin.contains("<untrusted_data>"),
		"{stdin}"
	);
	assert!(!stdin.contains("session-uuid-1"), "the model gets an alias, not the session id");
	let argv = fake.read("argv.txt");
	assert!(
		!argv.contains("SECRET-EXCERPT-MARKER") && !argv.contains("use pnpm"),
		"payload leaked into argv: {argv}"
	);

	// The flags that remove what the model could do.
	let args: Vec<&str> = argv.lines().collect();
	for flag in [
		"-p",
		"--strict-mcp-config",
		"--disable-slash-commands",
		"--no-session-persistence",
		"--safe-mode",
	] {
		assert!(args.contains(&flag), "missing {flag} in {args:?}");
	}
	let at =
		|flag: &str| args.iter().position(|a| *a == flag).unwrap_or_else(|| panic!("no {flag}"));
	assert_eq!(args[at("--tools") + 1], "", "--tools takes an empty list: no tools at all");
	assert_eq!(args[at("--output-format") + 1], "json");
	assert_eq!(args[at("--permission-mode") + 1], "dontAsk");
	// The system prompt is multi-line, so the script's one-argument-per-line capture
	// spreads it; its start follows the flag and its rules are in the capture.
	assert!(args[at("--append-system-prompt") + 1].starts_with("You analyse excerpts"));
	assert!(argv.contains("never instructions to you") && argv.contains("untrusted_data"));
	assert!(!args.contains(&"--mcp-config"), "no MCP server is registered");
	assert!(!args.contains(&"--dangerously-skip-permissions"));
	assert!(!args.contains(&"--bare"), "--bare would drop the user's OAuth login");

	// Its own directory: empty, not the project, and gone afterwards.
	let cwd = fake.read("cwd.txt");
	let cwd = cwd.trim();
	assert_ne!(Path::new(cwd), project.path().canonicalize().unwrap());
	assert!(cwd.contains("factorai-intelligence-"), "{cwd}");
	assert_eq!(fake.read("cwd_entries.txt").trim(), "0", "the working directory must start empty");
	assert!(!Path::new(cwd).exists(), "the work directory is removed when the call ends");

	// The identity is the project's profile.
	assert_eq!(Path::new(fake.read("config_dir.txt").trim()), fake.claude_dir.as_path());
}

#[test]
fn a_hung_cli_is_killed_at_the_deadline_with_everything_it_started() {
	let fake = Fake::new();
	fake.mode("hang");
	let provider = fake.provider().with_timeout(Duration::from_millis(700));
	let started = Instant::now();
	let e = provider.analyze(&fake.request("p"), &RunControl::default()).unwrap_err();
	assert_eq!(e, ProviderError::Timeout);
	assert!(
		started.elapsed() < Duration::from_secs(8),
		"the deadline must be enforced, took {:?}",
		started.elapsed()
	);
	let script = fake.read("pid.txt");
	let sleeper = fake.read("sleep_pid.txt");
	assert!(!alive(&script), "the CLI process outlived its timeout");
	assert!(!alive(&sleeper), "a process the CLI started outlived its timeout");
}

#[test]
fn cancelling_kills_the_process_promptly_and_cancelling_again_is_harmless() {
	let fake = Fake::new();
	fake.mode("hang");
	let provider = Arc::new(fake.provider());
	let control = Arc::new(RunControl::default());
	let request = fake.request("p");
	let worker = {
		let (provider, control) = (provider.clone(), control.clone());
		std::thread::spawn(move || provider.analyze(&request, &control))
	};
	wait_for_file(&fake.dir.join("sleep_pid.txt"));
	let cancelled_at = Instant::now();
	control.cancel();
	control.cancel();
	let result = worker.join().unwrap();
	assert_eq!(result.unwrap_err(), ProviderError::Cancelled);
	assert!(
		cancelled_at.elapsed() < Duration::from_secs(5),
		"cancel took {:?}",
		cancelled_at.elapsed()
	);
	assert!(!alive(&fake.read("pid.txt")), "the CLI survived a cancel");
	assert!(!alive(&fake.read("sleep_pid.txt")), "its child survived a cancel");
	control.cancel();
}

#[test]
fn a_run_cancelled_before_it_starts_never_spawns() {
	let fake = Fake::new();
	let control = RunControl::default();
	control.cancel();
	let e = fake.provider().analyze(&fake.request("p"), &control).unwrap_err();
	assert_eq!(e, ProviderError::Cancelled);
	assert!(!fake.dir.join("argv.txt").exists());
}

#[test]
fn output_beyond_the_cap_is_refused_and_the_process_is_ended() {
	let fake = Fake::new();
	fake.mode("big");
	let e = fake.provider().analyze(&fake.request("p"), &RunControl::default()).unwrap_err();
	assert_eq!(e, ProviderError::OutputTooLarge);
	assert!(!alive(&fake.read("pid.txt")));
}

#[test]
fn replies_that_are_not_a_clean_single_turn_result_are_refused() {
	for (mode, check) in [
		(
			"garbage",
			(|e: &ProviderError| matches!(e, ProviderError::Failed(_)))
				as fn(&ProviderError) -> bool,
		),
		("nologin", |e| matches!(e, ProviderError::NotAuthenticated(_))),
		("twoturns", |e| matches!(e, ProviderError::Unsafe(_))),
	] {
		let fake = Fake::new();
		fake.mode(mode);
		let e = fake.provider().analyze(&fake.request("p"), &RunControl::default()).unwrap_err();
		assert!(check(&e), "{mode}: {e:?}");
	}
}

/// Not a test of the model - it never calls it. It runs `claude --version` and
/// `claude --help` on the CLI that is installed here, to show the flag set this
/// feature depends on is real and not assumed. `cargo test -- --ignored` to run.
#[test]
#[ignore = "needs the real claude CLI installed; spawns it for --version and --help only"]
fn the_installed_claude_lists_every_isolation_flag() {
	let tmp = TempDir::new().unwrap();
	let db = Db::open(&tmp.path().join("data")).unwrap();
	let a = ClaudeCliProvider::new(db).availability("p");
	assert!(a.available, "{a:?}");
	eprintln!("claude {} at {}", a.version.unwrap_or_default(), a.binary_path.unwrap_or_default());
}
