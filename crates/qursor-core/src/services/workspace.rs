//! The workspace as rows: the projects you added and the sidebar's tree of them.
//! The pure reads behind the app's `commands::projects` and `commands::sidebar`,
//! here because resolving a server from a project file needs them in the core
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §5a).

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::error::{AppError, AppResult};
use crate::models::{Project, SidebarChild, SidebarRow};
use crate::services::wsl;

/// Columns and aggregates for one workspace row, shared by every query that
/// returns a [`Project`] so the shape can't drift between them.
pub const PROJECT_SELECT: &str = "SELECT p.id, p.real_path, p.display_name, p.missing,
	-- Sub-agents don't count: the number answers how many sessions the project
	-- has, and an agent run on a session's behalf is part of that session
	-- rather than another one. Last activity is not filtered the same way --
	-- an agent working is the project being worked on.
	(SELECT COUNT(*) FROM sessions s
	   JOIN discovered_projects d ON d.id = s.discovered_id
	  WHERE d.project_id = p.id AND s.subagent_of IS NULL
	    AND s.id NOT IN (SELECT session_id FROM delegate_sessions)),
	(SELECT MAX(s.updated_at) FROM sessions s
	   JOIN discovered_projects d ON d.id = s.discovered_id
	  WHERE d.project_id = p.id
	    AND s.id NOT IN (SELECT session_id FROM delegate_sessions)),
	-- The assignment, and NULL meaning the default (F25 slice 3). A LEFT JOIN
	-- rather than a subquery pair, since both columns come from the one row.
	pr.id, pr.name
	FROM projects p
	LEFT JOIN project_profiles pp ON pp.project_id = p.id AND pp.agent = 'claude'
	LEFT JOIN profiles pr ON pr.id = pp.profile_id";

pub fn map_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
	let real_path: String = row.get(1)?;
	Ok(Project {
		id: row.get(0)?,
		// Computed here rather than stored, so it covers every project that was
		// ever added — including the ones added before this existed, and the
		// ones added on a machine that was not WSL (ADR-0044).
		windows_filesystem: wsl::is_windows_filesystem(Path::new(&real_path)),
		real_path,
		display_name: row.get(2)?,
		missing: row.get::<_, i64>(3)? != 0,
		session_count: row.get(4)?,
		last_session_at: row.get(5)?,
		profile_id: row.get(6)?,
		profile_name: row.get(7)?,
	})
}

/// Every project in the workspace, flat and by name.
///
/// **Flat on purpose, and not the sidebar's query.** The sidebar reads
/// `list_sidebar`, which returns the tree; this one answers "which projects are
/// in the workspace" for the four surfaces that want exactly that — the tab
/// strip, the project route, the import dialog and the search route. Since
/// ADR-0025 the order lives on `sidebar_rows`, so there is nothing here for a
/// caller to sort by except the name, and a stable alphabetical order beats
/// whatever SQLite would otherwise hand back.
pub fn list_projects_in(conn: &Connection) -> AppResult<Vec<Project>> {
	let sql = format!("{PROJECT_SELECT} ORDER BY p.display_name COLLATE NOCASE");
	let mut stmt = conn.prepare(&sql)?;
	let rows = stmt.query_map([], map_project)?.collect::<rusqlite::Result<Vec<_>>>()?;
	Ok(rows)
}

/// The folder a project points at, for the commands that need to touch disk.
pub fn project_path(conn: &Connection, id: &str) -> AppResult<PathBuf> {
	conn.query_row("SELECT real_path FROM projects WHERE id = ?1", params![id], |r| {
		r.get::<_, String>(0)
	})
	.map(PathBuf::from)
	.map_err(|_| AppError::NotFound(format!("project {id}")))
}

