# ADR-0095 — Git mode laid out as the mock-up: repositories in the sidebar, detail on the right

Date: 2026-11-13
Status: Accepted. Amends F35 § Placement and § Layout, ADR-0093.

## Context

After ADR-0093 the top bar's second row existed only for Git mode's tabs, and
the selected commit's detail was docked under the history. The approved
mock-up has neither: the repository is picked at the top of the Git sidebar,
the history has a Session column, and the commit detail is a column on the
right.

## Decision

- **Git tabs become the sidebar's *Repositories* list**: a row per open Git tab
  with the project's tile and name, the branch with ahead/behind, and a close
  `×`; the `+` menu (open repository, filter projects) sits beside the heading.
  Shown with no tab open too, so there is always a way to open one. The top bar
  has one row in both modes.
- **The selected commit's detail is a 24rem column on the history's right**,
  with F41's session strip on top; the resizable bottom dock goes from Git mode
  (the panel's Graph tab keeps its own).
- **The history table gains a Session column** before the author: F41's
  inference for each commit, `✦ <title>`, blank when no session matches.

## Consequences

- Grouping Git tabs by project and folding groups go: a vertical list names the
  project on every row.
- Folders that are no project get no session column entry.
