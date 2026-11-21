# Project servers as `.server` files — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SSH servers can live as one JSON file each in `<project>/servers/**/<slug>.server`, shown in Servers mode under their project, editable from the IDE; Canvas and Servers sidebars list only projects that have files of their kind.

**Architecture:** A pure module `services/server_files.rs` parses, serialises and scans the files. `services/servers.rs` keeps its public functions and signatures (`list`, `get`, `save`, `delete`, folders, tunnels) and routes each to the database or to the files, so the nine "load server by id" call sites do not change. A process-wide index (last scan) answers `get(id)` for file servers. The renderer polls the book every 3 s as today; there is no file watcher.

**Tech Stack:** Rust (Tauri 2, rusqlite, serde_json, uuid, sha2), React + TanStack Query, Playwright smoke tests with the mock bridge.

**Spec:** `docs/superpowers/specs/2026-11-21-portable-servers-design.md` — read it first; this plan does not repeat its reasons.

## Global Constraints

- Worktree `E:\Developing\factorai-portable-servers`, branch `feat/portable-servers`, based on `origin/develop` (servers are not on `main`). PR targets `develop`.
- No secret ever goes into a `.server` file. Secrets stay in the vault under `server:<id>:<kind>`.
- **The scan never writes a file.** Only an explicit human action writes (save, create, move, delete, folder ops, *Make it a separate server*, *Give it an id*).
- A file that is not valid JSON or fails validation is never written over. A create uses `create_new` and never overwrites. Unknown JSON fields survive a rewrite unchanged.
- File writes are conditional on the sha256 last read; a mismatch returns `AppError::Conflict`.
- Every path is resolved with `services::ide::scope::resolve_within(root, …)` under `<project>/servers/`.
- Writing format: `serde_json::to_string_pretty` (2-space) + trailing `\n`.
- Slug: reuse `services::canvas::slugify` (make it `pub(crate)` if needed), fallback word `server`, `-2`, `-3` … on collision.
- `keyPath` under the home dir is written as `~/…`; `~` is expanded when the path is used for a login (not when shown).
- Code style per `AGENTS.md`: no `unwrap()` outside `setup()`, clippy `-D warnings`, `oxlint`, `tsc --noEmit`, types hand-mirrored in `packages/types/src/index.ts` and `models/mod.rs`.
- Verification commands (run from the worktree root):
  - Rust: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml` and `cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings`
  - TS: `pnpm typecheck && pnpm lint && pnpm --filter @factorai/desktop test`
  - Smoke: `pnpm e2e tests/smoke/servers.spec.ts tests/smoke/canvas.spec.ts`

## Review Focus

1. A merge conflict leaves `<<<<<<<` markers in a `.server` → listed as broken with the parse error, never written, live tabs keep working (Task 1 test `conflict_markers_are_broken`, Task 2 test `vanished_server_still_resolves`).
2. A project nested inside another project → each file is listed once (Task 1 test `nested_projects_list_a_file_once`).
3. The human edits a server in the IDE while `git pull` changed its file → `Conflict`, file untouched (Task 2 test `stale_write_is_refused`).
4. `delete_server_folder` on a project folder whose file name already exists one level up → moved as `-2`, nothing overwritten (Task 2 test `deleting_a_folder_never_overwrites`).
5. A copied `.server` file with the same tunnel ids as the original → two distinct app-wide tunnel ids (Task 2 test `tunnel_ids_are_scoped_by_server`).

---

### Task 1: `server_files` — format and scan (pure, no DB)

**Files:**
- Create: `apps/desktop/src-tauri/src/services/server_files.rs` (+ `pub mod server_files;` in `services/mod.rs`)
- Modify: `apps/desktop/src-tauri/src/services/canvas.rs` (`slugify` visibility only)
- Test: unit tests in `server_files.rs`, fixtures in a `tempfile::TempDir`

**Interfaces:**
- Produces:
  - `pub struct ScanProject { pub id: String, pub root: PathBuf }`
  - `pub struct FileServer { pub project_id: String, pub rel_path: String /* "servers/clienti/acme.server", '/' separators */, pub abs_path: PathBuf, pub sha256: String, pub server: Server /* id from file, folder_id = folder id below, source = Project */, pub tunnels: Vec<Tunnel> /* app-wide ids */, pub raw: serde_json::Map<String, Value> }`
  - `pub struct FileFolder { pub id: String /* "p:<projectId>:<relDir>" */, pub project_id: String, pub name: String /* relDir, e.g. "clienti" or "a/b" */ }`
  - `pub enum ProblemKind { Broken, Duplicate, NoId }` (serde `camelCase`)
  - `pub struct ServerFileProblem { pub project_id: String, pub rel_path: String, pub kind: ProblemKind, pub message: String }`
  - `pub struct Scan { pub servers: Vec<FileServer>, pub folders: Vec<FileFolder>, pub problems: Vec<ServerFileProblem> }`
  - `pub fn scan(projects: &[ScanProject], prior_owner: &HashMap<String, PathBuf>, db_ids: &HashSet<String>) -> Scan` — `prior_owner` maps id → canonical path that owned it in the previous scan.
  - `pub fn tunnel_app_id(server_id: &str, file_tunnel_id: &str) -> String` → `"<server_id>/<file_tunnel_id>"`; `pub fn split_tunnel_id(id: &str) -> Option<(&str, &str)>`
  - `pub fn to_json(raw: &Map<String, Value>, s: &Server, tunnels: &[Tunnel]) -> String` — merges known fields over `raw`, writes `keyPath` as `~/…` when under home, file-local tunnel ids, pretty + `\n`.
  - `pub fn folder_id(project_id: &str, rel_dir: &str) -> String`, `pub fn parse_folder_id(id: &str) -> Option<(&str, &str)>`
  - `pub fn new_file_path(root: &Path, rel_dir: &str, name: &str) -> AppResult<PathBuf>` — slug + collision suffix, does not create.
  - `pub fn expand_home(p: &str) -> String`

- [ ] **Step 1: Write the failing tests** in `server_files.rs` `mod tests`:
  - `parses_a_full_file`: the spec's example JSON → `server.host == "staging.example.com"`, `port == 22`, `persistent`, one tunnel with id `tunnel_app_id(id, "c1d2")`, `folder_id == None` at `servers/staging.server`.
  - `defaults_apply`: `{"id":"a","name":"n","host":"h","user":"u","login":"agent"}` → port 22, environment `dev`, agentPolicy `free`, persistent false, no tunnels.
  - `sub_folder_is_folder`: `servers/clienti/acme.server` → one `FileFolder { name: "clienti", id: "p:P1:clienti" }`, server's `folder_id == Some("p:P1:clienti")`.
  - `conflict_markers_are_broken`: file containing `<<<<<<< HEAD` → no server, one problem `Broken` with a non-empty message.
  - `invalid_fields_are_broken`: `"environment":"live"` → `Broken`.
  - `missing_id_is_no_id`: valid file without `id` → problem `NoId`, no server.
  - `duplicate_prefers_prior_owner`: two files with id `X`, `prior_owner` points at the second → second is the server, first is `Duplicate`.
  - `duplicate_without_prior_uses_project_then_path_order`: projects `[P1, P2]`, same id in both → P1's wins.
  - `db_id_collision_is_duplicate`: `db_ids` contains the file's id → `Duplicate`.
  - `nested_projects_list_a_file_once`: P2 root is `P1/sub`, file in `P1/sub/servers/x.server` → exactly one server or one problem total for that canonical path.
  - `dot_folders_and_other_files_are_skipped`: `servers/.git/x.server`, `servers/readme.md` → nothing.
  - `round_trip_keeps_unknown_fields`: parse a file with `"x-note":"keep"`, `to_json` → contains `"x-note": "keep"`, ends with `\n`.
  - `key_path_is_written_with_tilde`: `key_path = <home>/.ssh/id` → JSON has `"keyPath": "~/.ssh/id"`; `expand_home("~/.ssh/id")` == `<home>/.ssh/id`.
  - `new_file_path_suffixes`: existing `servers/web.server` → `new_file_path(root, "", "Web")` ends with `web-2.server`.

- [ ] **Step 2: Run** `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml server_files` — expected FAIL (module missing).

- [ ] **Step 3: Implement** the interfaces above. Validation must reuse `servers::validate` (make it `pub(crate)` taking a `&ServerInput`; build a `ServerInput` from the file fields). Walk with `std::fs::read_dir` recursively; canonicalise each file with `dunce::canonicalize` if already a dependency, else `std::fs::canonicalize`, and keep a `HashSet` of seen canonical paths. Projects are visited in the given order, files within a project sorted by `rel_path`.

- [ ] **Step 4: Run** the same command — expected PASS. Also clippy.

- [ ] **Step 5: Commit** `feat: parse and scan project .server files`

---

### Task 2: Route the address book to files

**Files:**
- Modify: `apps/desktop/src-tauri/src/services/servers.rs`, `apps/desktop/src-tauri/src/commands/servers.rs`, `apps/desktop/src-tauri/src/models/mod.rs` (Server, ServerInput, ServerFolder, ServerBook), `apps/desktop/src-tauri/src/lib.rs` (register `fix_server_file`), `commands/servers.rs` `target_for` (use `server_files::expand_home` on `key_path`)
- Test: `servers.rs` `mod tests` (existing, in-memory DB) + tempdir projects

**Interfaces:**
- Consumes: Task 1.
- Produces (Rust models, `camelCase` on the wire):
  - `Server.source: ServerSource` — `#[serde(tag = "kind", rename_all = "camelCase")] enum ServerSource { Local, Project { project_id: String, rel_path: String } }` (fields camelCase).
  - `ServerFolder.project_id: Option<String>`
  - `ServerBook.problems: Vec<ServerFileProblem>`
  - `ServerInput.home: Option<String>` — `None` = this computer, `Some(projectId)` = that project. `folder_id` must belong to the same home.
  - `servers::list(conn) -> AppResult<Vec<Server>>` now = DB rows + file servers (DB first, then by project sidebar order, then name). `servers::list_folders(conn)` likewise. New `servers::problems() -> Vec<ServerFileProblem>` (from the last scan).
  - `servers::rescan(conn) -> AppResult<()>` — projects from `commands::sidebar::list_sidebar_in(conn)` flattened, keeping those with `real_path` set and `!missing`; stores the `Scan` in a `static INDEX: LazyLock<Mutex<Index>>` inside `servers.rs`. `list` calls `rescan` first.
  - `servers::get(conn, id)` — DB, else index, else `rescan` once and retry, else the index's **retired** map (entries that were valid in an earlier scan and have since vanished). *shortcut: retired entries live until app restart instead of until their last tab/tunnel ends; upgrade if a stale entry ever matters (it is never listed, only resolved for already-open tabs).*
  - `servers::save(conn, vault, input, now_ms)` — same signature. Decides by `input.home` and, for an existing id, the server's current source:
    - local→local: today's code.
    - project→same project: read file, check sha == index sha else `Conflict`, `to_json`, write, rescan. A `folder_id` change moves the file into that folder (rename, `-2` on clash).
    - new in project: `new_file_path` + `OpenOptions::create_new`.
    - move (home differs): write the new home (row + `server_tunnels`, or file with tunnels), then delete the old home's row/file. Vault untouched. Id kept.
  - `servers::delete(conn, vault, id)` — project server: remove the file (after the vault prefix), rescan.
  - Folders: `create_folder(conn, name, project_id: Option<&str>)` → `mkdir` under `servers/` for a project; `rename_folder` / `delete_folder` accept `p:` ids (rename dir; delete = move each `.server` up one level with `-2` on clash, then `remove_dir` if empty).
  - Tunnels: `list_tunnels`, `get_tunnel`, `save_tunnel`, `delete_tunnel` keep signatures; ids containing `/` (or a `server_id` of a file server for a new tunnel) go to the file.
  - `servers::fix_file(conn, project_id, rel_path) -> AppResult<()>` — writes a fresh UUID `id` into a `Duplicate` or `NoId` file (conditional on its current sha), rescan.
  - Commands: `create_server_folder(name, project_id: Option<String>)`; new `fix_server_file(project_id, rel_path)`; `list_servers` fills `problems`.

