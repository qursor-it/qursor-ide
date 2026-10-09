# ADR-0074 — Native Windows is an additional desktop target

Date: 2026-10-09
Status: Accepted for this local fork. Supersedes ADR-0044 only for native Windows support; the existing WSL/Linux path remains available.

## Context

The owner of this fork explicitly requests the original FactorAI interface and functionality with an additional native Windows distribution. The existing Tauri 2 / Rust / React application is retained. This is a platform port, not a UX redesign or a merge of Qursor's provider engine.

## Decision

Add an x86_64-pc-windows-msvc target and an NSIS installer. On Windows, agents, repositories, profiles and transcripts are local Windows resources; WSL is not required and the native target does not bridge into a Linux distribution. Preserve macOS/Linux behavior using narrow platform branches.

Use ConPTY through portable-pty, native process discovery and termination, Windows PATH/PATHEXT resolution for Claude and Codex, and PowerShell for the project shell. Adapt path comparison, file reveal, IDE lockfile metadata and permissions, atomic file writes and Recycle Bin behavior to Windows. No orphaned agent processes may remain after quit. Existing confirmations, authenticated loopback services and recipient-preserving SOPS behavior remain required.

Keep the frontend's sidebar, routing, settings and workflows unchanged except platform labels, native keyboard conventions and shell identification where necessary. Preserve Claude and Codex; adding OpenCode or Qursor's routing/template engine is outside this port.

A local Windows build disables the upstream updater endpoint and signing key. A signed fork release must configure its own updater identity. Do not publish or push this fork without a separate instruction.

## Consequences

Windows builds need platform-specific validation of agent launch, PowerShell PTY I/O, shutdown, file paths, watching, profiles, IDE connection and packaging. Tests use synthetic fixtures and local fake CLI processes without real credentials. Existing macOS/Linux checks remain in CI. Windows ARM64 is not part of the first native target.
