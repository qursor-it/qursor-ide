//! What a person decides about a candidate, and what the backend will believe
//! about the project's files afterwards.
//!
//! **Nothing here writes a project file.** `prepare_*` reads the target and
//! returns text and a diff. The person reviews it in the viewer and presses Save
//! (ADR-0039). Then `record_*` reads the file again and records the change only
//! if the disk says it happened: the hash is the one the renderer reports it
//! saved, the added line is there (or, for a revert, gone), and the file is
//! still inside the project. Anything else is a typed error and no status moves.
//!
//! ```text
//! proposed --prepare--> (nothing changes) --Save--> record_implementation --> implemented
//! implemented | retained --prepare_revert--> (nothing changes) --Save--> record_revert --> reverted
//! ```
//!
//! `dismissed` and `reverted` are terminal. A verification is an event on an
//! `implemented` or `retained` candidate and never moves its status; it is typed
//! in by a person, because nothing else has seen whether the change helped.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::patch::{self, Block, Plan};
use super::store::{self, CandidateRow, EventFields};
use super::{collect, err, now_ms, sha256_hex, ErrorCode, IntelligenceService};
use crate::error::{AppError, AppResult};
use crate::models::intelligence::{
	ActionKind, CandidateStatus, ChangeConflict, ChangeKind, ChangeStatus, EventType,
	ImprovementCandidate, PreparedChange, VerificationInput,
};

const NOTE_MAX: usize = 1000;
const METHOD_MAX: usize = 200;

/// A note typed by a person: trimmed, bounded, no control characters but newline.
fn note(text: Option<String>, what: &str) -> AppResult<Option<String>> {
	let Some(text) = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) else {
		return Ok(None);
	};
	if text.chars().count() > NOTE_MAX {
		return Err(AppError::InvalidInput(format!("{what} is longer than {NOTE_MAX} characters")));
	}
	Ok(Some(text.chars().filter(|c| !c.is_control() || *c == '\n').collect()))
}

fn is_sha256(hash: &str) -> bool {
	hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
	value.get(key).and_then(Value::as_str)
}

impl IntelligenceService {
	/// The candidate's row and its project's canonical root. A candidate whose
	/// project is gone does not exist (the rows cascade), so this is also the
	/// ownership check.
	fn candidate_and_root(&self, candidate_id: &str) -> AppResult<(CandidateRow, PathBuf)> {
		let (row, root) = self.db().with(|conn| {
			let row = store::load_row(conn, candidate_id)?;
			let (root, _) = collect::project_root(conn, &row.project_id)?;
			Ok((row, root))
		})?;
		Ok((row, collect::canonical_root(&root)?))
	}

	fn after_change(
		&self,
		project_id: &str,
		candidate_id: &str,
	) -> AppResult<ImprovementCandidate> {
		self.changed(project_id, Some(candidate_id));
		self.db().with(|conn| store::get_candidate(conn, candidate_id))
	}

	// ── Deciding ──

	pub fn dismiss(
		&self,
		candidate_id: &str,
		reason: Option<String>,
	) -> AppResult<ImprovementCandidate> {
		self.require_enabled()?;
		let reason = note(reason, "the reason")?;
		let now = now_ms();
		let project_id = self.db().with_mut(|conn| {
			let tx = conn.transaction()?;
			let row = store::load_row(&tx, candidate_id)?;
			store::transition(
				&tx,
				candidate_id,
				&[CandidateStatus::Proposed],
				CandidateStatus::Dismissed,
				now,
			)?;
			store::insert_event(
				&tx,
				candidate_id,
				EventType::Dismissed,
				EventFields {
					details: reason.map(|n| json!({ "note": n })),
					..EventFields::default()
				},
				now,
			)?;
			tx.commit()?;
			Ok(row.project_id)
		})?;
		self.after_change(&project_id, candidate_id)
	}

	/// Flag a candidate to come back to - for a test or script suggestion, which
	/// is consult-only, this is the whole of acting on it.
	pub fn mark_follow_up(
		&self,
		candidate_id: &str,
		follow_up: bool,
		reason: Option<String>,
	) -> AppResult<ImprovementCandidate> {
		self.require_enabled()?;
		let reason = note(reason, "the note")?;
		let now = now_ms();
		let project_id = self.db().with_mut(|conn| {
			let tx = conn.transaction()?;
			let row = store::load_row(&tx, candidate_id)?;
			if matches!(row.status, CandidateStatus::Dismissed | CandidateStatus::Reverted) {
				return Err(err(
					ErrorCode::InvalidState,
					format!("an improvement that is {} cannot be flagged", row.status.as_str()),
				));
			}
			store::set_follow_up(&tx, candidate_id, follow_up, now)?;
			store::insert_event(
				&tx,
				candidate_id,
				EventType::FollowUp,
				EventFields {
					outcome: Some(if follow_up { "on" } else { "off" }.to_string()),
					details: reason.map(|n| json!({ "note": n })),
					..EventFields::default()
				},
				now,
			)?;
			tx.commit()?;
			Ok(row.project_id)
		})?;
		self.after_change(&project_id, candidate_id)
	}

