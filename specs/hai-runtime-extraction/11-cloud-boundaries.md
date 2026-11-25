# E7 — Future cloud design and independent build
Depends on E6. Design only; no deploy or repository split.
Host control plane: authenticated device pairing, workspace membership/roles, privileged isx broker, lifecycle, account registry, usage policies, audit. Workspace worker: same core plus native CLI binaries, provider-specific authentication, PTY, transcripts. Desktop/browser/mobile attach to server-owned state; SSH bootstrap/maintenance, no shared SSH keys.

Future validation:
- isx.run current docs/version/licence, OS/architecture, Incus/KVM, nested virtualization, clone/snapshot persistence, ownership, credential mediation, actual CLI contracts. Do not assume arbitrary VPS support.
- Claude Code, Codex, OpenCode, Gemini subscription-capable CLI and Grok: verify native executable/version, install, login/MFA, fixed subscription billing, store, resume, events/status, limits/logout. No metered fallback; fail closed on unknown billing. Use authorized accounts within supported access.
- Root in sandbox cannot reach host secrets, broker socket, control DB or other trust domains. Root peers in one sandbox are not isolated; verify separation and provider-supported credential mediation.
- Multi-client authorized chat/terminal, read/write/control roles, control lease, revocation, replay, audit without secrets; no public plaintext terminals.
- Idempotent bootstrap, host key checking, signed/versioned install, explicit privilege authorization, rollback, quotas, backups, Linux x86_64/arm64 and mobile UX.

Deliver extraction-readiness.md: reusable crate paths, dependency/feature matrix, remaining desktop coupling, test evidence, versioning, separate binary release/installation, migration/rollback, licence/upstream attribution. Default: retain single monorepo and build desktop/runtime independently; document any concrete reason to reconsider. No new repository without explicit decision.
