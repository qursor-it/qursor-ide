# Vision and scope
HAI is a self-hosted distributed agentic development environment. Qursor IDE is the starting point. Users ultimately bootstrap Linux servers over SSH, create persistent isolated isx environments, select native CLI agents, authenticate with their subscriptions and reconnect from desktop, browser and mobile. Agents remain active when the clients disconnect.

## Confirmed requirements
- Reuse Qursor IDE, preserve native Windows local mode, extract reusable Rust runtime incrementally.
- Native CLI-first: Claude Code, Codex, OpenCode, Gemini (actual subscription-capable executable to validate), Grok. No replacement LLM harness or model API gateway.
- Subscription-only billing; no implicit metered API fallback. Fixed-plan key and metered key are distinct billing modes even if both are authentication keys. Unknown billing is not assumed free.
- Automated SSH provisioning, sandbox lifecycle and installation; native login/MFA can require human intervention.
- Full agent control within sandbox, not over host; authorized shared workspace/chat and remote phone access.
- Runtime independently buildable/installable, desktop and cloud share implementations. Keep monorepo by default.

## Extraction success
A headless Linux runtime consumes the same Rust core as Tauri. Preserve desktop UX, native CLI semantics, persisted data and local kill-on-quit. Characterize dependencies, introduce explicit process and environment ownership, validate with offline fake CLI and regression evidence.

## Out of scope
Production remote service, public endpoints, deployment, isx installation, account onboarding, subscription routing, new CLI adapters, multiuser authentication, mobile PWA, distributed orchestration and repository split. Design seams only. Do not redesign UI or rewrite stable services.
