//! A child process that is killed, with everything it started, on every exit
//! path. Shared by the usage sources that run other programs.

use std::process::Child;

/// Kills the child and everything it started on every exit path, including a
/// panic or early return. An npm shim is powershell -> cmd -> node -> codex, so
/// killing only the direct child would orphan the real app-server. On Unix the
/// caller must start the child in its own process group (`process_group(0)`).
pub(crate) struct KillOnDrop {
	pub(crate) child: Child,
	/// Windows: the job the whole tree lives in; closing it kills the tree.
	#[cfg(windows)]
	job: Option<std::os::windows::io::OwnedHandle>,
	/// Cleared by `release`: the tree is left to run.
	armed: bool,
}

impl KillOnDrop {
	pub(crate) fn new(child: Child) -> Self {
		#[cfg(windows)]
		{
			let job = kill_on_close_job(&child);
			Self { child, job, armed: true }
		}
		#[cfg(not(windows))]
		Self { child, armed: true }
	}

	/// Leave the child and its descendants alone on drop: for a child that
	/// finished on its own and may have started background work on purpose.
	pub(crate) fn release(&mut self) {
		self.armed = false;
		#[cfg(windows)]
		if let Some(job) = &self.job {
			// Without this, closing the handle would still kill the tree.
			set_kill_on_close(job, false);
		}
	}
}

impl Drop for KillOnDrop {
	fn drop(&mut self) {
		#[cfg(windows)]
		drop(self.job.take());
		if !self.armed {
			return;
		}
		#[cfg(unix)]
		// SAFETY: plain signal to the process group this child leads.
		unsafe {
			libc::kill(-(self.child.id() as i32), libc::SIGKILL);
		}
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}

/// Turn the job's kill-on-close limit on or off. Returns whether it took.
#[cfg(windows)]
fn set_kill_on_close(job: &std::os::windows::io::OwnedHandle, on: bool) -> bool {
	use std::os::windows::io::AsRawHandle;
	use windows_sys::Win32::Foundation::HANDLE;
	use windows_sys::Win32::System::JobObjects::{
		JobObjectExtendedLimitInformation, SetInformationJobObject,
		JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
	};
	let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
	if on {
		limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
	}
	// SAFETY: a valid job handle and a correctly sized limits struct.
	unsafe {
		SetInformationJobObject(
			job.as_raw_handle() as HANDLE,
			JobObjectExtendedLimitInformation,
			(&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
			std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
		) != 0
	}
}

/// A job object with kill-on-close holding `child`; descendants spawned after
/// the assignment join it. `None` if the job could not be set up (the direct
/// child is still killed).
#[cfg(windows)]
fn kill_on_close_job(child: &Child) -> Option<std::os::windows::io::OwnedHandle> {
	use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
	use windows_sys::Win32::Foundation::HANDLE;
	use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};
	// SAFETY: plain Win32 calls; the raw handle is wrapped immediately so it is
	// closed on every path.
	unsafe {
		let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
		if raw.is_null() {
			return None;
		}
		let job = OwnedHandle::from_raw_handle(raw);
		if !set_kill_on_close(&job, true) {
			return None;
		}
		let assigned = AssignProcessToJobObject(
			job.as_raw_handle() as HANDLE,
			child.as_raw_handle() as HANDLE,
		);
		(assigned != 0).then_some(job)
	}
}
