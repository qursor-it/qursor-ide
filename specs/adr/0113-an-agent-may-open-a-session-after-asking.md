# ADR-0113 — An agent may open a session, but only after asking the human

**Date.** 2026-11-21
**Status.** Accepted. Arises from F50; keeps PRODUCT.md's "the human keeps every
irreversible decision".

## Context

Mid-task an agent can see that a part of the work would be better done in a
session of its own. It should be able to open that session, with a brief as the
first message, without the human copying text around. It must not open sessions
behind the human's back or in another project.

## Decision

1. **A tool, `startSession({ prompt })`** on the `factorai` server. Project,
   agent and folder come from the session that calls it, never from arguments.
2. **Consent is the agent asking in chat**, as the tool's description tells it
   to, plus the CLI's own MCP permission prompt.
3. **The new session starts headless** (ADR-0078): it appears in the sidebar
   with no tab and no focus change.

Alternatives:

- **A Qursor confirmation banner** (reusing the agent confirmations of
  ADR-0110). Consent that always holds, at the cost of more UI and a double
  ask. Not needed yet.
- **A one-shot `createRoutine`.** Rejected: a routine run has no tab (ADR-0026)
  and lands in the routines list.

## Consequences

- "Always allow" on the CLI's prompt removes the second gate; only the agent's
  own asking is left. Codex's MCP approval behaviour is not verified.
- No drain of missed starts, unlike routines (ADR-0030): only a live session can
  emit, so the window is alive.
- Fire-and-forget: no result goes back to the caller and the sessions are not
  linked.
