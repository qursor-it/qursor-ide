# ADR-0122 — OpenCode session metadata is read-only

## Context

ADR-0118 brought OpenCode to the local agent registry with a PTY-only identity.
OpenCode 1.x generates its own session IDs and stores titles and history in
its SQLite database.

## Decision

Qursor reads root session metadata from the profile's OpenCode SQLite store
in read-only mode and links it with the existing workspace reconciliation.
A per-terminal OpenCode plugin reports the first root session created by
that process, which Qursor verifies against the store before adopting its ID.
The CLI receives that persisted ID with `--session` on resume.

## Consequences

Existing sessions in a workspace project get titles and can be resumed.
Qursor owns the sidebar index. OpenCode owns the conversation data, transcript
operations and deletion. CLI versions without the SQLite store retain live
terminal support.
