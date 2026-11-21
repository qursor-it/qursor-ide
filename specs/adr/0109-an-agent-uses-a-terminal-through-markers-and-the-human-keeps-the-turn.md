# ADR-0109 — An agent uses a terminal through markers, and the human keeps the turn

**Date.** 2026-11-19
**Status.** Accepted. Extends ADR-0029 (model-facing tools live on the
session's own MCP server). Arises from F46.

## Context

The owner wants every SSH terminal they open to be usable by an agent session —
either their own shell, shared, or an SSH session opened for the agent — while
Claude Code and Codex stay installed and running only on their computer. The
agent keeps every one of its own tools; the remote side is reached through the
terminals a human lent it.

Typing into a PTY is trivial. Knowing when a command ended, which bytes were
its output and how it exited is not, and a guess (silence, a prompt regex) is
wrong often enough to mislead an agent.

## Decision

1. **Four tools on the session's MCP server**: `terminals_list`,
   `terminal_run`, `terminal_read`, `terminal_send_keys`. The backend is bound
   to the session at spawn, so no argument can reach another session's
   terminals; a terminal is lent to at most one session.
2. **Markers.** The first time an agent uses a shell, a one-line bootstrap
   defines two functions that print `OSC 633;C;<nonce>` and
   `OSC 633;D;<nonce>;<exit>;<cwd>`. Each command is typed as
   ` __qs N; { CMD\n}; __qe N`; the reader thread's capture finds the markers,
   so the agent gets the real output, exit status and directory. Nothing is
   written to the server's disk. A shell that does not answer the probe (fish,
   csh, a program that is not a shell) is reported as such, never guessed at.
3. **The human keeps the turn.** Nothing is typed while a full-screen program
   owns the terminal, while the cursor line asks for a password, within 1.5s of
   a human keystroke, or while another agent command is in flight. A password
   prompt stops the agent and is left to the human; `send_keys` refuses there.
4. **A screen in Rust.** A `vt100` parser fed the same bytes as xterm answers
   `terminal_read` and the state checks above.

## Consequences

- The command line and its markers' wrapper are visible in a shared terminal:
  the human sees exactly what the agent typed.
- An interrupted command never prints its end marker; `<ctrl-c>` through
  `send_keys` clears the pending run, and a timeout reports "still running".
- `/rewind` cannot undo what a remote command did; the agent's own local tools
  are unaffected.
