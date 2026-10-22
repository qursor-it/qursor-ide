//! Starting, running and cancelling an analysis.
//!
//! `start` does the quick, refusable work on the caller's thread - the switch,
//! the project, the stale-preview check, consent - and returns a run id; the
//! rest happens on a thread of its own, so nothing here waits for a model on the
//! command's thread and **no database lock is held while one is thinking**. The
//! result is written once, at the end, by [`store::complete_run`].
//!
//! **No spawn without consent.** A model is called only when the request carries
//! `consent_remote`, the provider says it can run, and there is evidence to send.
//! Anything else is a signals-only run: the deterministic observations, stored
//! and shown as unreviewed, with nothing started.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::analyzer::{self, ValidatedCandidate};
use super::collect::{self, Signal};
use super::provider::{self, ProviderError, RunControl};
use super::store::{self, CompleteArgs, NewRun};
use super::{err, now_ms, patch, redact, ErrorCode, IntelligenceEvent, IntelligenceService};
use crate::error::{AppError, AppResult};
use crate::models::intelligence::{
	AnalysisRequest, DuplicateHint, IntelligenceProgressEvent, IntelligenceRunFinishedEvent,
	RunState, StartedRun,
};

struct Job {
	run_id: String,
	project_id: String,
	project_name: String,
	root: PathBuf,
	signals: Vec<Signal>,
	session_count: u32,
	/// A model will be called.
	active: bool,
	/// Recorded on the run when a model was wanted and could not be used.
	degraded: Option<(&'static str, String)>,
	include_instructions: bool,
	control: Arc<RunControl>,
}

impl IntelligenceService {
	/// Begin an analysis and return its id. Everything that can be refused is
	/// refused here; what goes wrong afterwards ends the run as `failed` or
	/// `cancelled` and is reported by `intelligence:run-finished`.
	pub fn start(&self, request: AnalysisRequest) -> AppResult<StartedRun> {
		self.require_enabled()?;
		let project_id = request.project_id.clone();
		let limit = collect::clamp_limit(request.session_limit);
		let (root, project_name) =
			self.db().read(|conn| collect::project_root(conn, &project_id))?;
		if self.db().read(|conn| store::running_run(conn, &project_id))?.is_some() {
			return Err(err(
				ErrorCode::RunInProgress,
				"an analysis of this project is already running",
			));
		}
		// Retention before anything is read, so a stale excerpt is never re-sent.
		self.db().with(store::prune_unavailable_evidence)?;

		let live = (self.inner.live_ids)();
		let mut collected = collect::collect(self.db(), &project_id, limit, &live)?;
		if collected.session_count == 0 {
			return Err(err(ErrorCode::NoData, "there are no completed sessions to analyse yet"));
		}
		// What leaves the machine is what the person was shown, or nothing.
		if collect::preview_hash(limit, &collected.signals) != request.preview_hash {
			return Err(err(
				ErrorCode::PreviewStale,
				"the sessions changed since the preview; review it again before analysing",
			));
		}
		let excluded: HashSet<&str> =
			request.excluded_evidence_ids.iter().map(String::as_str).collect();
		collected.signals.retain(|s| !excluded.contains(s.evidence_id.as_str()));

		let mut active = false;
		let mut degraded = None;
		if request.consent_remote && !collected.signals.is_empty() {
			let availability = self.inner.provider.availability(&project_id);
			if availability.available {
				active = true;
			} else {
				degraded = Some((
					"provider_unavailable",
					availability
						.detail
						.unwrap_or_else(|| "the analysis provider is not available".to_string()),
				));
			}
		}

		let run_id = uuid::Uuid::new_v4().to_string();
		let control = Arc::new(RunControl::default());
		self.db().with(|conn| {
			// Checked again under the writer lock: two starts racing past the check
			// above still cannot both insert.
			if store::running_run(conn, &project_id)?.is_some() {
				return Err(err(
					ErrorCode::RunInProgress,
					"an analysis of this project is already running",
				));
			}
			store::insert_run(
				conn,
				&NewRun {
					id: &run_id,
					project_id: &project_id,
					provider: active.then(|| self.inner.provider.id()),
					session_count: collected.session_count,
					signal_count: collected.signals.len() as u32,
					payload_bytes: 0,
					now: now_ms(),
				},
			)
		})?;
		self.inner.runs.insert(run_id.clone(), control.clone());

		let job = Job {
			run_id: run_id.clone(),
			project_id,
			project_name,
			root,
			signals: collected.signals,
			session_count: collected.session_count,
			active,
			degraded,
			include_instructions: request.include_instruction_file,
			control,
		};
		let service = self.clone();
		let spawned = std::thread::Builder::new()
			.name("intelligence-run".into())
			.spawn(move || service.execute(job));
		if let Err(e) = spawned {
			self.inner.runs.remove(&run_id);
			let _ = self.db().with(|conn| {
				store::end_run(
					conn,
					&run_id,
					RunState::Failed,
					"internal",
					"could not start the analysis",
					now_ms(),
				)
			});
			return Err(AppError::Process(format!("could not start the analysis thread: {e}")));
		}
		Ok(StartedRun { run_id })
	}

