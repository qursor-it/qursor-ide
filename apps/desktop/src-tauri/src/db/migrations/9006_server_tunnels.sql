-- Saved local port forwards per server (F48). Started and stopped by hand;
-- a running tunnel is runtime state, not stored.
CREATE TABLE server_tunnels (
	id          TEXT PRIMARY KEY,
	server_id   TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
	label       TEXT NOT NULL DEFAULT '',
	local_port  INTEGER NOT NULL CHECK (local_port BETWEEN 1 AND 65535),
	dest_host   TEXT NOT NULL,
	dest_port   INTEGER NOT NULL CHECK (dest_port BETWEEN 1 AND 65535)
);
CREATE INDEX server_tunnels_server ON server_tunnels (server_id);
