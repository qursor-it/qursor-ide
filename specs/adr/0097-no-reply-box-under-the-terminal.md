# ADR-0097 — No reply box under the terminal

Date: 2026-11-15
Status: Accepted. Supersedes the reply-box half of ADR-0091; amends F39.

## Context

ADR-0091 added a reply box under the session's terminal: a field whose text
was pasted into the agent's prompt, followed by Enter. In use it read as a
second input box for the same agent, right under the first. Worse, it could not
do what the prompt does: Claude Code enters shell mode on a typed `!`, and a
pasted `!ls` arrives as text, so `!` commands, `/` commands with their
completion and the prompt's history only work in the terminal. The owner asked
for it to go.

## Decision

The session view has no reply box. The agent's own prompt, in the terminal, is
the one place a reply is typed. The waiting banner stays, with its permission
buttons and "Answer in the terminal", which focuses the prompt. Review mode's
note (F43) stays: it is a one-off message about a file, sent on a click, not a
second prompt.

## Consequences

- The terminal gets back the ~48px the box took.
- `!`, `/` and history behave exactly as in the agent's own CLI.
- Codex and other agents are answered in the terminal, as before ADR-0091.
