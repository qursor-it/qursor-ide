# ADR-0094 — The Review tab leads the right-hand panel

Date: 2026-11-13
Status: Accepted. Amends F12/F13 (the panel's tabs), F40 (where the plan and
cost are shown) and ADR-0090.

## Context

The approved mock-up has a Review panel beside every session: its state,
branch and cost, the files changed with their counts, and the agent's plan.
The shipped redesign had put the plan and the cost in chips in the session
header, and the panel opened on the file tree, closed by default.

## Decision

- **A `Review` tab, first in the panel**, before Files, Changes, Graph and PRs.
  On a session it shows *This session* (state pill, branch, turns, cost so far),
  *Changed files* (up to eight rows of the checkout's status, each opening its
  diff as the Changes tab's rows do, then *Review all changes* to that tab) and
  *Plan* (F40's list with its done count). Off a session it shows the changed
  files of the route's project.
- **A new install opens with the panel open on Review**, as the mock-up does; a
  persisted choice still wins.
- The plan chip and the cost leave the session header; the changes chip stays,
  as the mock-up's header keeps a changes count.

## Consequences

- Smoke specs start with the panel closed on Files (`installMockBridge` seeds
  it), as they were written against that.
- With the panel open by default the session column starts narrower; the
  viewer layout rules (ADR-0037) are unchanged.
