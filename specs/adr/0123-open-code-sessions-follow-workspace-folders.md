# ADR-0123 — OpenCode sessions follow workspace folders

## Context

OpenCode records the session's working directory. A session in a project
subdirectory persists in OpenCode, but exact-path reconciliation can leave
it disconnected from the open workspace.

## Decision

For OpenCode only, after exact project and checkout matching, walk the
recorded directory's ancestors and take the first open project or linked
checkout. At the same path, the explicitly opened project takes precedence.
Qursor keeps the provider's working directory for resume. OpenCode's default
store is also available through the existing project import picker.

## Consequences

Existing saved sessions become visible on the next scan. An explicitly
opened nested project keeps its own sessions. Users can import folders
outside the workspace. Other providers keep their existing matching rules
from ADR-0011 and ADR-0019.
