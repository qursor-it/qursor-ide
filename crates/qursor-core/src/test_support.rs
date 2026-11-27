#[cfg(unix)]
use std::future::Future;
use std::sync::OnceLock;

use parking_lot::{Mutex, MutexGuard};

static SPAWN: Mutex<()> = Mutex::new(());

/// Held by every test that starts a shell (PowerShell, Git Bash, cmd, a
/// ConPTY child, the fake `codex`). Their start-up is CPU-bound and their
/// assertions are timed, so they run one at a time while the rest of the
/// suite stays parallel.
pub fn spawn_lock() -> MutexGuard<'static, ()> {
	SPAWN.lock()
}

/// One runtime for the whole test binary, the way the app has one: a test
/// thread has none of its own, and what a test starts has to outlive it.
pub fn runtime() -> &'static tokio::runtime::Runtime {
	static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
	RT.get_or_init(|| tokio::runtime::Runtime::new().expect("test runtime"))
}

pub fn handle() -> tokio::runtime::Handle {
	runtime().handle().clone()
}

#[cfg(unix)]
pub fn block_on<F: Future>(f: F) -> F::Output {
	runtime().block_on(f)
}