	pub fn mark_retained(
		&self,
		candidate_id: &str,
		reason: Option<String>,
	) -> AppResult<ImprovementCandidate> {
		self.require_enabled()?;
		let reason = note(reason, "the reason")?;
		let now = now_ms();
		let project_id = self.db().with_mut(|conn| {
			let tx = conn.transaction()?;
			let row = store::load_row(&tx, candidate_id)?;
			store::transition(
				&tx,
				candidate_id,
				&[CandidateStatus::Implemented],
				CandidateStatus::Retained,
				now,
			)?;
			store::insert_event(
				&tx,
				candidate_id,
				EventType::Retained,
				EventFields {
					details: reason.map(|n| json!({ "note": n })),
					..EventFields::default()
				},
				now,
			)?;
			tx.commit()?;
			Ok(row.project_id)
		})?;
		self.after_change(&project_id, candidate_id)
	}

	/// Record what a person found when they checked whether the change helped.
	/// The candidate stays `implemented` (or `retained`): this is an event.
	pub fn record_verification(&self, input: VerificationInput) -> AppResult<ImprovementCandidate> {
		self.require_enabled()?;
		let method = input.method.trim().to_string();
		if method.is_empty() || method.chars().count() > METHOD_MAX {
			return Err(AppError::InvalidInput(format!(
				"the method must be 1 to {METHOD_MAX} characters"
			)));
		}
		let method: String = method.chars().filter(|c| !c.is_control()).collect();
		let detail = note(input.note, "the note")?;
		let now = now_ms();
		let candidate_id = input.candidate_id.as_str();
		let project_id = self.db().with_mut(|conn| {
			let tx = conn.transaction()?;
			let row = store::load_row(&tx, candidate_id)?;
			if !matches!(row.status, CandidateStatus::Implemented | CandidateStatus::Retained) {
				return Err(err(
					ErrorCode::InvalidState,
					format!(
						"only a saved change can be verified, and this one is {}",
						row.status.as_str()
					),
				));
			}
			store::insert_event(
				&tx,
				candidate_id,
				EventType::Verification,
				EventFields {
					outcome: Some(input.outcome.as_str().to_string()),
					method: Some(method),
					details: detail.map(|n| json!({ "note": n })),
					..EventFields::default()
				},
				now,
			)?;
			tx.execute(
				"UPDATE improvement_candidates SET updated_at = ?2 WHERE id = ?1",
				rusqlite::params![candidate_id, now],
			)?;
			tx.commit()?;
			Ok(row.project_id)
		})?;
		self.after_change(&project_id, candidate_id)
	}

	// ── Preparing a change ──

