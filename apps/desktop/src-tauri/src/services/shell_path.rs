//! The `PATH` a child process should get — the user's, not ours.
//!
//! A GUI application does not have the user's `PATH`. Launched from Finder, the
//! Dock or a `.desktop` file it inherits launchd's / the session manager's
//! environment, and no shell rc file has ever run in it. So `/opt/homebrew/bin`
//! (or `/usr/local/bin` on an Intel Mac) is absent, and so is every
//! version-manager shim — nvm, mise, asdf, fnm, volta. Handing that on to a
//! `claude` session breaks everything in it that resolves a program by name,
//! and none of the breakage looks like an environment problem from the inside:
//!
//! - A hook runs as `/bin/sh -c "<command>"`. `/bin/sh` is found because it is
//!   invoked by absolute path; the bare `bash` *inside* the command goes through
//!   `PATH` and isn't. What surfaces is
//!   `SessionStart:startup hook error … /bin/sh: bash: command not found`.
//! - A stdio MCP server is launched as `npx` / `node` / `uvx` / `docker` and
//!   fails its JSON-RPC handshake. What surfaces is `-32000`.
//! - A `statusLine` command fails silently — no banner at all.
//! - `git`, `gh`, `pnpm` and everything else the agent runs from `Bash`.
//!
//! The tell is always the same: it works when `claude` is started from a
//! terminal. That difference *is* the diagnosis.
//!
//! **So ask a shell.** Run the user's login shell once at startup and read the
//! `PATH` it produces. This is the `fix-path-for-mac` pattern VS Code and most
//! Electron developer tools use; it is prior art, not a scheme of ours. Four
//! details are what make it work in practice rather than in principle:
//!
//! - **`-ilc`, both flags.** `-l` sources `~/.zprofile`, where Homebrew's
//!   `shellenv` usually lands; `-i` sources `~/.zshrc`, where nvm / mise / asdf
//!   usually land. Either one alone misses half of real machines.
//! - **Sentinels, not raw stdout.** An interactive shell talks: MOTD,
//!   powerlevel10k's instant prompt, `direnv`, version-manager banners. The
//!   answer is fished out from between two markers.
//! - **Stdin from `/dev/null`, and a timeout.** An interactive shell may block
//!   waiting to be typed at. On timeout the child is killed and the floor below
//!   is used instead.
//! - **Read `$SHELL`.** Not everyone runs zsh. `/bin/zsh` is only the guess for
//!   when `$SHELL` is unset, and no further shells are tried after it: a
//!   `$SHELL` that cannot be run is pathological, and [`FALLBACK_PATH`] is a
//!   better answer to it than a cascade that quietly consults a shell whose rc
//!   files the user does not use.
//!
//! **Windows reads the registry instead** (PERF-32, `specs/10-performance.md`).
//! There `PATH` is not a shell's invention: it is the machine and user values
//! under `Environment` in the registry, which is what a new logon builds it
//! from. Reading them costs no process and no profile, and also catches an
//! entry added since this app's parent started.
//!
//! Resolution happens **once** and is cached for the app's lifetime — see
//! [`child_path`]. Everything else about a child's environment is inherited
//! whole; see [`super::child_env`], which is where this gets applied.

use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::sync::mpsc;
use std::sync::OnceLock;
#[cfg(unix)]
use std::time::Duration;

use tracing::{info, warn};

/// Markers around the answer, so the shell's own chatter can be discarded.
#[cfg(unix)]
const START: &str = "__FACTORAI_PATH_START__";
#[cfg(unix)]
const END: &str = "__FACTORAI_PATH_END__";

/// Where to look on Unix when the shell cannot be asked. Both Homebrew prefixes are
/// here on purpose: Apple Silicon puts it in `/opt/homebrew`, Intel in
/// `/usr/local`, and a floor that only knows one of them is a floor that drops
/// half of macOS through it.
#[cfg(unix)]
pub const FALLBACK_PATH: &str =
	"/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// Long enough for a slow `~/.zshrc` (nvm alone can take a second), short
/// enough that a shell which is never going to answer doesn't hold a session
/// launch. Only ever paid once.
#[cfg(unix)]
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// Programs whose absence means the resolved `PATH` is not usable, checked at
/// startup so the diagnosis is in the log rather than three layers down inside
/// a hook error. `bash` is what hooks shell out to; `node` is what stdio MCP
/// servers are.
#[cfg(unix)]
const EXPECTED: &[&str] = &["bash", "node"];
#[cfg(windows)]
const EXPECTED: &[&str] = &["powershell.exe", "node.exe"];

