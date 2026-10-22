-- Project Intelligence (F33, ADR-0084): runs, candidates, the evidence they
-- cite, and a timeline of what the person decided.
--
-- **Everything here hangs off `projects` with ON DELETE CASCADE.** Removing a
-- project from the workspace removes what was derived from it; nothing outlives
-- the folder that justified it, and nothing is shared between projects.
--
-- **There is no foreign key to `sessions`, deliberately** (0007, 0013): the
-- indexer reaps a row when its transcript goes, a person can delete a session,
-- and a profile removal cascades through `discovered_projects`. Evidence is a
-- *record that something was observed*, so it must be able to outlive the row
-- it points at. What may not outlive it is the excerpt: `services::intelligence::
-- store::prune_unavailable_evidence` blanks `excerpt` and `context` for any
-- session no longer in the index, on every read and every run.
--
-- **Nothing here holds a transcript or a file.** An excerpt is at most a few
-- hundred redacted characters; the delta needed to revert an instruction change
-- is the one appended block, in `improvement_events.details_json`, with the
-- hashes that prove the file is still what we left.

CREATE TABLE intelligence_runs (
	-- uuid v4.
	id              TEXT PRIMARY KEY,
	project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
	state           TEXT NOT NULL
	                CHECK (state IN ('running', 'completed', 'failed', 'cancelled')),
	-- Meaningful only once `state` is 'completed'. `no_change` is a legitimate
	-- result and is never represented by a made-up candidate.
	outcome         TEXT
	                CHECK (outcome IS NULL OR outcome IN ('candidates', 'no_change', 'signals_only')),
	-- NULL for a signals-only run: no model looked at anything.
	analysis_provider TEXT,
	session_count   INTEGER NOT NULL DEFAULT 0,
	signal_count    INTEGER NOT NULL DEFAULT 0,
	candidate_count INTEGER NOT NULL DEFAULT 0,
	-- Candidates the model proposed that validation rejected. Counted, never kept.
	dropped_count   INTEGER NOT NULL DEFAULT 0,
	payload_bytes   INTEGER NOT NULL DEFAULT 0,
	started_at      INTEGER NOT NULL,
	ended_at        INTEGER,
	-- A short code (`timeout`, `invalid_output`, `cancelled`, `interrupted`, ...),
	-- or `provider_unavailable` on a completed run that fell back to signals only.
	error_code      TEXT,
	-- Short, and never the payload or the model's output.
	error_message   TEXT
);

CREATE INDEX idx_intelligence_runs_project
	ON intelligence_runs(project_id, started_at DESC);

CREATE TABLE improvement_candidates (
	id              TEXT PRIMARY KEY,
	project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
	latest_run_id   TEXT REFERENCES intelligence_runs(id) ON DELETE SET NULL,
	-- sha256 of project + problem kind + action + the normalised recommendation
	-- tokens. Re-running on the same events lands on the same key; a reworded
	-- model answer is caught by the conservative evidence-overlap merge instead.
	dedupe_key      TEXT NOT NULL,
	problem_kind    TEXT NOT NULL
	                CHECK (problem_kind IN ('project_convention', 'recurring_correction',
	                                        'verification_gap', 'workflow_hint')),
	action_kind     TEXT NOT NULL
	                CHECK (action_kind IN ('instruction', 'test_suggestion',
	                                       'script_suggestion', 'manual_followup')),
	title           TEXT NOT NULL,
	problem         TEXT NOT NULL,
	recommendation  TEXT NOT NULL,
	rationale       TEXT NOT NULL,
	limitations     TEXT,
	-- `dismissed` and `reverted` are terminal. Verification is an event, not a
	-- status: an implemented change stays `implemented` whatever was verified.
	status          TEXT NOT NULL DEFAULT 'proposed'
	                CHECK (status IN ('proposed', 'dismissed', 'implemented', 'retained', 'reverted')),
	-- Set when the backend weakened the model's claim (a "recurring" correction
	-- seen in one session). Kept so the reason is shown, not guessed.
	downgrade_note  TEXT,
	-- The person asked to come back to this one (test and script suggestions are
	-- consult-only, so this is their whole workflow).
	follow_up       INTEGER NOT NULL DEFAULT 0 CHECK (follow_up IN (0, 1)),
	-- {path, line, text} of an instruction already covering this, found at run
	-- time. Rechecked against the file when a change is prepared.
	duplicate_json  TEXT,
	-- The change last prepared and not yet recorded: target, hashes, and the one
	-- block it adds or removes. A delta, never a file.
	prepared_json   TEXT,
	created_at      INTEGER NOT NULL,
	updated_at      INTEGER NOT NULL,
	UNIQUE (project_id, dedupe_key)
);

