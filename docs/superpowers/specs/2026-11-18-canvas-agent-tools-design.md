# Canvas agent tools: navigate and edit big canvases without rereading the file — design

Date: 2026-11-18
Builds on: F44 Canvas (`docs/superpowers/specs/2026-11-17-canvas-design.md`), the `factorai` agent-tools MCP server (`services/agent_tools/`).
Origin: the user's game-design canvas in another project (~250 nodes). An agent there wrote a per-repo script (`sections` / `show <prefix>` / `free <w> <h>`). The user wants this to become a standard part of the editor instead.

## Intent

On a big canvas the agent should read only what it needs and change only what it means to change. Today it rereads the whole JSON every turn and rewrites the whole file for every edit. That is slow and costs tokens. It also widens the window in which it can clash with the user's edits.

## Goals

1. **Four tools on the existing `factorai` MCP server**, available to Claude **and Codex** sessions. Both already get the server: Claude through `--mcp-config`, Codex through `-c mcp_servers.factorai.*`.
   - `canvasOutline({ path? })` returns the title, node and edge counts, and the **sections**. Each section has its group id, label, bbox, card count and cards per colour. Nodes in no group are listed as a "loose" bucket with their count and bbox. It also returns the file's `revision` (sha256).
   - `canvasRead({ path?, groupId? , nodeIds? })` returns only those nodes (id, type, x-kind, text, colour, x, y, width, height, x-author) and the edges that touch them, marked incoming or outgoing.
   - `canvasFreeSpot({ path?, width, height, near? })` returns an `{x, y}` where a box of that size overlaps nothing. With `near` (a group id) it looks next to that group first.
   - `canvasPatch({ path?, ops, revision? })` applies targeted edits in one atomic write:
     - the ops are `addNode`, `updateNode(id, fields)`, `removeNode(id)` (which also removes the node's edges), `addEdge`, `updateEdge(id, fields)` and `removeEdge(id)`;
     - unknown fields are preserved;
     - added nodes get `"x-author": "agent"`.
2. **Sections by containment, not by id prefix.** A node belongs to the smallest group that fully contains it, the same rule as `jsonCanvas.ts`. Cards the user drew by hand count too.
3. **`path` is optional.** It defaults to the canvas this session is linked to in `canvas_sessions`. It is always resolved under `<project root>/canvas/` with `resolve_within`.
4. **The agent is told the tools exist:**
   - the server instructions get one paragraph on them;
   - the context line and the linking primer get `prefer the canvas* tools over reading or rewriting the whole file`.

## Non-goals

- Pushing patches straight into the open editor without going through the file. The board keeps reloading from disk exactly as today, so a user with unsaved edits still gets the existing Reload / Keep mine banner.
- An outline in every message. The tools make it unnecessary.
- Shipping the per-repo script as a skill. The tools replace it.
- Layout algorithms beyond "first free spot".

## Constraints from the codebase

- **How the server works.** It is a loopback streamable-HTTP server, one per session (`server.rs`). Tools are declared inline in `tools_list()` and dispatched by name in `tools_call()` (`tools.rs`).
- **How tools reach the app.** Tools have no `AppState`. They reach the app through closures captured in `start_agent_tools` (`terminal.rs`), the way `RoutineStore` does. Project id, session id and cwd are fixed per session, so tool arguments cannot name another project.
- **Reading and writing.** `services/canvas.rs` already has pure `read` and `write` functions. `write` takes `expected_sha256` and returns `Conflict` on a mismatch. The patch reuses it.

## Design

- A new `services/canvas_tools.rs` with pure functions over the parsed document (`serde_json::Value`, so unknown fields survive):
  - `outline(doc) -> Outline`
  - `read_part(doc, sel) -> Part`
  - `free_spot(doc, w, h, near) -> (x, y)`
  - `apply_ops(doc, ops) -> Result<Value, OpError>`
- **`free_spot` searches simply.** It tries candidates to the right of and below `near` (or of the whole bbox), on a 40 px grid, and returns the first one with no overlap and a 40 px margin. `shortcut:` if the search finds nothing, it falls back to the right of everything.
- **`apply_ops` is all or nothing.** A missing id, a duplicate id on add, or an edge to a missing node fails the whole batch, and the error names which op failed.
- **What `canvasPatch` does:**
  1. Read the file and its sha.
  2. If the agent passed `revision` and it differs from the sha, return a conflict: "the canvas changed since you read it; call canvasOutline/canvasRead again". That way the agent does not edit blind on top of the user's moves.
  3. Apply the ops.
  4. Write through `services::canvas::write` with the sha read in step 1. If that conflicts, the file changed during these milliseconds: retry once, then return the conflict.
  5. Pretty-print the result the same way the editor does: 2 spaces, stable key order.
- **Wiring.** A `CanvasStore` closure set (read, write, linked path) is captured in `start_agent_tools`, next to `RoutineStore`. The four tools go into `tools_list()` and `tools_call()`.
- **Linked canvas.** A reverse lookup `canvas_for_session(project_id, session_id)` on `canvas_sessions`.

## Testing

- Rust unit tests in `canvas_tools.rs`, run on one fixture (a group with cards, a nested group, loose cards, edges):
  - containment, including a node inside a nested group;
  - per-colour counts;
  - `read_part` edges in and out;
  - `free_spot` returns no overlap, and goes next to `near`;
  - `apply_ops` preserves unknown fields and sets `x-author`, and a failing batch leaves the document unchanged;
  - `removeNode` also removes its edges.
- Tests in `tools.rs`: the tool list includes the four tools, plus one `tools/call` round trip per tool on a tempdir canvas, including a `revision` conflict.
- **Parity check:** a vitest test and a Rust test assert the same parent assignment on a shared fixture file. This keeps the containment rule identical on both sides.

## Specs

- F44 gets an "Agent tools" paragraph.
- New ADR-0104 records that agents edit canvases through MCP tools, not whole-file rewrites, and that sections are computed by containment.
- `specs/03-backend-rust.md` lists the new tools.

## Left to the builder

- The exact JSON shapes of the outputs (compact, with stable keys).
- The wording of the tool descriptions.
- Whether `canvasRead` without a selector returns the loose bucket or an error. Recommended: an error that suggests `canvasOutline`.