static CHILD_PATH: OnceLock<OsString> = OnceLock::new();

/// The `PATH` every child process gets. Resolved on first call and cached.
///
/// [`warm`] normally wins this race off the main thread; a caller arriving
/// while that is still in flight blocks on it rather than resolving a second
/// time, which is what `OnceLock` is for.
pub fn child_path() -> &'static OsStr {
	CHILD_PATH.get_or_init(resolve).as_os_str()
}

/// Resolve and cache off the caller's thread, and say in the log if the answer
/// looks unusable. Called from `setup()`: the window must not wait on a shell.
pub fn warm() {
	std::thread::spawn(|| {
		let path = child_path();
		let missing: Vec<&str> =
			EXPECTED.iter().copied().filter(|n| which_in(path, n).is_none()).collect();
		if missing.is_empty() {
			return;
		}
		warn!(
			path = %path.to_string_lossy(),
			missing = ?missing,
			"could not resolve your shell environment: these programs are not on the PATH \
			 sessions will run with, so hooks, MCP servers and the statusline will fail"
		);
	});
}

#[cfg(unix)]
fn resolve() -> OsString {
	let shell = user_shell();
	match path_from_shell(&shell, RESOLVE_TIMEOUT) {
		Some(path) => {
			info!(shell = %shell.display(), path = %path.to_string_lossy(), "resolved login shell PATH");
			path
		}
		None => {
			warn!(shell = %shell.display(), "could not read PATH from the login shell; using the fallback");
			OsString::from(FALLBACK_PATH)
		}
	}
}

/// The registry's `PATH`, machine then user, followed by whatever this
/// process inherited that the registry does not name — so nothing that worked
/// when the app read its own `PATH` stops working.
///
/// Until 2026-10-14 this asked Windows PowerShell, profile and all, the way
/// Unix asks the login shell. That cost a PowerShell start (about 140 ms with
/// no profile, seconds with oh-my-posh or conda in one), and the first session
/// spawn waited on it through `child_path`. The one thing it found that this
/// does not is an entry added *only* inside a PowerShell profile, which is not
/// where Windows keeps `PATH`.
#[cfg(windows)]
fn resolve() -> OsString {
	let registry = registry_path();
	let inherited = std::env::var_os("PATH");
	match merge_windows_paths(registry.as_deref(), inherited.as_deref()) {
		Some(path) => {
			info!(path = %path.to_string_lossy(), "resolved PATH from the Windows environment");
			path
		}
		None => {
			warn!("could not read PATH from the Windows environment; using the system floor");
			fallback_path()
		}
	}
}

/// `Path` from the machine's and the user's environment keys, in the order a
/// logon joins them. Either may be missing; both missing is `None`.
#[cfg(windows)]
fn registry_path() -> Option<OsString> {
	use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

	let machine = registry_env_value(
		HKEY_LOCAL_MACHINE,
		r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
	);
	let user = registry_env_value(HKEY_CURRENT_USER, "Environment");
	let parts: Vec<OsString> = [machine, user].into_iter().flatten().collect();
	if parts.is_empty() {
		return None;
	}
	let mut joined = OsString::new();
	for (i, part) in parts.iter().enumerate() {
		if i > 0 {
			joined.push(";");
		}
		joined.push(part);
	}
	Some(joined)
}