- [ ] **Step 1: Write the failing tests** (tempdir project registered in the in-memory DB through the same helpers `reorder_sidebar_integration.rs` uses, or direct inserts into `projects` + `sidebar_rows`):
  - `file_server_is_listed_and_got`: a valid file → in `list`, `get(id)` returns it with `source = Project`.
  - `save_rewrites_the_file_keeping_unknown_fields`.
  - `stale_write_is_refused`: change the file on disk after `list`, then `save` → `Err(Conflict)`, file content unchanged.
  - `create_in_project_writes_a_new_file` and never overwrites an existing slug (`-2`).
  - `move_local_to_project_keeps_id_and_tunnels`: local server with a tunnel → `save` with `home = Some(P1)` → row gone, file present with the same `id` and the tunnel, vault entry still present.
  - `move_project_to_local` mirror.
  - `delete_removes_the_file`.
  - `deleting_a_folder_never_overwrites`: `servers/a/web.server` and `servers/web.server` → delete folder `p:P1:a` → `servers/web.server` and `servers/web-2.server`, dir `a` gone.
  - `tunnel_ids_are_scoped_by_server`: two files (different server ids) with tunnel id `t1` → `list_tunnels` gives `"<s1>/t1"` and `"<s2>/t1"`; `get_tunnel("<s2>/t1").server_id == s2`.
  - `vanished_server_still_resolves`: list, then delete the file from disk (not via `delete`) → not in `list`, `get(id)` still Ok.
  - `fix_file_gives_a_duplicate_its_own_id`: duplicate problem → `fix_file` → two servers, problems empty.
  - `scan_never_writes`: record every file's mtime + bytes, call `list` twice → unchanged.

