# Proposed architecture decisions
Not accepted ADRs. Provisional identifiers avoid collisions with immutable numbered specs/adr entries. E0 must validate references and promote decisions through the normal ADR process; do not edit historical ADRs.

## HAI-P01 — Rust core and composition roots
Context: current Cargo workspace has desktop Tauri crate; lib.rs owns startup and GUI lifecycle. Decision: extract pure Rust logic and use Tauri as consumer; headless binary is second composition root. Reject rewrite in Go and duplicated backend. Verify cargo dependency graph and Windows regression.

## HAI-P02 — Native CLI capability model
Retain ADR-0060: Spawn required, Discovery/Transcripts/Status optional. Preserve provider-specific resume/id/store semantics. Future install/auth/usage capabilities optional and separate. Native subscription credentials remain provider-specific, no metered fallback. Reject universal mandatory adapter or direct model API gateway.

## HAI-P03 — Execution owner controls lifecycle
Reconcile kill-on-quit (ADR-0005, verify exact text) with persistent SSH tabs (ADR-0111). Desktop retains current local shutdown policy; headless owner supervises children independently from clients. Explicit stop distinct from disconnect. Reboot recovery restores metadata and resumes where supported, not process resurrection. Test cleanup.

## HAI-P04 — Transport-independent events
Wrap Tauri IPC and Emitter at shell edge, preserve payloads. Domain events distinct from PTY byte stream; define bounded buffering and future reconnect/replay. Remote protocol versioning and auth belong outside core. No public listener in extraction.

## HAI-P05 — Data authority execution-local
Retain ADR-0003 SQLite/FTS5, read-only native stores per ADR-0060/ADR-0004 and existing explicit deletion exception. Avoid duplicate writers and destructive schema changes. Future access via authorized runtime operations. Validate fixtures and migrations.

## HAI-P06 — Sandbox trust boundary
Future host broker/auth/credential storage outside sandbox; workspace worker/native CLIs inside. Root peers in same sandbox are not isolated from one another. Use separate trust domains or demonstrated supported credential mediation. No host SSH keys, privileged sockets or mounts exposed. No isx compatibility claims without testing.

## HAI-P07 — Monorepo, independent artifacts
Keep runtime and desktop in current repository, use Cargo crate/package boundaries and separate release/build targets. An independent repository is not a requirement for standalone execution. Consider splitting only if separate ownership, external consumers or release policy justify added coordination. Preserve MIT licence/upstream attribution. No repo split during this branch.
