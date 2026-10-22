//! SQLite for Project Intelligence: runs, candidates, evidence and the timeline.
//!
//! **A run writes once, at the end, in one transaction.** While the model is
//! thinking nothing is held in the database - no lock, no partial candidate - and
//! the only way a run's candidates and evidence reach it is [`complete_run`].
//! A failed, cancelled or timed-out run therefore leaves behind its own row and
//! nothing else; it cannot become `completed` with half an answer.
//!
//! Every function takes a connection, so the lifecycle code can compose them
//! inside its own transaction and the tests can call them without an app.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use super::analyzer::ValidatedCandidate;
use super::collect::Signal;
use super::dedupe::{self, Existing};
use super::{err, ErrorCode};
use crate::error::{AppError, AppResult};
use crate::models::intelligence::{
	ActionKind, CandidateStatus, DuplicateHint, EventType, EvidenceLocator, EvidenceView,
	ImplementationRecord, ImprovementCandidate, ImprovementEvent, IntelligenceRun, ProblemKind,
	RunOutcome, RunState, SignalType,
};

// ── Runs ───────────────────────────────────────────────────────────────────

pub struct NewRun<'a> {
	pub id: &'a str,
	pub project_id: &'a str,
	pub provider: Option<&'a str>,
	pub session_count: u32,
	pub signal_count: u32,
	pub payload_bytes: u64,
	pub now: i64,
}

pub fn insert_run(conn: &Connection, run: &NewRun<'_>) -> AppResult<()> {
	conn.execute(
		"INSERT INTO intelligence_runs(id, project_id, state, analysis_provider, session_count,
		   signal_count, payload_bytes, started_at)
		 VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?6, ?7)",
		params![
			run.id,
			run.project_id,
			run.provider,
			run.session_count,
			run.signal_count,
			run.payload_bytes as i64,
			run.now
		],
	)?;
	Ok(())
}

/// The run still going for a project, if any.
pub fn running_run(conn: &Connection, project_id: &str) -> AppResult<Option<String>> {
	Ok(conn
		.query_row(
			"SELECT id FROM intelligence_runs WHERE project_id = ?1 AND state = 'running' LIMIT 1",
			[project_id],
			|r| r.get(0),
		)
		.optional()?)
}

/// A run that was `running` when the app stopped cannot still be running. It is
/// marked failed, so the next one is not refused for a ghost.
pub fn mark_interrupted(conn: &Connection, now: i64) -> AppResult<usize> {
	Ok(conn.execute(
		"UPDATE intelligence_runs
		 SET state = 'failed', error_code = 'interrupted', ended_at = ?1,
		     error_message = 'The app stopped while this analysis was running.'
		 WHERE state = 'running'",
		[now],
	)?)
}

/// End a run that did not complete. A run that is no longer `running` is left
/// as it is: whoever ended it first decided how.
pub fn end_run(
	conn: &Connection,
	run_id: &str,
	state: RunState,
	code: &str,
	message: &str,
	now: i64,
) -> AppResult<bool> {
	let changed = conn.execute(
		"UPDATE intelligence_runs
		 SET state = ?2, error_code = ?3, error_message = ?4, ended_at = ?5
		 WHERE id = ?1 AND state = 'running'",
		params![run_id, state.as_str(), code, message, now],
	)?;
	Ok(changed == 1)
}

const RUN_COLUMNS: &str = "id, project_id, state, outcome, analysis_provider, session_count,
	signal_count, candidate_count, dropped_count, payload_bytes, started_at, ended_at,
	error_code, error_message";

fn map_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<IntelligenceRun> {
	let state: String = row.get(2)?;
	let outcome: Option<String> = row.get(3)?;
	Ok(IntelligenceRun {
		id: row.get(0)?,
		project_id: row.get(1)?,
		state: RunState::parse(&state).unwrap_or(RunState::Failed),
		outcome: outcome.as_deref().and_then(RunOutcome::parse),
		provider: row.get(4)?,
		session_count: row.get(5)?,
		signal_count: row.get(6)?,
		candidate_count: row.get(7)?,
		dropped_count: row.get(8)?,
		payload_bytes: row.get::<_, i64>(9)? as u64,
		started_at: row.get(10)?,
		ended_at: row.get(11)?,
		error_code: row.get(12)?,
		error_message: row.get(13)?,
	})
}

