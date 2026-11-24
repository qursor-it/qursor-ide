# ADR-0119 — Qursor confirms and records cross-agent delegation

## Context

ADR-0114 lets an agent start a same-agent session by asking the human in
chat. Selecting another agent may send the brief to a different provider.
The orchestrator also needs to receive the result.

## Decision

An explicit agent in the request opens a Qursor confirmation. The backend
binds the project, start folder, target and brief to a new request id.
An approved request can start one PTY. Its existing MCP endpoint can
publish one result, which the caller can retrieve by request id. The
same-agent path without an explicit target follows ADR-0114.

## Consequences

Requests and summaries live in memory. The caller retrieves the result on
demand. A delegated session cannot start another Qursor session. A rejected,
expired, failed or ended delegation reports its state to the caller.
