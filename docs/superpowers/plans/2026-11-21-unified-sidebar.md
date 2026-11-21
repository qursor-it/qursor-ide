# One Sidebar For Every Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One left sidebar frame shared by Sessions, Git, Canvas and Servers, running the full window height, with the top bar's two rows only above the work area.

**Architecture:** `AppShell` becomes three columns: `AppRail` | a new `ModeSidebar` frame (width, resize, collapse, brand header, footer) | a right column holding `TopBar` and the mode bodies. Each mode supplies only the sidebar body; the Sessions body stays mounted while hidden.

**Tech Stack:** React 19 + TypeScript, Tailwind, Zustand (`useSidebarStore`, `useGitModeStore`), TanStack Query, Playwright smoke tests (`pnpm e2e`, mock bridge in `tests/smoke/fixtures.ts`), Vitest.

**Spec:** `docs/superpowers/specs/2026-11-21-unified-sidebar-design.md`

## Global Constraints

- Work in `E:\Developing\factorai-terminal-split`, branch `feat/unified-sidebar`. On Windows run pnpm as `npx -y pnpm@10.28.0 <script>`; a dev server on port 1420 collides with `pnpm e2e`.
- Read `.claude/rules/frontend.md` and `.claude/skills/frontend-conventions/SKILL.md` first: `IconButton` for icon-only controls, one-line `title`, literal chrome heights.
- Header row `h-10.5` (aligned with the top bar's first row), footer `h-9`, rail `w-14` (56px), sidebar width from `useSidebarStore` clamped 180–480 by `clampSidebarWidth`.
- `data-testid="sidebar"` stays the frame's root; `sidebar-collapse`, `sidebar-footer`, `zoom-controls`, `git-sidebar`, `canvas-sidebar`, `server-book`, `vault-state` keep their ids.
- The Sessions body (`children` + `ShellDock`, viewer column, `FileTreePanel`) must stay mounted and `hidden` in other modes; no terminal may remount.
- No search added to Git, Canvas or Servers sidebars. No custom title bar.

## Review Focus

- A shell open in Sessions survives Sessions → Git → Sessions: same pane, `shell_spawn` called once (Task 1 test).
- Text typed in the Sessions sidebar search survives a mode round trip (Task 1 test).
- Collapsing in Git or Servers mode, then expanding from the top bar's button, brings the sidebar back in that mode (Task 3 test).
- At the window's 1100px minimum width with the sidebar at 480px, the palette button and the tab strip are still on screen (Task 1 test).
- The viewer column's four-column threshold still opens a file in its own column at 1600px wide with the sidebar at 256px (existing `file-viewer.spec.ts` must stay green; Task 1 runs it).

---

### Task 1: The frame, the full-height column, and Sessions in it

**Files:**
- Create: `apps/desktop/src/components/layout/ModeSidebar.tsx`
- Modify: `apps/desktop/src/components/layout/AppShell.tsx` (the `aside`/resizer move out of the Sessions row; TopBar moves into a right column)
- Modify: `apps/desktop/src/components/layout/TopBar.tsx` (drop `Brand`/`DevBadge`; render the expand button at the left of row 1 while collapsed)
- Modify: `apps/desktop/src/components/layout/Sidebar.tsx` (remove the collapse button and the footer; they move to the frame)
- Test: `tests/smoke/unified-sidebar.spec.ts` (new), plus fixes in `tests/smoke/app-rail.spec.ts`, `tests/smoke/sidebar.spec.ts`

**Interfaces:**
- Produces: `export function ModeSidebar(): JSX.Element | null` — reads `useGitModeStore((s) => s.mode)` and `useSidebarStore` (`width`, `collapsed`, `setWidth`); returns `null` while collapsed; otherwise `<aside data-testid="sidebar" style={{ width }}>` with header (`Brand`, `DevBadge`, `SidebarCollapseButton`), body slot by mode, `SidebarFooter`, followed by the `PanelResizer` (`label="Resize sidebar"`, `edge="right"`, `clamp={clampSidebarWidth}`).
- Produces: `export function SidebarCollapseButton(): JSX.Element` — the existing toggle (`data-testid="sidebar-collapse"`, labels `Collapse sidebar` / `Expand sidebar`, `PanelLeftClose` / `PanelLeftOpen`, focus rescue as today), usable from both the frame header and `TopBar`.
- Produces: `export function SidebarFooter({ extra }: { extra?: ReactNode }): JSX.Element` — `data-testid="sidebar-footer"`, `h-9`, indexing text or `UpdateBadge`, `extra`, `ZoomControls`.
- Body slots in this task: Sessions → `<Sidebar />` (kept mounted, `hidden` when mode ≠ sessions); other modes render nothing yet (Tasks 2–4 fill them).

- [ ] **Step 1: Write the failing smoke tests** in `tests/smoke/unified-sidebar.spec.ts` using `installMockBridge(page, fixtureOneProjectOneSession())`:
  - `the sidebar runs from the top and the top bar starts at its edge`: `sidebar.boundingBox().y` < `open-palette.boundingBox().y`; `open-palette.boundingBox().x` > `sidebar.x + sidebar.width`; the first `tab-group-chip` (after opening the session) has `x >= sidebar.x + sidebar.width`; `getByTestId('sidebar').getByText('Qursor')` is visible.
  - `a shell survives a mode round trip`: open `+ Terminal` in the footer, click `mode-git`, click `mode-sessions`; `shell-pane-host` count 1 and `window.__FACTORAI_TEST_CALLS__` has exactly one `shell_spawn`.
  - `the sidebar search survives a mode round trip`: type `foo` in `Search projects and sessions…`, go to Git and back, the input still has value `foo`.
  - `at 1100px with a 480px sidebar the search and tabs are on screen`: viewport 1100×800, set width by dragging `Resize sidebar` to its max; `open-palette` and `tab-group-chip` bounding boxes have `x + width <= 1100`.
- [ ] **Step 2: Run them, expect FAIL** — `npx -y pnpm@10.28.0 e2e tests/smoke/unified-sidebar.spec.ts --retries 0` (the first fails on the y/x assertions).
- [ ] **Step 3: Implement** `ModeSidebar`, `SidebarCollapseButton`, `SidebarFooter`; restructure `AppShell` to `AppRail | ModeSidebar | <div className="flex min-w-0 flex-1 flex-col"><TopBar/> {mode bodies}</div>` above `StatusBar`; the Sessions row keeps `ref={row}` and loses the `aside`/resizer. Re-check `maxViewerWidth(shellWidth, sidebarWidth, …)`: `row` no longer contains the sidebar, so pass `0` for the sidebar term or drop it (whichever keeps `file-viewer.spec.ts` green).
- [ ] **Step 4: Run** the new spec plus `app-rail.spec.ts sidebar.spec.ts file-viewer.spec.ts update-badge.spec.ts usage-status-bar.spec.ts shell-footer.spec.ts` — all PASS; fix selectors in the old specs only where the moved toggle/footer changed them.
- [ ] **Step 5: Commit** — `feat: one full-height sidebar frame, Sessions in it, the top bar beside it`

### Task 2: Canvas in the frame

**Files:**
- Modify: `apps/desktop/src/components/canvas/CanvasModeView.tsx` (remove its `aside` and `PanelResizer`)
- Modify: `apps/desktop/src/components/layout/ModeSidebar.tsx` (canvas slot → `<CanvasSidebar />`)
- Test: `tests/smoke/unified-sidebar.spec.ts`

**Interfaces:** Consumes `ModeSidebar` body slot from Task 1.

- [ ] **Step 1: Failing test** `canvas lists its canvases in the shared sidebar`: click `mode-canvas`; `getByTestId('sidebar').getByTestId('canvas-sidebar')` visible; `getByRole('separator', { name: 'Resize sidebar' })` count 1.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** the slot and the removal.
- [ ] **Step 4: Run** the new spec and the canvas specs (`tests/smoke/canvas*.spec.ts`) — PASS.
- [ ] **Step 5: Commit** — `feat: Canvas uses the shared sidebar`

### Task 3: Git in the frame, its section state per tab

**Files:**
- Modify: `apps/desktop/src/store/gitModeStore.ts` — add `sectionByDir: Record<string, GitSection>` (not persisted) and `setSection(dir: string, section: GitSection): void`; default `'history'`.
- Modify: `apps/desktop/src/components/gitmode/GitModeView.tsx` — `GitTabBody` reads/writes the section from the store instead of `useState`; no `GitSidebar` in it.
- Modify: `apps/desktop/src/components/gitmode/GitSidebar.tsx` — root loses `w-60` (fills the frame body).
- Modify: `apps/desktop/src/components/layout/ModeSidebar.tsx` — git slot: `GitSidebar` for the active tab (`tabs.find(t => t.dir === activeDir)`), section from the store; with no tab, a `PanelEmpty`-style line "Open a repository from the tab bar."
- Test: `apps/desktop/src/store/gitModeStore.test.ts` (unit), `tests/smoke/unified-sidebar.spec.ts`, fixes in `tests/smoke/git-mode.spec.ts`

**Interfaces:** Consumes Task 1 slot. Produces `setSection(dir, section)` and `sectionByDir`.

- [ ] **Step 1: Failing tests**
  - unit `a section is remembered per repository`: `setSection('/a','changes')`; `sectionByDir['/a'] === 'changes'`, `sectionByDir['/b'] ?? 'history'` is `'history'`.
  - smoke `git's tree is in the shared sidebar`: open a Git tab (as `git-mode.spec.ts` does); `getByTestId('sidebar').getByTestId('git-sidebar')` visible.
  - smoke `collapse in Git mode, expand from the top bar`: in Git mode click `sidebar-collapse`; `sidebar` count 0; click `getByRole('button', { name: 'Expand sidebar' })`; `git-sidebar` visible and mode still Git.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** store field, lifted state, slot, empty state.
- [ ] **Step 4: Run** unit tests (`npx -y pnpm@10.28.0 test`) and `git-mode.spec.ts unified-sidebar.spec.ts` — PASS.
- [ ] **Step 5: Commit** — `feat: Git mode uses the shared sidebar, its section kept per repository`

### Task 4: Servers in the frame

**Files:**
- Modify: `apps/desktop/src/components/servers/ServerModeView.tsx` — export `ServerBook()` (today's `ServerSidebar`, reading `useServerBook()` itself, without `VaultFooter`) and `VaultFooter`; remove the `aside` from `ServerModeView`.
- Modify: `apps/desktop/src/components/layout/ModeSidebar.tsx` — servers slot `<ServerBook />`; footer `extra={<VaultFooter />}` only in servers mode.
- Test: `tests/smoke/unified-sidebar.spec.ts`, fixes in `tests/smoke/servers.spec.ts`

**Interfaces:** Consumes Task 1 `SidebarFooter({ extra })`.

- [ ] **Step 1: Failing test** `servers: the book and the vault are in the shared sidebar`: click `mode-servers`; `sidebar` contains `server-book`; `sidebar-footer` contains `vault-state`; the sidebar width equals the stored width (same as in Sessions mode, compare bounding box widths).
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `servers.spec.ts unified-sidebar.spec.ts` — PASS.
- [ ] **Step 5: Commit** — `feat: Servers uses the shared sidebar, the vault in its footer`

### Task 5: Docs and the full gate

**Files:**
- Create: `specs/adr/0115-one-sidebar-for-every-mode-full-height.md` (context / decision / consequences; supersedes TopBar's "spans above the sidebar" note; a future custom title bar puts window controls at the right of row 1 and its drag region in row 1 plus the sidebar header).
- Modify: `DESIGN.md` (Layout, chrome heights, The Rail Rule: 56px rail), `specs/05-features.md` (F1 sidebar + collapse in every mode, F16 top bar rows over the work area, F35/F44/F45 sidebar bullets), `apps/desktop/src/components/layout/TopBar.tsx` doc comment.

- [ ] **Step 1: Write the docs.**
- [ ] **Step 2: Full gate** — `format:check`, `lint`, `typecheck`, `test`, `deps:check`, `deps:unused`, `--filter @factorai/desktop run vite:build`, and `pnpm e2e` (known pre-existing Windows failures: `file-links`, `terminal-copy`, `terminal-grid`, `spawn-failure`; anything else must pass).
- [ ] **Step 3: Real window** — `npx -y pnpm@10.28.0 dev:windows`, screenshot each mode expanded and Sessions collapsed.
- [ ] **Step 4: Commit** — `docs: ADR-0115 and the spec for one full-height sidebar`
