# ADR-0112 — An agent types only onto a clean prompt, and only what is shown

**Date.** 2026-11-19
**Status.** Accepted. Supersedes the typed-line shape in ADR-0109 § 2 and adds
to its turn rules; the rest of ADR-0109 stands. Arises from an independent
review of F46.

## Context

Review of F46 found ways an agent's line could do something the human did not
see or did not mean: a bare `\r` inside a command is a second line to readline
(and an invisible one in the confirmation banner); a command typed after the
human's half-typed text is appended to it; a confirmation can wait ten minutes
while the terminal changes hands; a lost marker function made the command run
with no start marker; and a human Ctrl+C left the agent waiting out its timeout.

## Decision

1. **No control characters** in an agent's command or literal keys, except
   newline and tab; named keys (`<ctrl-c>`) are the only way to send one.
2. **The typed shape is ` __qs N && { CMD\n}; __qe N`**: if the marker
   functions are gone (`exec bash`, `su -`, a nested ssh), nothing runs; the
   missing start marker triggers one fresh bootstrap and one retry.
3. **A clean prompt line.** The human's keystrokes are tracked: printable input
   after the last Enter, Ctrl+C or Ctrl+U is a half-typed line, and the agent
   refuses to type until it is cleared.
4. **Everything is judged again after a confirmation**: still lent, at rest,
   the human not typing.
5. **The human's Ctrl+C ends the agent's wait** and is reported as an interrupt.
6. **Only command lines are editable** in a confirmation; file operations and
   keys are run or refused as shown, and a write shows the content it writes.
7. Only the run's own nonce marks the shell as bootstrapped; a stray `ESC ]` in
   output is abandoned at the next ESC instead of swallowing a marker.

## Consequences

- A human who leaves text at the prompt blocks the agent until they clear it,
  which is the intended trade.
