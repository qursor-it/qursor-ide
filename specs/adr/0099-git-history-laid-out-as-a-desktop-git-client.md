# ADR-0099 — Git mode's history laid out as a desktop Git client

Date: 2026-11-16
Status: Accepted. Supersedes ADR-0095's "commit detail in a column on the right"
and ADR-0098's table rows; amends F35.

## Context

ADR-0095 put the selected commit's detail in a column right of the history, as
the redesign's mock-up drew it. Using it, the owner found the subject column
squeezed between the graph and that column, the rows too airy, and no clear
sign of which commit HEAD is on. They pointed at a desktop Git client (Fork):
a dense full-width list with author, sha and full date, the selection filled,
and the commit's details docked underneath.

## Decision

- The history table takes the card's full width with 24px rows; columns are
  Graph, Subject, Session (only when a commit has one), Author, SHA, Date.
- The conventional-commit prefix of a subject is bold; the selected row is
  filled with the accent; HEAD's subject is bold; commits HEAD lacks are dimmed.
- The commit detail is docked under the table at a resizable height kept in
  `gitModeStore`: author, refs, SHA and parents as labelled rows, the message,
  the files; the session card, the ✦ questions and the actions in a column on
  its right.
- No hover card on Git mode's rows; the panel's Graph tab keeps its own.

## Consequences

- The panel's Graph tab and Git mode now render `CommitRow` differently
  (`columns`); the shared component carries both.
- The detail no longer takes width from the subject; it takes height instead,
  and the handle gives that back.
