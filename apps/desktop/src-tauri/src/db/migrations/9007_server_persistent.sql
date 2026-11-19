-- A server whose tabs live in tmux sessions, so a dropped connection or a
-- restart does not end the work in them (F48, ADR-0109).
ALTER TABLE servers ADD COLUMN persistent INTEGER NOT NULL DEFAULT 0;
