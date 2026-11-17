# ADR-0103 — Canvas mode: JSON Canvas files on a React Flow board

Date: 2026-11-17
Status: Accepted. Feature F44.

## Context

The owner wants a Miro-like board to think with, next to the project's Claude
session. The agent has to be able to read and write the board with the tools it
already has, and a board must survive the app, git and a hand edit.

## Decision

- **Canvas is a third `AppMode`**, beside Sessions and Git. Sessions terminals
  stay mounted while it is hidden.
- **A canvas is a file**, `<project>/canvas/<slug>.canvas`, in JSON Canvas 1.0.
  Extras use the `x-` prefix (`x-kind`, `x-shape`, `x-points`, `x-author`) and
  every unknown field round-trips unchanged, so an agent's additions survive.
- **The board is `@xyflow/react`** (MIT). tldraw was rejected: it needs a
  production licence key and the editor stops rendering without it.
- **Writes are conditional.** `write_canvas` compares the sha256 of the disk with
  the hash the editor last read or wrote and refuses on a mismatch. A file that
  is gone counts as a mismatch and is not recreated. The write itself is
  `services::files::write_file`, so atomicity and permissions are shared. The
  window between the check and the write is accepted.
- **A second `FileWatch` instance** (`AppState.canvas_watch`, event
  `canvas:changed`) watches the open canvas. This replaces the design's "keyed
  slots" with less code and leaves the viewer's watch alone.
- **The canvas-to-session link is in the database**, `canvas_sessions` (migration
  `9003`), keyed by project and relative path, with no foreign key to `sessions`.

## Consequences

- A hand-edited or agent-broken file shows an error card and is never
  autosaved over.
- The agent writing while the user drags gives a conflict banner, not a lost edit.
- One more table, one more watch, and a new dependency.
