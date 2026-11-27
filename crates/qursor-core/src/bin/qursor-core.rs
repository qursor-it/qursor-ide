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
			eprintln!("qursor-core: {error:#}");
			ExitCode::FAILURE
		}
	}
}

fn run(data_dir: PathBuf, parent_pid: Option<u32>) -> anyhow::Result<ExitCode> {
	let dir = record::core_dir(&data_dir)?;
	// Held until the process ends.
	let Some(_lock) = record::lock(&dir)? else {
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
