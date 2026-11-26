# ADR-0124 — OpenCode adopts its own session IDs

## Context

ADR-0118 said an OpenCode session keeps the Qursor-minted ID for the life of
its PTY. ADR-0122 then added a per-terminal plugin that reports the session
OpenCode created, because OpenCode 1.x generates its own IDs and keeps the
history in its SQLite store.

## Decision

Qursor adopts the OpenCode session ID once the store confirms that session
belongs to the terminal's folder, as it does for Codex. The check runs once,
for the first ID the plugin reports. A folder that never matches leaves the
terminal on its Qursor ID instead of probing the store for the whole session.
This supersedes the ADR-0118 consequence that OpenCode sessions keep Qursor
IDs while the PTY lives. The signal files live under the app data folder, one
per terminal, and the folder is wiped at startup.

## Consequences

A resumed OpenCode session uses `--session` with the ID OpenCode owns.
Qursor opens the store read-only, but SQLite may still create `-wal` and
`-shm` files in OpenCode's data directory when OpenCode is closed. Qursor never
writes to the database itself.
