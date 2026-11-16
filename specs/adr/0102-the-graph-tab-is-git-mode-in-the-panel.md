# ADR-0102 — The panel's Graph tab is Git mode in the panel

Date: 2026-11-16
Status: Accepted. Amends F18 and F35.

## Context

The owner wanted git's quick jobs (fetch, pull, push, commit) without leaving
Sessions mode. A separate Git tab beside PRs was tried and judged worse than
redoing the Graph tab, which already showed the history in a different design
from Git mode's.

## Decision

- The panel's Graph tab renders Git mode's pieces for the active checkout: the
  action bar, the history table with the commit docked under it, Local changes
  with the commit box, and the op log, with a Commits / Changes switch and a
  link to Git mode.
- Columns and the detail's layout follow the container's width, so the panel's
  existing resize handle is how you get more of it.
- The old graph view, its working row and its tabbed commit detail are removed.
- Git's confirm and error dialogs move to the shell so both places share them.

## Consequences

- One design for history in the app, in two sizes.
- At the panel's default width the table shows graph and subject only.
- The hover card and the working-changes row no longer exist anywhere.
