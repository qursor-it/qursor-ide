# ADR-0118 — OpenCode uses the local agent registry

## Context

OpenCode can use an existing login and consume the same Qursor MCP tools as
Claude Code and Codex. It has different launch arguments, provider settings,
and storage. ADR-0060 allows agents to ship with only the capabilities Qursor
can verify.

## Decision

OpenCode 1.x is the third local agent. It uses the existing binary probe,
profile selection, and PTY lifecycle. Qursor passes the initial prompt in
argv and merges its loopback MCP registration into the child's inline config.
The registration resolves its bearer token from the child environment.

The default profile inherits OpenCode's XDG paths. A custom profile uses its
directory as the root for OpenCode data and settings. Authentication and
model selection happen in OpenCode itself.

## Consequences

OpenCode sessions use Qursor ids for the life of the PTY. They participate in
the live session list. Discovery, transcript indexing and resume become
available when Qursor can resolve the CLI session identity and read its store.
