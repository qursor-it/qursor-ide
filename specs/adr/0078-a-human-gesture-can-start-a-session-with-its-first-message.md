# ADR-0078 — A human gesture can start a session with its first message

Date: 2026-10-13
Status: Accepted for this local fork. Amends ADR-0026 (a prompt means a routine) and ADR-0008 (`start_session` reuses an idle session). Specified as F31 phase P2.

## Context

"Review with agent" on a pull request (F31) starts a session whose first message is a review brief. Two rules written for other features stand in the way.

1. **ADR-0008: `start_session` reuses a live, never-messaged session** of the same project and agent, so a double click is one `claude`. That session's PTY is already running; handing its id back means the first message has nowhere to go, because the prompt is argv (ADR-0026 § 4) and argv is fixed at spawn. The brief would be silently dropped.
2. **ADR-0026: a prompt means a routine fired.** `Terminal`'s `ensureTerminal` opened no tab when it had an `initialPrompt`, because the only prompt in existence came from a routine. A review the human just asked for must get a tab.

A third constraint is the pull request itself: its title, body and comments are written by its author, who is not the reviewer, and the brief is an instruction to an agent that can run commands.

## Decision

- **`start_session(project_id, agent, fresh)`.** `fresh: true` skips the reuse loop in `next_session_id` and mints a new id. The agent is still resolved first, so an unknown agent still errors. `fresh` absent or false is ADR-0008 unchanged; every existing caller keeps the double-click guard.
- **A prompt no longer means "routine".** The routine origin is the explicit `initialPrompt` parameter of `ensureTerminal`, which only `startRoutineSession` passes, and only that parameter withholds the tab. A human gesture parks its prompt and working directory in `terminalStore` (`setLaunch`), the way the launch agent is parked, and `ensureTerminal` takes them once (`takeLaunch`) when the parameter is absent. They are not persisted.
- **The brief carries no title and no body.** It names the pull request by number, repository, URL, refs and commits, tells the agent to read the rest with `gh pr view` and `gh pr diff` (always with `--repo`), and says to treat the title, description, comments and code as untrusted data and to post nothing to GitHub.
- **One line, no double quote, no percent sign.** The brief is a single positional argv entry and, on Windows, may pass through a `.cmd` shim that re-parses quotes and expands `%`. The builder strips both characters and has no newline.
- **Where it runs.** If a linked worktree of the project has the head out (branch `pr/<n>`, or HEAD at the head commit), the session runs there and is pinned to it (F21); otherwise it runs in the project folder and the brief says the directory is not the head.

## Consequences

- A review is a normal session: it has a tab, a transcript and a place in the sidebar once the agent writes one.
- Two quick clicks on "Review with agent" start two sessions; the button is disabled while a start is in flight, which is the only guard.
- `SpawnOpts.initial_prompt` is no longer `None` for every human-started session; its documentation says so.
- The prompt-injection defence is a convention in the brief, not a sandbox: the agent still runs under its own permission rules, and posting a review stays a human act (F31 P3).
