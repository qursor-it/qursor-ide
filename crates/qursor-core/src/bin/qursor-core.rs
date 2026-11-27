//! `qursor-core --data-dir <path> [--parent-pid <pid>]`: the headless process
//! that owns every terminal (docs/superpowers/specs/2026-11-26-local-core-design.md §1).
//!
//! Prints one line, `{"ready":true,"port":N}`, once it listens, then nothing.
//! Exits 3 when another core holds the data folder's lock.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use qursor_core::protocol::PROTOCOL_VERSION;
use qursor_core::server::{self, record, Config, GRACE};

struct Args {
	data_dir: PathBuf,
	parent_pid: Option<u32>,
}

fn args() -> Option<Args> {
	let mut data_dir = None;
	let mut parent_pid = None;
	let mut it = std::env::args_os().skip(1);
	while let Some(arg) = it.next() {
		match arg.to_str()? {
			"--data-dir" => data_dir = Some(PathBuf::from(it.next()?)),
			"--parent-pid" => parent_pid = Some(it.next()?.to_str()?.parse().ok()?),
			_ => return None,
		}
	}
	Some(Args { data_dir: data_dir?, parent_pid })
}

fn main() -> ExitCode {
	let Some(Args { data_dir, parent_pid }) = args() else {
		eprintln!("usage: qursor-core --data-dir <path> [--parent-pid <pid>]");
		return ExitCode::from(2);
	};
	match run(data_dir, parent_pid) {
		Ok(code) => code,
		Err(error) => {
			tracing::error!("{error:#}");
			eprintln!("qursor-core: {error:#}");
			ExitCode::FAILURE
		}
	}
}

/// Appends to `<data_dir>/core/core.log`: a detached process has no terminal,
/// so stderr goes nowhere. Same filter as the app's: `RUST_LOG`, else `info`.
fn init_log(dir: &std::path::Path) -> anyhow::Result<()> {
	let file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("core.log"))?;
	tracing_subscriber::fmt()
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
		)
		.with_ansi(false)
		.with_writer(std::sync::Mutex::new(file))
		.init();
	Ok(())
}

fn run(data_dir: PathBuf, parent_pid: Option<u32>) -> anyhow::Result<ExitCode> {
	let dir = record::core_dir(&data_dir)?;
	init_log(&dir)?;
	// Held until the process ends.
	let Some(_lock) = record::lock(&dir)? else {
		tracing::warn!("another core already serves this data folder");
		eprintln!("qursor-core: another core already serves {}", data_dir.display());
		return Ok(ExitCode::from(3));
	};
	let runtime = tokio::runtime::Runtime::new()?;
	let result = runtime.block_on(async {
		let server = server::start(Config { data_dir, grace: GRACE, parent_pid }).await?;
		record::write(
			&dir,
			&record::Record {
				pid: std::process::id(),
				protocol_version: PROTOCOL_VERSION,
				app_version: env!("CARGO_PKG_VERSION").into(),
				port: server.port,
				secret: server.secret.clone(),
			},
		)?;
		let mut out = std::io::stdout().lock();
		writeln!(out, "{}", serde_json::json!({ "ready": true, "port": server.port }))?;
		out.flush()?;
		drop(out);
		let _ = server.exited.await;
		record::remove(&dir);
		anyhow::Ok(())
	});
	// The terminals are dead by now; nothing left on the runtime is worth waiting for.
	runtime.shutdown_background();
	result.map(|()| ExitCode::SUCCESS)
}
