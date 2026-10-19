//! Codex's own answer to "how much is left": `codex app-server` speaks
//! newline-delimited JSON-RPC on stdio, and `account/rateLimits/read` returns
//! the live windows. Asking costs no quota.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use serde_json::{json, Value};

use super::child_guard::KillOnDrop;
use super::codex_log::place_by_length;
use crate::models::{ModelLimit, UsageReading, UsageSource, UsageWindow};

const INIT_ID: i64 = 1;
const READ_ID: i64 = 2;

/// One window object (`usedPercent`, `windowDurationMins`, `resetsAt` in Unix
/// seconds) with its length in minutes.
fn parse_window(w: &Value) -> Option<(i64, UsageWindow)> {
	if !w.is_object() {
		return None;
	}
	let used_percent = w.get("usedPercent").and_then(Value::as_f64)?;
	let minutes = w.get("windowDurationMins").and_then(Value::as_i64)?;
	let resets_at = w.get("resetsAt").and_then(Value::as_i64).map(|s| s * 1000);
	Some((minutes, UsageWindow { used_percent, resets_at }))
}

/// The reading in an `account/rateLimits/read` response. Windows are
/// classified by their length, not their slot. `None` for an error response or
/// one with no usable window.
pub fn parse_read_response(v: &Value, observed_at: i64) -> Option<UsageReading> {
	let result = v.get("result")?;
	let mut five_hour = None;
	let mut weekly = None;
	if let Some(limits) = result.get("rateLimits") {
		for slot in ["primary", "secondary"] {
			if let Some((minutes, window)) = limits.get(slot).and_then(parse_window) {
				place_by_length(minutes, window, &mut five_hour, &mut weekly);
			}
		}
	}
	let mut per_model = Vec::new();
	if let Some(by_id) = result.get("rateLimitsByLimitId").and_then(Value::as_object) {
		for (id, entry) in by_id.iter().filter(|(id, _)| id.as_str() != "codex") {
			let window = ["primary", "secondary"]
				.iter()
				.find_map(|slot| entry.get(*slot).and_then(parse_window))
				.map(|(_, w)| w);
			if let Some(window) = window {
				let label = entry.get("limitName").and_then(Value::as_str).unwrap_or(id);
				per_model.push(ModelLimit { label: label.to_string(), window });
			}
		}
	}
	if five_hour.is_none() && weekly.is_none() && per_model.is_empty() {
		return None;
	}
	Some(UsageReading {
		five_hour,
		weekly,
		per_model,
		source: UsageSource::CodexAppServer,
		observed_at,
	})
}

