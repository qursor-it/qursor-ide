-- Saved SSH servers and the folders of the address book (F44, ADR-0103).
-- No secret is stored here: passwords, keys and passphrases live in the vault
-- (ADR-0104) under `server:<id>:<kind>`.
CREATE TABLE server_folders (
	id         TEXT PRIMARY KEY,
	name       TEXT NOT NULL,
	sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE servers (
	id           TEXT PRIMARY KEY,
	name         TEXT NOT NULL,
	host         TEXT NOT NULL,
	port         INTEGER NOT NULL DEFAULT 22 CHECK (port BETWEEN 1 AND 65535),
	username     TEXT NOT NULL,
	start_dir    TEXT,
	folder_id    TEXT REFERENCES server_folders(id) ON DELETE SET NULL,
	environment  TEXT NOT NULL DEFAULT 'dev' CHECK (environment IN ('dev', 'staging', 'prod')),
	agent_policy TEXT NOT NULL DEFAULT 'free' CHECK (agent_policy IN ('free', 'confirm', 'readonly')),
	auth         TEXT NOT NULL CHECK (auth IN ('password', 'key', 'keyFile', 'agent', 'system')),
	key_path     TEXT,
	sort_order   INTEGER NOT NULL DEFAULT 0,
	created_at   INTEGER NOT NULL
);