/// The tree, already ordered.
///
/// Two queries and an in-memory assembly rather than one recursive CTE: the rows
/// are counted in tens, the project payload is the same `PROJECT_SELECT` every
/// other command uses (so the shape cannot drift), and a CTE that had to carry
/// those aggregate subqueries through a join would be harder to read than this
/// is to write.
///
/// The `display_name` tie-break is doing real work — see this module's header on
/// sparse ordinals.
pub fn list_sidebar_in(conn: &Connection) -> AppResult<Vec<SidebarRow>> {
	let projects = list_projects_in(conn)?;
	let by_id: std::collections::HashMap<&str, &Project> =
		projects.iter().map(|p| (p.id.as_str(), p)).collect();

	let mut stmt = conn.prepare(
		"SELECT r.id, r.kind, r.parent_id, r.name, r.project_id
		   FROM sidebar_rows r
		   LEFT JOIN projects p ON p.id = r.project_id
		  ORDER BY r.sort_order, COALESCE(p.display_name, r.name) COLLATE NOCASE",
	)?;
	let rows: Vec<RawRow> = stmt
		.query_map([], |r| {
			Ok(RawRow {
				id: r.get(0)?,
				kind: r.get(1)?,
				parent_id: r.get(2)?,
				name: r.get(3)?,
				project_id: r.get(4)?,
			})
		})?
		.collect::<rusqlite::Result<_>>()?;
	drop(stmt);

	// Two passes so a child can be attached whatever order the flat result
	// arrived in — the ORDER BY above sorts within each scope, but a group's rows
	// and its children are interleaved in one result set.
	let mut out: Vec<SidebarRow> = Vec::new();
	let mut group_index: std::collections::HashMap<String, usize> =
		std::collections::HashMap::new();
	for row in rows.iter().filter(|r| r.parent_id.is_none()) {
		if row.kind == "group" {
			group_index.insert(row.id.clone(), out.len());
			out.push(SidebarRow::Group {
				row_id: row.id.clone(),
				// NOT NULL for a group row by CHECK; the fallback is for a
				// database somebody edited by hand.
				name: row.name.clone().unwrap_or_default(),
				children: Vec::new(),
			});
			continue;
		}
		// A row whose project is gone should be impossible — ON DELETE CASCADE
		// retires it — so skipping is a guard, not a branch the UI depends on.
		let Some(project) = row.project(&by_id) else { continue };
		out.push(SidebarRow::Project { row_id: row.id.clone(), project });
	}
	for row in &rows {
		let Some(parent) = row.parent_id.as_deref() else { continue };
		let Some(&index) = group_index.get(parent) else { continue };
		let Some(project) = row.project(&by_id) else { continue };
		if let Some(SidebarRow::Group { children, .. }) = out.get_mut(index) {
			children.push(SidebarChild { row_id: row.id.clone(), project });
		}
	}
	Ok(out)
}

/// One `sidebar_rows` row as the database hands it back, before it becomes a
/// [`SidebarRow`]. A named struct rather than a five-wide tuple, which clippy
/// objects to and which nobody could read at the call site anyway.
struct RawRow {
	id: String,
	kind: String,
	parent_id: Option<String>,
	name: Option<String>,
	project_id: Option<String>,
}

impl RawRow {
	fn project(&self, by_id: &std::collections::HashMap<&str, &Project>) -> Option<Project> {
		self.project_id.as_deref().and_then(|pid| by_id.get(pid)).map(|p| (*p).clone())
	}
}

/// Give a project a row at the top of the sidebar, or leave the one it has.
///
/// Called by `add_project`, which is idempotent by path — so this has to be too,
/// or re-adding a folder would give it a second row and `list_sidebar` would
/// draw it twice. The UNIQUE index on `project_id` would catch that; returning
/// early is what stops it becoming an error the user sees for doing something
/// reasonable.
pub fn ensure_project_row(tx: &Transaction<'_>, project_id: &str) -> AppResult<()> {
	let exists: bool = tx
		.query_row("SELECT 1 FROM sidebar_rows WHERE project_id = ?1", params![project_id], |_| {
			Ok(true)
		})
		.optional()?
		.unwrap_or(false);
	if exists {
		return Ok(());
	}
	tx.execute(
		"INSERT INTO sidebar_rows(id, kind, parent_id, sort_order, project_id, name)
		 VALUES(?1, 'project', NULL,
		        (SELECT COALESCE(MIN(sort_order), 0) - 1
		           FROM sidebar_rows WHERE parent_id IS NULL),
		        ?2, NULL)",
		params![uuid::Uuid::new_v4().to_string(), project_id],
	)?;
	Ok(())
}
