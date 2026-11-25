# Target architecture (proposal)
Desktop React -> Tauri IPC adapter -> desktop composition root -> reusable Rust core -> local platform/agent/storage adapters.
Headless test client -> headless composition root -> same Rust core -> Linux platform/agent/storage adapters.
Future: desktop/web/mobile -> authenticated remote transport -> host control plane -> isolated workspace worker -> same core and native CLIs.

## Boundaries
- Core: agent descriptors, projects, sessions, profiles, routines, lifecycle, typed requests/errors/events; no Tauri or GUI libraries.
- Adapters: native CLI discovery/spawn/transcripts/status, PTY, filesystem/watcher, SQLite, Git, usage, clock/event sink. Use interfaces only where a real boundary exists.
- Desktop shell: commands, AppHandle/Emitter, windows, notifications, clipboard, updater, Windows taskbar, desktop-only behaviour.
- Headless shell: explicit data/config roots, Tokio candidate, signal handling and owned process supervisor.
- Frontend transport: retain packages/types and IPC shapes; distinguish shared runtime operations from desktop-only features.

Start with one small reusable Cargo library plus existing desktop and a headless binary in the same workspace. Potential layout crates/hai-core and apps/runtime; names subject to E0. Avoid speculative crate proliferation. Shared implementation does not mean shared credentials, database or trust domain.

## Invariants
Explicit environment/workspace identity, cwd, profile and session ownership. Distinguish internal id from native CLI id. Do not infer host from a path. Preserve serialization and data migrations. Runtime state cannot depend on a renderer being attached. Disconnect != stop for remote/headless clients; desktop shutdown retains current policy. Process recovery after reboot is capability-dependent, not exact continuation.

Read native transcript stores without altering them except existing explicit deletion contract. Single authoritative writer for DB. Preserve local Windows ConPTY, WSL and existing cross-platform branches. Never log secret-bearing env or expose privileged host control to agents.
