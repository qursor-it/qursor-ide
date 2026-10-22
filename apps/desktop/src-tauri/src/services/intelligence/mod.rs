//! Project Intelligence (F33, ADR-0084): observe a project's recent agent
//! sessions, propose the smallest remedy the evidence supports, and keep an
//! honest record of what was done about it.
//!
//! The cycle is Observe -> Verify -> Evaluate -> Implement -> Test ->
//! Retain/Revert -> Document, and this module owns the parts that can be done
//! without trusting anyone:
//!
//! - [`evidence`], [`collect`], [`redact`]: deterministic, offline extraction of
//!   what happened, from the original transcripts;
//! - [`provider`], [`analyzer`]: one tool-free, bounded, opt-in model call and the
//!   fail-closed validation of its answer;
//! - [`dedupe`], [`store`]: candidates that do not multiply and statuses that do
//!   not go backwards;
//! - [`patch`], [`lifecycle`]: an instruction change planned without being made,
//!   and recorded only after a person's Save has been checked against the disk.
//!
//! **Off until switched on.** Every operation below except reading the switch and
//! cancelling a run starts with [`IntelligenceService::require_enabled`]. Off
//! means no transcript is read, no process is started and nothing is written.
//!
//! **No project file is ever written from here** (ADR-0039). The writer is the
//! viewer's Save; this module hashes what is on disk before and after and refuses
//! to record anything that does not match.

pub mod analyzer;
pub mod collect;
pub mod dedupe;
pub mod evidence;
pub mod lifecycle;
pub mod patch;
pub mod provider;
pub mod redact;
pub mod run;
pub mod store;

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use dashmap::DashMap;
use sha2::{Digest, Sha256};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::intelligence::{
	AgentSessionCount, AnalysisPreview, ImprovementCandidate, InstructionFilePreview,
	IntelligenceChangedEvent, IntelligenceProgressEvent, IntelligenceRun, IntelligenceRunDetail,
	IntelligenceRunFinishedEvent, IntelligenceStatus, ProviderInfo, SignalPreview,
};
use crate::models::SettingKey;
use provider::{AnalysisProvider, Availability, EvidenceItem, ProviderRequest, RunControl};

/// Lowercase hex sha256. The one hash this feature uses: fingerprints, evidence
/// ids, dedupe keys and the file hashes a change is verified against.
pub fn sha256_hex(bytes: &[u8]) -> String {
	let digest = Sha256::digest(bytes);
	let mut out = String::with_capacity(64);
	for byte in digest {
		out.push_str(&format!("{byte:02x}"));
	}
	out
}

// ── Errors ─────────────────────────────────────────────────────────────────

/// The reasons a command refuses. They cross the boundary as
/// `AppError::Intelligence("<code>: <detail>")`, so the renderer can branch on
/// the code and show the detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
	/// The switch is off.
	Disabled,
	ProjectNotFound,
	/// The project's folder is not on disk.
	ProjectMissing,
	CandidateNotFound,
	RunNotFound,
	RunInProgress,
	/// There are no completed sessions to analyse.
	NoData,
	/// The preview the request names is no longer what the transcripts say.
	PreviewStale,
	/// The move is not allowed from where the candidate is.
	InvalidState,
	NotInstruction,
	NotPrepared,
	/// The file is not what was reviewed: it changed, or differs from the hash.
	FileConflict,
	/// The file does not (yet) hold the change.
	NotSaved,
	/// The target resolves outside the project, or is not an editable text file.
	UnsafeTarget,
	TargetTooLarge,
}

impl ErrorCode {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Disabled => "disabled",
			Self::ProjectNotFound => "project_not_found",
			Self::ProjectMissing => "project_missing",
			Self::CandidateNotFound => "candidate_not_found",
			Self::RunNotFound => "run_not_found",
			Self::RunInProgress => "run_in_progress",
			Self::NoData => "no_data",
			Self::PreviewStale => "preview_stale",
			Self::InvalidState => "invalid_state",
			Self::NotInstruction => "not_instruction",
			Self::NotPrepared => "not_prepared",
			Self::FileConflict => "file_conflict",
			Self::NotSaved => "not_saved",
			Self::UnsafeTarget => "unsafe_target",
			Self::TargetTooLarge => "target_too_large",
		}
	}
}