	/// The patch that adds this candidate's recommendation to the project's
	/// instruction file. **Writes nothing**: not the file, not a draft, not a
	/// backup. What it does remember, in the database, is the change it proposed
	/// (target, hashes and the one line) so `record_implementation` can check the
	/// Save against it.
	pub fn prepare_instruction_change(
		&self,
		candidate_id: &str,
		allow_duplicate: bool,
	) -> AppResult<PreparedChange> {
		self.require_enabled()?;
		let (row, root) = self.candidate_and_root(candidate_id)?;
		if row.action_kind != ActionKind::Instruction {
			return Err(err(
				ErrorCode::NotInstruction,
				"only an instruction can be prepared as a file change; tests and scripts are suggestions to follow up",
			));
		}
		if row.status != CandidateStatus::Proposed {
			return Err(err(
				ErrorCode::InvalidState,
				format!("an improvement that is {} cannot be prepared", row.status.as_str()),
			));
		}
		let now = now_ms();
		match patch::plan_instruction(&root, &row.recommendation, allow_duplicate)? {
			Plan::Duplicate { target, hint } => {
				self.db().with(|conn| store::set_duplicate(conn, candidate_id, Some(&hint)))?;
				self.changed(&row.project_id, Some(candidate_id));
				Ok(PreparedChange {
					candidate_id: candidate_id.to_string(),
					kind: ChangeKind::Instruction,
					status: ChangeStatus::Duplicate,
					target_path: patch::display_path(&target.path),
					relative_path: target.relative,
					exists: target.exists,
					created_file: false,
					base_hash: None,
					proposed_content: None,
					proposed_hash: None,
					appended_block: None,
					diff: None,
					note: target.note,
					duplicate_of: Some(hint),
					conflict: None,
				})
			}
			Plan::Ready(ready) => {
				let prepared = json!({
					"kind": "instruction",
					"targetPath": ready.target.path.to_string_lossy(),
					"relativePath": ready.target.relative,
					"baseHash": ready.base_hash,
					"proposedHash": ready.proposed_hash,
					"appendedBlock": ready.block.text,
					"appendedLine": ready.block.line,
					"createdFile": !ready.target.exists,
					"preparedAt": now,
				});
				self.db()
					.with(|conn| store::set_prepared(conn, candidate_id, Some(&prepared), now))?;
				Ok(PreparedChange {
					candidate_id: candidate_id.to_string(),
					kind: ChangeKind::Instruction,
					status: ChangeStatus::Ready,
					target_path: patch::display_path(&ready.target.path),
					relative_path: ready.target.relative.clone(),
					exists: ready.target.exists,
					created_file: !ready.target.exists,
					base_hash: ready.base_hash.clone(),
					proposed_content: Some(ready.proposed.clone()),
					proposed_hash: Some(ready.proposed_hash.clone()),
					appended_block: Some(ready.block.text.clone()),
					diff: Some(ready.diff.clone()),
					note: ready.target.note.clone(),
					duplicate_of: ready.duplicate.clone(),
					conflict: None,
				})
			}
		}
	}

	// ── Recording a Save ──

	/// Record that the prepared change was saved. **Reads the disk; writes
	/// nothing to the project.** `expected_hash` is the sha256 of what the
	/// renderer saved - `proposedHash` when the person saved the proposal
	/// unchanged.
	pub fn record_implementation(
		&self,
		candidate_id: &str,
		target_path: &str,
		expected_hash: &str,
	) -> AppResult<ImprovementCandidate> {
		self.require_enabled()?;
		if !is_sha256(expected_hash) {
			return Err(AppError::InvalidInput(
				"expectedHash must be a lowercase sha256 hex".into(),
			));
		}
		let (row, root) = self.candidate_and_root(candidate_id)?;
		if row.status != CandidateStatus::Proposed {
			return Err(err(
				ErrorCode::InvalidState,
				format!(
					"an improvement that is {} cannot be recorded as saved",
					row.status.as_str()
				),
			));
		}
		let prepared =
			row.prepared.filter(|p| text(p, "kind") == Some("instruction")).ok_or_else(|| {
				err(ErrorCode::NotPrepared, "prepare the change before recording that it was saved")
			})?;
		let stored = PathBuf::from(text(&prepared, "targetPath").unwrap_or_default());
		let line = text(&prepared, "appendedLine").unwrap_or_default().to_string();
		let base_hash = text(&prepared, "baseHash").map(str::to_owned);
		let relative = text(&prepared, "relativePath").unwrap_or_default().to_string();

		let (disk_hash, content) = self.read_saved_target(&root, &stored, target_path)?;
		// The file is exactly what it was before: nothing was saved.
		if base_hash.as_deref() == Some(disk_hash.as_str()) {
			return Err(err(
				ErrorCode::NotSaved,
				format!("{relative} has not changed on disk; save the change first"),
			));
		}
		if disk_hash != expected_hash {
			return Err(err(
				ErrorCode::FileConflict,
				format!("{relative} on disk is not the content that was saved; it changed since. Prepare the change again"),
			));
		}
		if !content.contains(&line) {
			return Err(err(
				ErrorCode::NotSaved,
				format!("{relative} does not contain the prepared line; the saved text differs from the proposal"),
			));
		}

		let now = now_ms();
		self.db().with_mut(|conn| {
			let tx = conn.transaction()?;
			store::transition(
				&tx,
				candidate_id,
				&[CandidateStatus::Proposed],
				CandidateStatus::Implemented,
				now,
			)?;
			store::insert_event(
				&tx,
				candidate_id,
				EventType::Implemented,
				EventFields {
					artifact_path: Some(relative.clone()),
					before_hash: base_hash.clone(),
					after_hash: Some(disk_hash.clone()),
					details: Some(json!({
						"targetPath": stored.to_string_lossy(),
						"relativePath": relative,
						"createdFile": prepared.get("createdFile").and_then(Value::as_bool).unwrap_or(false),
						"appendedBlock": text(&prepared, "appendedBlock"),
						"appendedLine": line,
						"proposedHash": text(&prepared, "proposedHash"),
					})),
					..EventFields::default()
				},
				now,
			)?;
			tx.commit()?;
			Ok(())
		})?;
		self.after_change(&row.project_id, candidate_id)
	}