	/// End a run and its process. Idempotent: a run that is unknown, finished or
	/// already cancelled is not an error. Allowed while the feature is off, since
	/// stopping work is never the dangerous direction.
	pub fn cancel(&self, run_id: &str) {
		if let Some(run) = self.inner.runs.get(run_id) {
			run.cancel();
		}
	}

	/// Cancel every run. Called when the app quits and when the switch goes off.
	pub fn cancel_all(&self) {
		for run in self.inner.runs.iter() {
			run.value().cancel();
		}
	}

	/// Runs whose worker has not finished. For the tests and for the quit path.
	pub fn runs_in_flight(&self) -> usize {
		self.inner.runs.len()
	}

	fn execute(&self, job: Job) {
		let outcome =
			std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.execute_inner(&job)));
		if outcome.is_err() {
			self.finish(&job, RunState::Failed, "internal", "The analysis stopped unexpectedly.");
		}
		job.control.detach();
		self.inner.runs.remove(&job.run_id);
	}

	fn execute_inner(&self, job: &Job) {
		if !self.enabled() {
			self.finish(
				job,
				RunState::Cancelled,
				"disabled",
				"Project Intelligence was switched off.",
			);
			return;
		}
		let mut candidates: Vec<ValidatedCandidate> = Vec::new();
		let mut dropped = 0u32;
		let mut payload_bytes = 0u64;
		let mut analysed = false;
		if job.active {
			self.progress(job, "analyzing");
			let instructions =
				job.include_instructions.then(|| load_instructions(&job.root)).flatten();
			let request = super::provider_request(
				&job.project_id,
				&job.project_name,
				&job.signals,
				instructions,
			);
			payload_bytes = provider::build_payload(&request).len() as u64;
			match self.inner.provider.analyze(&request, &job.control) {
				Ok(output) => {
					self.progress(job, "validating");
					let known: HashMap<String, String> = job
						.signals
						.iter()
						.map(|s| (s.evidence_id.clone(), s.session_id.clone()))
						.collect();
					match analyzer::validate(&output.text, &known) {
						Ok(validation) => {
							candidates = validation.candidates;
							dropped = validation.dropped;
							analysed = true;
						}
						Err(analyzer::InvalidOutput(why)) => {
							self.finish(job, RunState::Failed, "invalid_output", &why);
							return;
						}
					}
				}
				Err(error) => {
					let (state, code, message) = describe(error);
					self.finish(job, state, code, &message);
					return;
				}
			}
			for candidate in &mut candidates {
				if candidate.action_kind == crate::models::intelligence::ActionKind::Instruction {
					candidate.duplicate = duplicate_for(&job.root, &candidate.recommendation);
				}
			}
		}

		self.progress(job, "saving");
		let saved = self.db().with_mut(|conn| {
			store::complete_run(
				conn,
				CompleteArgs {
					run_id: &job.run_id,
					project_id: &job.project_id,
					provider: analysed.then(|| self.inner.provider.id()),
					analysed,
					signals: &job.signals,
					candidates,
					dropped,
					payload_bytes,
					degraded: job.degraded.clone(),
					now: now_ms(),
				},
				// Asked once, immediately before the commit: a cancel that arrives after
				// the answer but before the write still stops it.
				|| !job.control.is_cancelled() && self.enabled(),
			)
		});
		match saved {
			Ok(Some(_)) => {
				self.announce_finished(job);
				self.changed(&job.project_id, None);
			}
			Ok(None) => {
				let (code, message) = if self.enabled() {
					("cancelled", "The analysis was cancelled.")
				} else {
					("disabled", "Project Intelligence was switched off.")
				};
				self.finish(job, RunState::Cancelled, code, message);
			}
			Err(error) => {
				tracing::warn!(%error, "could not save an analysis");
				self.finish(job, RunState::Failed, "internal", "The result could not be saved.");
			}
		}
	}

	fn progress(&self, job: &Job, phase: &str) {
		self.emit(IntelligenceEvent::Progress(IntelligenceProgressEvent {
			run_id: job.run_id.clone(),
			project_id: job.project_id.clone(),
			phase: phase.to_string(),
			sessions_done: job.session_count,
			sessions_total: job.session_count,
			signals: job.signals.len() as u32,
		}));
	}

	/// End a run that did not complete, and say so. A run someone else already
	/// ended keeps the verdict it has.
	fn finish(&self, job: &Job, state: RunState, code: &str, message: &str) {
		let message = redact::excerpt(message, 300);
		let ended = self
			.db()
			.with(|conn| store::end_run(conn, &job.run_id, state, code, &message, now_ms()));
		if let Err(error) = ended {
			tracing::warn!(%error, "could not record how an analysis ended");
		}
		self.announce_finished(job);
	}

	fn announce_finished(&self, job: &Job) {
		let Ok(Some(run)) = self.run_row(&job.run_id) else { return };
		self.emit(IntelligenceEvent::RunFinished(IntelligenceRunFinishedEvent {
			run_id: run.id,
			project_id: run.project_id,
			state: run.state,
			outcome: run.outcome,
			error_code: run.error_code,
			candidate_count: run.candidate_count,
			signal_count: run.signal_count,
		}));
	}
}

