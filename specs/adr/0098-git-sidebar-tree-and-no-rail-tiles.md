# ADR-0098 — A tree in Git mode's sidebar, and no project tiles on the rail

Date: 2026-11-16
Status: Accepted. Amends ADR-0093 (the app rail), ADR-0095 (Git mode as the
mock-up) and F35, F36.

## Context

The owner used the redesign on real work and reported:

- The rail's project tiles are initials, which do not tell a dozen projects
  apart at a glance; the project tree and the Inbox already name them.
- Git mode's sidebar listed every ref flat, with no way to fold a section, and
  the Repositories list could not be given more or less room. The owner pointed
  at a desktop Git client's sidebar: collapsible sections, branches in folders
  by prefix, the current one bold with a check.
- Local changes had a Stage/Discard button on every row and no way to act on
  several files at once.
- The Inbox's rows led with the session title, while the owner finds a row by
  its project.

## Decision

- The rail keeps its places (Inbox, Projects, Board, Git, Settings) and loses the
  project tiles and the add button; adding a project is in the Projects header.
- Git mode's sidebar is a tree: collapsible section headings, branch folders by
  `/` prefix, remotes as nodes closed by default, the collapsed set and the
  Repositories height kept in `gitModeStore` (layout, ADR-0013).
- Local changes pick rows with click, Ctrl/Cmd+click and Shift+click; the list
  heading acts on the picked rows or on all; a double-click stages or unstages
  one row; rows have no buttons.
- The history table and commit detail follow the mock-up more closely: column
  heads, 36px rows, the detail's Ask and Actions sections.
- An Inbox row leads with the project's tile and name, the session title under it.

## Consequences

- A project is reached from the Projects tree, the Inbox, the palette or the
  Board; nothing on the rail stands for one.
- Discarding a file means picking it first (or using Discard all), one click
  more than a row button, and always confirmed.
- The panel's Graph tab keeps its own 26px rows; only Git mode's table grows.
