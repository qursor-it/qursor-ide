# ADR-0117 — Every sidebar opens with what is in progress

**Date.** 2026-11-22
**Status.** Accepted. Builds on ADR-0115.

## Context

ADR-0115 gave the four modes one sidebar frame, but each mode still laid out its
body in its own way. Sessions had a search field and an uppercase `PROJECTS`
heading. Git had no filter, and its headings were bold title-case rows. Canvas
had a `+` with no label and no filter. Servers had an uppercase heading with its
`+` in it. Switching mode meant reading a different layout each time, and what
needed you (a session waiting, a conflict, a command waiting for confirmation)
showed up only as badges on the rail.

## Decision

Every mode's body has the same three parts, top to bottom (`SidebarParts.tsx`):

1. **A toolbar row**: a filter field, and beside it the mode's one primary
   action as a filled `+` button: New session (in the active project), New branch
   (from HEAD), New canvas, New server. Sessions keeps its search, which opens
   the search page (F1). The other three filter their own lists as you type.
2. **"In progress"**, drawn only while something is happening, outside the
   scroller so it stays in view, capped at five rows plus `N more…`:
   - Sessions: the Inbox's entries (F36), needing you first, then working.
   - Git: conflicts in the active repository, and a merge, rebase, cherry-pick
     or revert left unfinished.
   - Canvas: canvases with edits not yet on disk.
   - Servers: agent commands waiting for a yes or no (ADR-0110), then each
     server with SSH sessions open.
3. **The mode's library**, in sections with the same heading:
   `SidebarSectionHeader`, a 12px uppercase label with IconButton actions at the
   right. In Git the heading also collapses its section.

Alternatives considered: a project selector shared by all modes, so switching
mode keeps the project. Rejected for now: it changes how Sessions and the Git
tabs work, not only how they look.

## Consequences

- A new mode uses `SidebarToolbar`, `LiveSection` and `SidebarSectionHeader`
  instead of building its own header.
- "In progress" reads from state that already exists: terminal statuses, the
  repository state the rail's badge polls, the canvas save buffer, the server
  store. It adds no command and no polling of its own.
- Servers' `+` moved from its heading into the toolbar, and Canvas's `+` from
  its own row. Their test ids (`new-server`, `new-canvas-top`) moved with them.
