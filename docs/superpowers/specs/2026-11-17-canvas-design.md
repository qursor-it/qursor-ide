# Canvas: a Miro-like whiteboard per project, shared with the agent — design

Date: 2026-11-17
Mockup: [2026-11-17-canvas/mockup.png](2026-11-17-canvas/mockup.png) (the look the user approved: light theme, Sessions-mode layout, chat on the right).

## Intent

The user wants to brainstorm **with the agent** on a whiteboard. They want to reason on stickies and arrows, select a few of them, ask the agent about them, and see the agent's answer appear **on the board**. The board has to feel like Miro (infinite canvas, stickies, shapes, connectors, frames, pen, minimap) and look like the rest of Qursor IDE: the Sessions screen's rail, sidebar and white cards.

The file is the conversation medium. The agent reads and writes the `.canvas` file directly, as plain JSON, and the open board reloads when the file changes on disk.

## Goals

1. **A third mode in the app rail: Canvas** (next to Sessions and Git). Its sidebar lists the projects, and under each one its canvases. Each project row has "+ New canvas".
2. **Creating a canvas** asks for a title, e.g. "Brainstorm MVP". It writes `<project root>/canvas/brainstorm-mvp.canvas`, creating `canvas/` if it is missing. If the slug is taken, it becomes `-2`, `-3`, and so on. Nothing is ever overwritten.
3. **The center card** is the board. Its header matches the session header: project chip, the file path in mono, the title, an "Agent connected" pill, and pin, more and close actions. The board itself has:
   - a floating left toolbar: select, hand, sticky, rectangle, ellipse, text, connector, frame, pen, file, link;
   - a context toolbar over the selection: colours, group into frame, connector, **✦ Ask the agent**;
   - a minimap and zoom controls bottom-right;
   - dotted background, pan and zoom, multi-select (box and shift), snapping, undo/redo, copy/paste, delete;
   - double-click to edit text, double-click empty space to add a sticky.
4. **The right card is the agent**: the canvas's **real Claude session terminal**, the existing `Terminal` component, not a new chat UI. Under it is a composer. It shows the selected nodes as a chip, `@` cites a node by its text, Enter sends and Shift+Enter makes a new line. Sending pastes into the session a context line plus the user's text (see "Talking to the agent").
5. **Agent-authored nodes are visible**: a blue `AGENT` badge, and a dashed outline until the user touches the node.
6. **The format is JSON Canvas 1.0** (jsoncanvas.org), so the file also opens in Obsidian and stays trivial for an agent to edit.

## Non-goals (v1)

- Real-time multiplayer, comments, voting, timers, templates.
- Exporting to PNG/PDF.
- Rename, duplicate and delete of a canvas from the Canvas sidebar. The file tree and the agent can already do these, and the list follows the folder.
- Embedding images by upload. A `file` node pointing at an image in the project is enough.
- A chat UI with bubbles parsed from the CLI output.
- Canvases outside `<project>/canvas/`.

## Anti-goals (failure even if it "works")

- **Losing an edit**: either the user's unsaved moves or what the agent just wrote.
- **A format only Qursor can read**, or one the agent cannot reasonably write by hand.
- **Dropping fields the editor doesn't know.** Unknown node and edge fields, and unknown top-level keys, must survive a load and save round-trip unchanged.
- A board that looks like a foreign app pasted into Qursor: it must use the `DESIGN.md` tokens and follow the app theme.

## Constraints from the codebase

- **Rail and modes.** `AppMode = 'sessions' | 'git'` (`store/gitModeStore.ts:4`). The rail entries are `ModeSwitch.tsx`. Git mode **hides** the Sessions body instead of unmounting it, so terminals keep running (`AppShell.tsx` ~228, ~303). Canvas mode must do the same.
- **No HTML5 drag-and-drop** (frontend-conventions). React Flow uses pointer events, which is fine. Keep a keyboard path: arrow keys move the selection, Tab cycles nodes, and there are tool shortcuts.
- **Files.**
  - `read_file`, `write_file`, `watch_file`/`unwatch_file` exist; `write_file` does not check what is on disk before writing (`commands/files.rs`). `write_file` refuses a missing parent directory on purpose.
  - There is **no** create-directory command, so canvas creation needs its own backend command.
  - `FileWatch` watches **one path at a time**, non-recursively, and emits `file:changed`.
  - `useEditBuffer` (`hooks/useEditBuffer.ts:86`) already owns dirty state, save, the "changed on disk" conflict, and deleted-file handling. It is shared with the file and diff views, and it only notices a conflict after the re-read, so the canvas does not use it (see Design).
