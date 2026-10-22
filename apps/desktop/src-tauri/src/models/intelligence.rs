//! The types Project Intelligence (F33, ADR-0084) sends across the Tauri
//! boundary. Mirrors `@factorai/types` under the same names.
//!
//! **Field names are camelCase, enum values are snake_case.** The values are the
//! ones the analysis contract (`specs/05-features.md` § F33) and the SQLite
//! columns already use (`recurring_correction`, `no_change`, ...), so one
//! spelling serves the model's JSON, the database and the renderer and there is
//! no mapping table to drift.

use serde::{Deserialize, Serialize};

/// Declares a string-valued enum with its wire/database spelling written once.
///
/// `as_str` is what is stored in SQLite and compared against; `parse` is the
/// only way a stored or model-supplied string becomes a value, so an unknown
/// spelling is `None` rather than a default.
macro_rules! string_enum {
	(
		$(#[$meta:meta])*
		pub enum $name:ident {
			$( $(#[$vmeta:meta])* $variant:ident => $text:literal ),+ $(,)?
		}
	) => {
		$(#[$meta])*
		#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
		pub enum $name {
			$( $(#[$vmeta])* #[serde(rename = $text)] $variant ),+
		}

		impl $name {
			pub fn as_str(self) -> &'static str {
				match self { $( Self::$variant => $text ),+ }
			}

			pub fn parse(text: &str) -> Option<Self> {
				match text { $( $text => Some(Self::$variant), )+ _ => None }
			}
		}
	};
}

string_enum! {
	/// What kind of problem a candidate describes. The model may only choose one
	/// of these four (`analyzer::validate`); anything else drops the candidate.
	pub enum ProblemKind {
		ProjectConvention => "project_convention",
		/// Claimed only with at least two independent parent sessions; the
		/// backend downgrades it otherwise (spec 6.4).
		RecurringCorrection => "recurring_correction",
		VerificationGap => "verification_gap",
		WorkflowHint => "workflow_hint",
	}
}

string_enum! {
	/// The remedy a candidate proposes. Only `instruction` can be prepared as a
	/// file change; the other three are consult-only in P0.
	pub enum ActionKind {
		Instruction => "instruction",
		TestSuggestion => "test_suggestion",
		ScriptSuggestion => "script_suggestion",
		ManualFollowup => "manual_followup",
	}
}

string_enum! {
	/// What the deterministic extractor saw. A signal is an observation, not a
	/// verdict.
	pub enum SignalType {
		UserCorrection => "user_correction",
		ToolFailure => "tool_failure",
		RetryAfterFailure => "retry_after_failure",
		RepeatedInstruction => "repeated_instruction",
	}
}

string_enum! {
	/// Where a candidate is in its life. `dismissed` and `reverted` are
	/// terminal, and no run moves a candidate back to `proposed`. A verification
	/// is an event, not a status: it never changes this.
	pub enum CandidateStatus {
		Proposed => "proposed",
		Dismissed => "dismissed",
		Implemented => "implemented",
		Retained => "retained",
		Reverted => "reverted",
	}
}

string_enum! {
	pub enum RunState {
		Running => "running",
		Completed => "completed",
		Failed => "failed",
		Cancelled => "cancelled",
	}
}

string_enum! {
	/// How a completed run ended. `no_change` is a legitimate result, not a
	/// candidate; `signals_only` means no model looked at the evidence.
	pub enum RunOutcome {
		Candidates => "candidates",
		NoChange => "no_change",
		SignalsOnly => "signals_only",
	}
}

string_enum! {
	pub enum EventType {
		Proposed => "proposed",
		EvidenceAdded => "evidence_added",
		Dismissed => "dismissed",
		FollowUp => "follow_up",
		Implemented => "implemented",
		Verification => "verification",
		Retained => "retained",
		Reverted => "reverted",
	}
}

string_enum! {
	/// A human-entered verification result. Never produced by a model.
	pub enum VerificationOutcome {
		Passed => "passed",
		Failed => "failed",
		Inconclusive => "inconclusive",
	}
}

string_enum! {
	pub enum ChangeKind {
		Instruction => "instruction",
		Revert => "revert",
	}
}

string_enum! {
	/// `ready` carries a patch; `duplicate` and `conflict` carry the reason there
	/// is none.
	pub enum ChangeStatus {
		Ready => "ready",
		Duplicate => "duplicate",
		Conflict => "conflict",
	}
}

// ── Preview ────────────────────────────────────────────────────────────────

/// Where an observation sits in its transcript. The session id lives on the
/// evidence row, so nothing here can name another session or a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLocator {
	/// 1-based physical line of the source event in the transcript file.
	pub line: u64,
	/// The agent's own ordinal for the line, where it writes one (Codex).
	pub ordinal: Option<i64>,
	/// The agent's own id for the event, where it writes one (Claude `uuid`,
	/// Codex tool `call_id`).
	pub event_id: Option<String>,
}

/// One observation that would be sent to the analysis provider, shown before the
/// run so the person can exclude it by `evidenceId`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalPreview {
	pub evidence_id: String,
	pub session_id: String,
	pub session_title: Option<String>,
	pub source_agent: String,
	pub signal_type: SignalType,
	/// Redacted and at most 280 characters. Exactly what would leave the machine.
	pub excerpt: String,
	/// Redacted and at most 160 characters.
	pub context: Option<String>,
	pub locator: EvidenceLocator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionCount {
	pub agent: String,
	pub sessions: u32,
}

/// The existing instruction file the model would see, bounded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionFilePreview {
	/// Relative to the project root.
	pub path: String,
	pub bytes: u64,
	/// Bytes of it that would be included in the payload (redacted).
	pub included_bytes: u64,
	pub truncated: bool,
}

/// What the analysis provider is and whether it can run right now.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
	pub id: String,
	pub display_name: String,
	/// A local CLI is not local inference: `true` means excerpts leave the
	/// machine and a run needs `consentRemote`.
	pub remote: bool,
	pub available: bool,
	pub version: Option<String>,
	pub binary_path: Option<String>,
	/// Why it is unavailable: `not_installed`, `missing_flag`, `probe_failed`.
	pub reason: Option<String>,
	pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisPreview {
	pub project_id: String,
	pub session_limit: u32,
	/// Sessions whose transcripts were read.
	pub session_count: u32,
	pub by_agent: Vec<AgentSessionCount>,
	/// Newer sessions skipped because a terminal is still running them.
	pub excluded_running: u32,
	/// Sub-agent transcripts, which are never independent evidence.
	pub excluded_subagents: u32,
	/// Selected sessions whose transcript could not be found or read.
	pub unreadable: u32,
	pub signals: Vec<SignalPreview>,
	/// More signals were found than the run cap allows.
	pub signals_capped: bool,
	/// Size of the evidence part of the payload, in bytes.
	pub estimated_payload_bytes: u64,
	pub instruction_file: Option<InstructionFilePreview>,
	pub provider: ProviderInfo,
	/// Identifies exactly this set of signals. `start_analysis` refuses a request
	/// whose hash no longer matches, so what is sent is what was shown.
	pub preview_hash: String,
}

// ── Starting and observing a run ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisRequest {
	pub project_id: String,
	#[serde(default)]
	pub session_limit: Option<u32>,
	/// The person agreed that the previewed excerpts may be processed by the
	/// provider. `false` runs offline: signals only, no process spawned.
	pub consent_remote: bool,
	/// Signals the person removed from the preview.
	#[serde(default)]
	pub excluded_evidence_ids: Vec<String>,
	/// `AnalysisPreview.previewHash` of the preview the person looked at.
	pub preview_hash: String,
	/// Include the project's existing instruction file (bounded, redacted) so
	/// the model can avoid duplicating it.
	#[serde(default)]
	pub include_instruction_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedRun {
	pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceRun {
	pub id: String,
	pub project_id: String,
	pub state: RunState,
	/// `None` until the run completes.
	pub outcome: Option<RunOutcome>,
	/// The provider that analysed it; `None` for a signals-only run.
	pub provider: Option<String>,
	pub session_count: u32,
	pub signal_count: u32,
	pub candidate_count: u32,
	/// Candidates the model proposed that validation rejected.
	pub dropped_count: u32,
	pub payload_bytes: u64,
	pub started_at: i64,
	pub ended_at: Option<i64>,
	/// Why a run failed or was cancelled, or why a completed run is degraded
	/// (`provider_unavailable` on a run that fell back to signals only).
	pub error_code: Option<String>,
	pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceRunDetail {
	pub run: IntelligenceRun,
	/// The run's observations. For a `signals_only` run these are *unreviewed*
	/// signals and must not be presented as insights.
	pub signals: Vec<EvidenceView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceStatus {
	pub enabled: bool,
	/// `None` while disabled: nothing is probed, nothing is spawned.
	pub provider: Option<ProviderInfo>,
}

// ── Candidates ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceView {
	pub evidence_id: String,
	pub session_id: String,
	pub session_title: Option<String>,
	/// The session is still in the index. When `false` the excerpt has been
	/// removed and the row only says that something was once observed.
	pub session_available: bool,
	pub source_agent: String,
	pub signal_type: SignalType,
	pub excerpt: Option<String>,
	pub context: Option<String>,
	pub locator: EvidenceLocator,
	pub fingerprint: String,
	pub observed_at: i64,
}

/// A line already in the instruction file that says what a candidate would add.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateHint {
	/// Relative to the project root.
	pub path: String,
	pub line: u32,
	pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImprovementEvent {
	pub id: i64,
	pub event_type: EventType,
	/// `passed` / `failed` / `inconclusive` for a verification.
	pub outcome: Option<String>,
	pub method: Option<String>,
	pub note: Option<String>,
	pub artifact_path: Option<String>,
	pub before_hash: Option<String>,
	pub after_hash: Option<String>,
	pub created_at: i64,
}

/// What was recorded when the change was saved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImplementationRecord {
	/// Absolute, canonical.
	pub target_path: String,
	pub relative_path: String,
	pub before_hash: Option<String>,
	pub after_hash: String,
	pub created_file: bool,
	pub implemented_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImprovementCandidate {
	pub id: String,
	pub project_id: String,
	pub latest_run_id: Option<String>,
	pub problem_kind: ProblemKind,
	pub action_kind: ActionKind,
	pub title: String,
	pub problem: String,
	pub recommendation: String,
	pub rationale: String,
	pub limitations: Option<String>,
	pub status: CandidateStatus,
	/// Fewer than two independent sessions: an hypothesis, not a pattern.
	pub hypothesis: bool,
	/// Set when the backend weakened what the model claimed, and why.
	pub downgrade_note: Option<String>,
	pub follow_up: bool,
	/// Distinct sessions among the evidence, computed by the backend.
	pub independent_sessions: u32,
	pub duplicate: Option<DuplicateHint>,
	pub evidence: Vec<EvidenceView>,
	/// Oldest first. The order is the order things happened in.
	pub events: Vec<ImprovementEvent>,
	pub implementation: Option<ImplementationRecord>,
	pub created_at: i64,
	pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationInput {
	pub candidate_id: String,
	pub method: String,
	pub outcome: VerificationOutcome,
	#[serde(default)]
	pub note: Option<String>,
}

// ── Prepared changes ───────────────────────────────────────────────────────

/// Why a revert cannot be offered as a patch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeConflict {
	pub reason: String,
	pub current_hash: Option<String>,
	pub recorded_hash: Option<String>,
	/// What to do by hand.
	pub manual_instructions: String,
}

/// A change the person may review and save. **Preparing it wrote nothing.**
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedChange {
	pub candidate_id: String,
	pub kind: ChangeKind,
	pub status: ChangeStatus,
	/// Absolute and canonical, inside the project root.
	pub target_path: String,
	pub relative_path: String,
	/// The file does not exist yet; saving creates it.
	pub exists: bool,
	pub created_file: bool,
	/// sha256 of the file as it is now; `None` when it does not exist.
	pub base_hash: Option<String>,
	pub proposed_content: Option<String>,
	/// sha256 of `proposedContent`. Pass it to `record_*` after saving it
	/// unchanged.
	pub proposed_hash: Option<String>,
	/// The exact text added (instruction) or removed (revert).
	pub appended_block: Option<String>,
	/// Unified diff from the current file to `proposedContent`.
	pub diff: Option<String>,
	/// The other name resolves to the same file, or exists separately.
	pub note: Option<String>,
	pub duplicate_of: Option<DuplicateHint>,
	pub conflict: Option<ChangeConflict>,
}

// ── Events pushed to the renderer ──────────────────────────────────────────

/// `intelligence:progress`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceProgressEvent {
	pub run_id: String,
	pub project_id: String,
	/// `analyzing` and `validating` (a run that calls the provider), then `saving`.
	pub phase: String,
	pub sessions_done: u32,
	pub sessions_total: u32,
	pub signals: u32,
}

/// `intelligence:run-finished`. Sent once per run, after the row is final.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceRunFinishedEvent {
	pub run_id: String,
	pub project_id: String,
	pub state: RunState,
	pub outcome: Option<RunOutcome>,
	pub error_code: Option<String>,
	pub candidate_count: u32,
	pub signal_count: u32,
}

/// `intelligence:changed`. The project's candidates or timeline moved; refetch
/// `list_improvements`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceChangedEvent {
	pub project_id: String,
	pub candidate_id: Option<String>,
}
