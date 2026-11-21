# ADR-0107 — The Server mode has a tab per SSH session

**Date.** 2026-11-19
**Status.** Accepted. Sits beside ADR-0100 (session tabs) and ADR-0102 (Git
mode); amends neither. Arises from F45.

## Context

The owner asked for an SSH session manager in the app — the MobaXterm shape:
an address book of servers, several terminals per server, and agent sessions
that use those terminals (F46). The session tab strip (ADR-0100) is one tab per
*agent* session, grouped by project; a server terminal is neither.

## Decision

1. **Servers are a third window mode**, beside Sessions and Git, switched from
   the app rail. Like Git mode, the Sessions body is hidden rather than
   unmounted, so every agent keeps running while the human is in Servers.
2. **The mode's top bar has one tab per SSH session**, not per server. Opening a
   server twice is two tabs, labelled `name · 1`, `name · 2`.
3. **An agent attached to an SSH session is a chip on that session's tab, never
   a tab of its own** (F46). An agent's own SSH session is still an SSH session:
   a tab, marked as the agent's.
4. The address book (folders, servers, connection dot, the production marker)
   is the mode's sidebar; its vault state sits at the foot.

## Consequences

- Tabs persist without their PTYs; after a restart each one logs in again when
  it is shown. A renderer reload re-adopts the live PTYs through `terminal_list`
  and their client keys, as the footer shells do (ADR-0032).
- The Sessions tab strip is untouched: the two strips never mix.
