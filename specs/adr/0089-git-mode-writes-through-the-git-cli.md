# ADR-0089 — Git mode writes through the `git` CLI

Date: 2026-11-12
Status: Accepted. Supersedes the read-only clause of ADR-0009 and F18's "a
viewer, not a git client" for Git mode (F35). Extends ADR-0079, which already
writes one worktree through the `git` CLI.

## Context

F35 adds a full-window git client per project: stage, commit, checkout, merge,
rebase, fetch, pull, push, stash. ADR-0009 chose `git2` because the app wrote
nothing, and named the condition under which shelling out wins: a tool that
**writes** wants to match the user's git exactly — its config, hooks,
credential helpers and SSH setup. That condition is now true.

`git2` is linked `default-features = false`, without network transport.
Enabling it would mean shipping our own TLS/SSH stack and reimplementing
credential lookup, and commits made through it would skip the user's hooks and
signing configuration.

## Decision

- **Every write in Git mode runs the system `git` CLI.** The binary is found as
  ADR-0079 finds it (`git_binary()` in `services/gh/checkout.rs`, moved to a
  shared place): PATH plus well-known install locations, `.cmd` / `.bat` shims
  refused. Each command is a one-shot child with the shared child environment,
  a timeout, kill-on-drop and `GIT_TERMINAL_PROMPT=0`, run with
  `-C <project root>`.
- **Reads stay on `git2`.** Status, graph, commit detail and blobs keep the
  code F13 and F18 already use. After a write the view re-reads through `git2`.
- **The app never adds an attribution trailer** to a commit it makes.
- **Agents are reached only through a click**, as a prepared session
  (`initial_prompt`) or an inline proposal whose commands the user runs; the
  app never lets an agent drive the CLI wrapper directly. Proposed commands
  run only if they pass F35's allowlist, as argv, never through a shell.
- **Quick `✦` actions run the project's own agent CLI once** (`claude -p` or
  `codex exec`, with the project's profile, in the project root). They send
  what the project's sessions already send, so the click is the consent; this
  is deliberately lighter than F33's consent-per-run provider (ADR-0084), which
  analyses many sessions at once and stays as it is.
- `git2` stays `default-features = false` and transport-free.

## Consequences

- Hooks, signing, credential helpers and SSH agents work as they do in a
  terminal, with no setup in the app.
- A machine without `git` gets "git not found on PATH" in place of the write
  actions; the history view still works.
- Two code paths touch the repository (`git2` reads, CLI writes); a write is
  always followed by a fresh read, so the view never trusts its own cache.
- Long operations (pull, push, fetch on a slow remote) need visible progress and
  a cancel; the timeout is a backstop, not the UX.
- Tests for writes need `git` installed on CI, which ADR-0079 already requires.