CREATE INDEX idx_improvement_candidates_project_status
	ON improvement_candidates(project_id, status, updated_at DESC);

-- What a run observed. The id is derived from (project, session, fingerprint),
-- so the same event gets the same id every time it is extracted — a re-run adds
-- no rows, and an id the person excluded in the preview means the same thing
-- when the run starts.
CREATE TABLE improvement_evidence (
	id              TEXT PRIMARY KEY,
	project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
	-- The agent's session id. No foreign key: see the header.
	session_id      TEXT NOT NULL,
	source_agent    TEXT NOT NULL CHECK (source_agent IN ('claude', 'codex')),
	signal_type     TEXT NOT NULL
	                CHECK (signal_type IN ('user_correction', 'tool_failure',
	                                       'retry_after_failure', 'repeated_instruction')),
	-- {line, ordinal, eventId}
	locator_json    TEXT NOT NULL,
	-- Redacted, at most 280 characters. NULL once the session is gone.
	excerpt         TEXT,
	context         TEXT,
	-- sha256 of the normalised source event.
	fingerprint     TEXT NOT NULL,
	observed_at     INTEGER NOT NULL,
	UNIQUE (project_id, session_id, fingerprint)
);

CREATE INDEX idx_improvement_evidence_session ON improvement_evidence(session_id);

-- Which evidence a run produced. A model may cite only ids in its own run's set;
-- this is the table that says what that set was.
CREATE TABLE intelligence_run_evidence (
	run_id          TEXT NOT NULL REFERENCES intelligence_runs(id) ON DELETE CASCADE,
	evidence_id     TEXT NOT NULL REFERENCES improvement_evidence(id) ON DELETE CASCADE,
	PRIMARY KEY (run_id, evidence_id)
);

-- Which evidence supports a candidate. The independent-session count is a
-- `COUNT(DISTINCT session_id)` over this, computed by the backend and never
-- taken from the model.
CREATE TABLE improvement_candidate_evidence (
	candidate_id    TEXT NOT NULL REFERENCES improvement_candidates(id) ON DELETE CASCADE,
	evidence_id     TEXT NOT NULL REFERENCES improvement_evidence(id) ON DELETE CASCADE,
	PRIMARY KEY (candidate_id, evidence_id)
);

CREATE INDEX idx_improvement_candidate_evidence_evidence
	ON improvement_candidate_evidence(evidence_id);

-- The timeline. INTEGER AUTOINCREMENT, so the order things happened in is the
-- order of `id` whatever the clock did.
CREATE TABLE improvement_events (
	id              INTEGER PRIMARY KEY AUTOINCREMENT,
	candidate_id    TEXT NOT NULL REFERENCES improvement_candidates(id) ON DELETE CASCADE,
	event_type      TEXT NOT NULL
	                CHECK (event_type IN ('proposed', 'evidence_added', 'dismissed', 'follow_up',
	                                      'implemented', 'verification', 'retained', 'reverted')),
	-- `passed` / `failed` / `inconclusive` for a verification. Entered by a
	-- person; nothing a model said ever lands here.
	outcome         TEXT,
	method          TEXT,
	-- {note, appendedBlock, createdFile, baseHash}: the minimum needed to show a
	-- reason or to build the inverse of an instruction change. No file contents.
	details_json    TEXT,
	artifact_path   TEXT,
	before_hash     TEXT,
	after_hash      TEXT,
	created_at      INTEGER NOT NULL
);

CREATE INDEX idx_improvement_events_candidate
	ON improvement_events(candidate_id, id);
