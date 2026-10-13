//! Pull requests of `origin`, read through the user's `gh` (F31, ADR-0077).
//!
//! Every command is `async` and awaits `gh` on the runtime; the libgit2 work
//! inside (reading `origin`, a merge base, a local blob) is moved to the
//! blocking pool by the service, as `commands/git.rs` does. Every command reads,
//! except `gh_pr_submit_review`: the one write to GitHub (ADR-0077), a review
//! the user confirmed. Only `gh_pr_checkout` writes to the repository, by adding
//! a worktree (ADR-0079).

use crate::error::AppResult;
use crate::models::{
	FileContents, GhStatus, GitWorktree, PrCheckoutPlan, PrComment, PrDetail, PrFiles,
	PrStateFilter, PrSummary, ReviewSubmission, SubmittedReview,
};
use crate::services::gh;

/// Can this project show pull requests — and if not, which of eight reasons.
///
/// **Never fails**: a missing `gh`, no login or no `origin` is a `state`, so the
/// panel draws one empty state naming the fix. `refresh` re-probes `gh` and
/// forgets the cached login; the panel's refresh button sets it.
#[tauri::command]
pub async fn gh_status(project_path: String, refresh: Option<bool>) -> GhStatus {
	gh::status(&project_path, refresh.unwrap_or(false)).await
}

/// The newest 50 pull requests of `origin` in the given state.
#[tauri::command]
pub async fn gh_pr_list(project_path: String, state: PrStateFilter) -> AppResult<Vec<PrSummary>> {
	gh::pr_list(&project_path, state).await
}

/// One pull request in full: body, checks, reviews, and the merge base its diff
/// is taken against.
#[tauri::command]
pub async fn gh_pr_detail(project_path: String, number: u64) -> AppResult<PrDetail> {
	gh::pr_detail(&project_path, number).await
}

/// The changed files of one pull request, each with the lines a review comment
/// may anchor to. The patch text stays in Rust.
#[tauri::command]
pub async fn gh_pr_files(project_path: String, number: u64) -> AppResult<PrFiles> {
	gh::pr_files(&project_path, number).await
}

/// One file at one commit of a pull request: the clone's copy when the commit is
/// there, GitHub's when it is not. `None` when the file does not exist at that
/// commit — a file the pull request adds has no base side.
#[tauri::command]
pub async fn gh_pr_blob(
	project_path: String,
	rel_path: String,
	sha: String,
	max_bytes: Option<usize>,
) -> AppResult<Option<FileContents>> {
	gh::pr_blob(&project_path, &rel_path, &sha, max_bytes).await
}

/// The inline review comments already on a pull request.
#[tauri::command]
pub async fn gh_pr_comments(project_path: String, number: u64) -> AppResult<Vec<PrComment>> {
	gh::pr_comments(&project_path, number).await
}

/// Publish a review: line comments, a general comment and a verdict. **The only
/// write to GitHub** (ADR-0077); the renderer calls it only after the user
/// confirmed. Never retried.
#[tauri::command]
pub async fn gh_pr_submit_review(
	project_path: String,
	number: u64,
	review: ReviewSubmission,
) -> AppResult<SubmittedReview> {
	gh::submit_review(&project_path, number, &review).await
}

/// What checking a pull request out would do: where the worktree would go, whether
/// `pr/<n>` is taken, whether the head is a fork's. Writes nothing.
#[tauri::command]
pub async fn gh_pr_checkout_plan(project_path: String, number: u64) -> AppResult<PrCheckoutPlan> {
	gh::checkout::plan(&project_path, number).await
}

/// Fetch a pull request's head and add a new linked worktree for it on `pr/<n>`
/// (or detached). **Never touches an existing checkout, its index or its HEAD**
/// (ADR-0079); the renderer calls it only after the user confirmed. Returns the
/// existing worktree, untouched, when the pull request already has one.
#[tauri::command]
pub async fn gh_pr_checkout(
	project_path: String,
	number: u64,
	head_oid: String,
	detached: bool,
) -> AppResult<GitWorktree> {
	gh::checkout::checkout(&project_path, number, &head_oid, detached).await
}
