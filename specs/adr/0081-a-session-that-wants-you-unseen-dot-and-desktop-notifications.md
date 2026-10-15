# ADR-0081 — A session that wants you: an unseen dot and desktop notifications

Date: 2026-10-14
Status: Accepted for this local fork. Builds on ADR-0015 (the terminal title is the one status source). Specified in F32.

## Context

Roadmap items 34 and 35 (upstream issues #29 and #30) ask for the same thing from two sides: with several sessions running, and the user often in another app, say which session wants them. ADR-0015 made the terminal title the single source of status and rejected hooks, so `waiting_input` covers both "finished its turn" and "is waiting on a permission prompt"; nothing the app can read tells them apart.

## Decision

- **One signal, no hooks.** A session *wants you* on `working → waiting_input` and on `working → stopped`, and on no other transition. `unknown → *` never counts.
- **A hybrid split.** Rust sends the desktop notification, because it only needs to know whether the window is focused and can observe that itself. The renderer owns the unseen mark, because it is the one that knows which session is the active route. The two never talk: both read the same `terminal:status` events.
- **A notification only when the window is not focused,** after a 1 s settle. The check is dropped if anything moved for that terminal in the meantime, so a title that flips and flips back sends nothing. A terminal's first status event never alerts, so Claude's boot title does not notify. Inside the app the red dot is enough; there is no toast.
- **Red is reserved for unseen** (besides errors). An unseen session's `StatusDot` is painted `status-unseen` (`destructive`) instead of its state colour, because a badge on a 6px mark cannot be seen. It shows on session tabs, sidebar session rows and pending rows, and the project avatar (an aggregate), including the collapsed rail.
- **Cleared by looking, or by typing.** The mark goes when the session is, or becomes, the active route session in a focused window, or when its status goes back to `working`. It is not cleared on exit (a `stopped` is followed at once by `terminal:exit`) and not persisted.
- **The notification names the project and the topic.** Title is the project's display name; body is "Waiting for you" or "Exited", plus the topic Claude puts in the terminal title. The context is captured when the status event arrives, since an exiting terminal is gone from the manager before the settle check runs. The window also asks for attention (`request_user_attention`, informational) until it regains focus.
- **A setting, `notifications.desktop`,** in the SQLite `settings` table (ADR-0013: Rust reads it). Unset is on, `off` is off. The dot has no setting.
- **tauri-plugin-notification, no JS capability.** Only Rust sends. A failed send is logged and ignored.

## Consequences

- The transition rule exists in two languages (`services/attention.rs::wants_you`, `lib/attention.ts::wantsYou`); each is tested against the same table, and F32 holds the one copy in prose.
- A click on a notification only brings the window forward; opening that session is not attempted, as click handling is not reliable across the desktop platforms.
- `working → waiting_input → stopped` inside one settle window sends nothing: the `stopped` is not a wants-you transition from `waiting_input`, and the pending check is stale.
- The two sides seed differently: Rust ignores a terminal's first event, the renderer treats the previous status as `working`. A session that boots to `✳` while not active and focused is marked unseen in the app but never notified.
- Delivery depends on the platform (F32 has the table): macOS notifications are tied to the bundle id and to System Settings, Linux needs a notification daemon, and Wayland may ignore the urgency hint.
- Telling "finished" from "needs permission" can come later through hooks, by superseding ADR-0015; the one signal here does not have to be reworked for it.
- Upstream roadmap items 34 (partly) and 35 are served by this fork's change; the upstream issues are referenced, not closed. The never-opened axis of item 34 stays deferred.
