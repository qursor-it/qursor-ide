# ADR-0077 — Pull requests go through the user's `gh`, and the app holds no credential

Date: 2026-10-13
Status: Accepted for this local fork. Extends ADR-0009 (git stays read-only in the first phase) and amends the "read-only" and "no network" statements in PRODUCT.md. Depends on ADR-0074 for native Windows process handling.

## Context

The owner reviews pull requests of the repository the project was cloned from, and wants to do it where the agents run, not in a browser tab. Until now the app made no network call of its own: ADR-0009 chose `git2` with no transport linked in so that every repository read is local, PRODUCT.md promised "Read-only … on git" and "no cloud sync, no accounts", and `services/sops.rs` set the pattern for anything external — run the user's own CLI, never hold its secrets.

Pull requests live on GitHub. Reading them means a network call and an identity. Three ways to get there:

1. **A GitHub client in the app** — an HTTP library, an OAuth flow or a pasted token, and a place to store it. That is a credential store, an account system and a refresh-token problem, in an app whose positioning is that it attaches to the CLIs the user already has logged in.
2. **`git2` with a transport** — links OpenSSL or libssh2 into the binary for clone-time features the app does not otherwise need, and still has no pull-request data: pull requests are not git objects.
3. **The user's `gh`** — already installed by the people who review pull requests from a terminal, already signed in, already handles GitHub Enterprise hosts, SSO and credential helpers, and prints JSON.

The clone this runs in is typically a fork: `origin` is the user's fork, `upstream` the original. `gh`, asked for "the repository" with no argument, resolves it from the remotes and prefers `upstream` — the repository the user did not choose.

## Decision

**Pull requests are read by running the user's `gh`.** `services/gh/` is `services/sops.rs`'s shape: locate the binary, run it once with a timeout, parse its stdout. Concretely:

- **A version floor, `gh` 2.40.** Older releases are reported as `tooOld` with the fix, not trusted to print JSON this code can read. `gh` is located on the child `PATH` and in the usual install folders; a `.cmd` or `.bat` shim is refused, because a batch wrapper does not forward stdin and would put arguments through a shell. At most four `gh` children run at once, each killed on timeout or when its request is dropped.
- **`origin`, else the one GitHub remote that is not `upstream`; every call names its repository.** The repository is read from `origin`'s URL, and when `origin` exists it is the answer (a non-GitHub `origin` is `notGitHub`, with no fallback past it). Without `origin`, every remote except `upstream` whose URL is GitHub is a candidate: exactly one is used (a clone whose only remote is called `xplorer` still works), none is `noOrigin`, and two or more are `noOrigin` too, naming the candidates so the user can rename the one they mean to `origin` rather than the app guessing whose pull requests to show. `upstream` is excluded because by convention it is the parent of a fork, never the user's own repository, and `gh` would pick it by itself, which is the surprise this ADR exists to avoid. The remote's name travels with the repository (`GhRepo.remote`), so the panel can show it when it is not `origin` and the P4 checkout fetches from it. Every invocation then carries it explicitly — `--repo HOST/OWNER/NAME`, or `gh api --hostname HOST repos/OWNER/NAME/…` — and `gh`'s `{owner}` / `{repo}` placeholders are never used. A unit test builds every argument list and fails on one that does not name the repository. GitHub Enterprise hosts are accepted; GitLab-shaped hosts are `notGitHub`.
- **The network is read, on request.** Calls happen when the pull-request panel is visible and asks: the list refreshes while the panel is shown, a detail while it is open, and nothing is polled when the panel is hidden. File lists and blobs are addressed by commit and never expire. Rate limits are met by caching on commit ids and by not polling, not by retrying.
- **The app stores no credential and never reads one.** `gh` finds its own login; the app sees only what `gh` prints. `gh auth token` is never run, no token appears in an argument list, a log or a setting, and the signed-in login (a name, not a secret) is the only identity the app learns. Sign-in is `gh auth login`, in the user's terminal; the panel says so and offers a refresh.
- **The only write the feature will ever make is submitting a review, and only behind an explicit confirmation** (a later phase). It will pass its body on stdin rather than in argv. Phase one has no write at all: no call to GitHub that is not a GET, and no `git fetch` — a pull request's head commit is read from the clone when it is there and from GitHub's contents API otherwise, so the repository's objects, refs and working tree are untouched.
- **No avatars and no other fetches from the network.** The renderer draws initials. Nothing but `gh`'s own requests for pull-request data leaves the machine, and only to the host named by `origin`.

## Consequences

- PRODUCT.md's "read-only on git" stays true of the repository and is amended where it was absolute about the network: the app now makes network reads, through `gh` and only on request, and still has no accounts, no telemetry and no credentials of its own. "No cloud sync, no accounts" is unchanged in spirit — nothing is synced, and identity belongs to `gh`.
- ADR-0009's no-transport `git2` is unchanged. Later phases that must write to the repository (checking a pull request out) will shell out to `git`, so the transport remains unlinked, and will need their own ADR.
- A machine without `gh`, or with `gh` signed out, gets an honest empty state in the panel; nothing else in the app changes. The rest of the app does not depend on `gh`.
- Error kinds grow by two — `Unauthenticated` for "no usable login" and `Remote` for a failure at the host (rate limit, network, SAML, validation) — mirrored in `packages/types`.
- Fork pull requests work without fetching: the base repository serves a fork head's commit by id.
- The cost is the dependency on `gh`'s output. The JSON fields are requested by name, parsed leniently (a missing field is a default), and pinned by captured samples in `tests/fixtures/gh/`; a field `gh` renames shows up as a failing fixture, not a blank panel.
- GitHub lists at most 3000 files of one pull request. Larger ones are shown truncated, with the count.