/// How a provider failure ends the run.
fn describe(error: ProviderError) -> (RunState, &'static str, String) {
	match error {
		ProviderError::Cancelled => {
			(RunState::Cancelled, "cancelled", "The analysis was cancelled.".into())
		}
		ProviderError::Timeout => (
			RunState::Failed,
			"timeout",
			"The analysis took too long and was stopped. Nothing was saved.".into(),
		),
		ProviderError::OutputTooLarge => (
			RunState::Failed,
			"output_too_large",
			"The analysis answered with far more than it was asked for and was stopped.".into(),
		),
		ProviderError::NotAuthenticated(m) => (RunState::Failed, "provider_unauthenticated", m),
		ProviderError::Unavailable(m) => (RunState::Failed, "provider_unavailable", m),
		ProviderError::Unsafe(m) => (RunState::Failed, "provider_unsafe", m),
		ProviderError::Failed(m) => (RunState::Failed, "provider_failed", m),
	}
}

/// The existing instruction file, bounded and redacted, for the model to read.
fn load_instructions(root: &Path) -> Option<String> {
	let root = std::fs::canonicalize(root).ok()?;
	let target = patch::resolve_target(&root).ok().filter(|t| t.exists)?;
	let text = patch::read_text(&target.path).ok()?;
	Some(super::instruction_excerpt(&text))
}

/// A line in the project's instruction files that already says this.
fn duplicate_for(root: &Path, recommendation: &str) -> Option<DuplicateHint> {
	let root = std::fs::canonicalize(root).ok()?;
	let target = patch::resolve_target(&root).ok()?;
	let mut files = Vec::new();
	if target.exists {
		files.push((target.relative.clone(), patch::read_text(&target.path).ok()?));
	}
	for (relative, path) in &target.also_read {
		if let Ok(text) = patch::read_text(path) {
			files.push((relative.clone(), text));
		}
	}
	patch::find_duplicate(&files, recommendation)
}
