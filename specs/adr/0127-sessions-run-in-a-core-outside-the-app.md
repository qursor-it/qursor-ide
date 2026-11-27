# ADR-0127 — Sessions run in a core outside the app

## Context

Every terminal lived inside the Tauri process, so closing the window, updating
the app or crashing it ended every agent (ADR-0005). The design in
`docs/superpowers/specs/2026-11-26-local-core-design.md` weighed three shapes:
a thread in the app, a core process the app starts, and a core that only a
service manager starts. The middle one is the only one that lets terminals
outlive the window without asking users to install a service.

## Decision

`crates/qursor-core` is a library and a headless binary with no Tauri
dependency. The app depends on the library and re-exports the old module paths.
Events leave through an `EventSink` and `CoreEvent` instead of an `AppHandle`.

The core owns every terminal (agent, footer shell, SSH), the SSH pool, tunnels
and SFTP, the vault (including the OpenCode Go key), agent policy, the
servers, server files and workspace listings, attention decisions, and the
read of `usage.json`. The app owns the window, the migrations, the usage
poller and the OS notification (`attention_show`); it reports window focus with
`attention.focus`. The core never migrates: `Db::open_existing` requires the
`_meta` `migration:*` ledger to match exactly, and exits 4 when it does not.

`qursor-core --data-dir <path> [--parent-pid <pid>]` listens on 127.0.0.1 on a
random port, prints `{"ready":true,"port":N}`, then silences stdout. It writes
`<data_dir>/core/core.json` (owner-only, atomic), holds `core.lock` (exit 3 when
taken), keeps a `host-id` and logs to `core.log`. The renderer connects over a
WebSocket. The first frame must be a hello with the 32-byte hex secret
(constant-time compare) and the Origin must be on an allowlist. Control traffic
is JSON envelopes. Terminal bytes are binary frames, `op u8 | slot u16 BE |
payload`, with Output 1, Input 2, Resize 3; 4-6 are reserved for snapshots.
A client whose queue passes 8 MiB is closed. Output flushes every 16 ms for a
visible client and every 100 ms otherwise. The core pings every 10 s and a
client reconnects after two silent windows.

The app's `CoreLink` starts the detached sidecar (`externalBin`), waits up to
10 s when an older core still holds the lock, never caches a failure to ensure
it, and gives the renderer `core_endpoint`. The app owns no terminal. It asks
the core for counts (quit guard, taskbar) and sends `core_shutdown` before an
updater restart. The NSIS hook stops this install's `qursor-core.exe`.

In this plan kill-on-quit is unchanged in effect. When the last client is gone
the core waits 10 s (the reload of ADR-0059 reconnects inside it), runs
`kill_all` and exits. It also exits when `--parent-pid` dies.

## Consequences

Every bundle ships the sidecar, and Task Manager shows two processes. The
protocol has a version and the app and core of one install are always the same
build. ADR-0005 and Q10 still hold, narrowed to say a different process does
the killing. A later plan lets the core outlive the app and replaces ADR-0005
with a "leave them running or stop all" choice, and a further one runs several
core versions side by side.
