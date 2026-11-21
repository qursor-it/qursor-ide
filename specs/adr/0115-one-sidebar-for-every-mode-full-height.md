# ADR-0115 — One sidebar for every mode, full height

**Date.** 2026-11-21
**Status.** Accepted. Design: `docs/superpowers/specs/2026-11-21-unified-sidebar-design.md`.

## Context

Sessions, Git, Canvas and Servers each grew their own left sidebar, and they
disagreed on width, resizing, collapse, header and footer: Sessions had a stored
width, a resizer and collapse; Canvas shared the width but not the frame; Git was
a fixed `w-60`; Servers a fixed `w-64` with its own header and vault footer. The
top bar's two rows also spanned the whole window above the rail and the sidebar
(the TopBar comment "spans above the sidebar for the custom titlebar", M5).

## Decision

1. **One frame, `ModeSidebar`, mounted once by `AppShell`**, beside the app rail
   and running from the top of the window content to the status bar. It owns the
   width, the `PanelResizer`, collapse, a `h-10.5` header (brand, DEV badge,
   collapse button) and a `h-9` footer (update badge, zoom). Each mode supplies
   only its body; Servers adds its vault state to the footer.
2. **One stored width** (`sidebarStore`, 180-480px) for every mode. Git and
   Servers lose their fixed widths.
3. **Collapse is available in every mode.** Collapsed, only the app rail remains
   and the frame is not rendered, so its resizer is gone too (ADR-0038). The
   toggle moves to the left of the top bar's first row while collapsed.
   Clicking the rail icon of the mode you are in expands it; switching to
   another mode never changes the collapsed state.
4. **The rail is 56px** (ADR-0093). DESIGN.md said 48px, the width of the old
   collapsed project rail; that rail is gone, so the Rail Rule now speaks of the
   app rail.
5. **The top bar's two rows sit only above the work area**, starting at the
   sidebar's right edge. This **supersedes the TopBar's old "spans above the
   sidebar" note** (M5). The brand moved into the sidebar header.
6. **A future custom title bar** puts the window controls at the right of row 1,
   and its drag region in row 1 plus the sidebar header.

Alternative: keep four sidebars and align their classes. Rejected: it is how they
drifted apart, and every new mode would copy the frame again.

## Consequences

- The Sessions body stays mounted while hidden in other modes, so terminals keep
  running; moving the sidebar out of the Sessions row remounts nothing. Git's
  section state lives in `gitModeStore`, per repository, so the frame and the
  body share it.
- The sidebar is outside the row `AppShell` measures, so `shellWidth` is the
  width right of it and nothing subtracts the sidebar: `columnThreshold`,
  `maxPanelWidth` and `maxViewerWidth` lost their `sidebarWidth` term, and
  `effectiveSidebarWidth` is gone. This amends ADR-0037 and ADR-0038.
- No search or filter was added to Git, Canvas or Servers.
- While collapsed the brand is not shown.
