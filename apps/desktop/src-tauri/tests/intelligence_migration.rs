//! Migration 9002 (Project Intelligence, F33, ADR-0084) against a database that
//! already has data in it, and the properties its schema exists to guarantee.
//!
//! The migration only adds tables, so there is no rebuild to get wrong. What is
//! worth asserting is what the schema promises the code that sits on it:
//! upgrading keeps what was there, removing a project removes everything derived
//! from it and nothing else, evidence outlives the session it points at (and so
//! can be blanked by retention rather than cascaded away), and the constraints
//! turn a bad value away instead of storing it.

use std::path::Path;

use factorai_lib::commands::projects::add_project_in;
use factorai_lib::db::Db;
use rusqlite::{params, Connection};
use tempfile::TempDir;

const TABLES: [&str; 6] = [
	"intelligence_runs",
	"improvement_candidates",
	"improvement_evidence",
	"intelligence_run_evidence",
	"improvement_candidate_evidence",
	"improvement_events",
];

fn project(db: &Db, root: &Path, name: &str) -> String {
	let dir = root.join(name);
	std::fs::create_dir_all(&dir).unwrap();
	add_project_in(db, &dir.to_string_lossy()).expect("add project").id
}

fn count(conn: &Connection, table: &str) -> i64 {
	conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
}

fn seed_run(conn: &Connection, id: &str, project_id: &str) {
	conn.execute(
		"INSERT INTO intelligence_runs(id, project_id, state, started_at) VALUES (?1, ?2, 'running', 1)",
		params![id, project_id],
	)
	.unwrap();
}

fn seed_candidate(
	conn: &Connection,
	id: &str,
	project_id: &str,
	key: &str,
) -> rusqlite::Result<usize> {
	conn.execute(
		"INSERT INTO improvement_candidates(id, project_id, dedupe_key, problem_kind, action_kind,
		   title, problem, recommendation, rationale, created_at, updated_at)
		 VALUES (?1, ?2, ?3, 'project_convention', 'instruction', 't', 'p', 'r', 'why', 1, 1)",
		params![id, project_id, key],
	)
}

fn seed_evidence(conn: &Connection, id: &str, project_id: &str, session: &str) {
	conn.execute(
		"INSERT INTO improvement_evidence(id, project_id, session_id, source_agent, signal_type,
		   locator_json, excerpt, context, fingerprint, observed_at)
		 VALUES (?1, ?2, ?3, 'claude', 'user_correction', '{\"line\":1}', 'excerpt', 'ctx', ?1, 1)",
		params![id, project_id, session],
	)
	.unwrap();
}

/// Everything one project can own, linked together.
fn seed_everything(conn: &Connection, project_id: &str, tag: &str) {
	let run = format!("run-{tag}");
	let cand = format!("cand-{tag}");
	let ev = format!("ev-{tag}");
	seed_run(conn, &run, project_id);
	seed_candidate(conn, &cand, project_id, &format!("key-{tag}")).unwrap();
	seed_evidence(conn, &ev, project_id, &format!("session-{tag}"));
	conn.execute(
		"INSERT INTO intelligence_run_evidence(run_id, evidence_id) VALUES (?1, ?2)",
		params![run, ev],
	)
	.unwrap();
	conn.execute(
		"INSERT INTO improvement_candidate_evidence(candidate_id, evidence_id) VALUES (?1, ?2)",
		params![cand, ev],
	)
	.unwrap();
	conn.execute(
		"INSERT INTO improvement_events(candidate_id, event_type, created_at) VALUES (?1, 'proposed', 1)",
		params![cand],
	)
	.unwrap();
}

#[test]
fn upgrading_a_database_from_before_the_migration_keeps_its_data_and_adds_the_tables() {
	let tmp = TempDir::new().unwrap();
	let data = tmp.path().join("data");
	let project_id = {
		let db = Db::open(&data).unwrap();
		let id = project(&db, tmp.path(), "shop");
		// Put the file back to the 0022 mark: the six tables gone, the ledger
		// forgetting 9002. This is the file an install has before it updates.
		db.with(|conn| {
			for table in TABLES.iter().rev() {
				conn.execute_batch(&format!("DROP TABLE {table};"))?;
			}
			conn.execute(
				"DELETE FROM _meta WHERE key = 'migration:9002_project_intelligence'",
				[],
			)?;
			Ok(())
		})
		.unwrap();
		id
	};

	let db = Db::open(&data).expect("the upgrade applies");
	db.with(|conn| {
		for table in TABLES {
			assert_eq!(count(conn, table), 0, "{table} should exist and be empty");
		}
		let name: String = conn
			.query_row("SELECT display_name FROM projects WHERE id = ?1", [&project_id], |r| {
				r.get(0)
			})
			.unwrap();
		assert_eq!(name, "shop", "the upgrade must leave existing rows alone");
		let applied: i64 = conn
			.query_row(
				"SELECT COUNT(*) FROM _meta WHERE key = 'migration:9002_project_intelligence'",
				[],
				|r| r.get(0),
			)
			.unwrap();
		assert_eq!(applied, 1);
		Ok(())
	})
	.unwrap();

	// And opening again is a no-op, not a second CREATE TABLE.
	drop(db);
	Db::open(&data).expect("reopening does not re-apply");
}