- **Path safety.** The file commands take any absolute path. Canvas commands must resolve under the project root with `services/ide/scope.rs::resolve_within`.
- **Sessions.**
  - `start_session(project_id, agent?, fresh?)` mints an id without spawning a PTY. The PTY spawns when a `Terminal` mounts.
  - `sendToSession(sessionId, text, submit)` (`Terminal.tsx:953`) pastes into a live session. It returns `false` when no PTY is live.
- **DB.** Migrations are registered in `db/mod.rs`. The fork numbers 9001 and 9002 exist, so this is **9003**.
- **Types** cross the boundary through `packages/types/src/index.ts`, hand-mirrored with the Rust structs.
- **Specs.** Features go in `specs/05-features.md` (the last one is F43, so this is **F44**). ADRs go in `specs/adr/` (the last is 0102, so this is **ADR-0103**). The data model change goes in `specs/02-data-model.md`. Follow the `spec-and-adr-workflow` and `pull-requests` skills for branch, worktree and PR.

## Approaches considered

| | Approach | Trade-off | Verdict |
|---|---|---|---|
| A | **React Flow (`@xyflow/react`, MIT) + JSON Canvas**, with custom node components for the Miro look | The graph model maps 1:1 onto JSON Canvas: nodes, edges, groups. The Miro look and the pen are ours to build. | **Picked** (user approved) |
| B | tldraw SDK | The closest to Miro out of the box. Since v4, production needs a paid commercial key: without one the editor stops rendering after 5 s, which is incompatible with this MIT app. Its store format is not agent-friendly. | Rejected |
| C | Excalidraw (MIT) | Great freehand, but a sketchy style far from Miro, its own `.excalidraw` format, and noisier JSON for an agent. | Rejected |

Chat placement: right was picked over bottom. It keeps the board's full height and mirrors the session screen's right panel.

## Design

### File format

JSON Canvas 1.0, pretty-printed with 2-space indentation and keys in a stable order, so diffs stay readable and the agent's edits stay small.

```json
{
  "title": "Brainstorm MVP",
  "nodes": [
    { "id": "a1", "type": "text", "x": 0, "y": 0, "width": 160, "height": 160,
      "color": "3", "text": "Create a league in 2 minutes", "x-kind": "sticky" },
    { "id": "f1", "type": "group", "x": -20, "y": -40, "width": 520, "height": 380,
      "label": "Users & problems" },
    { "id": "s1", "type": "text", "x": 700, "y": 0, "width": 170, "height": 70,
      "text": "Create league", "x-kind": "shape", "x-shape": "rect" },
    { "id": "k1", "type": "text", "x": 0, "y": 500, "width": 200, "height": 120,
      "text": "", "x-kind": "ink", "x-points": [[0,0,0.5],[4,2,0.6]] },
    { "id": "g1", "type": "text", "x": 900, "y": 0, "width": 160, "height": 160,
      "text": "Event log + deterministic recompute", "x-kind": "sticky", "x-author": "agent" }
  ],
  "edges": [
    { "id": "e1", "fromNode": "a1", "fromSide": "right", "toNode": "s1", "toSide": "left",
      "toEnd": "arrow", "label": "" }
  ]
}
```

- **Standard types** (`text`, `file`, `link`, `group`) keep their spec meaning. A `group` is a Miro **frame**.
- **Miro-only concepts** are `text` nodes with `x-` fields, so Obsidian still shows them as text cards:
  - `x-kind`: `sticky` | `shape` | `ink`;
  - `x-shape`: `rect` | `ellipse` | `diamond`;
  - `x-points`: the pen strokes, relative to the node;
  - `x-author`: `"agent"`.
- `title` is an extra top-level key. If it is missing, the UI shows the file name.
- **Colours** are the spec's presets `"1"`–`"6"` or hex. The editor's sticky palette maps onto them so they read the same in Obsidian.
- **Round-trip rule:** the mapper keeps every unknown field, at node, edge and top level, and writes it back unchanged.
- Still unverified: whether Obsidian preserves `x-` fields when *it* saves. If it doesn't, Qursor still reads the result: the nodes fall back to plain text cards.

### Backend (Rust)

New `commands/canvas.rs` and `services/canvas.rs`:

- **`list_canvases(project_id) -> Vec<CanvasEntry>`**
  - Lists `<root>/canvas/*.canvas`, non-recursively, sorted by modified time with the newest first.
  - Reads `title` from each file cheaply: parse the JSON and take only `title`. If parsing fails, fall back to the file name and do not error.
  - A missing folder returns an empty list.
- **`create_canvas(project_id, title) -> CanvasEntry`**
  - Slugifies the title: ASCII-fold accents, lowercase, non-alphanumerics become `-`, collapse and trim, cap at 60 characters. An empty slug becomes `canvas`.
  - `create_dir_all(<root>/canvas)`.
  - Picks the first free name among `slug`, `slug-2`, `slug-3`, …
  - Writes `{"title": <title>, "nodes": [], "edges": []}` with `create_new`, so nothing is ever overwritten, even under a race.
  - Validates the path with `resolve_within(root, …)`.
- **`CanvasEntry { project_id, path (absolute), rel_path, slug, title, modified_ms }`**, mirrored in `packages/types`.
- **Canvas ↔ session link.** `get_canvas_session(project_id, rel_path) -> Option<String>` and `set_canvas_session(project_id, rel_path, session_id)`, backed by migration **`9003_canvas_sessions.sql`**:
  - `canvas_sessions(project_id TEXT, rel_path TEXT, session_id TEXT, updated_at INTEGER, PRIMARY KEY(project_id, rel_path))`.
  - The link lives in the local DB, not the file, because session ids are machine-local and the file may be committed.
  - If the linked session no longer exists, treat it as unlinked.
- **`write_canvas(path, contents, expected_sha256: Option<String>) -> Result<String /* new sha */, Conflict>`**: path scoped to the project's `canvas/`. It compares the disk content's sha256 with `expected` before writing, and keeps `write_file`'s atomic-write and permission behaviour (reuse its internals).
- **Watch slots.** `FileWatch` is single-slot today, so a canvas watch would silently steal the viewer's. Give it **keyed slots** (`"viewer"`, `"canvas"`): `watch_file(path, slot?)` with the existing callers defaulting to `"viewer"`. Each event carries its slot. Record this in ADR-0103, since it reverses the one-path choice.
- Unit tests: slugify (accents, emoji, empty, long), unique-name selection, refusing paths outside the root.

### Frontend

- **Mode.**
  - Extend `AppMode` with `'canvas'` and add a third entry to `ModeSwitch` (lucide `Shapes`, label "Canvas", `data-testid="mode-canvas"`).
  - `AppShell` hides the Sessions body in both Git and Canvas modes and renders `<CanvasModeView />`, the way it renders `<GitModeView />`.
  - The selected canvas lives in a small Zustand store: `{ projectId, path } | null`, persisted like `gitModeStore`. There is no new route, matching how Git mode works.
- **`CanvasSidebar`.**
  - Projects in the user's sidebar order (reuse `SidebarRow` order and groups if cheap, otherwise a flat list in that order).
  - Under each project, its canvases from `list_canvases`, a TanStack Query keyed per project, refetched on window focus and every 15 s like the session list, and invalidated after create.
  - "+ New canvas" opens a small dialog (`@factorai/ui` Dialog + Input) asking for the title, then calls `create_canvas` and selects the result.
