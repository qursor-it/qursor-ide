# Agent starts a new session — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An MCP tool `startSession({ prompt })` on the `factorai` server that opens a new agent session in the caller's project, with `prompt` as its first message.

**Architecture:** The tool lives in `services/agent_tools/tools.rs` as a new closure group bound in `TerminalManager::start_agent_tools` (project, agent, cwd baked in). The closure emits `agent-session:start`; a renderer hook mints a session id with `cmd.startSession(projectId, agent, true)` and spawns it headless exactly like a routine fire, so it shows up in the sidebar without taking focus.

**Tech Stack:** Rust/Tauri 2, React + Zustand, vitest, cargo test.

**Spec:** `docs/superpowers/specs/2026-11-21-agent-start-session-design.md`

## Global Constraints

- Project, agent and cwd come only from the closure, never from tool arguments (same rule as `Routines`, ADR-0028).
- `AppHandle::emit`, best-effort `let _ = app.emit(...)` (`.claude/rules/rust.md`).
- Tool texts and instructions contain no `\t` (existing test `initialize_tells_the_session_where_it_is_running`).
- Spec entry in `specs/05-features.md` and an ADR in `specs/` in the same PR. Use the **next free** F-number and ADR number at implementation time (currently F50 / ADR-0113, but the untracked cloud-sessions draft also eyes ADR-0113 — re-check).
- Branch in its own worktree off `develop` (`pull-requests` / `spec-and-adr-workflow` skills).

## Review Focus

- Empty or whitespace-only `prompt` → tool error, no event (test in Task 1).
- Extra `projectId`/`cwd`/`agent` arguments are ignored; the event carries the session's own (test in Task 1).
- A Codex caller starts a Codex session, not Claude (event carries `agent`; test in Task 1 via the closure input).
- Window not ready when the event fires: acceptable — the event can only come from a live session, so the window is alive. No drain needed (note it in the ADR).
- `cmd.startSession` failing in the renderer → `console.error`, no crash (Task 2).

---

### Task 1: Rust tool, event and wiring

**Files:**
- Modify: `apps/desktop/src-tauri/src/services/agent_tools/tools.rs` (struct + `AgentTools::new`, `instructions`, `tools_call` at :179, `tools_list` at :534, tests at :796+)
- Modify: `apps/desktop/src-tauri/src/services/agent_tools/mod.rs` (re-export if `Routines`/`Canvases` are re-exported there)
- Modify: `apps/desktop/src-tauri/src/services/terminal.rs` (new `on_session_start` callback beside `on_worktree` at ~:840 and in `for_app` at :858; `start_agent_tools` at :1400 gains `agent: &str`, call site :1832 passes `env_agent.id`)
- Modify: `packages/types/src/index.ts` (next to `RoutineFireEvent` at :1165)

**Interfaces:**
- Produces (Rust):
  ```rust
  pub struct SessionStart { pub project_id: String, pub agent: String, pub prompt: String, pub cwd: String } // Serialize, camelCase
  pub type StartSession = Arc<dyn Fn(&str /*prompt*/) -> AppResult<()> + Send + Sync>;
  // AgentTools::new(routines, canvases, start_session: StartSession)
  ```
  Event name `agent-session:start`, payload `SessionStart`.
- Produces (TS): `export interface AgentSessionStartEvent { projectId: string; agent: AgentId; prompt: string; cwd: string }`

- [ ] **Step 1: Failing tests in `tools.rs` tests module.** Fixture gets a `started: Arc<Mutex<Vec<String>>>` recording prompts the closure received.
  - `the_server_is_not_named_ide_and_offers_exactly_these`: add `"startSession"` after `"canvasPatch"`.
  - `createroutine_is_always_loaded_and_the_others_are_searchable`: topic for `startSession` is `"session"`.
  - `startsession_hands_the_prompt_to_this_session_and_says_so`: call with `{ "prompt": "Fix the flaky test", "projectId": "other", "cwd": "/elsewhere", "agent": "codex" }` → not an error, text contains `"new session"`; `started == ["Fix the flaky test"]` (extra arguments never reach the closure — its signature has no room for them).
  - `startsession_refuses_an_empty_prompt`: `{ "prompt": "  " }` and `{}` → `isError == true`, `started` empty.
  - `initialize_tells_the_session_where_it_is_running`: also assert `instructions.contains("startSession")`.
