-- Every decision about an agent command on a server (F46, ADR-0110): allowed,
-- confirmed, edited or refused. Local only; never sent anywhere.
CREATE TABLE agent_audit (
	id          INTEGER PRIMARY KEY,
	at          INTEGER NOT NULL,
	server_id   TEXT NOT NULL,
	session_id  TEXT NOT NULL,
	terminal_id TEXT NOT NULL,
	command     TEXT NOT NULL,
	decision    TEXT NOT NULL
);
CREATE INDEX agent_audit_server ON agent_audit (server_id, at);
