# ADR-0079 — Checking out a pull request writes a new worktree and nothing else

Date: 2026-10-13
Status: Accepted for this local fork. Amends ADR-0009 (read-only git) and ADR-0019 § 4 (nothing writes to git) for this one action. Specified as F31 phase P4.

## Context

A reviewer who wants to run a pull request needs its head on disk. ADR-0009 made every git operation in the app read-only and linked `git2` without a transport, and ADR-0019 § 4 left `git worktree add` to the agent in the terminal. F31 P2 already prefers a worktree that has the head out; until now the human had to create it by hand.

Three things make this different from the rest of git in the app.

1. **It writes to the repository**: a new directory, a new entry under `.git/worktrees/`, and a new branch.
2. **It needs the network.** The head of a pull request is not in the clone, least of all a fork's. Fetching it needs the user's credential helpers and SSH setup, which libgit2 without a transport does not have and which the app should not reimplement.
3. **The code is someone else's.** A checkout puts a stranger's files on disk, and the next step is usually to run them.

## Decision

- **The `git` CLI, not `git2`, for this action.** The app finds `git` on the PATH (and a few well-known install locations), refuses a `.cmd` / `.bat` shim, and runs it as a one-shot child with the shared child environment, a timeout (120 s for the fetch, 60 s for `worktree add`) and kill-on-drop. `GIT_TERMINAL_PROMPT=0` makes a missing credential an error instead of a hang; the user's credential helpers keep working. `git2` stays `default-features = false` and everything else in the app stays read-only.
- **Two commands, both naming the main checkout with `-C`.** `git -C <main> fetch --no-tags --no-write-fetch-head <remote> refs/pull/<n>/head`, then one of `git -C <main> worktree add -b pr/<n> <path> <oid>`, `worktree add <path> pr/<n>` (the branch already points at the head) or `worktree add --detach <path> <oid>`. The commit is the one the human was shown, and it is verified present with `git2` before anything is added.
- **The guarantee.** The app never runs `checkout`, `switch`, `reset`, `stash`, `restore`, `clean` or `push`, never passes `--force`, and never writes to the current checkout, its index or HEAD, or to any other worktree. What changes on disk is one new linked worktree and, at most, one new branch `pr/<n>`. A branch `pr/<n>` that exists at another commit is left alone: the human is offered a detached checkout instead. A test holds every argument list to this.
- **Where.** Next to the main checkout, as `<main folder>-pr-<n>`, with `-2`, `-3` appended while the path is taken. The path is shown before anything happens.
- **A confirmation first**, naming the folder, the branch and the commit. When the head lives in a fork the dialog says so: running its code can execute anything it contains. A worktree that already has the head out turns the button into "Open checkout" and nothing is run.
- **`<remote>`** is the remote the PRs tab resolved (`origin` unless the repository has none, ADR-0077); its name is checked not to start with `-` or contain whitespace before it reaches the argument list.

## Consequences

- `gh_pr_checkout_plan` reads GitHub and the worktree list and writes nothing; `gh_pr_checkout` is the write. The renderer calls it only after the confirmation, and never retries it.
- A machine without `git` on the PATH gets "git not found on PATH" instead of a button that cannot work.
- The fetch adds objects to the repository's object store, which is the point and the only side effect outside the new worktree.
- Hooks configured in the repository (for example `post-checkout`) run in the new worktree as they do for any `git worktree add`.
- The agent can still create worktrees itself in the terminal; this is the human's one-click version for reviewing.
