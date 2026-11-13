# ADR-0093 — The app rail, and a column that shows the Inbox or the projects

Date: 2026-11-13
Status: Accepted. Supersedes ADR-0038's collapsed sidebar rail; amends F1, F35's
placement of the `Sessions | Git` switch, F36 and ADR-0090's layout.

## Context

The owner compared the shipped redesign with the mock-ups it was approved from
and found it still read as the old app: a project tree down the left with the
Inbox squeezed above it, and the mode switch in the top bar. The mock-up has a
column of places at the window's edge and the Inbox as the main list.

## Decision

- **An app rail, 56px, always on screen** at the window's left edge, in both
  modes: the `Sessions | Git` switch as icons (the Sessions icon is the Inbox;
  accessible names `Sessions` and `Git` unchanged), between them **Projects**
  and the **Board**, then a 26px tile per project with its state badge (F10,
  F32), **Add a project**, and **Settings** at the foot. Pressed items take the
  `background` fill and `shadow-sm` — a deliberate exception to icon buttons
  painting no background, because the rail is navigation and its place has to
  read at a glance.
- **The column beside it shows one list**: the Inbox (default) or the project
  tree, picked from the rail and persisted (`sidebarStore.view`). Collapsing the
  sidebar hides the column; the rail stays. `Mod+F`'s sidebar search shows the
  tree.
- The Inbox is that column's whole height, under an `Inbox` header with the
  collapse control, and with nothing open it says so and offers the projects.
- The open list's keys (`Mod+W`, `Mod+PageUp/PageDown`) and their confirm are
  mounted once at the shell, so they work whichever list is showing.
- The top bar keeps the brand, the palette's door and the panel toggle.

## Consequences

- ADR-0038's collapsed rail (glyphs with hover cards, its own search and
  footer menu) is removed with its tests; the app rail replaces it.
- Smoke specs start on the project tree (`installMockBridge` seeds it), and the
  ones about the Inbox open it from the rail.
