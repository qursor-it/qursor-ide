-- Sessions a delegation started (F50, ADR-0126). Delegates are disposable: their
-- transcripts stay where their agent wrote them (ADR-0004), and these ids keep
-- them out of the session lists, search and counts for good. No foreign key to
-- `sessions`: the id is recorded at spawn, before any transcript exists (0007).
CREATE TABLE delegate_sessions (
	session_id TEXT PRIMARY KEY
);
