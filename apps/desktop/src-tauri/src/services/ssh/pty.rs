//! An SSH shell channel presented as a `portable_pty` master and child (F44).
//!
//! `TerminalManager::attach` already knows how to stream, resize, write to and
//! kill a PTY; giving it the same two traits for a remote shell is what lets an
//! SSH tab share the pooled xterm, the base64 events and kill-on-quit with
//! every other terminal, instead of growing a parallel path.
//!
//! One task owns the channel. The PTY threads talk to it through queues, so no
//! blocking thread ever touches the async channel and no lock is held across
//! an await.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};

use parking_lot::Mutex;
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use russh::client::Msg;
use russh::{Channel, ChannelMsg};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

enum Ctl {
	Data(Vec<u8>),
	Resize(u16, u16),
	Close,
}

/// Exit code reported when the channel closed without an exit status — the
/// connection dropped, or we closed it.
const NO_STATUS: u32 = 255;

/// Start the task that owns `channel` and return the PTY pair for it.
pub fn bridge(
	channel: Channel<Msg>,
	cols: u16,
	rows: u16,
) -> (Box<dyn MasterPty + Send>, Box<dyn Child + Send + Sync>) {
	let (ctl, mut ctl_rx) = unbounded_channel::<Ctl>();
	let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
	let (exit_tx, exit_rx) = mpsc::channel::<u32>();

	tauri::async_runtime::spawn(async move {
		let mut channel = channel;
		let mut code = None;
		loop {
			tokio::select! {
				c = ctl_rx.recv() => match c {
					Some(Ctl::Data(d)) => {
						if channel.data(&d[..]).await.is_err() { break; }
					}
					Some(Ctl::Resize(c, r)) => {
						let _ = channel.window_change(u32::from(c), u32::from(r), 0, 0).await;
					}
					Some(Ctl::Close) | None => {
						let _ = channel.close().await;
						break;
					}
				},
				m = channel.wait() => match m {
					Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
						if out_tx.send(data.to_vec()).is_err() { break; }
					}
					Some(ChannelMsg::ExitStatus { exit_status }) => code = Some(exit_status),
					Some(ChannelMsg::Close) | None => break,
					Some(_) => {}
				},
			}
		}
		drop(out_tx);
		let _ = exit_tx.send(code.unwrap_or(NO_STATUS));
	});

	let master = SshMaster {
		ctl: ctl.clone(),
		size: Mutex::new(PtySize { cols, rows, pixel_width: 0, pixel_height: 0 }),
		reader: Mutex::new(Some(ChanReader { rx: out_rx, buf: Vec::new(), pos: 0 })),
	};
	let child = SshChild { ctl, exit: Mutex::new(exit_rx), done: None };
	(Box::new(master), Box::new(child))
}

struct SshMaster {
	ctl: UnboundedSender<Ctl>,
	size: Mutex<PtySize>,
	/// Handed out once: `attach` clones exactly one reader, and a second reader
	/// on the same queue would steal half of the bytes.
	reader: Mutex<Option<ChanReader>>,
}

impl MasterPty for SshMaster {
	fn resize(&self, size: PtySize) -> anyhow::Result<()> {
		*self.size.lock() = size;
		self.ctl
			.send(Ctl::Resize(size.cols, size.rows))
			.map_err(|_| anyhow::anyhow!("ssh channel closed"))
	}

	fn get_size(&self) -> anyhow::Result<PtySize> {
		Ok(*self.size.lock())
	}

	fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
		let r = self.reader.lock().take().ok_or_else(|| anyhow::anyhow!("reader already taken"))?;
		Ok(Box::new(r))
	}

	fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
		Ok(Box::new(ChanWriter(self.ctl.clone())))
	}

	#[cfg(unix)]
	fn process_group_leader(&self) -> Option<libc::pid_t> {
		None
	}

	#[cfg(unix)]
	fn as_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
		None
	}
}

struct ChanReader {
	rx: Receiver<Vec<u8>>,
	buf: Vec<u8>,
	pos: usize,
}

impl Read for ChanReader {
	fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
		if self.pos >= self.buf.len() {
			match self.rx.recv() {
				Ok(b) => {
					self.buf = b;
					self.pos = 0;
				}
				// The channel task ended: end of stream, as a closed PTY reads.
				Err(_) => return Ok(0),
			}
		}
		let n = out.len().min(self.buf.len() - self.pos);
		out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
		self.pos += n;
		Ok(n)
	}
}

struct ChanWriter(UnboundedSender<Ctl>);

impl Write for ChanWriter {
	fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
		self.0.send(Ctl::Data(data.to_vec())).map_err(|_| {
			std::io::Error::new(std::io::ErrorKind::BrokenPipe, "ssh channel closed")
		})?;
		Ok(data.len())
	}

	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}

#[derive(Debug)]
struct SshKiller(UnboundedSender<Ctl>);

impl ChildKiller for SshKiller {
	/// Closing the channel hangs up the remote shell (SIGHUP to its session),
	/// which is the remote half of kill-on-quit (ADR-0005).
	fn kill(&mut self) -> std::io::Result<()> {
		let _ = self.0.send(Ctl::Close);
		Ok(())
	}

	fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
		Box::new(SshKiller(self.0.clone()))
	}
}

struct SshChild {
	ctl: UnboundedSender<Ctl>,
	/// Behind a mutex only because `Child` must be `Sync` and a `Receiver` is
	/// not; the waiter thread is its one caller.
	exit: Mutex<Receiver<u32>>,
	done: Option<u32>,
}

impl std::fmt::Debug for SshChild {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("SshChild")
	}
}

impl ChildKiller for SshChild {
	fn kill(&mut self) -> std::io::Result<()> {
		let _ = self.ctl.send(Ctl::Close);
		Ok(())
	}

	fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
		Box::new(SshKiller(self.ctl.clone()))
	}
}

impl Child for SshChild {
	fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
		if self.done.is_none() {
			self.done = self.exit.lock().try_recv().ok();
		}
		Ok(self.done.map(ExitStatus::with_exit_code))
	}

	fn wait(&mut self) -> std::io::Result<ExitStatus> {
		if self.done.is_none() {
			let code = self.exit.lock().recv().unwrap_or(NO_STATUS);
			self.done = Some(code);
		}
		Ok(ExitStatus::with_exit_code(self.done.unwrap_or(NO_STATUS)))
	}

	fn process_id(&self) -> Option<u32> {
		None
	}

	#[cfg(windows)]
	fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
		None
	}
}
