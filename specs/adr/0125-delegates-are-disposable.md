# ADR-0125 — Delegates are disposable

## Context

ADR-0119 records each delegation and lets the parent read one result. Kept
as they were, finished delegates stayed in the sidebar, kept their tabs and
counted towards the per-session limit for the life of the app. The human
supervises a delegate while it works. Once it has reported, nothing about it
needs a place on screen.

## Decision

The renderer shows a delegation while it is pending, starting or running.
The parent session's view lists those under its header and the sidebar nests
them under the parent. Each row shows the child's title, or its brief until
the child has one. The approval dialog names the session that asked.

A delegation that completes, ends, fails or is declined leaves the list.
Completing stops the child's PTY. Its tab then closes, and the child's session
stays out of the lists for the rest of the run. The parent can read a final
state once. The result is gone after that read, after the parent exits, or
ten minutes after the delegation finished. The per-session limit counts only
live delegations.

## Consequences

The renderer learns of every change from `delegations:changed` and reads
`list_delegations`. Nothing is persisted. After a restart, a finished
delegate's transcript appears in the session lists like any other session
its agent recorded. A delegate whose parent exits keeps running until it
finishes, and its result is then dropped.
