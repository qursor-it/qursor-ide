# ADR-0101 — No banner for an ordinary wait

Date: 2026-11-16
Status: Accepted. Amends ADR-0091 and F39.

## Context

ADR-0091's waiting banner showed "<agent> is waiting for you" over the terminal
whenever a session waited and the bottom of its terminal was not Claude's
permission menu. The owner found it unnecessary: the terminal shows the
question, and the status dot and tab already say the session waits.

## Decision

The banner appears only for Claude's numbered permission menu, with its
question and a button per option. Any other wait shows nothing over the
terminal.

## Consequences

- One strip less over the terminal on most waits.
- Codex and other agents never get a banner: their waits are read in the
  terminal and from the dot.