/// Ask `codex app-server` for the live limits of the profile at `codex_home`.
/// The child is killed before this returns, whatever the outcome.
pub fn read(
	codex_bin: &Path,
	codex_home: &Path,
	timeout: Duration,
) -> anyhow::Result<UsageReading> {
	let args: Vec<OsString> =
		["-s", "read-only", "-a", "never", "app-server"].iter().map(OsString::from).collect();
	let (program, args) =
		crate::services::agent_cli::launch_command(codex_bin, &args).map_err(|e| anyhow!("{e}"))?;
	let mut cmd = Command::new(program);
	cmd.args(args)
		.env("CODEX_HOME", codex_home)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::null());
	crate::services::child_env::changes_for_current_env().apply_to_command(&mut cmd);
	// The profile's home wins over whatever the app's own env carried.
	cmd.env("CODEX_HOME", codex_home);
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
	}
	#[cfg(unix)]
	{
		use std::os::unix::process::CommandExt;
		// Its own group, so the guard can kill the whole tree.
		cmd.process_group(0);
	}
	let mut child = KillOnDrop::new(cmd.spawn().context("could not start codex app-server")?);
	let mut stdin = child.child.stdin.take().context("no stdin")?;
	let stdout = child.child.stdout.take().context("no stdout")?;

	let (tx, rx) = mpsc::channel::<String>();
	std::thread::Builder::new().name("codex-app-server-read".into()).spawn(move || {
		for line in BufReader::new(stdout).lines().map_while(Result::ok) {
			if tx.send(line).is_err() {
				break;
			}
		}
	})?;

	let version = env!("CARGO_PKG_VERSION");
	let messages = [
		json!({"jsonrpc":"2.0","id":INIT_ID,"method":"initialize",
			"params":{"clientInfo":{"name":"factorai","version":version}}}),
		json!({"jsonrpc":"2.0","method":"initialized"}),
		json!({"jsonrpc":"2.0","id":READ_ID,"method":"account/rateLimits/read","params":{}}),
	];
	for m in &messages {
		writeln!(stdin, "{m}").context("could not write to codex app-server")?;
	}
	stdin.flush()?;

	let deadline = Instant::now() + timeout;
	loop {
		let left = deadline.saturating_duration_since(Instant::now());
		let line = rx
			.recv_timeout(left)
			.map_err(|_| anyhow!("codex app-server gave no limits within {timeout:?}"))?;
		let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
		if v.get("id").and_then(Value::as_i64) != Some(READ_ID) {
			continue;
		}
		if let Some(message) = v.pointer("/error/message").and_then(Value::as_str) {
			return Err(anyhow!("codex app-server refused the read: {message}"));
		}
		return parse_read_response(&v, super::now_ms())
			.ok_or_else(|| anyhow!("codex app-server returned no usable limits"));
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_primary_and_secondary() {
		let v = json!({"id":2,"result":{"rateLimits":{
			"primary":{"usedPercent":42.0,"windowDurationMins":300,"resetsAt":1790000000},
			"secondary":{"usedPercent":7.5,"windowDurationMins":10080,"resetsAt":1790600000}}}});
		let r = parse_read_response(&v, 5).unwrap();
		assert_eq!(r.source, UsageSource::CodexAppServer);
		assert_eq!(r.observed_at, 5);
		let five = r.five_hour.unwrap();
		assert_eq!(five.used_percent, 42.0);
		assert_eq!(five.resets_at, Some(1_790_000_000_000));
		assert_eq!(r.weekly.unwrap().used_percent, 7.5);
		assert!(r.per_model.is_empty());
	}

	#[test]
	fn per_limit_id_entries_become_per_model() {
		let v = json!({"id":2,"result":{
			"rateLimits":{"primary":{"usedPercent":1.0,"windowDurationMins":300,"resetsAt":1}},
			"rateLimitsByLimitId":{
				"codex":{"limitId":"codex","primary":{"usedPercent":1.0,"windowDurationMins":300,"resetsAt":1}},
				"codex_other":{"limitId":"codex_other","limitName":"GPT-5 Spark",
					"primary":{"usedPercent":60.0,"windowDurationMins":300,"resetsAt":2}},
				"bare":{"primary":{"usedPercent":3.0,"windowDurationMins":300,"resetsAt":2}}}}});
		let r = parse_read_response(&v, 0).unwrap();
		let mut labels: Vec<_> = r.per_model.iter().map(|m| m.label.as_str()).collect();
		labels.sort();
		assert_eq!(labels, ["GPT-5 Spark", "bare"]);
		assert_eq!(
			r.per_model.iter().find(|m| m.label == "GPT-5 Spark").unwrap().window.used_percent,
			60.0
		);
	}

	#[test]
	fn classifies_by_window_duration() {
		let v = json!({"id":2,"result":{"rateLimits":{
			"primary":{"usedPercent":80.0,"windowDurationMins":10080,"resetsAt":1790600000},
			"secondary":null}}});
		let r = parse_read_response(&v, 0).unwrap();
		assert!(r.five_hour.is_none());
		assert_eq!(r.weekly.unwrap().used_percent, 80.0);
	}

	#[test]
	fn error_response_is_none() {
		let v = json!({"id":2,"error":{"code":-32600,"message":"not logged in"}});
		assert!(parse_read_response(&v, 0).is_none());
	}

	/// A fake `codex` that starts a long-lived grandchild, records its pid in
	/// `$FACTORAI_TEST_PIDFILE`, and never answers.
	fn run_silent_fake(timeout: Duration) -> (anyhow::Result<UsageReading>, Duration, u32) {
		let dir = tempfile::tempdir().unwrap();
		let pidfile = dir.path().join("pid");
		std::env::set_var("FACTORAI_TEST_PIDFILE", &pidfile);
		#[cfg(windows)]
		let bin = {
			let bin = dir.path().join("codex.cmd");
			std::fs::write(
				&bin,
				"@echo off\r\npowershell -NoProfile -Command \"$p = Start-Process ping -ArgumentList '-n','120','127.0.0.1' -PassThru -WindowStyle Hidden; Set-Content -Path $env:FACTORAI_TEST_PIDFILE -Value $p.Id; Start-Sleep 120\"\r\n",
			)
			.unwrap();
			bin
		};
		#[cfg(unix)]
		let bin = {
			use std::os::unix::fs::PermissionsExt;
			let bin = dir.path().join("codex");
			std::fs::write(
				&bin,
				"#!/bin/sh\nsleep 120 &\necho $! > \"$FACTORAI_TEST_PIDFILE\"\nwait\n",
			)
			.unwrap();
			std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
			bin
		};
		let started = Instant::now();
		let result = read(&bin, dir.path(), timeout);
		let took = started.elapsed();
		let pid = std::fs::read_to_string(&pidfile)
			.ok()
			.and_then(|t| t.trim().trim_start_matches('\u{feff}').parse::<u32>().ok())
			.unwrap_or(0);
		(result, took, pid)
	}

	fn alive(pid: u32) -> bool {
		#[cfg(windows)]
		let out = Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).output();
		#[cfg(unix)]
		let out = Command::new("sh")
			.args(["-c", &format!("kill -0 {pid} 2>/dev/null && echo {pid}")])
			.output();
		out.map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string())).unwrap_or(false)
	}

	#[test]
	fn a_silent_server_times_out_and_its_descendants_are_killed() {
		let (result, took, pid) = run_silent_fake(Duration::from_secs(8));
		assert!(result.is_err());
		assert!(took < Duration::from_secs(15), "read overran its timeout: {took:?}");
		assert!(pid != 0, "the fake never recorded its grandchild");
		// Termination is asynchronous; give it a moment.
		let deadline = Instant::now() + Duration::from_secs(5);
		while alive(pid) && Instant::now() < deadline {
			std::thread::sleep(Duration::from_millis(100));
		}
		assert!(!alive(pid), "grandchild {pid} outlived the read");
	}

	#[test]
	#[ignore = "needs a logged-in Codex on this machine"]
	fn live_codex_app_server() {
		let bin = crate::services::agent_cli::find_agent_binary(
			crate::agents::descriptor(crate::agents::CODEX).unwrap(),
			None,
		)
		.unwrap();
		let home = std::env::var_os("CODEX_HOME")
			.map(std::path::PathBuf::from)
			.unwrap_or_else(|| dirs::home_dir().unwrap().join(".codex"));
		let reading = read(&bin, &home, Duration::from_secs(15)).unwrap();
		println!("{reading:#?}");
	}
}
