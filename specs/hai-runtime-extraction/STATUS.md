# Extraction ledger
Baseline: develop @ b5cb1963e160b1cbaf304eff29e1847f767b17be. Branch: refactor/hai-runtime-extraction.
Context preparation only. No code extraction, compilation, tests or provider validation performed. Initial evidence: README, Cargo workspace, AGENTS.md, selected Rust sources and ADRs; full dependency graph not yet established.

| Slice | State | Next |
| --- | --- | --- |
| E0 | Ready | Full inventory and baseline |
| E1 | Blocked | E0 |
| E2 | Blocked | E1 |
| E3 | Blocked | E2 |
| E4 | Blocked | E3 interfaces |
| E5 | Blocked | E3/E4 |
| E6 | Blocked | E2–E5 |
| E7 | Blocked | E6 |

Coordinator updates this ledger after each slice: owner, worktree/commit, changed files, evidence commands/results, failures, decisions, remaining dependencies and next task. Do not mark done without evidence.