	/// Read the prepared target as it is now, after checking that `provided` names
	/// it and that it is still a file inside the project. Returns its sha256 and
	/// text.
	fn read_saved_target(
		&self,
		root: &Path,
		stored: &Path,
		provided: &str,
	) -> AppResult<(String, String)> {
		let provided = Path::new(provided);
		if !provided.is_absolute() {
			return Err(AppError::InvalidInput(format!(
				"not an absolute path: {}",
				provided.display()
			)));
		}
		if !stored.exists() {
			return Err(err(
				ErrorCode::NotSaved,
				format!("{} does not exist yet; save the change first", stored.display()),
			));
		}
		if !patch::same_file(provided, stored) {
			return Err(err(
				ErrorCode::UnsafeTarget,
				"targetPath is not the file this change was prepared for",
			));
		}
		let canonical = std::fs::canonicalize(stored)?;
		if !canonical.starts_with(root) {
			return Err(err(ErrorCode::UnsafeTarget, "the file now resolves outside the project"));
		}
		let bytes = patch::read_bytes_bounded(&canonical)?;
		let hash = sha256_hex(&bytes);
		let content = String::from_utf8(bytes)
			.map_err(|_| err(ErrorCode::FileConflict, "the file on disk is not UTF-8 text"))?;
		Ok((hash, content))
	}

	// ── Reverting ──