- **`CanvasView`** (center card), lazy-loaded through the `lazyViews.ts` pattern so React Flow stays out of the main bundle.
  - Loads with `read_file`.
  - **Its own save path, not `useEditBuffer`** (builder-check call, see "Calls made after review"). The buffer is the React Flow state. Autosave writes about 600 ms after the last change, on blur and on mode switch, through `write_canvas(path, contents, expected_sha256)`. That command refuses the write when the file on disk no longer matches the hash of the last version the board loaded or saved. This closes the race between autosave and an agent's write that a check-after-the-fact would leave open. The header shows "Saved / Saving… / Unsaved", and Ctrl+S saves now.
  - **Watches** the open file through its own watch slot (see the backend).
  - **When the file changes on disk:**
    - nothing unsaved: reload silently, and mark node ids that weren't there before as **new**: dashed outline, plus the badge if `x-author: "agent"`;
    - unsaved edits, or a `write_canvas` refused: keep the local state and show a banner: "The canvas changed on disk (the agent?)", with **Reload** (drops local edits) and **Keep mine** (overwrites). Never auto-merge in v1.
  - **The canvas session is an ordinary, visible session** (user's decision). It shows up in the Sessions list with an origin label "◇ <canvas title>", the way Git mode labels the sessions it starts (`gitModeStore.origin`), except persisted through `canvas_sessions`, so it survives a restart. The user can continue it from either place. Only one mode is visible at a time, so the builder must make the xterm pool's single DOM host per session move between the session view and the canvas panel (reparent on mode switch) rather than mount it twice.
- Touching a "new" node (select + move or edit) clears its dashed outline. That state is in memory only.
- **Mapping** (`lib/jsonCanvas.ts`): `parse(text) -> {doc, rfNodes, rfEdges}` and `serialize(doc, rfNodes, rfEdges) -> text`. A pure module, round-trip tested with vitest, including unknown-field preservation and pretty-print stability.
- **Node components:** Sticky, Shape, Text, Frame (group), File (project file path; double-click opens it in the viewer), Link, Ink. The pen draws with pointer events into an `ink` node, and strokes are smoothed with `perfect-freehand` (MIT, small). That dependency is the builder's call.
- **Theme:** colours from `DESIGN.md` tokens (card, border, primary, muted). Sticky colours are fixed pastels, readable in both themes.
- **Agent panel** (right card, resizable, collapsible; the width is persisted):
  - It mounts the existing `Terminal` for the linked session.
  - With no linked session, the panel shows the composer and a short line: "The first message starts a Claude session for this canvas."
  - On first send: `start_session(projectId, 'claude')`, then `set_canvas_session`, then mount `Terminal`, then send once the PTY is live (`sendToSession` returns `true`). Retry briefly, then show an error.

### Talking to the agent

The composer pastes one message into the session:

```
[canvas: E:\code\billing-api\canvas\brainstorm-mvp.canvas · selected nodes: a1, b7 · JSON Canvas 1.0; mark nodes you add with "x-author":"agent"; keep fields you don't know]
<user's text>
```

- The path is **absolute** (the project root's copy), so a session that moved into a worktree still edits the file the board watches.
- The `@` citation inserts a node's id with its first words, e.g. `@a1 "Create a league…"`.
- "✦ Ask the agent" in the context toolbar focuses the composer with the selection already attached.
- The agent edits the file with its normal tools. The watcher plus silent reload bring the result onto the board.
- If the user is mid-edit (dirty), they get the conflict banner instead of a lost edit. Autosave keeps that window short.

### Specs and docs, in the same PR

- **F44** in `specs/05-features.md`.
- **ADR-0103**: Canvas mode, JSON Canvas with `x-` extensions, React Flow, and tldraw rejected for its license.
- `canvas_sessions` in `specs/02-data-model.md`.
- The new commands in `specs/03-backend-rust.md`.

## Testing

- **Rust:** the slugify, unique-name and path-scope unit tests above, plus migration 9003 applying cleanly.
- **Rust:** `write_canvas` refuses on a hash mismatch; the two watch slots don't evict each other.
- **Vitest:** `jsonCanvas` round-trip (unknown fields, colours, groups, ink), and new-node detection on reload.
- **Smoke** (`tests/smoke/`):
  1. Switch to Canvas mode, create "Brainstorm MVP" in a fixture project, and assert `canvas/brainstorm-mvp.canvas` exists with the title.
  2. Add a sticky, wait for autosave, and assert the file contains it.
  3. Write a node with `x-author: "agent"` to the file externally, and assert the `AGENT` badge appears.

## Left to the builder

- Exact icon, dialog copy, and the default sizes of nodes and the panel.
- The undo/redo implementation (React Flow has none built in): a simple snapshot stack is fine.
- Whether "Users & problems"-style frames move their children (React Flow `parentId`) or are visual-only. Recommended: they move their children, like Miro.
- The sidebar's reuse of `SidebarRow` grouping versus a flat ordered list.
- How the slice is split into commits and PRs. A suggested order:
  1. backend + mode + sidebar + empty board;
  2. editing + autosave;
  3. agent panel;
  4. pen.

## Calls made after review

- **Visibility of the canvas session**: visible in Sessions mode with a canvas origin label (decided by the user).


A builder read this document; these are the calls that came out of it:

- **Conditional save** (`write_canvas` with an expected hash) instead of reusing `useEditBuffer`. Without it, a debounced autosave could overwrite an agent's write that landed between the write and the watcher's re-read, which breaks the "never lose an edit" anti-goal. Not changing `useEditBuffer` keeps the file and diff views untouched.
- **Keyed watch slots** in `FileWatch` instead of having the canvas take over and hand back the viewer's single watch, which is fragile and silently breaks the viewer's live refresh.
- **Absolute path** in the context line, so worktree sessions edit the watched file.
