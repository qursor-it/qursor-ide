# ADR-0105 — Server sessions ride one `russh` connection per server

**Date.** 2026-11-19
**Status.** Accepted. Arises from F45 (server sessions over SSH). Narrows
ADR-0005 (kill-on-quit) to say what it means for a process that is not ours.

## Context

The Server mode is an SSH session manager inside the app: saved servers, several
tabs per server, SFTP, tunnels, and agent sessions that use those terminals.
Two engines were weighed:

| | system OpenSSH (`ssh`, `ssh.exe`) | `russh` in-process |
|---|---|---|
| a second tab on the same login | needs `ControlMaster`, which Win32-OpenSSH lacks | a second channel on one connection |
| saved passwords and keys | only through `SSH_ASKPASS` | handed to the library from the vault |
| SFTP, tunnels | separate processes per feature | channels on the same connection |
| `~/.ssh/config` parity | total | partial: what the server form stores |
| agent | works | `SSH_AUTH_SOCK`; on Windows the OpenSSH pipe, then Pageant |

Windows is the owner's main platform, and the features asked for — saved
credentials, sibling tabs, SFTP, tunnels — are the column the system client
cannot fill there.

## Decision

1. **`russh` is the engine**, with `ring` as its crypto backend instead of the
   default `aws-lc-rs`, which needs cmake and NASM on Windows. One connection per
   server; every shell tab is a channel on it.
2. **A shell channel is a PTY.** `services/ssh/pty.rs` presents the channel as a
   `portable_pty` master and child, and `TerminalManager::attach` registers it
   exactly as it registers a local process. The terminal's kind is `Ssh`: no
   session id, outside the work count, owned by no project.
3. **Host keys are verified.** The user's `~/.ssh/known_hosts` is read; a key the
   human confirms (by fingerprint, after an unknown-host refusal) is written to
   the app's own `known_hosts`, never to theirs. A changed key is refused with no
   override.
4. **Kill-on-quit, for a process that is not ours, means hanging up.** Killing an
   SSH terminal closes its channel; the server sends the shell SIGHUP. The app
   cannot and does not promise more about the remote side.

## Consequences

- `ProxyCommand`, `Match` blocks and host certificates are not supported by the
  library path. A per-server "system ssh" mode (a plain `ssh -t` PTY) is the
  escape hatch for them, at the cost of a fresh login per tab on Windows.
- The loopback `sshd` fixture in `services/ssh` makes the SSH tests real; CI
  installs `openssh-server` for them. They are `cfg(unix)`: Windows exercises the
  same code paths only through the manual QA pass until a Windows sshd lane exists.
- Windows agent support (OpenSSH pipe, Pageant) compiles from `russh` but has not
  been exercised on a Windows machine yet.
