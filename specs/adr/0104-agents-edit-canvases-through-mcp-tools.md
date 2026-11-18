# ADR-0104 — Agents edit canvases through MCP tools, and sections are computed by containment

Date: 2026-11-18
Status: Accepted. Feature F44. Builds on ADR-0103 and ADR-0029.

## Context

On a canvas of a few hundred nodes an agent that edits "with its own tools"
rereads the whole JSON every turn and rewrites the whole file for every change.
That is slow, costs tokens and widens the window in which it can overwrite the
user's moves. A user's agent had already grown a per-repo script to list
sections, show a section and find free space.

## Decision

- **Four tools on the existing `factorai` MCP server**: `canvasOutline`,
  `canvasRead`, `canvasFreeSpot`, `canvasPatch`. Claude and Codex sessions both
  already get that server, so no transport changes.
- **Edits are targeted ops, not a rewrite.** `canvasPatch` applies its ops
  all-or-nothing to the parsed document (`serde_json::Value` with
  `preserve_order`, so unknown fields and key order survive), and writes through
  `services::canvas::write` with the hash it read.
- **No blind edits.** The agent passes the `revision` (sha256) it last saw; a
  mismatch is refused with "read again". A clash during the write gets one retry,
  then the same refusal.
- **Sections are containment, not an id prefix.** A node belongs to the smallest
  group that fully contains it, the rule in `jsonCanvas.ts`. A shared fixture is
  asserted from Rust and from vitest so the two cannot drift.
- **The session's tools reach the app through closures**, like routines
  (`CanvasStore` in `terminal.rs`): project and session are fixed per session, and
  `path` is always resolved under `<project>/canvas/` with `resolve_within`.
- **No push into the editor.** The existing canvas watch reloads the board.

## Consequences

- `serde_json` now builds with `preserve_order` for the whole app: objects keep
  insertion order instead of sorting keys.
- The agent is told the tools exist in the server instructions and in the canvas
  context line.
- A patch cannot lose to the user silently, but it can be refused while the user
  is dragging; the agent re-reads and retries.
- The check-then-write gap of ADR-0103 stays; a user save inside it can still be
  overwritten.