pub fn get_run(conn: &Connection, run_id: &str) -> AppResult<Option<IntelligenceRun>> {
	let sql = format!("SELECT {RUN_COLUMNS} FROM intelligence_runs WHERE id = ?1");
	Ok(conn.query_row(&sql, [run_id], map_run).optional()?)
}

pub fn latest_run(conn: &Connection, project_id: &str) -> AppResult<Option<IntelligenceRun>> {
	let sql = format!(
		"SELECT {RUN_COLUMNS} FROM intelligence_runs WHERE project_id = ?1
		 ORDER BY started_at DESC, rowid DESC LIMIT 1"
	);
	Ok(conn.query_row(&sql, [project_id], map_run).optional()?)
}

// ── Evidence ───────────────────────────────────────────────────────────────

/// Retention: an excerpt is kept only while the session it quotes is in the
/// index. A session that was deleted, reaped with its transcript, or dropped
/// with its profile leaves the evidence row - the fact that something was once
/// observed - and loses the words.
///
/// Run on every read and every start, rather than hooked into each of the ways a
/// session can go, so a new way cannot be forgotten.
pub fn prune_unavailable_evidence(conn: &Connection) -> AppResult<usize> {
	Ok(conn.execute(
		"UPDATE improvement_evidence SET excerpt = NULL, context = NULL
		 WHERE (excerpt IS NOT NULL OR context IS NOT NULL)
		   AND session_id NOT IN (SELECT id FROM sessions)",
		[],
	)?)
}

const EVIDENCE_COLUMNS: &str = "e.id, e.session_id, s.title, s.id IS NOT NULL, e.source_agent,
	e.signal_type, e.excerpt, e.context, e.locator_json, e.fingerprint, e.observed_at";