pub fn err(code: ErrorCode, detail: impl std::fmt::Display) -> AppError {
	AppError::Intelligence(format!("{}: {detail}", code.as_str()))
}

/// The code of an intelligence error, for the tests and for callers that branch.
pub fn error_code(error: &AppError) -> Option<&str> {
	match error {
		AppError::Intelligence(message) => message.split(':').next(),
		_ => None,
	}
}

// ── Events ─────────────────────────────────────────────────────────────────

/// What the service tells the outside. `lib.rs` turns these into Tauri events;
/// a test collects them.
#[derive(Debug, Clone)]
pub enum IntelligenceEvent {
	Progress(IntelligenceProgressEvent),
	RunFinished(IntelligenceRunFinishedEvent),
	Changed(IntelligenceChangedEvent),
}

pub type Emit = Arc<dyn Fn(IntelligenceEvent) + Send + Sync>;
/// Session ids a terminal is running right now.
pub type LiveIds = Arc<dyn Fn() -> HashSet<String> + Send + Sync>;

// ── The service ────────────────────────────────────────────────────────────

pub(crate) struct Inner {
	pub(crate) db: Db,
	pub(crate) provider: Arc<dyn AnalysisProvider>,
	pub(crate) emit: Emit,
	pub(crate) live_ids: LiveIds,
	/// Runs in flight, by id. Held so cancel and quit can reach a run's process.
	pub(crate) runs: DashMap<String, Arc<RunControl>>,
}

/// Cheap to clone; every clone is the same service.
#[derive(Clone)]
pub struct IntelligenceService {
	pub(crate) inner: Arc<Inner>,
}

impl IntelligenceService {
	/// A service over `db` with `provider` as its one analysis backend.
	///
	/// A run that was `running` when the app last stopped cannot still be running;
	/// it is marked failed here so the project is not refused for a ghost.
	pub fn new(db: Db, provider: Arc<dyn AnalysisProvider>, emit: Emit, live_ids: LiveIds) -> Self {
		if let Err(error) = db.with(|conn| store::mark_interrupted(conn, now_ms())) {
			tracing::warn!(%error, "could not mark interrupted analyses");
		}
		Self { inner: Arc::new(Inner { db, provider, emit, live_ids, runs: DashMap::new() }) }
	}

	/// The production wiring: the Claude CLI provider, events through Tauri.
	pub fn for_app(db: Db, app: tauri::AppHandle, live_ids: LiveIds) -> Self {
		use tauri::Emitter;
		let provider = Arc::new(provider::ClaudeCliProvider::new(db.clone()));
		let emit: Emit = Arc::new(move |event| match event {
			IntelligenceEvent::Progress(p) => {
				let _ = app.emit("intelligence:progress", p);
			}
			IntelligenceEvent::RunFinished(r) => {
				let _ = app.emit("intelligence:run-finished", r);
			}
			IntelligenceEvent::Changed(c) => {
				let _ = app.emit("intelligence:changed", c);
			}
		});
		Self::new(db, provider, emit, live_ids)
	}

	pub(crate) fn db(&self) -> &Db {
		&self.inner.db
	}

	pub(crate) fn emit(&self, event: IntelligenceEvent) {
		(self.inner.emit)(event);
	}

	pub(crate) fn changed(&self, project_id: &str, candidate_id: Option<&str>) {
		self.emit(IntelligenceEvent::Changed(IntelligenceChangedEvent {
			project_id: project_id.to_string(),
			candidate_id: candidate_id.map(str::to_owned),
		}));
	}

	// ── The switch ──

