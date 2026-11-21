# ADR-0116 — A project's servers are one file each, in the repository

**Date.** 2026-11-21
**Status.** Accepted. Arises from F45 and F48. Design:
`docs/superpowers/specs/2026-11-21-portable-servers-design.md`.

## Context

Servers (F45) were rows in the app's SQLite database: global, and tied to one
machine. The human wants the servers of a project to travel with it, so that
every computer and every colleague who clones the repo finds them, and a repo
holding only server files works as a shared address book.

## Decision

1. **One `.server` file per server**, `<project>/servers/<folder…>/<name>.server`:
   JSON, fields as the database row (and tunnels), unknown fields kept. Servers in
   the database stay as they are, shown as *This computer*; the human moves the
   ones they want (`ServerInput.home`).
2. **The `id` in the file is the server's id** everywhere the app keys by it:
   vault entries, the pool, terminals, agent policy and audit, tunnels. Moving
   or renaming a file loses nothing. A project server's tunnel id is
   `<server id>/<tunnel id in the file>`. Because it reaches folder names and
   vault prefixes, a file's id is `[A-Za-z0-9._-]{1,128}`, never `.` or `..`.
3. **The scan never writes.** A file with no id, a second file with the same id
   and a file that is not valid are listed as problems (`noId`, `duplicate`,
   `broken`); the human fixes them. `fix_server_file` writes a fresh id for
   `noId` and `duplicate` files; a `broken` file is fixed by editing it.
4. **Polling, not watching.** `list_servers` rescans the projects of the
   Sessions sidebar that have a folder on disk; the renderer already polls it
   every 3 s.
5. **Writes are conditional** on the sha256 last read (`AppError::Conflict`),
   and a create never overwrites. The edit form sends back the sha it opened
   with (`baseSha`), so a pull while it is open is a conflict. A moved server
   keeps its file name.
6. **A vanished file keeps its last valid entry** for lookups until restart, so
   a live tab, tunnel or agent policy check still answers; it is never listed.
   Deleting the server ends that entry.
7. **Secrets stay in each machine's vault** (ADR-0106). `keyPath` keeps `~/…`.

Alternatives:

- **Export / import.** Copies drift from the file and duplicate on re-import.
- **A read-only import.** In step with git but nothing editable from the IDE.
- **One `servers.json`.** More merge conflicts, no per-server history.

## Consequences

- Each machine enters a server's password or key once.
- Duplicates are resolved by the human, never by the scan, so a second checkout
  or a nested project is not re-id'd behind their back.
- The scan costs a directory walk per project every 3 s; many large projects
  may need a recursive `notify` watch.
- `environment` and `agentPolicy` come from the file, so a cloned repo can
  declare itself `free`. Host-key confirmation is unchanged and never skipped.
- A file is untrusted input: a user or host starting with `-` is refused and
  the system client gets `--` before the address, so a file cannot pass ssh an
  option such as `ProxyCommand`. Files over 1 MiB are not read.
- A new error kind, `Conflict`, reaches the server dialog.
