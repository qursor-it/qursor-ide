//! What an agent needs to use a terminal it did not start (F45, ADR-0107):
//! where a command's output begins and ends, its exit status, the shell's
//! directory afterwards, and what is on the screen right now.
//!
//! **Markers, not guesses.** Each command an agent runs is wrapped between two
//! invisible `OSC 633` sequences carrying a nonce — the same family VS Code and
//! Windows Terminal use for shell integration. xterm.js ignores an OSC it does
//! not know, so the human sees the command line and its output, nothing else.
//! The shell functions that print them are defined in the session by a one-line
//! bootstrap the first time an agent uses that terminal; nothing is written to
//! the server's disk.
//!
//! Pure and synchronous: the reader thread feeds bytes in, the tool server reads
//! the state out. Nothing here touches a PTY.

use std::collections::HashMap;
use std::num::NonZeroU16;

/// Bytes kept of a running command's output; past this the head is dropped and
/// the result says so.
const MAX_OUTPUT: usize = 512 * 1024;
/// An unterminated escape sequence held across reads, capped so a stream that
/// never terminates one cannot grow it without bound.
const MAX_CARRY: usize = 4096;
const SCROLLBACK: usize = 2000;

/// The bootstrap: two shell functions, sent once per shell. `$?` at the entry
/// of `__qe` is still the status of the command before it, in bash, zsh, dash
/// and busybox sh. Leading space: kept out of history where `HISTCONTROL`
/// honours it.
pub const BOOTSTRAP: &str = " __qs(){ printf '\\033]633;C;%s\\007' \"$1\"; }; __qe(){ printf '\\033]633;D;%s;%s;%s\\007' \"$1\" \"$?\" \"$PWD\"; }\n";

/// What is typed for one agent command. The command sits in a `{ …\n}` group
/// so a trailing comment, `&` or heredoc in it cannot swallow the end marker,
/// and the whole group is parsed before anything runs — a command that reads
/// stdin never eats the marker line.
pub fn wrap(nonce: &str, command: &str) -> String {
	format!(" __qs {nonce}; {{ {}\n}}; __qe {nonce}\n", command.trim_end())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
	pub exit: i32,
	pub cwd: String,
	pub output: String,
	/// The output was longer than [`MAX_OUTPUT`] and its head was dropped.
	pub truncated: bool,
}

#[derive(Default)]
struct Pending {
	nonce: String,
	started: bool,
	out: Vec<u8>,
	truncated: bool,
}

pub struct Capture {
	screen: vt100::Parser,
	carry: Vec<u8>,
	pending: Option<Pending>,
	finished: HashMap<String, Finished>,
	/// The shell answered a bootstrap with its marker: functions are defined.
	pub bootstrapped: bool,
	/// The directory the last finished command left the shell in.
	pub cwd: Option<String>,
}

fn nz(v: u16, min: u16) -> NonZeroU16 {
	NonZeroU16::new(v.max(min)).unwrap_or(NonZeroU16::MIN)
}

impl Capture {
	pub fn new(cols: u16, rows: u16) -> Self {
		Self {
			screen: vt100::Parser::new(nz(rows, 5), nz(cols, 20), SCROLLBACK),
			carry: Vec::new(),
			pending: None,
			finished: HashMap::new(),
			bootstrapped: false,
			cwd: None,
		}
	}

	pub fn resize(&mut self, cols: u16, rows: u16) {
		self.screen.set_size(nz(rows, 5), nz(cols, 20));
	}

	/// Start waiting for the markers of `nonce`.
	pub fn expect(&mut self, nonce: &str) {
		self.pending = Some(Pending { nonce: nonce.to_string(), ..Default::default() });
	}

	pub fn take_finished(&mut self, nonce: &str) -> Option<Finished> {
		self.finished.remove(nonce)
	}

	/// The output so far of the command being waited on, if it has started.
	pub fn partial(&self, nonce: &str) -> Option<String> {
		self.pending
			.as_ref()
			.filter(|p| p.nonce == nonce && p.started)
			.map(|p| clean(&String::from_utf8_lossy(&p.out)))
	}

	/// A command is wrapped and has not printed its end marker yet.
	pub fn has_pending(&self) -> bool {
		self.pending.is_some()
	}

