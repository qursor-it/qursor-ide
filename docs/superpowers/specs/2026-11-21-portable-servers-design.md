# Servers that live in the project repo — design

## Intent

The SSH servers of a project should travel with the project. Today every server
is a row in the app's SQLite database (`servers`, F45), global and tied to one
machine. The human wants a server to be a file in the project repository,
committed with the code, so that:

- they find the same servers on every computer they work on;
- a colleague who clones the repo finds the project's servers ready;
- a repo that holds nothing but server files works as a shared "address book"
  (opened as a project like any other).

## Goals / non-goals / anti-goals

- Goal: one file per server, `<project>/servers/**/<slug>.server`, readable,
  diff-friendly, committable. Sub-folders are the address book's folders.
- Goal: the IDE reads, creates, edits, moves and deletes those files; a change
  made outside the IDE (git pull, editor, file explorer) shows up by itself.
- Goal: Servers mode's sidebar shows the servers kept on this computer, then
  every project that has at least one `.server` file. Canvas mode's sidebar
  follows the same rule: only projects with at least one canvas.
- Non-goal: secrets in the repo. Passwords, keys and passphrases stay in each
  machine's vault (ADR-0106); each machine enters them once.
- Non-goal: migrating today's servers. They stay in the database, shown as
  *On this computer*; the human moves the ones they want, one at a time.
- Non-goal: syncing through anything but the files (no cloud, no git commands
  run by the IDE).
- Anti-goal: a file from a cloned repo weakening a safety check. A `.server`
  file never skips host-key confirmation (F45), and an unknown or changed host
  key is handled exactly as for a database server.
- Anti-goal: losing data. A file that is not valid JSON is shown as an error and
  is never written over; unknown fields are written back unchanged; an existing
  file is never overwritten by a create.

## Approaches

- **A. The file is the source of truth, live (picked).** Project servers are
  read from the files on every listing; IDE edits write the file. Always in step
  with git; the canvas file pattern already exists to copy. Cost: two kinds of
  server behind one id space.
- B. Export / import buttons. Little code, but imported copies drift from the
  file and duplicate on re-import; colleagues never see later changes.
- C. Read-only auto-import. In step with git, but nothing can be edited from
  the IDE; anyone who doesn't know the format is stuck.

The human picked A, and asked for one file per server (fewer merge conflicts,
per-server history) instead of one `servers.json`.

## The file

`<project root>/servers/<folder…>/<slug>.server`, JSON, UTF-8, written with a
trailing newline and 2-space indent:

```json
{
  "id": "8f3c2a1e-…",
  "name": "staging",
  "host": "staging.example.com",
  "port": 22,
  "user": "deploy",
  "startDir": "/srv/app",
  "environment": "staging",
  "agentPolicy": "confirm",
  "login": "keyFile",
  "keyPath": "~/.ssh/id_ed25519",
  "persistent": true,
  "tunnels": [
    { "id": "c1d2…", "label": "db", "localPort": 5433, "destHost": "localhost", "destPort": 5432 }
  ]
}
```

- Fields mirror the `servers` / `server_tunnels` rows (F45, F48, F49) with the
  same validation (`services/servers.rs` `validate`). Required: `id`, `name`,
  `host`, `user`, `login`. Defaults when missing: `port` 22, `environment`
  `dev`, `agentPolicy` `free`, `persistent` false, `tunnels` [].
- `keyPath` is written with `~/` when it lies under the home directory, and
  `~` is expanded on read, so the path works on every machine.
- **Unknown fields are kept** and written back unchanged (as canvases do).
- **The folder** is the file's directory relative to `servers/`
  (`servers/clienti/acme.server` → folder `clienti`). One level of nesting is
  shown, as in today's book; deeper paths show their full relative path as the
  folder name.
- **The slug** follows the canvas rule (`services/canvas.rs` `slugify`): folded
  to ASCII, lowercased, other runs as `-`, at most 60 characters, `server` when
  empty; a collision becomes `-2`, `-3`, and a create never overwrites.
  Renaming a server in the IDE does not rename its file (the path is not the
  identity; renaming files churns git history). *Builder's call left open: a
  "rename file to match" action is not required.*

## Identity

The `id` inside the file is the server's id everywhere the app already keys by
server id: vault entries (`server:<id>:<kind>`), the connection pool
(`ssh/pool.rs`), SSH terminals (`terminal.rs`, id in `project_id`), agent
policy and audit (`agent_policy.rs`), tunnels, `server_agent_project`
(`data_dir/servers/<id>`), and the renderer's tabs (`serverStore.ts`,
`serverId`). Keeping the id in the file means renaming or moving the file loses
nothing, and the vault on each machine finds its secrets again.

- **The scan never rewrites an id** (decided after review: a second checkout
  of the same repo, or a project nested inside another, would otherwise be
  re-id'd behind the human's back, losing its vault secrets and leaving a
  tracked change). Files are first de-duplicated by canonical path, so a
  nested project does not see one file twice.
- **Same id, different files** (a file copied by hand, or one repo opened as
  two projects): one id is one server. The first location — the one already in
  the index from the previous scan, else the first by sidebar project order
  then path — is the server. Every other location shows as a *duplicate* row
  under its project, with one action, **Make it a separate server**, which
  writes a fresh UUID into that file (a conditional write the human asked for).
  A file whose id equals a database server's id is a duplicate the same way.
- **A file with no `id`** is listed as broken with the action **Give it an
  id**; nothing is written until the human clicks it.
