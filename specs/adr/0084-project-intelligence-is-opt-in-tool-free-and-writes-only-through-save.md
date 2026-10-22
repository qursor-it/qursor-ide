# ADR-0084 — Project Intelligence is opt-in, tool-free, and writes only through Save

Date: 2026-10-22
Status: Accepted for this local fork. Builds on ADR-0004 (an agent's store is read-only), ADR-0039 (a project file is written only when a human presses Save) and ADR-0029 (model-facing tools need their own server). Specified in F33.

## Context

A correction made twice, a test command that fails the same way in three sessions: today that knowledge dies with the session. The proposal behind this ADR (`Qursor IDE — Project Intelligence MVP`) is an *improvement engine*: observe the last sessions of a project, decide whether any lasting remedy is justified, and keep an honest record of what was done about it. Its value depends on three things going right, and each is a way to do harm:

1. **Reading transcripts and sending excerpts to a model.** A transcript holds paths, code, and sometimes secrets. A local CLI is not local inference: `claude -p` talks to a hosted model. Doing this silently, or for text the person never saw, is the worst outcome.
2. **Believing the model.** Its input is text written by other agents, tools and web pages. "Ignore previous instructions" in a transcript is data about what was said, not an instruction. Its output is untrusted for the same reason: ids it cites, counts it claims, and text it wants written into `AGENTS.md`.
3. **Writing project files.** `AGENTS.md` tells every future agent what to do. A feature that edits it on its own is a way for a stray sentence in a transcript to become standing policy.

The existing decisions already draw most of the lines. ADR-0004 keeps transcripts read-only. ADR-0039 allows exactly one writer, a human's Save. ADR-0053 says structured evidence has to come from the original transcript, because the FTS index flattens it. ADR-0060 and ADR-0061 say Claude and Codex transcripts differ and a project names its agent through its profile. What they do not settle is the shape of an analysis that sends text to a model.

## Decision

**1. It is off, and off means nothing happens.** A setting, `intelligence.enabled` (Rust `SettingKey::ProjectIntelligence`, TS `'projectIntelligence'`), in the SQLite `settings` table (ADR-0013: Rust enforces it). Unset is off; only the exact string `on` is on, so a typo can never enable it. While off, every intelligence command refuses with a typed `disabled` error, with two exceptions: reading the switch (which probes nothing), and cancelling a run (stopping work is never the dangerous direction). No transcript is read, no process started, no row written. Turning the switch off cancels runs in flight.

**2. Evidence is extracted deterministically, offline, from the original transcripts.** The last N completed top-level sessions of the project (20 by default, 50 at most) are read through the canonical `projects -> discovered_projects -> sessions` link, from the store of the profile that wrote them. Running sessions and sub-agents are skipped and counted. Four signals are extracted: a human correction, a verification command that failed, a re-run after a failure, and an instruction given twice. A Claude `user` record that carries a tool result, a caveat, a slash command or a sidechain turn is not a person. Excerpts are at most 280 characters, secret-redacted (best effort, stated as such), and capped per session and per run. **The backend mints the evidence ids**, derived from project, session and a fingerprint of the normalised source event, so the same event is the same id on every run and the model can only cite ids it was given.

**3. One tool-free, bounded, consented model call.** `AnalysisProvider` is a seam with one adapter, the Claude CLI run headless. Before a binary is used its `--help` must list every flag the isolation depends on (`--print --output-format --tools --strict-mcp-config --disable-slash-commands --no-session-persistence --permission-mode --safe-mode`); a binary that does not is `provider_unavailable`, not a weaker run. The call is `-p --output-format json --tools "" --strict-mcp-config --disable-slash-commands --no-session-persistence --permission-mode dontAsk --safe-mode`, which leaves the model no tools, no MCP servers, no hooks, no skills, no `CLAUDE.md`, nothing that asks permission, and no session file written into the store this feature reads. It runs in a fresh empty temporary directory, never the project. **The payload is on stdin**, never argv and never logged. It has a 120 second deadline and a 256 KiB output cap, in its own process group, registered on the run so cancel, timeout and quitting the app each kill it and everything it started. A reply that reports more than one turn or a denied tool is refused. `--bare` is not used: it would drop the user's OAuth login.

The call needs **consent per run**: `start_analysis` takes `consentRemote`, and the preview that precedes it names the provider as remote, lists every excerpt that would be sent so any can be excluded, and returns a hash that pins the set; a request whose hash no longer matches is refused (`preview_stale`), so what leaves the machine is what was shown. Without consent, or without a usable provider, the run is **signals only**: the deterministic observations are stored and shown as unreviewed, and nothing is spawned.

A Codex adapter is not built: `codex exec` was not verifiable against an installed binary here, and ADR-0060's rule applies — do not advertise a capability nobody tested.

**4. The model's answer is validated by the backend, and fails closed.** It must be a JSON object with a `candidates` array, or the run fails `invalid_output` and writes nothing. `kind` and `action` are allowlists. Every `evidence_id` must be one minted for this run in this project; unknown ones are dropped, and a candidate left with none is dropped. How many independent sessions support a candidate is computed from the real session ids, never read from the reply; a `recurring_correction` with fewer than two is weakened and flagged a hypothesis. Strings are single-line, free of control and direction-changing characters, redacted again and bounded; a recommendation that is too long or carries a code fence is dropped rather than shortened. At most five candidates. `{"candidates": []}` is `no_change`, a result, not a candidate. Extra fields, confidences and "tests passed" claims are not read. A run writes once, at the end, in one transaction; a failed, cancelled or timed-out run leaves only its own row.

**5. Candidates do not multiply and decisions stay decided.** A dedupe key (project, kind, action, normalised recommendation tokens) catches the same wording; a conservative merge (same action, same polarity, shared evidence, similar advice) catches a reworded one. Re-running on the same events adds nothing. New evidence attaches to an existing candidate without changing its status. `dismissed` and `reverted` are terminal; nothing moves a decided candidate back to `proposed`. A verification is an event typed in by a person, never a status and never inferred.

**6. A project file is written only by the human's Save, and the backend checks the disk afterwards.** Only an `instruction` candidate can become a file change. `prepare_instruction_change` resolves the target safely inside the project (`AGENTS.md`, else `CLAUDE.md`, else a proposal to create `AGENTS.md`; a link to the same file is one file; a link out of the project, a directory or non-UTF-8 text is refused), looks for an equivalent line, and returns the proposed content, its hash, the one appended block and a unified diff. **It writes nothing.** The viewer saves through the existing `write_file`. `record_implementation` then re-reads the file and records `implemented` only if the on-disk hash equals the one the renderer reports saving *and* the prepared line is there; otherwise it is `not_saved` or `file_conflict` and no status moves. The delta kept for revert is that one block and the hashes, never a file snapshot. `prepare_revert` proposes the exact inverse only while the file still hashes to what the Save left; otherwise it returns a conflict with manual instructions and overwrites nothing. `record_revert` marks `reverted` only after the instruction is verifiably gone.

**7. Nothing new is offered to a model.** No MCP tool, no write path, no capability, no plugin. The commands are the renderer's. No telemetry.

**8. One dependency is made explicit.** `sha2` (already in the tree through Tauri) for the stored hashes — fingerprints, evidence ids, dedupe keys and the file hashes a change is verified against. `std`'s `DefaultHasher` is not stable across Rust versions and these are persisted.

## Consequences

- The feature costs nothing and exposes nothing until a person turns it on, and a run costs nothing until they consent to it.
- A person sees, and can strike, every excerpt before it leaves the machine. Redaction is best effort and the preview is the control; the docs and the UI say so rather than promise anonymisation.
- The analysis depends on a CLI flag set that exists today (verified against Claude Code 2.1.308). A CLI that drops one is reported as unavailable and the feature degrades to signals only; it does not run weaker.
- Evidence ids being stable across runs lets the preview's exclusions survive into the run and lets a re-run add no rows, at the price that an id is a function of its inputs and not a secret. Membership in the run's own set, not unguessability, is what rejects an invented one.
- Recording an implementation is only as strong as the renderer's report of what it saved. The backend can prove the disk matches that hash and contains the line; it cannot prove which keystroke put it there. That is the same trust ADR-0039 places in the Save button.
- Because the target's hash is compared at record time and not locked in between, a file edited in the gap is a `file_conflict`, which asks for a fresh preparation. This is the chosen failure.
- An excerpt is removed from the database when its session leaves the index (retention), so a candidate can outlive the words that justified it. It then says the evidence is gone instead of showing it.
- The Windows `.cmd` wrapper path for the CLI passes `--tools ""` through PowerShell; an empty argument is not verified there.
- Codex sessions are read as evidence (messages, shell calls and their results) but cannot yet be the analyser. Their tool-result shapes are modelled on how the CLI is documented to write them and are not verified against a live rollout beyond the messages `F30` already covers.

## Alternatives rejected

- **Give the model read tools over the repository.** Better candidates, and a model with a file-reading tool is a way to exfiltrate any file in the project through a crafted transcript. The input is bounded excerpts on purpose.
- **Apply the instruction change automatically after review.** ADR-0039. "The agent already did it" is not a reason to skip asking.
- **Scan the user's harness (skills, hooks, MCP, plugins) for duplicates.** A separate consent and scope; this is P2 in the proposal. Only the one instruction file is read, and only for duplicates.
- **One consent per project, remembered.** Consent is about specific excerpts. A new session changes them, which is why the preview hash exists.
- **Persist the full evidence in the database.** The record is an excerpt and a pointer; the transcript stays where the agent put it.
- **`--bare`.** Strictly API-key auth. It would make the feature unusable for anyone signed in with OAuth.
- **A confidence score on candidates.** Nothing measures one. The honest quantities are the number of independent sessions and whether it is a hypothesis.

## Related

- `specs/05-features.md` F33, `specs/02-data-model.md` § "Project Intelligence", `specs/03-backend-rust.md` § `intelligence`
- ADR-0004, ADR-0013, ADR-0029, ADR-0039, ADR-0053, ADR-0060, ADR-0061
