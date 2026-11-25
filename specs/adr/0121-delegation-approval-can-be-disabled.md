# ADR-0121 — Delegation approval can be disabled

## Context

ADR-0119 asks for approval on each explicit delegation. Some users prefer to
authorize their orchestrators to launch subagents for them.

## Decision

Settings → Confirmations has an app-wide switch, enabled by default.
Turning it off authorizes future explicit-agent delegations at request time.
Qursor records the authorized request before asking the renderer to spawn
its PTY. It checks the caller's lifetime and the original launch parameters
when starting the PTY.

## Consequences

The orchestrator selects the target and brief and can retrieve the result
over MCP. The user's existing agent permissions, project scope, and
one-level delegation limit apply to automatic starts.
