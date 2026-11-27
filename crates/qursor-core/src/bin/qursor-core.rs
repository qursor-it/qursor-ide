//! `qursor-core --data-dir <path> [--parent-pid <pid>]`: the headless process
//! that owns every terminal (docs/superpowers/specs/2026-11-26-local-core-design.md §1).
//!
//! Prints one line, `{"ready":true,"port":N}`, once it listens, then nothing.
//! Exit codes: 2 bad arguments; 3 another core holds the data folder's lock;
//! 4 the database is missing or not migrated to this core's schema (only the
//! app migrates, design §5); 1 anything else.

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
			if error.downcast_ref::<qursor_core::db::NotReady>().is_some() {
				ExitCode::from(4)
			} else {
				ExitCode::FAILURE
			}
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

/// Nobody reads stdout after `ready`: the app drops its end of the pipe, and a
/// child that inherited it would get EPIPE or SIGPIPE writing there.
#[cfg(unix)]
fn stdout_to_null() -> std::io::Result<()> {
	use std::os::fd::AsRawFd;
	let null = std::fs::File::options().write(true).open("/dev/null")?;
	// SAFETY: both descriptors are open; dup2 replaces fd 1 atomically.
	if unsafe { libc::dup2(null.as_raw_fd(), 1) } < 0 {
		return Err(std::io::Error::last_os_error());
	}
	Ok(())
}

#[cfg(windows)]
fn stdout_to_null() -> std::io::Result<()> {
	use std::os::windows::io::IntoRawHandle;
	use windows_sys::Win32::System::Console::{SetStdHandle, STD_OUTPUT_HANDLE};
	let null = std::fs::File::options().write(true).open("NUL")?;
	// The handle stays open for the life of the process, as stdout's does.
	// SAFETY: a valid handle we own and hand over.
	if unsafe { SetStdHandle(STD_OUTPUT_HANDLE, null.into_raw_handle()) } == 0 {
		return Err(std::io::Error::last_os_error());
	}
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
		if let Err(error) = stdout_to_null() {
			tracing::warn!(%error, "could not point stdout at the null device");
		}
		let _ = server.exited.await;
		record::remove(&dir);
		anyhow::Ok(())
	});
	// The terminals are dead by now; nothing left on the runtime is worth waiting for.
	runtime.shutdown_background();
	result.map(|()| ExitCode::SUCCESS)
}