#[test]
fn removing_a_project_removes_what_was_derived_from_it_and_nothing_else() {
	let tmp = TempDir::new().unwrap();
	let db = Db::open(&tmp.path().join("data")).unwrap();
	let a = project(&db, tmp.path(), "a");
	let b = project(&db, tmp.path(), "b");
	db.with(|conn| {
		seed_everything(conn, &a, "a");
		seed_everything(conn, &b, "b");
		conn.execute("DELETE FROM projects WHERE id = ?1", [&a])?;
		for table in TABLES {
			assert_eq!(count(conn, table), 1, "{table}: only project b's row should remain");
		}
		let left: String =
			conn.query_row("SELECT project_id FROM improvement_candidates", [], |r| r.get(0))?;
		assert_eq!(left, b);
		Ok(())
	})
	.unwrap();
}

#[test]
fn evidence_does_not_reference_sessions_so_a_vanished_session_cannot_cascade_it_away() {
	let tmp = TempDir::new().unwrap();
	let db = Db::open(&tmp.path().join("data")).unwrap();
	let p = project(&db, tmp.path(), "a");
	db.with(|conn| {
		// The session id names nothing in `sessions`; the insert must still land.
		seed_evidence(conn, "ev-1", &p, "a-session-the-indexer-never-saw");
		assert_eq!(count(conn, "improvement_evidence"), 1);
		let fks: Vec<String> = conn
			.prepare("SELECT \"table\" FROM pragma_foreign_key_list('improvement_evidence')")?
			.query_map([], |r| r.get(0))?
			.collect::<Result<_, _>>()?;
		assert_eq!(fks, vec!["projects".to_string()], "evidence may reference projects only");
		Ok(())
	})
	.unwrap();
}

#[test]
fn the_constraints_turn_a_bad_value_away() {
	let tmp = TempDir::new().unwrap();
	let db = Db::open(&tmp.path().join("data")).unwrap();
	let p = project(&db, tmp.path(), "a");
	db.with(|conn| {
		assert!(conn
			.execute(
				"INSERT INTO intelligence_runs(id, project_id, state, started_at) VALUES ('r', ?1, 'half-done', 1)",
				[&p]
			)
			.is_err());
		assert!(conn
			.execute(
				"INSERT INTO intelligence_runs(id, project_id, state, outcome, started_at) VALUES ('r', ?1, 'completed', 'great', 1)",
				[&p]
			)
			.is_err());
		seed_candidate(conn, "c1", &p, "k1").unwrap();
		for (column, bad) in [("status", "verified"), ("action_kind", "auto_fix"), ("problem_kind", "bug")] {
			let sql = format!("UPDATE improvement_candidates SET {column} = ?1 WHERE id = 'c1'");
			assert!(conn.execute(&sql, [bad]).is_err(), "{column} = {bad} was accepted");
		}
		assert!(conn
			.execute(
				"INSERT INTO improvement_events(candidate_id, event_type, created_at) VALUES ('c1', 'auto_applied', 1)",
				[]
			)
			.is_err());
		assert!(conn
			.execute(
				"INSERT INTO improvement_evidence(id, project_id, session_id, source_agent, signal_type,
				   locator_json, fingerprint, observed_at) VALUES ('e', ?1, 's', 'gemini', 'user_correction', '{}', 'f', 1)",
				[&p]
			)
			.is_err());
		Ok(())
	})
	.unwrap();
}

#[test]
fn a_dedupe_key_is_unique_within_a_project_and_free_across_projects() {
	let tmp = TempDir::new().unwrap();
	let db = Db::open(&tmp.path().join("data")).unwrap();
	let a = project(&db, tmp.path(), "a");
	let b = project(&db, tmp.path(), "b");
	db.with(|conn| {
		seed_candidate(conn, "c1", &a, "same-key").unwrap();
		assert!(seed_candidate(conn, "c2", &a, "same-key").is_err());
		seed_candidate(conn, "c3", &b, "same-key").expect("another project may reuse a key");
		Ok(())
	})
	.unwrap();
}

#[test]
fn the_timeline_is_ordered_by_id_whatever_the_clock_says() {
	let tmp = TempDir::new().unwrap();
	let db = Db::open(&tmp.path().join("data")).unwrap();
	let p = project(&db, tmp.path(), "a");
	db.with(|conn| {
		seed_candidate(conn, "c1", &p, "k1").unwrap();
		// Written out of clock order on purpose.
		for (kind, at) in [("proposed", 500), ("implemented", 100), ("verification", 900)] {
			conn.execute(
				"INSERT INTO improvement_events(candidate_id, event_type, created_at) VALUES ('c1', ?1, ?2)",
				params![kind, at],
			)?;
		}
		let order: Vec<String> = conn
			.prepare(
				"SELECT event_type FROM improvement_events WHERE candidate_id = 'c1' ORDER BY id",
			)?
			.query_map([], |r| r.get(0))?
			.collect::<Result<_, _>>()?;
		assert_eq!(order, ["proposed", "implemented", "verification"]);
		let ids: Vec<i64> = conn
			.prepare("SELECT id FROM improvement_events ORDER BY id")?
			.query_map([], |r| r.get(0))?
			.collect::<Result<_, _>>()?;
		assert!(ids.windows(2).all(|w| w[0] < w[1]));
		Ok(())
	})
	.unwrap();
}

#[test]
fn the_setting_is_off_until_it_is_written_as_on() {
	use factorai_lib::models::SettingKey;
	use factorai_lib::services::settings;
	let tmp = TempDir::new().unwrap();
	let db = Db::open(&tmp.path().join("data")).unwrap();
	db.with(|conn| {
		assert_eq!(settings::get(conn, SettingKey::ProjectIntelligence)?, None);
		settings::set(conn, SettingKey::ProjectIntelligence, Some("on"))?;
		let row: String = conn.query_row(
			"SELECT value FROM settings WHERE key = 'intelligence.enabled'",
			[],
			|r| r.get(0),
		)?;
		assert_eq!(row, "on");
		Ok(())
	})
	.unwrap();
}