/// One `Path` value, with `%VAR%` references expanded when it is stored as
/// `REG_EXPAND_SZ` — which the machine value usually is (`%SystemRoot%`).
#[cfg(windows)]
fn registry_env_value(
	root: windows_sys::Win32::System::Registry::HKEY,
	subkey: &str,
) -> Option<OsString> {
	use windows_sys::Win32::Foundation::ERROR_SUCCESS;
	use windows_sys::Win32::System::Registry::{
		RegGetValueW, REG_EXPAND_SZ, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
	};

	let subkey = wide_nul(OsStr::new(subkey));
	let name = wide_nul(OsStr::new("Path"));
	// Expanded by hand below rather than by `RegGetValueW`, so the size asked
	// for is the size returned.
	let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
	let mut kind = 0u32;
	let mut size = 0u32;
	// SAFETY: both strings are NUL-terminated and outlive the call; a null data
	// pointer asks only for the size.
	let status = unsafe {
		RegGetValueW(
			root,
			subkey.as_ptr(),
			name.as_ptr(),
			flags,
			&mut kind,
			std::ptr::null_mut(),
			&mut size,
		)
	};
	if status != ERROR_SUCCESS || size == 0 {
		return None;
	}
	let mut buf = vec![0u16; (size as usize).div_ceil(2)];
	// SAFETY: `buf` holds at least `size` bytes, and `size` says so.
	let status = unsafe {
		RegGetValueW(
			root,
			subkey.as_ptr(),
			name.as_ptr(),
			flags,
			&mut kind,
			buf.as_mut_ptr().cast(),
			&mut size,
		)
	};
	if status != ERROR_SUCCESS {
		return None;
	}
	buf.truncate(size as usize / 2);
	while buf.last() == Some(&0) {
		buf.pop();
	}
	if kind == REG_EXPAND_SZ {
		expand_env(&buf)
	} else {
		Some(OsString::from_wide(&buf))
	}
}

/// `%SystemRoot%\System32` to `C:\Windows\System32`, against this process's
/// environment — the same expansion a logon applies.
#[cfg(windows)]
fn expand_env(value: &[u16]) -> Option<OsString> {
	use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;

	let mut source = value.to_vec();
	source.push(0);
	// SAFETY: `source` is NUL-terminated; a null destination asks for the size,
	// which includes the terminator.
	let needed = unsafe { ExpandEnvironmentStringsW(source.as_ptr(), std::ptr::null_mut(), 0) };
	if needed == 0 {
		return None;
	}
	let mut out = vec![0u16; needed as usize];
	// SAFETY: `out` holds `needed` units, and `needed` says so.
	let written = unsafe { ExpandEnvironmentStringsW(source.as_ptr(), out.as_mut_ptr(), needed) };
	if written == 0 || written > needed {
		return None;
	}
	out.truncate(written as usize - 1);
	Some(OsString::from_wide(&out))
}

#[cfg(windows)]
fn wide_nul(value: &OsStr) -> Vec<u16> {
	value.encode_wide().chain(std::iter::once(0)).collect()
}

/// The registry's entries first, then the inherited ones it does not already
/// have, compared the way Windows compares paths: case-insensitively and with
/// or without a trailing separator. Empty entries are dropped (see
/// `sanitize_windows`); `None` when nothing is left.
#[cfg(windows)]
fn merge_windows_paths(registry: Option<&OsStr>, inherited: Option<&OsStr>) -> Option<OsString> {
	let mut seen = std::collections::HashSet::new();
	let mut kept: Vec<Vec<u16>> = Vec::new();
	for source in [registry, inherited].into_iter().flatten() {
		let wide: Vec<u16> = source.encode_wide().collect();
		for entry in wide.split(|unit| *unit == u16::from(b';')) {
			if entry.is_empty() {
				continue;
			}
			let key = String::from_utf16_lossy(entry).trim_end_matches(['\\', '/']).to_lowercase();
			if seen.insert(key) {
				kept.push(entry.to_vec());
			}
		}
	}
	sanitize_windows(&kept.join(&u16::from(b';')))
}

/// The shell the user actually runs: `$SHELL`, or `/bin/zsh` when it is unset.
///
/// Public because F23's footer spawns this same shell for the user to type in,
/// and two answers to "which shell is yours" is one too many — a footer running
/// bash while the `PATH` was read from zsh would be a difference nobody could
/// see and everybody would feel.
///
/// The single-guess rule is the module note's: no cascade past `/bin/zsh`.
#[cfg(unix)]
pub fn user_shell() -> PathBuf {
	match std::env::var_os("SHELL") {
		Some(s) if !s.is_empty() => PathBuf::from(s),
		_ => PathBuf::from("/bin/zsh"),
	}
}

