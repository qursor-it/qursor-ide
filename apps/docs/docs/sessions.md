---
id: sessions
title: Sessions
---

import Shot from '@site/src/components/Shot';

# Sessions

A session is one agent process (`claude` or `codex`) and its transcript; see [Agents](agents).

## Starting and resuming

<Shot
  src={require('../../../assets/images/guide/sessions-project-page@2x.png').default}
  alt="The billing-api project page: its path, Sessions and Routines tabs, a New session button, one pinned session and five recent sessions with turn counts and ages"
/>

- **New session**: the `+` on a project row, **New session** on the project page or in the
  project's right-click menu, or `Mod+N`. With both agents installed, the chevron beside `+` and
  **New session with ▸** pick one.
- **Resume**: click any session.
- **Close**: `×` in the session header (**Close session**), or on its row in the sidebar, kills
  the process and closes the session.
- **Restart**: when the process exits on its own, its output stays and **Restart** replaces `×`.
  Clicking a stopped session in the sidebar also restarts it.

## Status

<Shot
  src={require('../../../assets/images/guide/sessions-status@2x.png').default}
  alt="The sidebar with every project expanded: a green dot on the working billing-api session, amber on the docs-site session waiting for input, grey on the stopped homelab session, and the same colour badged on each project's avatar"
/>

Shown on the session, its tab and its project's avatar.

| Dot | Meaning |
| --- | --- |
| Green | Working. |
| Amber | Waiting for you: a permission prompt, a question, a turn handed back. |
| Blue | Working without being open: a routine running in the background. |
| Grey | Stopped. |
| Hollow grey | Live, but the agent has not said what it is doing yet: a Codex session in its first second. |

## Tabs

<Shot
  src={require('../../../assets/images/guide/sessions-tabs@2x.png').default}
  alt="Session tabs in the top bar, grouped by project with a tinted chip, each with its status dot; the open session's header shows its title, a Working pill and the agent's terminal output"
/>

- Each open session is a tab in the top bar, grouped by project; it stays after the process
  exits. Drag to reorder. Git mode has its own tabs, one per repository.
- Tabs return, stopped, on relaunch (**off**: **Settings → Sessions → Restore open tabs on
  launch**).
- `Mod+W` closes the focused tab, `Mod+PageDown` / `Mod+PageUp` step through tabs, middle-click
  closes a tab.
- Closing a working session asks first. **Settings → Confirmations**: **Ask before closing
  a running session** and **Ask when a middle-click closes a tab**.
- `Mod+K` opens the command palette: sessions, projects and actions in one field.

## Answering the agent

- When the agent waits for you, a banner over its terminal says what it asks. For Claude's
  permission menu it shows a button per option, each pressing that number for you.
- Anything else you answer at the agent's prompt in the terminal, as always — `!` commands
  and `/` commands included.
- Nothing is sent without your click.

## Pinning

**Pin session** in the right-click menu, or the pin in the session header. Pinned sessions lead
their project's list.

## Deleting

<Shot
  src={require('../../../assets/images/guide/sessions-row-menu@2x.png').default}
  alt="The right-click menu on a session row in the sidebar: Pin session, Copy transcript path and Delete session"
/>

The right-click menu also has **Copy transcript path** and **Delete session**. Deleting asks,
stops the session, and moves the transcript to the system trash, where you can restore it.

## Search

<Shot
  src={require('../../../assets/images/guide/sessions-search@2x.png').default}
  alt="Search results for retry: four hits across billing-api, homelab and docs-site, each with its project, session title, a matching snippet and whether the user or the assistant said it"
/>

`Mod+F` (outside the viewer) focuses the sidebar search. It full-text searches every added project; every word must
match. Click a result to open the session.