- [ ] **Step 2: Run** `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml servers` — FAIL.

- [ ] **Step 3: Implement** as specified in Interfaces. `commands/servers.rs` `save_server`/`delete_server` keep their teardown; `list_servers` returns `ServerBook { folders, servers, problems }`. `target_for` expands `~` in `key_path`.

- [ ] **Step 4: Run** the Rust test and clippy commands — PASS. Also `cargo test … wire_shape` if it covers `ServerBook` (update its expected JSON).

- [ ] **Step 5: Commit** `feat: servers kept as project files, routed through the address book`

---

### Task 3: Servers sidebar, Save in, problem rows

**Files:**
- Modify: `packages/types/src/index.ts` (`Server.source`, `ServerSource`, `ServerFolder.projectId`, `ServerBook.problems`, `ServerFileProblem`, `ServerInput.home`), `apps/desktop/src/lib/tauri.ts` (`createServerFolder(name, projectId?)`, `fixServerFile(projectId, relPath)`), `apps/desktop/src/components/servers/ServerModeView.tsx` (sidebar), `apps/desktop/src/components/servers/ServerDialog.tsx` (Save in), `tests/fixtures/*` mock bridge (`serverBook.problems`, `fix_server_file`, `create_server_folder` arg)
- Test: `tests/smoke/servers.spec.ts`