/// Windows ships Windows PowerShell with the OS, so a native footer has a
/// stable shell even when PowerShell 7 is not installed or not on PATH.
#[cfg(windows)]
pub fn user_shell() -> PathBuf {
	std::env::var_os("SystemRoot")
		.map(PathBuf::from)
		.map(|root| root.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
		.filter(|path| path.is_file())
		.unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

/// What a footer chip calls the shell `user_shell` names: its basename, `zsh`
/// for `/bin/zsh` (`specs/05-features.md` § F23 as amended by F24). One value
/// per app, asked once by the renderer, because the label is the same on every
/// chip and never changes.
pub fn shell_name() -> String {
	#[cfg(windows)]
	{
		"powershell".to_string()
	}
	#[cfg(not(windows))]
	{
		shell_basename(&user_shell())
	}
}

/// The basename, or `shell` for a path that has none. `$SHELL=/` is not a shell
/// anybody has, but a chip with no text on it is a worse answer than a generic
/// one.
#[cfg(not(windows))]
fn shell_basename(shell: &Path) -> String {
	shell
		.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_else(|| "shell".to_string())
}

/// Ask one shell. `None` for every failure mode — unspawnable, timed out, no
/// sentinels, nothing usable between them — because they all mean the same
/// thing to the caller.
#[cfg(unix)]
fn path_from_shell(shell: &Path, timeout: Duration) -> Option<OsString> {
	let script = format!("printf '%s' \"{START}${{PATH}}{END}\"");
	let mut child = Command::new(shell)
		.arg("-ilc")
		.arg(&script)
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		// The noise is the reason for the sentinels; it is not worth reading.
		.stderr(Stdio::null())
		.spawn()
		.map_err(|e| warn!(shell = %shell.display(), error = %e, "could not run the login shell"))
		.ok()?;

	// Reading on this thread would be the hang the timeout exists to prevent, so
	// stdout is drained on its own. The thread is deliberately never joined: a
	// backgrounded grandchild can hold the write end open after the shell itself
	// is gone, and waiting for that is the same deadlock in a different place.
	let mut stdout = child.stdout.take()?;
	let (tx, rx) = mpsc::channel();
	std::thread::spawn(move || {
		let mut buf = Vec::new();
		let _ = stdout.read_to_end(&mut buf);
		let _ = tx.send(buf);
	});

	let received = rx.recv_timeout(timeout);
	// Unconditionally, and before looking at the result: on the happy path the
	// shell has already exited and this is a no-op that reaps it, and on the
	// timeout it is the only thing that stops a stuck interactive shell living
	// as long as the app does.
	let _ = child.kill();
	let _ = child.wait();

	let out = received
		.map_err(
			|_| warn!(shell = %shell.display(), ?timeout, "the login shell did not answer in time"),
		)
		.ok()?;
	extract(&out)
}

/// The bytes between the sentinels, sanitised. Bytes, not `str`: a `PATH` is
/// not required to be UTF-8 and lossy-converting one would corrupt the entry
/// it touched rather than fail.
#[cfg(unix)]
fn extract(out: &[u8]) -> Option<OsString> {
	let after_start = &out[find(out, START.as_bytes())? + START.len()..];
	let value = &after_start[..find(after_start, END.as_bytes())?];
	sanitize(value)
}

#[cfg(windows)]
fn sanitize_windows(value: &[u16]) -> Option<OsString> {
	let mut result = Vec::new();
	for entry in value.split(|unit| *unit == u16::from(b';')).filter(|entry| !entry.is_empty()) {
		if !result.is_empty() {
			result.push(u16::from(b';'));
		}
		result.extend_from_slice(entry);
	}
	(!result.is_empty()).then(|| OsString::from_wide(&result))
}

#[cfg(windows)]
fn fallback_path() -> OsString {
	if let Some(path) = std::env::var_os("PATH") {
		if let Some(path) = sanitize_windows(&path.encode_wide().collect::<Vec<_>>()) {
			return path;
		}
	}
	let root = std::env::var_os("SystemRoot")
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
	let entries = [
		root.join("System32"),
		root.clone(),
		root.join("System32/Wbem"),
		root.join("System32/WindowsPowerShell/v1.0"),
	];
	std::env::join_paths(entries).unwrap_or_default()
}

#[cfg(unix)]
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
	haystack.windows(needle.len()).position(|w| w == needle)
}

/// Drop empty entries and refuse a `PATH` with nothing left in it. An empty
/// entry means the current directory — a session's cwd is a project checkout,
/// so that is a program-execution hazard rather than a cosmetic flaw, and
/// `PATH=""` is worse than having asked nothing at all.
#[cfg(unix)]
fn sanitize(value: &[u8]) -> Option<OsString> {
	let kept: Vec<&[u8]> = value.split(|b| *b == b':').filter(|e| !e.is_empty()).collect();
	if kept.is_empty() {
		return None;
	}
	Some(OsString::from_vec(kept.join(&b':')))
}

/// First executable named `name` in `path`. Deliberately not `which`, which
/// would answer for *our* `PATH` and so could never see the problem.
fn which_in(path: &OsStr, name: &str) -> Option<PathBuf> {
	std::env::split_paths(path).map(|dir| dir.join(name)).find(|p| is_executable(p))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
	std::fs::metadata(p)
		.map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
		.unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(p: &Path) -> bool {
	p.is_file()
}

#[cfg(all(test, unix))]
mod tests {
	use std::os::unix::ffi::OsStrExt;

	use super::*;

	/// A stand-in shell: ignores `-ilc` and the script entirely, and writes
	/// exactly what the test wants on stdout. Asking a real shell would make
	/// these tests assertions about the machine they run on.
	fn fake_shell(dir: &Path, body: &str) -> PathBuf {
		let p = dir.join("fake-shell");
		std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
		std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
		p
	}

	#[test]
	fn the_answer_is_fished_out_from_between_the_shell_s_chatter() {
		// What a real interactive shell looks like: a MOTD, an instant prompt,
		// direnv, an nvm banner, and the one line we asked for somewhere in it.
		let tmp = tempfile::TempDir::new().unwrap();
		let shell = fake_shell(
			tmp.path(),
			"echo 'Welcome to Ubuntu 24.04'\n\
			 printf 'direnv: loading .envrc\\n'\n\
			 printf '%s' \"__FACTORAI_PATH_START__/opt/homebrew/bin:/usr/bin:/bin__FACTORAI_PATH_END__\"\n\
			 echo 'Now using node v22.11.0'",
		);
		assert_eq!(
			path_from_shell(&shell, Duration::from_secs(5)),
			Some(OsString::from("/opt/homebrew/bin:/usr/bin:/bin"))
		);
	}

	#[test]
	fn a_shell_that_never_answers_is_killed_and_gives_no_path() {
		// The interactive-shell-blocks-on-input case, with a timeout a test can
		// afford. `resolve` turns this None into FALLBACK_PATH.
		let tmp = tempfile::TempDir::new().unwrap();
		let shell = fake_shell(tmp.path(), "sleep 60");
		assert_eq!(path_from_shell(&shell, Duration::from_millis(200)), None);
	}

	#[test]
	fn a_shell_that_cannot_be_run_gives_no_path() {
		assert_eq!(path_from_shell(Path::new("/nonexistent/shell"), Duration::from_secs(5)), None);
	}

	#[test]
	fn a_shell_that_says_nothing_useful_gives_no_path() {
		let tmp = tempfile::TempDir::new().unwrap();
		// No sentinels at all — a shell whose rc files exited before our command.
		assert_eq!(
			path_from_shell(&fake_shell(tmp.path(), "echo hi"), Duration::from_secs(5)),
			None
		);
		// Sentinels, but nothing between them.
		let empty =
			fake_shell(tmp.path(), "printf '%s' '__FACTORAI_PATH_START____FACTORAI_PATH_END__'");
		assert_eq!(path_from_shell(&empty, Duration::from_secs(5)), None);
	}

	#[test]
	fn the_fallback_is_used_when_the_shell_cannot_be_asked() {
		// The acceptance case: $SHELL pointing at a binary that isn't there.
		let path = match path_from_shell(Path::new("/nonexistent/shell"), Duration::from_secs(5)) {
			Some(p) => p,
			None => OsString::from(FALLBACK_PATH),
		};
		assert_eq!(path, OsString::from(FALLBACK_PATH));
		// And it is a floor a hook can actually run in: /bin/sh finds bash there.
		for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
			assert!(std::env::split_paths(&path).any(|p| p == Path::new(dir)), "{dir} missing");
		}
	}

	#[test]
	fn an_empty_entry_never_survives_as_the_current_directory() {
		// A trailing or doubled colon means "." to execvp, and a session's cwd is
		// a project checkout someone else may have written to.
		assert_eq!(sanitize(b"/usr/bin::/bin:"), Some(OsString::from("/usr/bin:/bin")));
		assert_eq!(sanitize(b":"), None);
		assert_eq!(sanitize(b""), None);
	}

	#[test]
	fn a_path_that_is_not_utf8_survives_intact() {
		// Not hypothetical on a machine with a mojibake directory name in PATH;
		// lossy conversion would corrupt that entry rather than report anything.
		let mut raw = Vec::from(START.as_bytes());
		raw.extend_from_slice(b"/usr/bin:/home/\xff\xfe/bin");
		raw.extend_from_slice(END.as_bytes());
		let got = extract(&raw).unwrap();
		assert_eq!(got.as_bytes(), b"/usr/bin:/home/\xff\xfe/bin");
	}

	#[test]
	fn which_in_reads_the_given_path_and_not_ours() {
		let tmp = tempfile::TempDir::new().unwrap();
		let bin = tmp.path().join("bin");
		std::fs::create_dir(&bin).unwrap();
		std::fs::write(bin.join("not-executable"), "").unwrap();
		let exe = fake_shell(&bin, "true");
		let path = OsString::from(bin.to_str().unwrap());

		assert_eq!(which_in(&path, "fake-shell"), Some(exe));
		// A file that is there but cannot be run is not a hit — that distinction
		// is the whole value of the startup self-check.
		assert_eq!(which_in(&path, "not-executable"), None);
		// And `sh` is on the real PATH but not on this one.
		assert_eq!(which_in(&path, "sh"), None);
	}

	/// What a footer chip says (F23 as amended by F24): the shell's basename,
	/// and a word rather than nothing when the path has none.
	#[test]
	fn a_chip_is_named_after_the_shells_basename() {
		assert_eq!(shell_basename(Path::new("/bin/zsh")), "zsh");
		assert_eq!(shell_basename(Path::new("/opt/homebrew/bin/fish")), "fish");
		assert_eq!(shell_basename(Path::new("/")), "shell");
	}
}

#[cfg(all(test, windows))]
mod windows_tests {
	use super::*;

	#[test]
	fn windows_path_sanitizing_preserves_unicode_and_drive_colons() {
		let value: Vec<u16> =
			r"C:\Users\Zoë\AppData\Roaming\npm;;D:\tools\node".encode_utf16().collect();
		assert_eq!(
			sanitize_windows(&value),
			Some(OsString::from(r"C:\Users\Zoë\AppData\Roaming\npm;D:\tools\node"))
		);
	}

	#[test]
	fn windows_shell_is_power_shell() {
		assert_eq!(shell_name(), "powershell");
		assert!(user_shell().to_string_lossy().to_ascii_lowercase().ends_with("powershell.exe"));
	}

	fn merged(registry: Option<&str>, inherited: Option<&str>) -> Option<String> {
		merge_windows_paths(registry.map(OsStr::new), inherited.map(OsStr::new))
			.map(|p| p.to_string_lossy().into_owned())
	}

	#[test]
	fn the_registry_comes_first_and_the_inherited_extras_follow() {
		assert_eq!(
			merged(
				Some(r"C:\Windows\System32;C:\Users\u\bin"),
				Some(r"C:\Tools;C:\Windows\System32")
			),
			Some(r"C:\Windows\System32;C:\Users\u\bin;C:\Tools".to_string())
		);
	}

	#[test]
	fn duplicates_differ_only_in_case_or_a_trailing_separator() {
		assert_eq!(
			merged(
				Some(r"C:\Program Files\nodejs\;;"),
				Some(r"c:\program files\NODEJS;C:\Git\cmd")
			),
			Some(r"C:\Program Files\nodejs\;C:\Git\cmd".to_string())
		);
	}

	#[test]
	fn nothing_at_all_is_none() {
		assert_eq!(merged(None, None), None);
		assert_eq!(merged(Some(";;"), Some("")), None);
	}

	#[test]
	fn the_real_registry_names_system32_expanded() {
		// Every Windows install has it in the machine value, stored as
		// `%SystemRoot%\system32`: this checks the read and the expansion.
		let path =
			registry_path().expect("a PATH in the registry").to_string_lossy().to_lowercase();
		assert!(path.contains(r"\system32"), "{path}");
		assert!(!path.contains("%systemroot%"), "{path}");
	}
}
