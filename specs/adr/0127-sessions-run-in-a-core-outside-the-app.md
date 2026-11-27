# ADR-0127 — Sessions run in a core outside the app

## Context

Every terminal lived inside the Tauri process, so closing the window, updating
the app or crashing it ended every agent (ADR-0005). The design in
`docs/superpowers/specs/2026-11-26-local-core-design.md` weighed three shapes:
A, a separate binary the renderer connects to directly over a WebSocket; B, a
separate core the app bridges to over a pipe or socket; C, a core plus a
separate terminal worker. A takes base64 and Tauri IPC out of the data path and
speaks the protocol a phone or a VPS would later use. C is deferred.

## Decision

`crates/qursor-core` is a library and a headless binary with no Tauri
dependency, and it owns every terminal (agent, footer shell, SSH), the SSH pool,
the vault, agent policy, the server and workspace listings, and attention
decisions. The app keeps the window, the migrations, the usage poller and the
OS notification, and depends on the library, re-exporting the module paths it
still uses.
The core never migrates: it opens the database only when the `_meta`
`migration:*` ledger matches exactly, and exits 4 otherwise.

The app's `CoreLink` starts the core as a detached sidecar and hands the
renderer `core_endpoint`. The renderer connects to 127.0.0.1 on a random port
over a WebSocket, authenticated by a secret in the first frame, with an Origin
that is on an allowlist or absent. Terminal bytes travel as binary frames, never
through Tauri IPC. The app owns no terminal. The wire format, exit codes and
timings are in `specs/03-backend-rust.md` § "Where the code lives".

In this plan kill-on-quit is unchanged in effect. A confirmed or idle quit
sends the wire method `core.shutdown`, which kills every PTY and stops the core;
the updater reaches the same method through the Tauri command `core_shutdown`.
When the last client is gone the core waits 10 s (the reload of ADR-0059
reconnects inside it), kills them and exits. When the `--parent-pid` process is
gone, which the core checks every 500 ms, it shuts down at once, with no wait.

## Consequences

Every bundle ships the sidecar, and Task Manager shows two processes. ADR-0005
and Q10 still hold, narrowed to say a different process does the killing. A
later plan lets the core outlive the app and supersedes ADR-0005 with a "leave
them running or stop all" choice, and a further one runs several core versions
side by side.
