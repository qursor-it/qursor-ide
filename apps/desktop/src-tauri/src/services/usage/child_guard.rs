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
}

impl KillOnDrop {
	pub(crate) fn new(child: Child) -> Self {
		#[cfg(windows)]
		{
			let job = kill_on_close_job(&child);
			Self { child, job }
		}
		#[cfg(not(windows))]
		Self { child }
	}
}

impl Drop for KillOnDrop {
	fn drop(&mut self) {
		#[cfg(windows)]
		drop(self.job.take());
		#[cfg(unix)]
		// SAFETY: plain signal to the process group this child leads.
		unsafe {
			libc::kill(-(self.child.id() as i32), libc::SIGKILL);
		}
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}

/// A job object with kill-on-close holding `child`; descendants spawned after
/// the assignment join it. `None` if the job could not be set up (the direct
/// child is still killed).
#[cfg(windows)]
fn kill_on_close_job(child: &Child) -> Option<std::os::windows::io::OwnedHandle> {
	use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
	use windows_sys::Win32::Foundation::HANDLE;
	use windows_sys::Win32::System::JobObjects::{
		AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
		SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
		JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
	};
	// SAFETY: plain Win32 calls; the raw handle is wrapped immediately so it is
	// closed on every path.
	unsafe {
		let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
		if raw.is_null() {
			return None;
		}
		let job = OwnedHandle::from_raw_handle(raw);
		let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
		limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
		let set = SetInformationJobObject(
			job.as_raw_handle() as HANDLE,
			JobObjectExtendedLimitInformation,
			(&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
			std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
		);
		if set == 0 {
			return None;
		}
		let assigned = AssignProcessToJobObject(
			job.as_raw_handle() as HANDLE,
			child.as_raw_handle() as HANDLE,
		);
		(assigned != 0).then_some(job)
	}
}
