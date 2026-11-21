# Agent hands work off to a new session — design

## Intent

An agent started from Qursor IDE can, mid-task, propose to the human "I'd pass
this part to a new session" and, if the human agrees, open that session itself
with a first message that briefs it. Fire-and-forget: the new session lives on
its own, exactly as if the human had opened it; the caller does not wait for it
or read its result.

## Goals / non-goals

- Goal: one MCP tool on the `factorai` server, available to every agent session
  Qursor starts (Claude and Codex).
- Non-goal: reporting back, waiting, parent/child links between sessions.
- Non-goal (for now): choosing another agent or a worktree as a parameter.
- Anti-goal: an agent opening sessions behind the human's back, or in another
  project.

## Approaches

- **A. Tool + agent-side consent (picked).** The agent asks in chat; Claude
  Code's own MCP permission prompt is the second gate. Smallest: one tool, one
  event, one listener. Ceiling: an "always allow" removes the prompt; Codex's
  MCP approval behaviour not verified.
- B. A + a Qursor confirm banner (reusing `agent_policy::Confirmations`).
  Consent that always holds, at the cost of more UI and a double ask.
- C. A one-shot `createRoutine`. Rejected: a routine fire carries a routine
  origin (`setRoutineOrigin`), is claimed and recorded as a routine run
  (ADR-0030) and lands in the routines list.

## Design

- Tool `startSession({ prompt })` in `services/agent_tools/tools.rs`. The
  description tells the model to ask the human first and to write a
  self-contained brief (the new session has no memory of this one), like
  `createRoutine`'s prompt guidance.
- Bound in `start_agent_tools` (`services/terminal.rs:1400`) like routines:
  project, caller's agent and caller's cwd come from the closure, never from
  arguments.
- The closure emits a Tauri event (e.g. `agent-session:start` with
  `projectId`, `agent`, `prompt`, `cwd`). Tool answers at once with a short
  confirmation.
- Renderer listener (pattern of `useRoutineFires` / `useStartReviewSession`):
  `cmd.startSession(projectId, agent, true)`, `setLaunchAgent`, `setLaunch({
  prompt, cwd })`, spawn the terminal, invalidate the project's sessions. No
  `navigate`: the new session appears in the sidebar without stealing focus.
- Same project, same agent, same folder as the caller.
- Spec entry in `specs/05-features.md` and an ADR in the same PR, per the
  repo's spec-and-ADR workflow.

## Left to the builder

- Event name, payload type in `packages/types`, and whether the listener
  reuses `startRoutineSession`/`ensureTerminal` or the `startSession` +
  `setLaunch` path.
- What the tool returns besides "started" (e.g. the new session id).
- Whether the event can be missed (window not ready) — unlike routines it is
  only emitted while a session, and so the window, is alive.
