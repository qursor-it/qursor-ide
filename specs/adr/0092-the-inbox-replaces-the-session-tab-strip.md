# ADR-0092 — The Inbox replaces the session tab strip

Date: 2026-11-13
Status: Accepted. Supersedes ADR-0080 (tab groups) for Sessions mode; Git mode
keeps its own grouped strip. Amends F16 and F36.

## Context

After ADR-0090 the same open sessions were listed three times: the project
tree, the Inbox (F36) and the top bar's tab strip. The strip truncated titles,
needed horizontal scrolling past a handful of sessions, and grouped by project,
not by what each session needed from you. The owner asked for the mock-up's
layout, which has no strip.

## Decision

- **Sessions mode has no tab strip.** The top bar's second row exists only in
  Git mode, for its own tabs.
- **The Inbox lists everything open**: Needs you, Working, then **Open** (open
  sessions that have stopped) and **Pull requests** (open pull request tabs).
- It carries what the strip did: a close `×` on every row (visible on hover, on
  focus and on the current row), middle-click to close, the same
  `CloseSessionConfirm` with the same gestures and settings, `Mod+W` for the
  current session or pull request, and `Mod+PageUp/PageDown` stepping in list
  order. A stopped session is restarted when opened, as a stopped tab was.
- The persisted list of open sessions (`terminalStore.tabs`) and of pull
  requests (`prTabStore`) is unchanged, so what was open before the upgrade is
  open after it.
- **Drag to reorder and per-project groups go.** The Inbox orders by state,
  which is the point of it; `tabLayoutStore` keeps serving Git mode.

## Consequences

- Nine smoke tests about dragging, folding and moving groups of session tabs
  are deleted with the feature; the closing, confirming, stepping and restoring
  ones move to `open-sessions.spec.ts`.
- The settings rows still say "tabs" ("Restore open tabs on launch", "Ask when
  a middle-click closes a tab"); renaming them is left for a settings pass, since
  their stored keys do not change.