	/// Forget the command in flight: an interrupt ended it before its marker.
	pub fn clear_pending(&mut self) {
		self.pending = None;
	}

	pub fn cancel(&mut self, nonce: &str) {
		if self.pending.as_ref().is_some_and(|p| p.nonce == nonce) {
			self.pending = None;
		}
	}

	/// A full-screen program (an editor, a pager, `top`) owns the terminal.
	pub fn in_full_screen_program(&self) -> bool {
		self.screen.screen().alternate_screen()
	}

	/// The visible screen as text, one line per row, trailing blanks trimmed.
	pub fn screen_text(&self) -> String {
		let s = self.screen.screen().contents();
		let lines: Vec<&str> = s.lines().map(str::trim_end).collect();
		let end = lines.iter().rposition(|l| !l.is_empty()).map_or(0, |i| i + 1);
		lines[..end].join("\n")
	}

	/// The line the cursor is on, as the shell or program last drew it.
	pub fn cursor_line(&self) -> String {
		let screen = self.screen.screen();
		let (row, _) = screen.cursor_position();
		let (_, cols) = screen.size();
		screen.contents_between(row, 0, row, cols.get()).trim_end().to_string()
	}

	/// The screen is asking for a secret: the agent must stop and the human type
	/// it (F45). Judged on the cursor's line, where a prompt waits.
	pub fn asks_for_secret(&self) -> bool {
		looks_like_secret_prompt(&self.cursor_line())
	}

	/// Feed bytes from the PTY.
	pub fn feed(&mut self, bytes: &[u8]) {
		self.screen.process(bytes);
		let mut buf = std::mem::take(&mut self.carry);
		buf.extend_from_slice(bytes);
		let mut i = 0;
		let mut plain_from = 0;
		while i < buf.len() {
			if buf[i] == 0x1b && buf.get(i + 1) == Some(&b']') {
				let Some((end, term_len)) = osc_end(&buf, i + 2) else {
					// Unterminated: keep it for the next read.
					self.push_output(&buf[plain_from..i]);
					if buf.len() - i <= MAX_CARRY {
						self.carry = buf[i..].to_vec();
					}
					return;
				};
				self.push_output(&buf[plain_from..i]);
				self.on_osc(&buf[i + 2..end]);
				i = end + term_len;
				plain_from = i;
				continue;
			}
			i += 1;
		}
		self.push_output(&buf[plain_from..]);
	}

	fn push_output(&mut self, bytes: &[u8]) {
		let Some(p) = self.pending.as_mut().filter(|p| p.started) else { return };
		p.out.extend_from_slice(bytes);
		if p.out.len() > MAX_OUTPUT {
			let cut = p.out.len() - MAX_OUTPUT;
			p.out.drain(..cut);
			p.truncated = true;
		}
	}

	fn on_osc(&mut self, body: &[u8]) {
		let Ok(text) = std::str::from_utf8(body) else { return };
		let Some(rest) = text.strip_prefix("633;") else { return };
		let mut parts = rest.splitn(4, ';');
		let (kind, nonce) = (parts.next(), parts.next().unwrap_or_default());
		let Some(p) = self.pending.as_mut().filter(|p| p.nonce == nonce) else {
			// A marker from a bootstrap probe, or a stale run: the shell has the
			// functions either way.
			self.bootstrapped = true;
			return;
		};
		self.bootstrapped = true;
		match kind {
			Some("C") => p.started = true,
			Some("D") => {
				let exit = parts.next().and_then(|e| e.parse().ok()).unwrap_or(-1);
				let cwd = parts.next().unwrap_or_default().to_string();
				let Some(p) = self.pending.take() else { return };
				self.cwd = Some(cwd.clone());
				self.finished.insert(
					p.nonce,
					Finished {
						exit,
						cwd,
						output: clean(&String::from_utf8_lossy(&p.out)),
						truncated: p.truncated,
					},
				);
			}
			_ => {}
		}
	}
}

/// Where the OSC starting at `from` ends: `(index of terminator, its length)`.
fn osc_end(buf: &[u8], from: usize) -> Option<(usize, usize)> {
	let mut j = from;
	while j < buf.len() {
		match buf[j] {
			0x07 => return Some((j, 1)),
			0x1b if buf.get(j + 1) == Some(&b'\\') => return Some((j, 2)),
			0x1b if j + 1 == buf.len() => return None,
			_ => j += 1,
		}
	}
	None
}

