//! Pull requests of `origin`, read through the user's `gh` (F31, ADR-0077).
//!
//! Every command is `async` and awaits `gh` on the runtime; the libgit2 work
//! inside (reading `origin`, a merge base, a local blob) is moved to the
//! blocking pool by the service, as `commands/git.rs` does. Phase one is
//! read-only: no command here writes to GitHub or to the repository.

use crate::error::AppResult;
use crate::models::{FileContents, GhStatus, PrDetail, PrFiles, PrStateFilter, PrSummary};
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
