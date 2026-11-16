# ADR-0100 — Session tabs are back; no Inbox, no Board

Date: 2026-11-16
Status: Accepted. Supersedes ADR-0092 (the Inbox replaces the session tab
strip) and the Board (F38); amends ADR-0093 (the app rail), ADR-0095 and
ADR-0098 (Git tabs as a sidebar list), F16, F35, F36.

## Context

After using the redesign the owner asked to go back to the session tab strip
with its status dots, which they found worked well, in both modes, with
Sessions and Git keeping separate lists; to drop the Inbox and the Board; and
to keep only the project tree in the sidebar. They also asked for the tabs to
be redrawn nicer than before.

## Decision

- The top bar's second row is the work row again: the session tabs (grouped by
  project, drag and keyboard reorder, collapse, middle-click, `Mod+W`,
  `Mod+PageUp/PageDown`) in Sessions mode and the repository tabs in Git mode,
  each its own list. The pre-Inbox components are restored, not rewritten.
- They are restyled: 32px, 8px-radius tabs, the active one a raised card, the
  group a soft tint of its project's swatch.
- The Inbox column, the open-list keys that went with it and the Board route
  are removed. The command palette keeps "next session that needs you".
- The app rail holds the two modes and settings. Sessions shows the project
  tree and, when it is collapsed, brings it back; its badge still counts the
  sessions that need you.
- Git mode's sidebar no longer lists the repositories; they are tabs.

## Consequences

- One place again for "what is open": the strip, as before 2026-11-13.
- A session waiting in a tab scrolled off the strip is only shown by its dot
  in the tree and the Sessions badge, the gap the Inbox was meant to close.