**Interfaces:**
- Consumes: Task 2 wire shapes.
- Produces: nothing used by later tasks.

Behaviour (copy is English, like the rest of the app):
- Sidebar order: section **This computer** (servers with `source.kind === 'local'`, today's rendering), then one section per project in `cmd.listSidebar()` order that has ≥1 server or problem with that `projectId`, headed with `ProjectIcon` + `displayName`. Inside: folders with `projectId === section` (by name), then top-level servers (by name), then problem rows.
- Problem row: warning icon, file name, `title` = message. `Duplicate` → button **Make it a separate server**; `NoId` → **Give it an id**; both call `fixServerFile` and invalidate `queryKeys.servers()`. `Broken` → no action. No problem row ever opens a connection.
- A `[+]` button at the top of the sidebar (`data-testid="new-server"`) opens `ServerDialog` for a new server.
- `ServerDialog` gains **Save in** (`data-testid="server-home"`): *This computer* + every sidebar project with a folder on disk and not missing. Default: *This computer* for new; current home when editing. The folder select lists only folders of the chosen home and resets when the home changes. Sends `home` (`null` | projectId).

- [ ] **Step 1: Write the failing smoke tests** in `servers.spec.ts`:
  - `project servers appear under their project`: book with one local and one project server (P1) → sections `This computer` then `P1 name`; the project server row is inside the P1 section.
  - `a project with no server files is not listed`: sidebar has P2 with no servers → no P2 section.
  - `a duplicate offers to become a separate server`: problem `duplicate` → click button → mock receives `fix_server_file` with `{ projectId, relPath }`.
  - `a broken file shows its error and has no action`.
  - `save in a project sends home`: open `[+]`, choose P1 in Save in, fill required fields, save → `save_server` input has `home: "P1"`.
- [ ] **Step 2: Run** `pnpm e2e tests/smoke/servers.spec.ts` — FAIL.
- [ ] **Step 3: Implement** per Behaviour, following `.claude/skills/frontend-conventions` (load it first) and existing components in `ServerModeView.tsx`.
- [ ] **Step 4: Run** smoke + `pnpm typecheck && pnpm lint && pnpm --filter @factorai/desktop test` — PASS.
- [ ] **Step 5: Commit** `feat: Servers sidebar shows project servers and the dialog saves into a project`

---

### Task 4: Canvas sidebar lists only projects with canvases

**Files:**
- Modify: `apps/desktop/src/components/canvas/CanvasSidebar.tsx`, `apps/desktop/src/components/canvas/NewCanvasDialog.tsx`
- Test: `tests/smoke/canvas.spec.ts`

**Interfaces:**
- Consumes: existing `cmd.listCanvases(projectId)`, `cmd.createCanvas(projectId, title)`, `useCanvasStore`.
- Produces: `NewCanvasDialog` props become `{ open: boolean; projects: { id: string; displayName: string }[]; initialProjectId: string | null; error; pending; onCancel; onSubmit: (projectId: string, title: string) => void }` — when `projects.length === 1` the picker is hidden and the title reads *New canvas in <name>* as today.

Behaviour:
- `ProjectCanvases` renders nothing until its `listCanvases` data arrives with at least one canvas (loading and empty both render nothing).
- A `[+]` at the top (`data-testid="new-canvas-top"`) opens the dialog with every project that has a folder on disk and is not missing; preselected = project of `useCanvasStore().selected`, else the first.
- The per-project *New canvas* row stays (dialog with that single project).
- Empty state when no project has a canvas: the `[+]` plus one line *No canvases yet. Create one in any project.*

- [ ] **Step 1: Write the failing smoke tests**: `projects without canvases are hidden`; `the top + creates a canvas in the chosen project` (mock receives `create_canvas` with the picked project id); `empty state when no canvas exists`. Update existing tests that relied on an empty project showing *New canvas*.
- [ ] **Step 2: Run** `pnpm e2e tests/smoke/canvas.spec.ts` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** smoke + typecheck + lint — PASS.
- [ ] **Step 5: Commit** `feat: Canvas sidebar lists only projects with canvases, with a top + to start one`

---

### Task 5: Spec, ADR, guide

**Files:**
- Create: `specs/adr/0115-project-servers-are-one-file-each.md` (check `specs/adr/` for the next free number first; follow the format of `0114-…`)
- Modify: `specs/05-features.md` F45 (A server: two homes, file format, identity, duplicates/broken, Save in, sidebar sections, new commands `fix_server_file` and the `create_server_folder` arg), F48 (where a project server's tunnels are kept, `<server>/<tunnel>` ids), F44 (sidebar rule, top `[+]`, empty state); the user guide page for Servers if `docs/` has one (`grep -ril "server" docs/guide` or equivalent).

- [ ] **Step 1:** Write the ADR: context (servers were machine-local), decision (one `.server` per server under `servers/`, file id = server id, scan never writes, polling not watching, retired entries), consequences (secrets per machine, duplicates are human-resolved, scan cost with many projects).
- [ ] **Step 2:** Update the three spec sections; run `pnpm format:check` and `pnpm bytes:check`.
- [ ] **Step 3: Commit** `docs: ADR-0115 and F44/F45/F48 for project server files`

---

## Final verification (before the PR)

- [ ] All commands in Global Constraints pass.
- [ ] Manual check in the dev app (`pnpm dev`): add `servers/test.server` by hand in this repo, see it under the project within 3 s; edit it from the dialog and see the file change with `x-` fields kept; break the JSON and see the error row; move a local server into the project and back.
- [ ] Open the PR against `develop` following the `pull-requests` skill (screenshots of the two sidebars).