	/// The patch that takes the saved instruction out again - when the file is
	/// still exactly as the Save left it. If it is not, there is no patch: the
	/// answer says so and says how to do it by hand. **Writes nothing.**
	pub fn prepare_revert(&self, candidate_id: &str) -> AppResult<PreparedChange> {
		self.require_enabled()?;
		let (row, root) = self.candidate_and_root(candidate_id)?;
		let implemented = self.reverting_record(&row)?;
		let stored = PathBuf::from(text(&implemented.details, "targetPath").unwrap_or_default());
		let relative = text(&implemented.details, "relativePath").unwrap_or_default().to_string();
		let block = Block {
			text: text(&implemented.details, "appendedBlock").unwrap_or_default().to_string(),
			line: text(&implemented.details, "appendedLine").unwrap_or_default().to_string(),
		};
		let recorded = implemented.after_hash.clone();
		let created_file =
			implemented.details.get("createdFile").and_then(Value::as_bool).unwrap_or(false);

		let conflict = |reason: &str, current: Option<String>| PreparedChange {
			candidate_id: candidate_id.to_string(),
			kind: ChangeKind::Revert,
			status: ChangeStatus::Conflict,
			target_path: patch::display_path(&stored),
			relative_path: relative.clone(),
			exists: stored.exists(),
			created_file,
			base_hash: current.clone(),
			proposed_content: None,
			proposed_hash: None,
			appended_block: Some(block.text.clone()),
			diff: None,
			note: None,
			duplicate_of: None,
			conflict: Some(ChangeConflict {
				reason: reason.to_string(),
				current_hash: current,
				recorded_hash: recorded.clone(),
				manual_instructions: format!(
					"Open {relative}, delete this line yourself, save, then record the revert:\n{}",
					block.line.trim_end()
				),
			}),
		};

		let canonical = match std::fs::canonicalize(&stored) {
			Ok(path) => path,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
				return Ok(conflict(&format!("{relative} no longer exists"), None));
			}
			Err(e) => return Err(AppError::Io(format!("{}: {e}", stored.display()))),
		};
		if !canonical.starts_with(&root) {
			return Err(err(ErrorCode::UnsafeTarget, "the file now resolves outside the project"));
		}
		let bytes = patch::read_bytes_bounded(&canonical)?;
		let current = sha256_hex(&bytes);
		if recorded.as_deref() != Some(current.as_str()) {
			return Ok(conflict(
				&format!("{relative} changed after the instruction was saved, so an automatic revert could overwrite those edits"),
				Some(current),
			));
		}
		let Ok(content) = String::from_utf8(bytes) else {
			return Ok(conflict(&format!("{relative} is not UTF-8 text"), Some(current)));
		};
		let Some(reverted) = patch::remove_block(&content, &block) else {
			return Ok(conflict("the saved line is no longer where it was left", Some(current)));
		};
		let proposed_hash = sha256_hex(reverted.as_bytes());
		let diff = patch::unified_diff(&content, &reverted, &relative, true);
		let now = now_ms();
		let prepared = json!({
			"kind": "revert",
			"targetPath": canonical.to_string_lossy(),
			"relativePath": relative,
			"baseHash": current,
			"proposedHash": proposed_hash,
			"appendedBlock": block.text,
			"appendedLine": block.line,
			"preparedAt": now,
		});
		self.db().with(|conn| store::set_prepared(conn, candidate_id, Some(&prepared), now))?;
		Ok(PreparedChange {
			candidate_id: candidate_id.to_string(),
			kind: ChangeKind::Revert,
			status: ChangeStatus::Ready,
			target_path: patch::display_path(&canonical),
			relative_path: relative,
			exists: true,
			created_file,
			base_hash: Some(current),
			proposed_content: Some(reverted),
			proposed_hash: Some(proposed_hash),
			appended_block: Some(block.text),
			diff: Some(diff),
			note: created_file.then(|| {
				"This file was created for the instruction. Reverting leaves it empty or with whatever else was added; delete it by hand if you no longer want it.".to_string()
			}),
			duplicate_of: None,
			conflict: None,
		})
	}

	/// Record that the revert was saved: the instruction is gone from the file
	/// and the file is what the renderer says it saved. Needs no prepared revert -
	/// removing the line by hand is a revert too - only the disk's word.
	pub fn record_revert(
		&self,
		candidate_id: &str,
		target_path: &str,
		expected_hash: &str,
	) -> AppResult<ImprovementCandidate> {
		self.require_enabled()?;
		if !is_sha256(expected_hash) {
			return Err(AppError::InvalidInput(
				"expectedHash must be a lowercase sha256 hex".into(),
			));
		}
		let (row, root) = self.candidate_and_root(candidate_id)?;
		let implemented = self.reverting_record(&row)?;
		let stored = PathBuf::from(text(&implemented.details, "targetPath").unwrap_or_default());
		let relative = text(&implemented.details, "relativePath").unwrap_or_default().to_string();
		let line = text(&implemented.details, "appendedLine").unwrap_or_default().to_string();

		let (disk_hash, content) = self.read_saved_target(&root, &stored, target_path)?;
		if implemented.after_hash.as_deref() == Some(disk_hash.as_str()) {
			return Err(err(ErrorCode::NotSaved, format!("{relative} is unchanged since the instruction was saved; save the revert first")));
		}
		if disk_hash != expected_hash {
			return Err(err(
				ErrorCode::FileConflict,
				format!("{relative} on disk is not the content that was saved; it changed since"),
			));
		}
		if !line.is_empty() && content.contains(&line) {
			return Err(err(
				ErrorCode::NotSaved,
				format!("{relative} still contains the instruction"),
			));
		}

		let now = now_ms();
		self.db().with_mut(|conn| {
			let tx = conn.transaction()?;
			store::transition(
				&tx,
				candidate_id,
				&[CandidateStatus::Implemented, CandidateStatus::Retained],
				CandidateStatus::Reverted,
				now,
			)?;
			store::insert_event(
				&tx,
				candidate_id,
				EventType::Reverted,
				EventFields {
					artifact_path: Some(relative.clone()),
					before_hash: implemented.after_hash.clone(),
					after_hash: Some(disk_hash.clone()),
					details: Some(
						json!({ "targetPath": stored.to_string_lossy(), "relativePath": relative }),
					),
					..EventFields::default()
				},
				now,
			)?;
			tx.commit()?;
			Ok(())
		})?;
		self.after_change(&row.project_id, candidate_id)
	}

	/// The saved change a revert undoes: the candidate must be `implemented` or
	/// `retained`, and the delta needed to undo it must have been recorded.
	fn reverting_record(&self, row: &CandidateRow) -> AppResult<store::ImplementedEvent> {
		if !matches!(row.status, CandidateStatus::Implemented | CandidateStatus::Retained) {
			return Err(err(
				ErrorCode::InvalidState,
				format!(
					"only a saved change can be reverted, and this one is {}",
					row.status.as_str()
				),
			));
		}
		let event =
			self.db().with(|conn| store::latest_implemented(conn, &row.id))?.ok_or_else(|| {
				err(ErrorCode::InvalidState, "no saved change is recorded for this improvement")
			})?;
		if text(&event.details, "appendedBlock").is_none()
			|| text(&event.details, "targetPath").is_none()
		{
			return Err(err(
				ErrorCode::InvalidState,
				"the delta needed to undo this change was not recorded, so there is no automatic revert; remove the line by hand",
			));
		}
		Ok(event)
	}
}