fn map_evidence(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<EvidenceView> {
	let available: bool = row.get(offset + 3)?;
	let signal: String = row.get(offset + 5)?;
	let locator: String = row.get(offset + 8)?;
	Ok(EvidenceView {
		evidence_id: row.get(offset)?,
		session_id: row.get(offset + 1)?,
		session_title: row.get(offset + 2)?,
		session_available: available,
		source_agent: row.get(offset + 4)?,
		signal_type: SignalType::parse(&signal).unwrap_or(SignalType::UserCorrection),
		// Words from a session that is gone are not shown, whatever is left in the row.
		excerpt: if available { row.get(offset + 6)? } else { None },
		context: if available { row.get(offset + 7)? } else { None },
		locator: serde_json::from_str(&locator).unwrap_or(EvidenceLocator {
			line: 0,
			ordinal: None,
			event_id: None,
		}),
		fingerprint: row.get(offset + 9)?,
		observed_at: row.get(offset + 10)?,
	})
}

/// The signals of one run.
pub fn run_signals(conn: &Connection, run_id: &str) -> AppResult<Vec<EvidenceView>> {
	let sql = format!(
		"SELECT {EVIDENCE_COLUMNS}
		 FROM intelligence_run_evidence re
		 JOIN improvement_evidence e ON e.id = re.evidence_id
		 LEFT JOIN sessions s ON s.id = e.session_id
		 WHERE re.run_id = ?1
		 ORDER BY e.observed_at, e.id"
	);
	let mut stmt = conn.prepare(&sql)?;
	let rows = stmt.query_map([run_id], |r| map_evidence(r, 0))?;
	Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ── Completing a run ───────────────────────────────────────────────────────

pub struct Completion {
	pub outcome: RunOutcome,
	/// Candidates this run proposed or re-confirmed, new and existing alike.
	pub candidate_ids: Vec<String>,
	pub new_candidates: u32,
}

pub struct CompleteArgs<'a> {
	pub run_id: &'a str,
	pub project_id: &'a str,
	/// The provider that analysed the run; `None` when no model looked at it.
	pub provider: Option<&'a str>,
	/// A model reviewed the evidence and its answer passed validation. Without it
	/// a run can only be `signals_only` (or `no_change` when there was nothing
	/// to look at), however many candidates are handed in.
	pub analysed: bool,
	pub signals: &'a [Signal],
	pub candidates: Vec<ValidatedCandidate>,
	pub dropped: u32,
	pub payload_bytes: u64,
	/// A completed run that is not what was asked for: `(code, message)`.
	pub degraded: Option<(&'a str, String)>,
	pub now: i64,
}

/// Write a finished run: its evidence, its candidates (merged into what already
/// exists), their timeline, and the run row itself - or nothing.
///
/// `still_wanted` is asked once, immediately before the commit. If it says no
/// (the run was cancelled in the meantime) everything is rolled back and `None`
/// comes back, so a late cancel cannot leave a half-visible result.
pub fn complete_run(
	conn: &mut Connection,
	args: CompleteArgs<'_>,
	still_wanted: impl FnOnce() -> bool,
) -> AppResult<Option<Completion>> {
	let tx = conn.transaction()?;

	for signal in args.signals {
		tx.execute(
			"INSERT INTO improvement_evidence(id, project_id, session_id, source_agent, signal_type,
			   locator_json, excerpt, context, fingerprint, observed_at)
			 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
			 ON CONFLICT(id) DO UPDATE SET excerpt = excluded.excerpt, context = excluded.context",
			params![
				signal.evidence_id,
				args.project_id,
				signal.session_id,
				signal.source_agent,
				signal.signal_type.as_str(),
				serde_json::to_string(&signal.locator)?,
				signal.excerpt,
				signal.context,
				signal.fingerprint,
				signal.observed_at,
			],
		)?;
		tx.execute(
			"INSERT OR IGNORE INTO intelligence_run_evidence(run_id, evidence_id) VALUES (?1, ?2)",
			params![args.run_id, signal.evidence_id],
		)?;
	}

	let mut existing = load_existing(&tx, args.project_id)?;
	let mut touched: Vec<String> = Vec::new();
	let mut new_candidates = 0u32;
	// Candidates are only ever stored from a run a model actually reviewed.
	for candidate in args.candidates.iter().filter(|_| args.analysed) {
		let tokens = dedupe::tokens(&candidate.recommendation);
		let key = dedupe::dedupe_key(
			args.project_id,
			candidate.problem_kind,
			candidate.action_kind,
			&candidate.recommendation,
		);
		let evidence: HashSet<String> = candidate.evidence_ids.iter().cloned().collect();
		let by_key: Option<String> = tx
			.query_row(
				"SELECT id FROM improvement_candidates WHERE project_id = ?1 AND dedupe_key = ?2",
				params![args.project_id, key],
				|r| r.get(0),
			)
			.optional()?;
		let target = by_key.or_else(|| {
			dedupe::find_merge_target(&existing, candidate.action_kind, &tokens, &evidence)
				.map(|e| e.id.clone())
		});
		let duplicate_json = candidate.duplicate.as_ref().map(serde_json::to_string).transpose()?;
		let id = match target {
			Some(id) => {
				merge_into(&tx, &id, candidate, duplicate_json.as_deref(), args.run_id, args.now)?;
				if let Some(e) = existing.iter_mut().find(|e| e.id == id) {
					e.evidence.extend(evidence);
				}
				id
			}
			None => {
				let id = uuid::Uuid::new_v4().to_string();
				insert_candidate(&tx, &id, &key, candidate, duplicate_json.as_deref(), &args)?;
				existing.push(Existing {
					id: id.clone(),
					action: candidate.action_kind,
					tokens,
					evidence,
				});
				new_candidates += 1;
				id
			}
		};
		if !touched.contains(&id) {
			touched.push(id);
		}
	}

	let outcome = if args.analysed {
		if touched.is_empty() {
			RunOutcome::NoChange
		} else {
			RunOutcome::Candidates
		}
	} else if args.signals.is_empty() {
		RunOutcome::NoChange
	} else {
		RunOutcome::SignalsOnly
	};
	let (code, message) = match &args.degraded {
		Some((code, message)) => (Some(*code), Some(message.as_str())),
		None => (None, None),
	};
	let updated = tx.execute(
		"UPDATE intelligence_runs
		 SET state = 'completed', outcome = ?2, analysis_provider = ?3, signal_count = ?4,
		     candidate_count = ?5, dropped_count = ?6, payload_bytes = ?7, ended_at = ?8,
		     error_code = ?9, error_message = ?10
		 WHERE id = ?1 AND state = 'running'",
		params![
			args.run_id,
			outcome.as_str(),
			args.provider,
			args.signals.len() as i64,
			touched.len() as i64,
			args.dropped,
			args.payload_bytes as i64,
			args.now,
			code,
			message,
		],
	)?;
	if updated != 1 || !still_wanted() {
		// Dropped, not committed: the run was ended by someone else.
		return Ok(None);
	}
	tx.commit()?;
	Ok(Some(Completion { outcome, candidate_ids: touched, new_candidates }))
}

fn load_existing(conn: &Connection, project_id: &str) -> AppResult<Vec<Existing>> {
	let mut by_id: HashMap<String, Existing> = HashMap::new();
	let mut order: Vec<String> = Vec::new();
	let mut stmt = conn.prepare(
		"SELECT id, action_kind, recommendation FROM improvement_candidates WHERE project_id = ?1",
	)?;
	let rows = stmt.query_map([project_id], |r| {
		Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
	})?;
	for row in rows {
		let (id, action, recommendation) = row?;
		let Some(action) = ActionKind::parse(&action) else { continue };
		order.push(id.clone());
		by_id.insert(
			id.clone(),
			Existing {
				id,
				action,
				tokens: dedupe::tokens(&recommendation),
				evidence: HashSet::new(),
			},
		);
	}
	let mut stmt = conn.prepare(
		"SELECT ce.candidate_id, ce.evidence_id
		 FROM improvement_candidate_evidence ce
		 JOIN improvement_candidates c ON c.id = ce.candidate_id
		 WHERE c.project_id = ?1",
	)?;
	let links =
		stmt.query_map([project_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
	for link in links {
		let (candidate, evidence) = link?;
		if let Some(e) = by_id.get_mut(&candidate) {
			e.evidence.insert(evidence);
		}
	}
	Ok(order.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

fn insert_candidate(
	tx: &Connection,
	id: &str,
	key: &str,
	c: &ValidatedCandidate,
	duplicate_json: Option<&str>,
	args: &CompleteArgs<'_>,
) -> AppResult<()> {
	tx.execute(
		"INSERT INTO improvement_candidates(id, project_id, latest_run_id, dedupe_key, problem_kind,
		   action_kind, title, problem, recommendation, rationale, limitations, status,
		   downgrade_note, follow_up, duplicate_json, created_at, updated_at)
		 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'proposed', ?12, 0, ?13, ?14, ?14)",
		params![
			id,
			args.project_id,
			args.run_id,
			key,
			c.problem_kind.as_str(),
			c.action_kind.as_str(),
			c.title,
			c.problem,
			c.recommendation,
			c.rationale,
			c.limitations,
			c.downgrade_note,
			duplicate_json,
			args.now,
		],
	)?;
	for evidence in &c.evidence_ids {
		tx.execute(
			"INSERT OR IGNORE INTO improvement_candidate_evidence(candidate_id, evidence_id)
			 VALUES (?1, ?2)",
			params![id, evidence],
		)?;
	}
	insert_event(
		tx,
		id,
		EventType::Proposed,
		EventFields {
			details: Some(json!({ "runId": args.run_id, "evidence": c.evidence_ids.len() })),
			..EventFields::default()
		},
		args.now,
	)
}

/// A candidate this run saw again. Its status and its words are left alone;
/// only new evidence is attached and the run it was last seen in advances.
fn merge_into(
	tx: &Connection,
	id: &str,
	c: &ValidatedCandidate,
	duplicate_json: Option<&str>,
	run_id: &str,
	now: i64,
) -> AppResult<()> {
	let mut added = 0u32;
	for evidence in &c.evidence_ids {
		added += tx.execute(
			"INSERT OR IGNORE INTO improvement_candidate_evidence(candidate_id, evidence_id)
			 VALUES (?1, ?2)",
			params![id, evidence],
		)? as u32;
	}
	tx.execute(
		"UPDATE improvement_candidates SET latest_run_id = ?2, updated_at = ?3 WHERE id = ?1",
		params![id, run_id, now],
	)?;
	// The duplicate hint is advice about a proposal; a decided candidate keeps
	// the hint it had when it was decided.
	tx.execute(
		"UPDATE improvement_candidates SET duplicate_json = ?2 WHERE id = ?1 AND status = 'proposed'",
		params![id, duplicate_json],
	)?;
	if added > 0 {
		insert_event(
			tx,
			id,
			EventType::EvidenceAdded,
			EventFields {
				details: Some(json!({ "runId": run_id, "added": added })),
				..EventFields::default()
			},
			now,
		)?;
	}
	Ok(())
}

// ── The timeline ───────────────────────────────────────────────────────────

#[derive(Default)]
pub struct EventFields {
	pub outcome: Option<String>,
	pub method: Option<String>,
	pub details: Option<Value>,
	pub artifact_path: Option<String>,
	pub before_hash: Option<String>,
	pub after_hash: Option<String>,
}

pub fn insert_event(
	conn: &Connection,
	candidate_id: &str,
	event_type: EventType,
	fields: EventFields,
	now: i64,
) -> AppResult<()> {
	let details = fields.details.as_ref().map(serde_json::to_string).transpose()?;
	conn.execute(
		"INSERT INTO improvement_events(candidate_id, event_type, outcome, method, details_json,
		   artifact_path, before_hash, after_hash, created_at)
		 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
		params![
			candidate_id,
			event_type.as_str(),
			fields.outcome,
			fields.method,
			details,
			fields.artifact_path,
			fields.before_hash,
			fields.after_hash,
			now,
		],
	)?;
	Ok(())
}

// ── Reading candidates ─────────────────────────────────────────────────────

/// One candidate's row, as the lifecycle needs it. Ownership is the row's
/// `project_id`: every command that takes a candidate id goes through this.
#[derive(Debug, Clone)]
pub struct CandidateRow {
	pub id: String,
	pub project_id: String,
	pub action_kind: ActionKind,
	pub status: CandidateStatus,
	pub recommendation: String,
	pub follow_up: bool,
	pub prepared: Option<Value>,
}

/// `improvement_candidates` columns read by [`load_row`], in order.
type RawRow = (String, String, String, String, String, bool, Option<String>);

pub fn load_row(conn: &Connection, candidate_id: &str) -> AppResult<CandidateRow> {
	let row: Option<RawRow> = conn
		.query_row(
			"SELECT id, project_id, action_kind, status, recommendation, follow_up, prepared_json
			 FROM improvement_candidates WHERE id = ?1",
			[candidate_id],
			|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
		)
		.optional()?;
	let (id, project_id, action, status, recommendation, follow_up, prepared) =
		row.ok_or_else(|| {
			err(ErrorCode::CandidateNotFound, format!("no improvement {candidate_id}"))
		})?;
	Ok(CandidateRow {
		id,
		project_id,
		action_kind: ActionKind::parse(&action)
			.ok_or_else(|| AppError::Db(format!("unknown action {action}")))?,
		status: CandidateStatus::parse(&status)
			.ok_or_else(|| AppError::Db(format!("unknown status {status}")))?,
		recommendation,
		follow_up,
		prepared: prepared.and_then(|p| serde_json::from_str(&p).ok()),
	})
}

/// Move a candidate from one of `from` to `to`, or say why not.
pub fn transition(
	conn: &Connection,
	candidate_id: &str,
	from: &[CandidateStatus],
	to: CandidateStatus,
	now: i64,
) -> AppResult<()> {
	let current = load_row(conn, candidate_id)?.status;
	if !from.contains(&current) {
		return Err(err(
			ErrorCode::InvalidState,
			format!("an improvement that is {} cannot become {}", current.as_str(), to.as_str()),
		));
	}
	conn.execute(
		"UPDATE improvement_candidates SET status = ?2, updated_at = ?3, prepared_json = NULL
		 WHERE id = ?1 AND status = ?4",
		params![candidate_id, to.as_str(), now, current.as_str()],
	)?;
	Ok(())
}

pub fn set_prepared(
	conn: &Connection,
	candidate_id: &str,
	prepared: Option<&Value>,
	now: i64,
) -> AppResult<()> {
	let text = prepared.map(serde_json::to_string).transpose()?;
	conn.execute(
		"UPDATE improvement_candidates SET prepared_json = ?2, updated_at = ?3 WHERE id = ?1",
		params![candidate_id, text, now],
	)?;
	Ok(())
}

pub fn set_duplicate(
	conn: &Connection,
	candidate_id: &str,
	hint: Option<&DuplicateHint>,
) -> AppResult<()> {
	let text = hint.map(serde_json::to_string).transpose()?;
	conn.execute(
		"UPDATE improvement_candidates SET duplicate_json = ?2 WHERE id = ?1",
		params![candidate_id, text],
	)?;
	Ok(())
}

pub fn set_follow_up(
	conn: &Connection,
	candidate_id: &str,
	value: bool,
	now: i64,
) -> AppResult<()> {
	conn.execute(
		"UPDATE improvement_candidates SET follow_up = ?2, updated_at = ?3 WHERE id = ?1",
		params![candidate_id, value, now],
	)?;
	Ok(())
}

/// The most recent `implemented` event of a candidate, with its stored delta.
pub struct ImplementedEvent {
	pub details: Value,
	pub before_hash: Option<String>,
	pub after_hash: Option<String>,
	pub artifact_path: Option<String>,
	pub created_at: i64,
}

/// `details_json`, `before_hash`, `after_hash`, `artifact_path`, `created_at`.
type RawImplemented = (Option<String>, Option<String>, Option<String>, Option<String>, i64);

pub fn latest_implemented(
	conn: &Connection,
	candidate_id: &str,
) -> AppResult<Option<ImplementedEvent>> {
	let row: Option<RawImplemented> = conn
		.query_row(
			"SELECT details_json, before_hash, after_hash, artifact_path, created_at
			 FROM improvement_events
			 WHERE candidate_id = ?1 AND event_type = 'implemented'
			 ORDER BY id DESC LIMIT 1",
			[candidate_id],
			|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
		)
		.optional()?;
	Ok(row.map(|(details, before_hash, after_hash, artifact_path, created_at)| ImplementedEvent {
		details: details.and_then(|d| serde_json::from_str(&d).ok()).unwrap_or(Value::Null),
		before_hash,
		after_hash,
		artifact_path,
		created_at,
	}))
}

pub fn list_candidates(
	conn: &Connection,
	project_id: &str,
) -> AppResult<Vec<ImprovementCandidate>> {
	let mut stmt = conn.prepare(
		"SELECT id, project_id, latest_run_id, problem_kind, action_kind, title, problem,
		        recommendation, rationale, limitations, status, downgrade_note, follow_up,
		        duplicate_json, created_at, updated_at
		 FROM improvement_candidates WHERE project_id = ?1
		 ORDER BY (status = 'proposed') DESC, updated_at DESC, id",
	)?;
	let mut candidates: Vec<ImprovementCandidate> = stmt
		.query_map([project_id], |r| {
			let kind: String = r.get(3)?;
			let action: String = r.get(4)?;
			let status: String = r.get(10)?;
			let duplicate: Option<String> = r.get(13)?;
			Ok(ImprovementCandidate {
				id: r.get(0)?,
				project_id: r.get(1)?,
				latest_run_id: r.get(2)?,
				problem_kind: ProblemKind::parse(&kind).unwrap_or(ProblemKind::WorkflowHint),
				action_kind: ActionKind::parse(&action).unwrap_or(ActionKind::ManualFollowup),
				title: r.get(5)?,
				problem: r.get(6)?,
				recommendation: r.get(7)?,
				rationale: r.get(8)?,
				limitations: r.get(9)?,
				status: CandidateStatus::parse(&status).unwrap_or(CandidateStatus::Proposed),
				hypothesis: false,
				downgrade_note: r.get(11)?,
				follow_up: r.get(12)?,
				independent_sessions: 0,
				duplicate: duplicate.and_then(|d| serde_json::from_str(&d).ok()),
				evidence: Vec::new(),
				events: Vec::new(),
				implementation: None,
				created_at: r.get(14)?,
				updated_at: r.get(15)?,
			})
		})?
		.collect::<rusqlite::Result<Vec<_>>>()?;
	if candidates.is_empty() {
		return Ok(candidates);
	}
	let mut index: HashMap<String, usize> = HashMap::new();
	for (i, c) in candidates.iter().enumerate() {
		index.insert(c.id.clone(), i);
	}

	let sql = format!(
		"SELECT ce.candidate_id, {EVIDENCE_COLUMNS}
		 FROM improvement_candidate_evidence ce
		 JOIN improvement_evidence e ON e.id = ce.evidence_id
		 LEFT JOIN sessions s ON s.id = e.session_id
		 WHERE ce.candidate_id IN (SELECT id FROM improvement_candidates WHERE project_id = ?1)
		 ORDER BY e.observed_at, e.id"
	);
	let mut stmt = conn.prepare(&sql)?;
	let rows =
		stmt.query_map([project_id], |r| Ok((r.get::<_, String>(0)?, map_evidence(r, 1)?)))?;
	for row in rows {
		let (candidate, view) = row?;
		if let Some(&i) = index.get(&candidate) {
			candidates[i].evidence.push(view);
		}
	}

	let mut stmt = conn.prepare(
		"SELECT candidate_id, id, event_type, outcome, method, details_json, artifact_path,
		        before_hash, after_hash, created_at
		 FROM improvement_events
		 WHERE candidate_id IN (SELECT id FROM improvement_candidates WHERE project_id = ?1)
		 ORDER BY id",
	)?;
	let rows = stmt.query_map([project_id], |r| {
		let kind: String = r.get(2)?;
		let details: Option<String> = r.get(5)?;
		let details: Option<Value> = details.and_then(|d| serde_json::from_str(&d).ok());
		let note =
			details.as_ref().and_then(|d| d.get("note")).and_then(Value::as_str).map(str::to_owned);
		Ok((
			r.get::<_, String>(0)?,
			ImprovementEvent {
				id: r.get(1)?,
				event_type: EventType::parse(&kind).unwrap_or(EventType::Proposed),
				outcome: r.get(3)?,
				method: r.get(4)?,
				note,
				artifact_path: r.get(6)?,
				before_hash: r.get(7)?,
				after_hash: r.get(8)?,
				created_at: r.get(9)?,
			},
			details,
		))
	})?;
	let mut implemented: HashMap<String, (ImprovementEvent, Option<Value>)> = HashMap::new();
	for row in rows {
		let (candidate, event, details) = row?;
		if let Some(&i) = index.get(&candidate) {
			if event.event_type == EventType::Implemented {
				implemented.insert(candidate, (event.clone(), details));
			}
			candidates[i].events.push(event);
		}
	}

	for c in &mut candidates {
		let sessions: HashSet<&str> = c.evidence.iter().map(|e| e.session_id.as_str()).collect();
		c.independent_sessions = sessions.len() as u32;
		c.hypothesis = c.independent_sessions < 2;
		if let Some((event, details)) = implemented.remove(&c.id) {
			let text = |key: &str| {
				details.as_ref().and_then(|d| d.get(key)).and_then(Value::as_str).map(str::to_owned)
			};
			c.implementation = Some(ImplementationRecord {
				target_path: text("targetPath").unwrap_or_default(),
				relative_path: text("relativePath")
					.or_else(|| event.artifact_path.clone())
					.unwrap_or_default(),
				before_hash: event.before_hash.clone(),
				after_hash: event.after_hash.clone().unwrap_or_default(),
				created_file: details
					.as_ref()
					.and_then(|d| d.get("createdFile"))
					.and_then(Value::as_bool)
					.unwrap_or(false),
				implemented_at: event.created_at,
			});
		}
	}
	Ok(candidates)
}

pub fn get_candidate(conn: &Connection, candidate_id: &str) -> AppResult<ImprovementCandidate> {
	let row = load_row(conn, candidate_id)?;
	list_candidates(conn, &row.project_id)?
		.into_iter()
		.find(|c| c.id == candidate_id)
		.ok_or_else(|| err(ErrorCode::CandidateNotFound, format!("no improvement {candidate_id}")))
}
