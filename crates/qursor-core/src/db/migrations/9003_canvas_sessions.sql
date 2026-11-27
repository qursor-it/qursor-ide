-- Canvas mode (F44, ADR-0103): which Claude session a canvas talks to.
--
-- Keyed by the canvas's path relative to the project root, so renaming the
-- project folder keeps the link. Hangs off `projects` with ON DELETE CASCADE.
-- **No foreign key to `sessions`, deliberately**, for the reason 0007 gives: the
-- indexer reaps a session row when its transcript goes, and the reader
-- (`services::canvas::get_session`) treats a link to a missing session as none.
CREATE TABLE canvas_sessions (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  rel_path   TEXT NOT NULL,
  session_id TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (project_id, rel_path)
);
