#![cfg(windows)]

use std::path::Path;
use std::sync::Arc;

use factorai_lib::agents::CODEX;
use factorai_lib::commands::projects::{add_project_in, import_candidates, list_projects_in};
use factorai_lib::db::Db;
use factorai_lib::services::indexer::Indexer;
use rusqlite::params;
use tempfile::TempDir;

fn write_rollout(home: &Path, id: &str, cwd: &str, minute: u8) {
	let day = home.join("sessions").join("2026").join("10").join("09");
	std::fs::create_dir_all(&day).unwrap();
	let path = day.join(format!("rollout-2026-10-09T12-{:02}-00-{id}.jsonl", minute));
	let meta = serde_json::json!({
		"timestamp": format!("2026-10-09T12:{minute:02}:00Z"),
		"type": "session_meta",
		"payload": { "id": id, "cwd": cwd },
	});
	let turn = serde_json::json!({
		"timestamp": format!("2026-10-09T12:{minute:02}:01Z"),
		"type": "event_msg",
		"payload": { "type": "task_started" },
	});
	std::fs::write(path, format!("{meta}\n{turn}\n")).unwrap();
}

#[test]
fn codex_case_and_separator_variants_index_to_one_windows_project() {
	let tmp = TempDir::new().unwrap();
	let db = Db::open(&tmp.path().join("data")).unwrap();
	let project_dir = tmp.path().join("work").join("FactorAI Path Identity");
	std::fs::create_dir_all(&project_dir).unwrap();
	let project = add_project_in(&db, project_dir.to_str().unwrap()).unwrap();
	let recorded_cwd = project.real_path.clone();
	let alternate_cwd = recorded_cwd.to_lowercase().replace('\\', "/");
	assert_ne!(recorded_cwd, alternate_cwd);

	let codex_home = tmp.path().join("codex-home");
	std::fs::create_dir_all(&codex_home).unwrap();
	factorai_lib::services::profiles::ensure_default_for(&db, CODEX, &codex_home).unwrap();
	write_rollout(&codex_home, "11111111-1111-4111-8111-111111111111", &recorded_cwd, 0);
	write_rollout(&codex_home, "22222222-2222-4222-8222-222222222222", &alternate_cwd, 1);

	let indexer = Indexer::with_callbacks(db.clone(), Arc::new(|_| {}), Arc::new(|_| {}));
	indexer.full_scan().unwrap();

	let projects = db.with(list_projects_in).unwrap();
	assert_eq!(projects.len(), 1);
	assert_eq!(projects[0].session_count, 2);
	assert_eq!(projects[0].real_path, recorded_cwd);
	db.with(|conn| {
		let linked: i64 = conn.query_row(
			"SELECT COUNT(*) FROM sessions s
			 JOIN discovered_projects d ON d.id = s.discovered_id
			 WHERE d.project_id = ?1",
			params![projects[0].id],
			|row| row.get(0),
		)?;
		assert_eq!(linked, 2);
		Ok(())
	})
	.unwrap();

	let candidates =
		import_candidates(CODEX, &codex_home, std::slice::from_ref(&alternate_cwd)).unwrap();
	assert_eq!(candidates.len(), 1);
	assert_eq!(candidates[0].session_count, 2);
	assert!(candidates[0].already_open);
	assert_eq!(candidates[0].real_path, recorded_cwd);

	std::fs::remove_dir_all(&project_dir).unwrap();
	let missing_variant = format!(r"\\?\{}", alternate_cwd.replace('/', r"\"));
	let readded = add_project_in(&db, &missing_variant).unwrap();
	assert!(readded.missing);
	assert_eq!(readded.id, project.id);
	assert_eq!(readded.real_path, recorded_cwd);
	let projects = db.with(list_projects_in).unwrap();
	assert_eq!(projects.len(), 1, "case/prefix variants must not duplicate a gone folder");
	assert_eq!(projects[0].session_count, 2, "the indexed history remains linked");
}
