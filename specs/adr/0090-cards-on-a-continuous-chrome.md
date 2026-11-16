# ADR-0090 — Cards on a continuous chrome, and softer palettes in both themes

Date: 2026-11-13
Status: Accepted. Amends `DESIGN.md` § Overview, § Neutral, § Light Theme,
§ Layout, § Elevation & Depth and § Shapes. First slice of the layout redesign
whose later slices (the Inbox, the session header, the palette, the project
page, Git mode) land as their own specs and ADRs.

## Context

A UI/UX review of the app (2026-11-13) found the shell hard to read at a
glance, which is the one thing `PRODUCT.md` says the window is for:

- The dark theme was four near-black neutral steps two or three lightness
  points apart. The sidebar, the top bar, the session header and the terminal
  were near enough in tone that nothing told the eye where the work was.
- Every region was separated by a 1px hairline: top bar, sidebar, session
  header, shell footer, status bar. The result read as a grid of rules rather
  than as a place where one thing (the session) is in front of everything else.
- The light theme was a pure-grey translation of the same grid, so switching
  to it changed the brightness and kept the problem.

The owner reviewed mock-ups of alternatives and chose the direction recorded
here: a warm light theme ("Paper"), a soft blue-grey dark theme ("Slate"), and
a layout in which the chrome is one continuous surface and the work area floats
on it as a card.

## Decision

- **A new `--chrome` token is the window's ground.** The top bar, the sidebar
  and the status bar sit on it with **no border between them**, as one surface.
- **The work area is a card on the chrome**: the session column (route plus the
  shell footer), the viewer column and the file panel each get a 12px radius, a
  hairline border, the `background` (or `card`) fill and `shadow-card`. The gap
  between two cards is the resize handle, which paints nothing until it is
  hovered or focused. Stacked panes inside one card keep their hairline.
- **Two elevation tokens, `--elevation-card` and `--elevation-popover`**,
  exposed as the `shadow-card` and `shadow-popover` utilities. Overlays
  (dialog, popover, menus, select, tooltip, hover card, toast) use
  `shadow-popover`, whose first layer is a 1px ring, so they need no border.
- **Dark ("Slate")**: neutrals move from pure grey at 14–26% lightness to a
  blue-grey (hue 262, chroma ~0.016) at 21.5–35%: chrome 21.5%, ground 26%,
  card 28.5%, popover 29.5%, pressed 32.5%, hairline 35%, terminal well 24.5%.
  The brand blue `#7193C8` stays the dark accent.
- **Light ("Paper")**: neutrals move from pure grey to a warm ivory (hue 80,
  chroma ~0.008): chrome 96.2%, ground 99.2%, card and popover white, hairline
  90%, terminal well 99.6%. The light accent moves to `oklch(50% 0.13 264)`, a
  slightly more saturated step of the same hue, 6:1 on the ground.
- **`--radius` goes from 8px to 10px**, so the derived steps become 10 / 8 / 6px
  (`lg` / `md` / `sm`). Overlays take 12px (`rounded-xl`), dialogs 16px
  (`rounded-2xl`), menu rows 6px.
- **The dialog overlay is `black/40` with a 2px backdrop blur** instead of
  `black/80`: the app behind a confirmation stays recognisable, which is what
  tells the user what they are confirming.
- **The terminal's dark ANSI `black` and `brightBlack` move up** (to 57% and
  66% lightness) so they keep the 3:1 floor of ADR-0082 on the lighter well.

## Consequences

- `DESIGN.md`'s "Flat-By-Default Rule" no longer holds for the shell: a card
  that stays on screen now has a shadow. It is replaced by the Card-On-Chrome
  Rule: chrome carries no shadow and no border, cards carry both, overlays carry
  the stronger popover shadow. Rows, tabs, inputs and buttons stay flat.
- The dark theme is no longer the identity with light as a translation; both
  are first-class. `system` stays the default.
- Every `bg-card` header inside a card (session header, viewer header, shell
  footer) keeps working unchanged: it is now one light step inside the card
  rather than the same tone as the chrome.
- The README and guide screenshots are out of date until re-shot with
  `pnpm docs:shots`.
