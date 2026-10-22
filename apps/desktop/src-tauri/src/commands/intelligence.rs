//! Project Intelligence's command surface (F33, ADR-0084).
//!
//! Thin on purpose: each command resolves the service, hops to the blocking pool
//! where it reads transcripts or files (PERF-07), and returns the service's
//! answer. **Every one of them refuses with `disabled` while the switch is off**
//! except `get_intelligence_status`, which only reads it, and
//! `cancel_analysis`, since stopping work is never the dangerous direction.
//!
//! Nothing here writes a project file. `prepare_*` return text and a diff for the
//! viewer; `record_*` read the disk after the person's Save and note what they
//! find. The one command that writes is the one that always did, `write_file`,
//! and it is not wrapped, extended or called from here (ADR-0039).
//!
//! No command here is offered to an agent over the MCP tool server (ADR-0029).

use tauri::State;

use crate::commands::off_main;
use crate::error::AppResult;
use crate::models::intelligence::{
	AnalysisPreview, AnalysisRequest, ImprovementCandidate, IntelligenceRunDetail,
	IntelligenceStatus, PreparedChange, StartedRun, VerificationInput,
};
use crate::state::AppState;

/// Whether Project Intelligence is on and, when it is, whether the analysis
/// provider can run. While it is off nothing is probed or started.
#[tauri::command]
pub async fn get_intelligence_status(state: State<'_, AppState>) -> AppResult<IntelligenceStatus> {
	let service = state.intelligence.clone();
	// The provider probe runs `claude --version` and `--help`.
	off_main(move || Ok(service.status())).await
}

/// What an analysis of this project would read and send: sessions by agent, the
/// signals with their redacted excerpts (so any can be excluded), the estimated
/// payload, the provider and whether it is remote, and a hash that pins the set.
#[tauri::command]
pub async fn preview_analysis(
	state: State<'_, AppState>,
	project_id: String,
	session_limit: Option<u32>,
) -> AppResult<AnalysisPreview> {
	let service = state.intelligence.clone();
	off_main(move || service.preview(&project_id, session_limit)).await
}

/// Start an analysis and return its id; progress and the end arrive as events.
/// `consentRemote: false` runs offline: signals only, nothing started.
#[tauri::command]
pub async fn start_analysis(
	state: State<'_, AppState>,
	request: AnalysisRequest,
) -> AppResult<StartedRun> {
	let service = state.intelligence.clone();
	off_main(move || service.start(request)).await
}

/// Stop a run and its process. Idempotent: an unknown or finished run is fine.
#[tauri::command]
pub fn cancel_analysis(state: State<'_, AppState>, run_id: String) -> AppResult<()> {
	state.intelligence.cancel(&run_id);
	Ok(())
}

#[tauri::command]
pub async fn get_intelligence_run(
	state: State<'_, AppState>,
	run_id: String,
) -> AppResult<IntelligenceRunDetail> {
	let service = state.intelligence.clone();
	off_main(move || service.get_run(&run_id)).await
}

/// The project's most recent run with its signals, or `null` if it never ran.
#[tauri::command]
pub async fn latest_intelligence_run(
	state: State<'_, AppState>,
	project_id: String,
) -> AppResult<Option<IntelligenceRunDetail>> {
	let service = state.intelligence.clone();
	off_main(move || service.latest_run(&project_id)).await
}

/// Every candidate of the project, any status, with evidence and timeline.
#[tauri::command]
pub async fn list_improvements(
	state: State<'_, AppState>,
	project_id: String,
) -> AppResult<Vec<ImprovementCandidate>> {
	let service = state.intelligence.clone();
	off_main(move || service.list(&project_id)).await
}

#[tauri::command]
pub async fn dismiss_improvement(
	state: State<'_, AppState>,
	candidate_id: String,
	reason: Option<String>,
) -> AppResult<ImprovementCandidate> {
	let service = state.intelligence.clone();
	off_main(move || service.dismiss(&candidate_id, reason)).await
}

#[tauri::command]
pub async fn mark_follow_up(
	state: State<'_, AppState>,
	candidate_id: String,
	follow_up: bool,
	note: Option<String>,
) -> AppResult<ImprovementCandidate> {
	let service = state.intelligence.clone();
	off_main(move || service.mark_follow_up(&candidate_id, follow_up, note)).await
}

/// The patch for an `instruction` candidate. Writes nothing.
/// `allowDuplicate` asks for the patch even when the file already says it.
#[tauri::command]
pub async fn prepare_instruction_change(
	state: State<'_, AppState>,
	candidate_id: String,
	allow_duplicate: Option<bool>,
) -> AppResult<PreparedChange> {
	let service = state.intelligence.clone();
	off_main(move || {
		service.prepare_instruction_change(&candidate_id, allow_duplicate.unwrap_or(false))
	})
	.await
}

/// After the person saved the prepared change: verify it on disk, then mark the
/// candidate `implemented`. Writes nothing to the project.
#[tauri::command]
pub async fn record_implementation(
	state: State<'_, AppState>,
	candidate_id: String,
	target_path: String,
	expected_hash: String,
) -> AppResult<ImprovementCandidate> {
	let service = state.intelligence.clone();
	off_main(move || service.record_implementation(&candidate_id, &target_path, &expected_hash))
		.await
}

/// A person's verification result. The candidate keeps its status.
#[tauri::command]
pub async fn record_verification(
	state: State<'_, AppState>,
	input: VerificationInput,
) -> AppResult<ImprovementCandidate> {
	let service = state.intelligence.clone();
	off_main(move || service.record_verification(input)).await
}

#[tauri::command]
pub async fn mark_retained(
	state: State<'_, AppState>,
	candidate_id: String,
	reason: Option<String>,
) -> AppResult<ImprovementCandidate> {
	let service = state.intelligence.clone();
	off_main(move || service.mark_retained(&candidate_id, reason)).await
}

/// The inverse patch, or `status: "conflict"` with manual instructions when the
/// file is no longer as the Save left it. Writes nothing.
#[tauri::command]
pub async fn prepare_revert(
	state: State<'_, AppState>,
	candidate_id: String,
) -> AppResult<PreparedChange> {
	let service = state.intelligence.clone();
	off_main(move || service.prepare_revert(&candidate_id)).await
}

/// After the person saved the revert: verify the instruction is gone and the
/// hash matches, then mark the candidate `reverted`. Writes nothing.
#[tauri::command]
pub async fn record_revert(
	state: State<'_, AppState>,
	candidate_id: String,
	target_path: String,
	expected_hash: String,
) -> AppResult<ImprovementCandidate> {
	let service = state.intelligence.clone();
	off_main(move || service.record_revert(&candidate_id, &target_path, &expected_hash)).await
}
