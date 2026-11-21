# One left sidebar for every mode, full height

Status: approved in conversation, 2026-11-21.

## Intent

Sessions, Git, Canvas and Servers each grew their own left sidebar, and they
disagree on width, resizing, collapse, header and footer. The user wants one
sidebar that is built and behaves the same in every mode, and wants it to run
the full height of the window so the top bar's tab strip starts where the
sidebar ends instead of spanning the whole width.

## Today

- `TopBar` is two rows (`h-10.5` brand + palette search + file-panel toggle,
  `h-10` tab strip) spanning the full width, above `AppRail` and every sidebar.
- Sessions: `aside[data-testid=sidebar]` in `AppShell`, store width
  (`useSidebarStore`, 180–480, persisted), `PanelResizer`, collapsible
  (`sidebar-collapse`), search + `PROJECTS` header, `h-9` footer (update badge,
  zoom).
- Canvas: own `aside` in `CanvasModeView`, same store width, own resizer, no
  header, no footer.
- Git: `GitSidebar` inside `GitTabBody` (per open tab), fixed `w-60`, no
  resizer, header or footer.
- Servers: own `aside` in `ServerModeView`, fixed `w-64`, own header row and a
  vault-state footer.
- The OS title bar is native (no `decorations: false`); this change is below it.

## Goals

- One sidebar frame shared by all four modes: same width (one stored value),
  same resize handle and clamp, collapse available in every mode, same header
  and footer.
- The rail and the sidebar run from the top of the window content to the status
  bar. The top bar's two rows sit only above the work area.
- Each mode keeps its own sidebar content and behaviour.

## Non-goals

- No search/filter field added to Git, Canvas or Servers.
- No custom title bar or window controls.
- No change to tabs, cards, the right panel or the status bar beyond where they
  sit.

## Anti-goals

- The Sessions body must stay mounted while hidden (its terminals keep running,
  F35/F44/F45). Moving the sidebar must not unmount or remount terminals.
- A mode switch must not lose a sidebar's state (Sessions search text and
  expansion, Git section/tree state, Servers inline edits) more than today.

## Approaches

1. **One shared sidebar component mounted once by `AppShell` (picked).** A
   full-height column beside `AppRail`; it owns width, resize, collapse, header
   and footer, and renders the active mode's content in its body.
2. Keep four sidebars and align their classes/sizes. Rejected: it is how they
   drifted apart; every new mode copies the frame again.

Layout chosen by the user (option A): the top bar's two rows are kept but only
over the work area; the brand moves to the top of the sidebar.

```
┌──┬────────────┬───────────────────────────────┐
│◆ │ Q Qursor ⟨ │       🔍 Search…            ▣ │  h-10.5
│⑂ │ (mode      ├───────────────────────────────┤
│▦ │  content)  │ tab tab tab                   │  h-10
│⌘ │            │ ┌───────────────────────────┐ │
│  │            │ │ card                      │ │
│⚙ │ zoom · upd │ └───────────────────────────┘ │
├──┴────────────┴───────────────────────────────┤
│ status bar                                    │
└───────────────────────────────────────────────┘
```

## Design

- **Shell columns** (`AppShell`): `AppRail` (56px, full height) | the shared
  sidebar (full height, hidden when collapsed) | a right column holding
  `TopBar` (both rows) and the mode bodies. `StatusBar` stays full width under
  everything.
- **Shared sidebar frame** (new `ModeSidebar`, `data-testid="sidebar"` kept):
  - Width from `useSidebarStore` (one value for every mode), `PanelResizer`
    with `clampSidebarWidth`, not rendered while collapsed (ADR-0038 rule kept).
  - Header `h-10.5`, aligned with the top bar's first row: `Brand` + `DevBadge`
    + the collapse button (moved from the Sessions header; works in every mode).
  - Body: the active mode's content. Sessions content stays mounted and hidden
    in other modes, like its body today.
  - Footer `h-9`: update badge / indexing text and zoom in every mode; Servers
    adds its vault state.
- **Per mode content**:
  - Sessions: `Sidebar` minus the collapse button (search row, `PROJECTS`
    header, list, footer content moved to the frame's footer).
  - Git: `GitSidebar` for the active tab (`activeDir`). Its `section` state is
    lifted out of `GitTabBody` so the body and the sidebar share it, per tab.
    With no tab open, the sidebar body shows a short empty state.
  - Canvas: `CanvasSidebar`, its `aside` and resizer removed from
    `CanvasModeView`.
  - Servers: `ServerSidebar` (book) with its own data query; the vault state
    moves to the frame's footer slot.
- **`TopBar`**: `Brand`/`DevBadge` leave it; it renders inside the right column
  so both rows start at the sidebar's edge. The palette button stays centred in
  row 1, the file-panel toggle at its right.
- **Collapsed**: only `AppRail` remains; the top bar rows extend to the rail.
  Clicking the rail icon of the mode you are in expands it (switching to another
  mode never changes the collapsed state); a collapsed sidebar can be reopened from any mode (the expand button sits at the left of the top
  bar's first row while collapsed).
- **Width maths**: the viewer-column maximum (`maxViewerWidth`) currently
  measures the Sessions row including the sidebar; once the sidebar leaves that
  row, the measurement and the subtraction must not count it twice.

## Docs to update in the same change

- ADR-0115 "One sidebar for every mode, full height" (supersedes the TopBar's
  "spans above the sidebar" note; notes that a future custom title bar puts its
  controls at the right of row 1 and its drag region in row 1 plus the sidebar
  header).
- `DESIGN.md` Layout / chrome heights / Rail Rule; `specs/05-features.md` F1
  (sidebar, collapse), F16 (top bar rows), F35 / F44 / F45 sidebar bullets.

## Calls made without the user (correctable)

- One shared width for all modes; Git and Servers lose their fixed widths.
- Brand at the top of the sidebar; while collapsed the brand is not shown.
- Footer zoom/update in every mode.
- No new search in Git/Canvas/Servers.

## Left to the builder

Component names and file split, how Git's per-tab section state is stored,
exact empty-state copy, and test updates (`sidebar.spec.ts`,
`app-rail.spec.ts`, `git-mode.spec.ts`, `servers.spec.ts`, canvas specs).