	/// Whether the person has turned Project Intelligence on. Anything but the
	/// exact value `on`, and any failure to read it, is off.
	pub fn enabled(&self) -> bool {
		self.inner.db.with(|conn| Ok(switch_is_on(conn))).unwrap_or(false)
	}

	pub fn require_enabled(&self) -> AppResult<()> {
		if self.enabled() {
			Ok(())
		} else {
			Err(err(
				ErrorCode::Disabled,
				"Project Intelligence is off. Turn it on in Settings to analyse sessions.",
			))
		}
	}

	/// The switch, and (only when it is on) whether the analysis provider can run.
	/// Reading this is the one thing that works while the feature is off, and it
	/// starts nothing when it is.
	pub fn status(&self) -> IntelligenceStatus {
		if !self.enabled() {
			return IntelligenceStatus { enabled: false, provider: None };
		}
		IntelligenceStatus { enabled: true, provider: Some(self.provider_info("")) }
	}

	pub(crate) fn provider_info(&self, project_id: &str) -> ProviderInfo {
		let provider = &self.inner.provider;
		let Availability { available, version, binary_path, reason, detail } =
			provider.availability(project_id);
		ProviderInfo {
			id: provider.id().to_string(),
			display_name: provider.display_name().to_string(),
			remote: provider.is_remote(),
			available,
			version,
			binary_path,
			reason: reason.map(|r| r.as_str().to_string()),
			detail,
		}
	}

	// ── Reading ──

	pub fn get_run(&self, run_id: &str) -> AppResult<IntelligenceRunDetail> {
		self.require_enabled()?;
		self.db().with(|conn| {
			let run = store::get_run(conn, run_id)?
				.ok_or_else(|| err(ErrorCode::RunNotFound, format!("no analysis {run_id}")))?;
			store::prune_unavailable_evidence(conn)?;
			let signals = store::run_signals(conn, run_id)?;
			Ok(IntelligenceRunDetail { run, signals })
		})
	}

	pub fn latest_run(&self, project_id: &str) -> AppResult<Option<IntelligenceRunDetail>> {
		self.require_enabled()?;
		self.db().with(|conn| {
			collect::project_root(conn, project_id)?;
			let Some(run) = store::latest_run(conn, project_id)? else { return Ok(None) };
			store::prune_unavailable_evidence(conn)?;
			let signals = store::run_signals(conn, &run.id)?;
			Ok(Some(IntelligenceRunDetail { run, signals }))
		})
	}

	pub fn list(&self, project_id: &str) -> AppResult<Vec<ImprovementCandidate>> {
		self.require_enabled()?;
		self.db().with(|conn| {
			collect::project_root(conn, project_id)?;
			store::prune_unavailable_evidence(conn)?;
			store::list_candidates(conn, project_id)
		})
	}

	// ── Preview ──

	/// What an analysis of this project would read, and what it would send.
	/// Reads transcripts; starts nothing and writes nothing.
	pub fn preview(
		&self,
		project_id: &str,
		session_limit: Option<u32>,
	) -> AppResult<AnalysisPreview> {
		self.require_enabled()?;
		let limit = collect::clamp_limit(session_limit);
		let (root, name) = self.db().read(|conn| collect::project_root(conn, project_id))?;
		let live = (self.inner.live_ids)();
		let collected = collect::collect(self.db(), project_id, limit, &live)?;
		let request = provider_request(project_id, &name, &collected.signals, None);
		let signals = collected
			.signals
			.iter()
			.map(|s| SignalPreview {
				evidence_id: s.evidence_id.clone(),
				session_id: s.session_id.clone(),
				session_title: s.session_title.clone(),
				source_agent: s.source_agent.to_string(),
				signal_type: s.signal_type,
				excerpt: s.excerpt.clone(),
				context: s.context.clone(),
				locator: s.locator.clone(),
			})
			.collect();
		Ok(AnalysisPreview {
			project_id: project_id.to_string(),
			session_limit: limit,
			session_count: collected.session_count,
			by_agent: collected
				.by_agent
				.iter()
				.map(|(agent, sessions)| AgentSessionCount {
					agent: agent.clone(),
					sessions: *sessions,
				})
				.collect(),
			excluded_running: collected.excluded_running,
			excluded_subagents: collected.excluded_subagents,
			unreadable: collected.unreadable,
			signals_capped: collected.capped,
			estimated_payload_bytes: provider::build_payload(&request).len() as u64,
			instruction_file: instruction_preview(&root),
			provider: self.provider_info(project_id),
			preview_hash: collect::preview_hash(limit, &collected.signals),
			signals,
		})
	}

