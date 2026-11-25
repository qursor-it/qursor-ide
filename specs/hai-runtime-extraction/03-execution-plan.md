# Execution plan and quality gates
E0 inventory first; E1 contracts; E2 agent integration; E3 process ownership; E4 storage/event/scheduler; E5 desktop adapters; E6 proof; E7 future architecture review. Parallelize analysis and tests, not overlapping shared files. Coordinator merges contract changes.

Each slice: inspect existing specs and actual call sites; identify affected files/dependencies; establish characterization tests; move smallest viable vertical slice; run targeted and regression checks; record evidence and residual coupling. Separate behaviour changes from mechanical moves. Never force push or bypass hooks.

Follow root AGENTS.md current quality commands: pnpm install --frozen-lockfile, pnpm format:check, pnpm bytes:check, pnpm lint, pnpm typecheck, pnpm test, pnpm e2e, pnpm deps:check, pnpm deps:unused, cargo fmt --check, cargo clippy --all-targets -- -D warnings, cargo test. Verify exact invocation and prerequisites; capture OS, toolchain, commit and exit statuses. Existing failures are baseline, not permission to add new failures.

Additional gates: Linux headless build without Tauri/WebView/GUI normal/build dependencies (cargo tree and native linkage); native Windows x64 desktop build/ConPTY smoke; preserve existing macOS/Linux build paths and disclose untested platforms. Verify IPC shape, SQLite migrations/FTS, transcript parsing, native session IDs and routine catch-up. Offline fake CLI start/input/output/resize/stop/detach/reattach/shutdown, bounded output, no orphan children. No real subscription test without explicit authorization.

Definition of done: reviewable integration commits, mandatory gates pass or remain explicitly blocked, no deployment, architecture evidence and final ADR decisions reviewed.