pub fn looks_like_secret_prompt(line: &str) -> bool {
	let l = line.trim_end().to_ascii_lowercase();
	(l.ends_with(':') || l.ends_with(": "))
		&& ["password", "passphrase", "passcode", "verification code", "otp", "pin"]
			.iter()
			.any(|w| l.contains(w))
}

/// Output as an agent should read it: escape sequences removed, carriage-return
/// overwrites resolved, line endings normalised.
pub fn clean(raw: &str) -> String {
	let mut out = String::with_capacity(raw.len());
	let mut chars = raw.chars().peekable();
	while let Some(c) = chars.next() {
		match c {
			'\u{1b}' => match chars.peek() {
				Some('[') => {
					chars.next();
					// CSI: parameters, then one final byte in @..~
					for n in chars.by_ref() {
						if ('@'..='~').contains(&n) {
							break;
						}
					}
				}
				Some(']') => {
					chars.next();
					while let Some(n) = chars.next() {
						if n == '\u{7}' {
							break;
						}
						if n == '\u{1b}' && chars.peek() == Some(&'\\') {
							chars.next();
							break;
						}
					}
				}
				Some(_) => {
					chars.next();
				}
				None => {}
			},
			'\u{7}' | '\u{8}' => {}
			_ => out.push(c),
		}
	}
	out.split('\n')
		.map(|line| {
			let line = line.strip_suffix('\r').unwrap_or(line);
			line.rsplit('\r').next().unwrap_or(line)
		})
		.collect::<Vec<_>>()
		.join("\n")
		.trim_matches('\n')
		.to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn run_bytes(nonce: &str, output: &str, exit: i32, cwd: &str) -> Vec<u8> {
		format!(
			"prompt$ __qs {nonce}; ls\r\n\x1b]633;C;{nonce}\x07{output}\x1b]633;D;{nonce};{exit};{cwd}\x07prompt$ "
		)
		.into_bytes()
	}

	#[test]
	fn a_wrapped_command_yields_its_output_exit_and_cwd() {
		let mut c = Capture::new(80, 24);
		c.expect("n1");
		c.feed(&run_bytes("n1", "a.txt\r\n\x1b[1;34mdir\x1b[0m\r\n", 0, "/srv"));
		let f = c.take_finished("n1").expect("finished");
		assert_eq!(
			f,
			Finished { exit: 0, cwd: "/srv".into(), output: "a.txt\ndir".into(), truncated: false }
		);
		assert_eq!(c.cwd.as_deref(), Some("/srv"));
		assert!(c.bootstrapped);
	}

	#[test]
	fn markers_split_across_reads_are_still_found() {
		let mut c = Capture::new(80, 24);
		c.expect("n2");
		let bytes = run_bytes("n2", "hello\r\n", 3, "/tmp");
		for chunk in bytes.chunks(3) {
			c.feed(chunk);
		}
		let f = c.take_finished("n2").expect("finished");
		assert_eq!((f.exit, f.output.as_str(), f.cwd.as_str()), (3, "hello", "/tmp"));
	}

	#[test]
	fn another_nonce_does_not_finish_the_run() {
		let mut c = Capture::new(80, 24);
		c.expect("mine");
		c.feed(&run_bytes("theirs", "x", 0, "/"));
		assert!(c.take_finished("mine").is_none());
	}

	#[test]
	fn the_secret_prompt_is_read_off_the_cursor_line() {
		let mut c = Capture::new(80, 24);
		c.feed(b"$ sudo systemctl restart nginx\r\n[sudo] password for deploy: ");
		assert!(c.asks_for_secret());
		let mut c = Capture::new(80, 24);
		c.feed(b"Password changed successfully\r\n$ ");
		assert!(!c.asks_for_secret());
	}

	#[test]
	fn a_full_screen_program_is_noticed() {
		let mut c = Capture::new(80, 24);
		c.feed(b"\x1b[?1049h");
		assert!(c.in_full_screen_program());
		c.feed(b"\x1b[?1049l");
		assert!(!c.in_full_screen_program());
	}

	#[test]
	fn carriage_return_overwrites_keep_the_last_write() {
		assert_eq!(clean("progress 10%\rprogress 100%\r\ndone"), "progress 100%\ndone");
	}
}
