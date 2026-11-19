# ADR-0109 — A persistent tab outlives the app, by the human's choice

**Date.** 2026-11-19
**Status.** Accepted. Narrows ADR-0005 (kill-on-quit) for one opt-in case, and
leaves its rule for every process on this computer untouched. Arises from F48.

## Context

ADR-0103 made killing an SSH tab hang up its remote shell, so nothing the app
started is left behind on a server. The owner also asked for MobaXterm's
persistent sessions: a long deploy or migration must survive a dropped Wi-Fi,
a laptop lid, or quitting the app.

## Decision

1. **Per server, off by default**: "Keep running in tmux". A tab of such a
   server opens `tmux new-session -A -s qursor-<tab>` when the server has tmux,
   and a plain login shell when it does not. Nothing is installed.
2. **Only closing the tab ends the work.** Closing it (×, middle click, deleting
   the server) runs `tmux kill-session` for it. Quitting the app, a dropped
   connection or Disconnect leave the session, and the tab reattaches when it
   is next shown.
3. **Agents still use the tab** (ADR-0107): the marker bootstrap turns on tmux's
   passthrough for that pane only and wraps its markers for it; and because
   tmux draws its own alternate screen, whether the pane is at rest (no
   full-screen program, nothing but the shell running) is asked of tmux itself
   over a separate exec channel before anything is typed.

## Consequences

- Processes on this computer still die on quit, without exception.
- A tmux older than 3.3 has no passthrough: agents get "did not answer the
  command markers" in such a tab, and the human can turn persistence off.
- Sessions of tabs closed while the app could not reach the server stay on the
  server until the human kills them.