- [ ] **Step 2: Run, expect FAIL.** `cd apps/desktop/src-tauri && cargo test agent_tools` (filtered run is fine for the red/green loop; the gate in Task 3 is the plain command).
- [ ] **Step 3: Implement.**
  - Tool definition (not `alwaysLoad`, not `readOnlyHint`; `searchHint` containing "session" and "Qursor IDE"). Description, exact copy:
    "Open a new agent session in this same project and folder, with `prompt` as its first message, to hand off a part of the work. Ask the human first and call this only after they agree. Write the prompt for an agent starting cold, with no memory of this conversation: the goal, the files involved, what is already decided and how to verify. The new session runs on its own; you will not see its result. Say what you handed off."
  - `startSession` handler: trim-check `prompt`, call the closure, reply `tool_text("Started a new session with that prompt. It is in the sidebar of this project in Qursor IDE.")`; closure error → `tool_error`.
  - `instructions()`: one sentence naming `startSession` for handing off a part of the work after the human agrees.
  - `terminal.rs`: `on_session_start: Arc<dyn Fn(SessionStart) + Send + Sync>` emitting `agent-session:start` in `for_app`, no-op wherever else `TerminalManager` is constructed (tests). In `start_agent_tools` build the closure from `project_id`, `agent`, `cwd`.
  - TS type in `packages/types`.
- [ ] **Step 4: Run, expect PASS.** Same command.
- [ ] **Step 5: Commit** `feat: startSession tool lets an agent open a new session with a first message`

### Task 2: Renderer starts the session

**Files:**
- Modify: `apps/desktop/src/lib/tauri.ts` (events, next to `onRoutineFire` at :917)
- Modify: `apps/desktop/src/components/terminal/Terminal.tsx` (next to `startRoutineSession` at :1046)
- Create: `apps/desktop/src/hooks/useAgentSessionStarts.ts`
- Modify: `apps/desktop/src/routes/__root.tsx:60` (mount beside `useRoutineFires()`)

**Interfaces:**
- Consumes: `AgentSessionStartEvent`, event `agent-session:start` (Task 1).
- Produces: `events.onAgentSessionStart(cb)`; `export function startAgentSession(sessionId: string, e: AgentSessionStartEvent): void` in `Terminal.tsx`.

- [ ] **Step 1: Implement `startAgentSession`.** Same as `startRoutineSession` minus `setRoutineOrigin`: `setLaunchAgent(sessionId, e.agent)`, `getOrCreateTerm(sessionId, getRoutinePane(), …)`, `fitToHost`, `attachPty(entry, sessionId, e.projectId, e.cwd, e.prompt)`. Passing the prompt as the parameter keeps it headless (`openTab: false`), which is the "no focus stolen" behaviour.
- [ ] **Step 2: Hook.** `useAgentSessionStarts()`: on event → `cmd.startSession(e.projectId, e.agent, true)` → `startAgentSession(id, e)` → invalidate `queryKeys.sessions(e.projectId)`; `.catch(e => console.error('agent startSession failed', e))`. No drain (see Review Focus).
- [ ] **Step 3: Mount in `__root.tsx`.**
- [ ] **Step 4: Verify.** `pnpm typecheck && pnpm lint && pnpm test` → all pass.
- [ ] **Step 5: Commit** `feat: an agent's startSession opens a session in the sidebar without taking focus`

### Task 3: Spec, ADR, gate, real window

**Files:**
- Modify: `specs/05-features.md` (new `## F50 — An agent hands work off to a new session` after F49, or next free number)
- Create: `specs/0113-an-agent-may-open-a-session-after-asking.md` (or next free number), following `specs/0112-*.md` structure: decision = approach A, consent via chat + CLI MCP permission; ceilings = "always allow", Codex approval unverified; no drain because only a live session emits.

- [ ] **Step 1: Write spec entry and ADR** (content from the design doc).
- [ ] **Step 2: Full gate** per `quality-gate` skill: `pnpm install && pnpm format && pnpm lint && pnpm typecheck && pnpm test && pnpm e2e && pnpm deps` and `cargo fmt --check && cargo clippy -- -D warnings && cargo test` in `apps/desktop/src-tauri` → all green.
- [ ] **Step 3: Real window** (`manual-qa` skill): start a Claude session, ask it to hand off "list the files in specs/", approve the MCP prompt, confirm a new session row appears in the sidebar, the current tab keeps focus, and opening it shows the prompt as first message.
- [ ] **Step 4: Commit** `docs: spec F50 and ADR for agent-started sessions`
