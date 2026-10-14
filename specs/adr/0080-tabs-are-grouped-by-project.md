# ADR-0080 — Tabs are grouped by project

Date: 2026-10-14
Status: Accepted for this local fork. Amends F16 (session tabs) and F31 (pull request tabs).

## Context

The top-bar strip held every session tab, then every pull request tab, each wearing its project's avatar. With several projects open the strip was a run of near-identical tabs told apart only by a 16px tile, and a pull request lived at the far end from the project it belonged to. Finding "the tabs of this project" meant reading every tooltip.

## Decision

- **A group per project.** Each project with at least one open tab, session or pull request, is a group: a label chip, its tabs, and a 2px line under the lot, like Chrome's tab groups.
- **Colour from the project swatch** (ADR-0072, ADR-0073). The chip's ground is a 25% tint of the swatch `fill` (a tint, so the solid avatar on it still reads as a tile), and the line is the `fill`, so the strip agrees with the sidebar and the collision handling is inherited.
- **A separate layout store orders; the two existing stores still say what is open.** `tabLayoutStore` (`factorai.tabLayout`) holds `groupOrder`, `itemOrder` (`s:<sessionId>`, `pr:<projectId>#<number>`) and `collapsed`. `groupTabs` joins it with `terminalStore.tabs` and `prTabStore.tabs`; anything the layout does not name is appended in open order, so existing tabs group themselves and nothing is migrated. Stale keys are ignored. The per-store `reorder` actions are removed.
- **Status moves to a dot.** The avatar's corner badge was how a session tab showed its status. The project is now the chip, so a session tab shows a `StatusDot` alone.
- **Collapse like Chrome.** The chip toggles a fold; a folded group shows the chip and a count, plus the active tab if it is in the group. The folded set is persisted.
- **Drag and keyboard.** One `DndContext`. A tab reorders within its group only; the chip is the drag handle of the whole group. `Alt`+arrows do the same on a tab or on a chip (ADR-0016).

## Consequences

- Next / previous tab step through what is drawn, so a folded group's hidden tabs are skipped.
- A tab can no longer be moved into another project's group; the group is the project.
- A pull request tab now sits beside its project's sessions instead of after all of them.
- Removing a project also forgets its layout.
- Sessions and pull requests sharing a group is a presentation choice only; neither store changed shape.
