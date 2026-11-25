# HAI Runtime architectural extraction
Status: proposed programme, 2026-11-25. Baseline: develop @ b5cb1963e160b1cbaf304eff29e1847f767b17be. Integration branch: refactor/hai-runtime-extraction.

Start here: [Vision](00-vision.md), [Architecture](01-architecture.md), [Proposed ADRs](02-proposed-adrs.md), [Execution plan](03-execution-plan.md), [Status](STATUS.md). Existing AGENTS.md, PRODUCT.md, feature specs and accepted ADRs remain authoritative for current desktop behaviour. These proposals do not silently supersede them.

The objective is an independent headless runtime, not necessarily a separate repository. Keep the code in this monorepo; decide on a separate repository only if independent distribution/ownership creates a concrete need. No new repository is needed to produce independently installable binaries.

| Slice | Context | Dependencies |
| --- | --- | --- |
| E0 | [Inventory and baseline](04-inventory.md) | none |
| E1 | [Core contracts](05-core-contracts.md) | E0 |
| E2 | [Agents and profiles](06-agents-profiles.md) | E1 |
| E3 | [Processes and lifecycle](07-process-lifecycle.md) | E1, E2 |
| E4 | [Storage and routines](08-storage-events.md) | E1, E2, E3 interfaces |
| E5 | [Desktop and transport](09-desktop-transport.md) | E1, E3, E4 |
| E6 | [Headless proof](10-headless-proof.md) | E2–E5 |
| E7 | [Cloud boundaries](11-cloud-boundaries.md) | E6 |

Agent workflow: coordinator owns contracts, integration and STATUS.md. One slice per worktree, starting from this integration branch; no switching the shared checkout or concurrent edits to the same modules. Small commits and focused PRs target this branch. Do not merge into develop/main, deploy, or bypass hooks. Read actual call sites and tests before implementation. Report blockers honestly. No live account credentials, metered API calls or new provider integrations in extraction.