	/// The run's row, for the lifecycle code that only needs its state.
	pub(crate) fn run_row(&self, run_id: &str) -> AppResult<Option<IntelligenceRun>> {
		self.db().with(|conn| store::get_run(conn, run_id))
	}
}

impl Drop for Inner {
	fn drop(&mut self) {
		// The backstop behind the explicit `cancel_all` on quit: a service that goes
		// away takes its analysis processes with it.
		for run in self.runs.iter() {
			run.value().cancel();
		}
	}
}

/// The switch, read on a connection the caller already holds.
pub(crate) fn switch_is_on(conn: &rusqlite::Connection) -> bool {
	crate::services::settings::get(conn, SettingKey::ProjectIntelligence).ok().flatten().as_deref()
		== Some("on")
}

pub(crate) fn now_ms() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_millis() as i64)
		.unwrap_or(0)
}

/// The evidence in the shape the provider takes.
pub(crate) fn provider_request(
	project_id: &str,
	project_name: &str,
	signals: &[collect::Signal],
	instructions: Option<String>,
) -> ProviderRequest {
	ProviderRequest {
		project_id: project_id.to_string(),
		project_name: project_name.to_string(),
		instructions,
		evidence: signals
			.iter()
			.map(|s| EvidenceItem {
				evidence_id: s.evidence_id.clone(),
				session_id: s.session_id.clone(),
				agent: s.source_agent.to_string(),
				signal_type: s.signal_type,
				excerpt: s.excerpt.clone(),
				context: s.context.clone(),
			})
			.collect(),
	}
}

/// The instruction file the model would see, as the preview describes it.
/// Never an error: a file that cannot be read is simply not included.
fn instruction_preview(root: &Path) -> Option<InstructionFilePreview> {
	let root = std::fs::canonicalize(root).ok()?;
	let target = patch::resolve_target(&root).ok().filter(|t| t.exists)?;
	let text = patch::read_text(&target.path).ok()?;
	let included = instruction_excerpt(&text);
	Some(InstructionFilePreview {
		path: target.relative,
		bytes: text.len() as u64,
		included_bytes: included.len() as u64,
		truncated: included.len() < text.len(),
	})
}

/// The head of an instruction file, redacted, cut at a character boundary.
pub(crate) fn instruction_excerpt(text: &str) -> String {
	let mut end = text.len().min(patch::PROMPT_INSTRUCTION_BYTES);
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	redact::redact(&text[..end])
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sha256_is_the_standard_lowercase_hex() {
		assert_eq!(
			sha256_hex(b"abc"),
			"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
		);
	}

	#[test]
	fn an_intelligence_error_carries_a_code_first() {
		let e = err(ErrorCode::FileConflict, "the file changed");
		assert_eq!(error_code(&e), Some("file_conflict"));
		assert_eq!(e.to_string(), "intelligence: file_conflict: the file changed");
		assert_eq!(error_code(&AppError::Io("x".into())), None);
	}

	#[test]
	fn an_instruction_excerpt_is_bounded_redacted_and_whole_characters() {
		let text = format!("password=hunter2\n{}", "é".repeat(patch::PROMPT_INSTRUCTION_BYTES));
		let out = instruction_excerpt(&text);
		assert!(out.len() <= patch::PROMPT_INSTRUCTION_BYTES + 32);
		assert!(!out.contains("hunter2"));
	}
}
