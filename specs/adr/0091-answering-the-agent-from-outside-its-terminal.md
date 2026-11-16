# ADR-0091 — Answering the agent from outside its terminal

Date: 2026-11-13
Status: Accepted. Extends ADR-0090's redesign; amends F3 (the session view).

## Context

Until now everything the agent received came from keys typed into its xterm
(plus file drops, ADR-0083, and paste). The redesign review found that a
session waiting for a permission answer looks almost like one that is thinking:
the question is a few lines at the bottom of a terminal, and answering it means
finding them, clicking into the terminal and pressing the right number. The
owner asked for the mock-up's waiting banner and reply box to be built.

## Decision

- **The app may send input to an agent's PTY on an explicit click or Enter by
  the user, and never otherwise.** No timer, status change or parsed output
  sends anything.
- **Text goes through `term.paste`**, the path a file drop already uses, so
  bracketed paste applies where the agent asked for it and the bytes leave
  through `onData` exactly as typing does. **A menu choice is one keystroke
  through `term.input`**, because a pasted digit is text, not a choice.
- **Permission menus are recognised, not assumed.** The banner reads the last
  lines of the xterm buffer and offers buttons only when they parse as a
  "Do you want to …?" question followed by options numbered from 1. Anything
  else gets "answer in the terminal" and the reply box, never a guessed key.
- The terminal stays the source of truth: everything sent appears in it, and the
  terminal remains fully usable for anything the banner does not cover.

## Consequences

- A change in Claude Code's menu wording or layout degrades the banner to its
  generic form; it cannot press a wrong key, because it offers keys only for a
  menu it parsed.
- Codex and other agents get the generic banner and the reply box.
- The session view loses ~48px of terminal height to the reply box.
