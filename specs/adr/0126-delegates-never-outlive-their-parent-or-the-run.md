# ADR-0126 — Delegates never outlive their parent or the run

## Context

ADR-0125 left two cases open. A delegate whose parent exited kept running
with nowhere to report. After a restart, a finished delegate's transcript came
back in the session lists like any other session.

## Decision

Closing a session closes its running delegates first. Qursor signals them,
waits up to three seconds for them to stop, then closes the parent. A parent
that exits on its own takes its running delegates with it. On quit, delegates
are signalled before every other PTY. Closing a session with live delegates
always asks first, whatever the close-confirmation settings say, and the
dialog says that its delegates close with it.

Every session id a delegate runs under, provisional and adopted, is recorded
in `delegate_sessions` when it starts. The session list, search, and a
project's count and last activity leave those sessions out. The renderer does
not persist a delegate's tab. A delegate does not survive a restart, and the
parent delegates again if the work is still needed. When a parent's agent has
adopted its own id, the request carries that id, so the approval dialog can
name the parent by its title.

## Consequences

This supersedes the last two consequences of ADR-0125. The transcripts stay
where their agents wrote them (ADR-0004); only Qursor's lists hide them, for
good. A parent that crashes still takes its delegates with it.
