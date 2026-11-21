# ADR-0110 — Every agent command on a server passes that server's policy

**Date.** 2026-11-19
**Status.** Accepted. Arises from F46; keeps PRODUCT.md's "the human keeps every
irreversible decision".

## Decision

Each saved server carries an agent policy, chosen by the human:

- **free** — agent commands run.
- **confirm** — each command (or key press) waits in a banner on that terminal:
  Refuse, Edit, Run. An edited command is what runs, and the agent is told it
  was edited. No answer in ten minutes is a refusal. A production server
  starts here.
- **readonly** — only commands that only look at things run (a list of
  programs and read-only subcommands, no redirection, no substitution); the rest
  is refused without being typed. This is a fast lane, not a security
  boundary: the boundary is the Unix user the server is logged into as.

Every decision is written to a local `agent_audit` table (never sent
anywhere). The policy is decided in Rust, before a byte is typed; the agent CLI's
own permission prompt still applies to the tool call itself.

## Consequences

- Claude Code's permission rules cannot tell servers apart (MCP tools take no
  argument patterns), so the per-server decision is ours.
- The audit records decisions, not exit codes.
