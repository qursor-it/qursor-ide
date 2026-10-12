-- Drop junk UNC entries from `sessions.touched_paths` (a JSON array of strings,
-- migration 0010).
--
-- Until parse version 7 a `\\` in a Bash command (a regex, a `sed` script, a
-- JSON-escaped Windows path) was harvested as the start of a UNC path, so rows
-- hold values like `\\s\S`. Canonicalising one on Windows triggers a network
-- name lookup that stalled the UI for seconds. The re-index that version 7
-- forces cannot be relied on to remove them: the upsert keeps the stored value
-- when the new parse finds no paths at all (COALESCE), so they are removed here.
--
-- A real UNC path (`\\server\share`) from a tool field is dropped too; the next
-- re-index puts it back. Only the `\\?\` extended form is kept as is.
UPDATE sessions
SET touched_paths = (
    SELECT json_group_array(value)
    FROM json_each(sessions.touched_paths)
    WHERE NOT (substr(value, 1, 2) = '\\' AND substr(value, 1, 4) <> '\\?\')
)
WHERE touched_paths IS NOT NULL
  AND json_valid(touched_paths)
  AND EXISTS (
    SELECT 1 FROM json_each(sessions.touched_paths)
    WHERE substr(value, 1, 2) = '\\' AND substr(value, 1, 4) <> '\\?\'
  );
