//! The agent's own plan for a session (F40): the todo list Claude Code keeps
//! with its `TodoWrite` tool, read from the transcript.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::models::PlanItem;

/// The last `TodoWrite` call's list, or `None` when the session never wrote
/// one (or the file cannot be read). Lines without the tool's name are skipped
/// before any JSON parse, so a long transcript costs one byte scan.
pub fn latest_plan(path: &Path) -> Option<Vec<PlanItem>> {
	let reader = BufReader::new(File::open(path).ok()?);
	let mut latest = None;
	for line in reader.split(b'\n').map_while(Result::ok) {
		if !line.windows(11).any(|w| w == b"\"TodoWrite\"") {
			continue;
		}
		let Ok(entry) = serde_json::from_slice::<Value>(&line) else { continue };
		if let Some(items) = todos_in(&entry) {
			latest = Some(items);
		}
	}
	latest
}

fn todos_in(entry: &Value) -> Option<Vec<PlanItem>> {
	let content = entry.pointer("/message/content")?.as_array()?;
	let call = content.iter().rev().find(|c| {
		c.get("type").and_then(Value::as_str) == Some("tool_use")
			&& c.get("name").and_then(Value::as_str) == Some("TodoWrite")
	})?;
	let todos = call.pointer("/input/todos")?.as_array()?;
	Some(
		todos
			.iter()
			.filter_map(|t| {
				Some(PlanItem {
					content: t.get("content")?.as_str()?.to_string(),
					status: t
						.get("status")
						.and_then(Value::as_str)
						.unwrap_or("pending")
						.to_string(),
				})
			})
			.collect(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn todo_line(items: &[(&str, &str)]) -> String {
		let todos: Vec<Value> = items
			.iter()
			.map(|(c, s)| serde_json::json!({ "content": c, "status": s, "activeForm": c }))
			.collect();
		serde_json::json!({
			"type": "assistant",
			"message": { "content": [
				{ "type": "text", "text": "Planning" },
				{ "type": "tool_use", "name": "TodoWrite", "input": { "todos": todos } }
			]}
		})
		.to_string()
	}

	#[test]
	fn the_last_todo_write_wins() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("s.jsonl");
		let text = [
			todo_line(&[("Find the bug", "in_progress"), ("Fix it", "pending")]),
			r#"{"type":"user","message":{"content":"ok"}}"#.to_string(),
			todo_line(&[("Find the bug", "completed"), ("Fix it", "in_progress")]),
		]
		.join("\n");
		std::fs::write(&path, text).unwrap();
		let plan = latest_plan(&path).unwrap();
		assert_eq!(
			plan,
			vec![
				PlanItem { content: "Find the bug".into(), status: "completed".into() },
				PlanItem { content: "Fix it".into(), status: "in_progress".into() },
			]
		);
	}

	#[test]
	fn no_todo_write_is_no_plan() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("s.jsonl");
		std::fs::write(&path, r#"{"type":"assistant","message":{"content":[{"type":"text","text":"TodoWrite is a word here"}]}}"#).unwrap();
		assert_eq!(latest_plan(&path), None);
		assert_eq!(latest_plan(&dir.path().join("missing.jsonl")), None);
	}
}