- **Tunnel ids.** The tunnel runtime and `tunnel_start` key by tunnel id alone
  (`ssh/tunnel.rs`, `commands/servers.rs` `get_tunnel`). For a project server
  the app-wide tunnel id is `<server id>/<tunnel id in the file>`, so a copied
  file never collides; the file keeps the short id.
- **A server that disappears** (file deleted by a pull, turned invalid by a
  conflict, project removed or missing) keeps its last valid entry in the index
  while any tab, tunnel or agent terminal still uses that id, so
  `servers::get` — agent policy included — answers with what was last valid.
  It drops out of the sidebar at once; the entry goes when its last user ends.
  No other file can claim that id meanwhile.

## Backend

One lookup for every "load server by id": `servers::get(id)` (and `list`)
answer from the database **or** the project files, so the call sites listed
below need no other change. Places that load a server by id today:
`commands/servers.rs` 156, 267, 319, 468, 484, 510; `lib.rs` 308, 335;
`agent_policy.rs` 199. `Server` gains `source`: `{ kind: "local" }` or
`{ kind: "project", projectId, relPath }`.

- **Scanning.** A new `services/server_files.rs` walks `servers/` (recursive,
  `.server` only, skipping dot-folders) of every project in the Sessions
  sidebar (`list_sidebar`: folders the human opened, not discovered ones) that
  has a folder on disk and is not missing, parses each file, and keeps an in-memory index
  `id → (projectId, path, sha256, parsed)`. `list_servers` rescans; the
  renderer already polls the book every 3 s (`ServerModeView.tsx` `POLL_MS`).
  *shortcut: polling, no file watcher; upgrade to a recursive `notify` watch if
  scanning many projects every 3 s shows up in profiles.* `get(id)` uses the
  index and rescans once on a miss.
- **Bad files.** A file that is not valid JSON or fails validation is listed as
  a broken entry `{ projectId, relPath, error }` in the book, never written.
  Its id (if readable) is not claimed.
- **Writes.** `save_server` for a project server reads the file, merges the
  known fields over the parsed object (unknown fields kept), and writes
  conditionally on the sha256 it last read (as `write_canvas` does); a stale
  hash returns `Conflict` and the renderer reloads the book and shows the
  error. Secrets go to the vault exactly as today, before the file.
- **Creating** in a project writes a new file with `create_new` in the chosen
  folder.
- **Deleting** a project server deletes its file, after the same teardown as
  today (stop tunnels, kill tabs, disconnect, remove the vault prefix). Git
  keeps the history.
- **Moving** between *This computer* and a project (or between projects) keeps
  the id: write the new home (file or row, tunnels included), then remove the
  old one; vault entries are untouched. Live tabs and tunnels of that server
  keep running (same id).
- **Folders in a project** are directories: create = `mkdir`, rename =
  rename the directory, delete = move its `.server` files up to the parent
  (no file is overwritten: a clash gets the `-2` suffix), then remove the
  directory if empty. Mirrors the database rule that deleting a folder keeps
  its servers.
- **Tunnels of a project server** are the file's `tunnels` array; the tunnel
  commands (`list_tunnels`, `save_tunnel`, `delete_tunnel`, `tunnel_start`)
  route to the file when the server is a project server.
- **Agent audit** stays in the database, keyed by id, on each machine.

## Renderer

- **Servers sidebar.** First *This computer* (today's book, unchanged), then
  one section per project with at least one `.server` file (valid or broken),
  in the Sessions sidebar's order, headed by the project icon and name. Inside
  a project: its folders, then its servers sorted by name. A broken file shows
  as a row with an error mark and the parse error on hover; clicking it opens
  the file in the editor (or shows its path) and never tries to connect.
- **A `[+]` at the top** opens the server dialog. The dialog gains **Save in**:
  *This computer* (default) or one of the sidebar projects that have a folder
  on disk (the same set that is scanned); plus the folder field, whose choices follow the chosen home. Editing an
  existing server shows the same field; changing it moves the server (see
  Backend). Per-section "new server" shortcuts are the builder's call.
- **Canvas sidebar.** Lists only projects with at least one canvas. A `[+]` at
  the top opens *New canvas* with a project picker, preselecting the active
  project (the project of the selected canvas, else of the active session). The
  per-project *New canvas* row stays.
- Empty states: Servers with nothing at all shows the `[+]` and a line on what
  it does; Canvas with no canvases likewise.

## Safety

- Host keys: unchanged rules for every server (F45). A cloned repo cannot pin
  or trust a key: the app's own `known_hosts` is per machine.
- `environment: "prod"` and `agentPolicy` come from the file, as the human
  chose. *Not added (named so it can be rejected): a per-machine policy
  override.*
- Paths: every file read/written goes through the project root with
  `resolve_within`, so a `relPath` or folder name cannot escape `servers/`.

## Specs and decisions to update in the same PR

- New ADR (next free number after 0114): project servers are one file each,
  the file id is the server id, polling instead of a watcher.
- `specs/05-features.md` F45 (servers), F48 (tunnels: where they are kept), F44
  (canvas sidebar rule and the top `[+]`).
- `packages/types` `Server.source`, broken-entry type, `ServerInput.home`;
  Rust `models/mod.rs` mirror.

## Left to the builder

- Module boundaries inside `services/`, the exact command signatures, and how
  the book carries project sections (a new field on `ServerBook` is fine).
- Copy, icons and spacing, following `DESIGN.md` and the frontend conventions.
- Tests: Rust unit tests for scan (nesting, duplicates, missing id, broken
  file, unknown fields round-trip, `~` expansion), conditional write and move;
  a smoke test for the sidebar sections and Save in.
