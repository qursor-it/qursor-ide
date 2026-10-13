//! Pull requests, read through the user's own `gh` (F31, ADR-0077).
//!
//! The app holds no credential and talks to no GitHub endpoint itself. It runs
//! the `gh` the user already installed and signed in, asks it for JSON, and
//! shows what comes back. That is the only reason the panel can exist without
//! an account system, and it is why none of this ever reads, stores or requests
//! the token (`gh auth token` is never run).
//!
//! The shape follows `services::sops`: locate a CLI, run it once, parse its
//! stdout. The modules:
//!
//! - [`run`] — finding `gh`, and a bounded, killable, rate-limited child.
//! - [`repo`] — which repository `origin` is.
//! - [`args`] — every argument list, so one test can hold them all to "names
//!   its repository".
//! - [`parse`] — `gh`'s JSON into the wire models, and its failures into a
//!   classification.
//! - [`patch`] — which lines of a diff a comment may anchor to.
//! - [`review`] — checking a review before it is sent, and its JSON body.
//! - [`blob`] — a file at a commit, from the clone when it is there.
//!
//! **Reads, and one write.** Every call here is a GET except [`submit_review`],
//! which POSTs a review the user confirmed (ADR-0077) and passes it on stdin.
//! Nothing is written to the repository: no `git fetch`, so a fork's head commit
//! is read from GitHub rather than pulled in.

pub mod args;
pub mod blob;
pub mod parse;
pub mod patch;
pub mod repo;
pub mod review;
pub mod run;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::models::{
	FileContents, GhRepo, GhState, GhStatus, PrComment, PrDetail, PrFiles, PrStateFilter,
	PrSummary, ReviewSubmission, SubmittedReview,
};

use run::{Call, Output, BLOB_TIMEOUT, READ_TIMEOUT, STATUS_TIMEOUT};

/// The oldest `gh` this feature drives.
///
/// 2.40 (December 2023) is where `gh pr list --json` gained `statusCheckRollup`
/// in the shape parsed here and where `gh api --hostname` and `--paginate` behave
/// as used. Older releases are reported as `tooOld` rather than trusted to
/// produce JSON this module can read.
pub const MIN_VERSION: (u32, u32, u32) = (2, 40, 0);

/// `pulls/N/files` with patches for thousands of files is megabytes; past this
/// the answer is refused rather than held.
const FILES_OUTPUT_CAP: usize = 64 * 1024 * 1024;

/// Signed-in login per host. A login does not change under a running app often
/// enough to pay a network round trip on every status read; `refresh` clears it.
static LOGINS: LazyLock<Mutex<HashMap<String, String>>> =
	LazyLock::new(|| Mutex::new(HashMap::new()));

/// A review's answer is a few hundred bytes; this is a ceiling, not an estimate.
const REVIEW_OUTPUT_CAP: usize = 1024 * 1024;

/// Longer than a read: GitHub validates every line comment of the review.
const SUBMIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

type MergeBaseKey = (String, String, String);

/// `(repo slug, base, head)` → merge base. Keyed by commits, which never change
/// meaning, so an entry is never stale.
static MERGE_BASES: LazyLock<Mutex<HashMap<MergeBaseKey, String>>> =
	LazyLock::new(|| Mutex::new(HashMap::new()));

// ── input validation ───────────────────────────────────────────────────────

pub fn validate_number(number: u64) -> AppResult<u64> {
	if number == 0 {
		return Err(AppError::InvalidInput("a pull request number starts at 1".into()));
	}
	Ok(number)
}

/// A full, lower-case object id. Short ids and ref names are refused: the
/// renderer is given full ids and anything else is a bug or an attempt to
/// smuggle a ref expression into a URL.
pub fn validate_sha(sha: &str) -> AppResult<()> {
	if sha.len() == 40 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
		return Ok(());
	}
	Err(AppError::InvalidInput(format!("not a full commit id: {sha}")))
}

/// A repository-relative path: not absolute, and no way up out of the tree.
pub fn validate_rel_path(rel_path: &str) -> AppResult<()> {
	let bad = rel_path.is_empty()
		|| rel_path.contains('\0')
		|| rel_path.starts_with(['/', '\\'])
		|| rel_path.as_bytes().get(1) == Some(&b':')
		|| rel_path.split(['/', '\\']).any(|segment| segment == "..");
	if bad {
		return Err(AppError::InvalidInput(format!(
			"not a path inside the repository: {rel_path}"
		)));
	}
	Ok(())
}

// ── plumbing ───────────────────────────────────────────────────────────────

/// Blocking work (libgit2, a PATH lookup that may spawn a login shell) off the
/// async worker threads.
async fn blocking<T, F>(work: F) -> AppResult<T>
where
	F: FnOnce() -> T + Send + 'static,
	T: Send + 'static,
{
	tokio::task::spawn_blocking(work)
		.await
		.map_err(|e| AppError::Process(format!("background task failed: {e}")))
}

/// Everything a call needs: the program, and the repository it is about.
struct Ctx {
	binary: PathBuf,
	repo: GhRepo,
	root: PathBuf,
}

fn not_installed() -> AppError {
	AppError::NotFound(
		"the GitHub CLI (gh) is not installed, or is not on the PATH the app uses".into(),
	)
}

async fn context(project_path: &str) -> AppResult<Ctx> {
	let path = project_path.to_string();
	let (resolved, binary) = blocking(move || (repo::resolve(&path), run::binary(false))).await?;
	let resolved = resolved.map_err(|why| match why {
		repo::Unresolved::NotRepository => {
			AppError::InvalidInput("this folder is not inside a git repository".into())
		}
		repo::Unresolved::NoOrigin => {
			AppError::InvalidInput("this repository has no `origin` remote".into())
		}
		repo::Unresolved::NotGitHub(url) => {
			AppError::InvalidInput(format!("`origin` is not a GitHub repository: {url}"))
		}
	})?;
	let binary = binary.ok_or_else(not_installed)?;
	Ok(Ctx { binary, repo: resolved.repo, root: resolved.root })
}

fn call(args: Vec<String>, timeout: std::time::Duration) -> Call {
	Call { args, timeout: Some(timeout), ..Call::default() }
}

/// Run a read and return its stdout, or the classified failure.
async fn read(
	ctx: &Ctx,
	args: Vec<String>,
	timeout: std::time::Duration,
	what: &str,
) -> AppResult<Vec<u8>> {
	let call = Call { stdout_cap: Some(FILES_OUTPUT_CAP), ..call(args, timeout) };
	let output = run::run(&ctx.binary, call).await?;
	if output.overflowed {
		return Err(AppError::Remote(format!("the answer for {what} was too large to read")));
	}
	succeeded(output, what)
}

fn succeeded(output: Output, what: &str) -> AppResult<Vec<u8>> {
	if output.succeeded() {
		return Ok(output.stdout);
	}
	Err(parse::classify(output.code, &output.stderr).into_app_error(what))
}

fn unreadable(what: &str, error: impl std::fmt::Display) -> AppError {
	AppError::Remote(format!("gh returned {what} this app could not read: {error}"))
}

/// The signed-in login on `repo`'s host: cached, else asked of `gh`. `None` when
/// it cannot be had — the callers degrade (nothing is "requested from you")
/// rather than fail, since the main request reports an auth problem itself.
async fn viewer_login(ctx: &Ctx) -> Option<String> {
	if let Some(login) = LOGINS.lock().get(&ctx.repo.host) {
		return Some(login.clone());
	}
	let out =
		read(ctx, args::viewer(&ctx.repo.host), STATUS_TIMEOUT, "the signed-in user").await.ok()?;
	let login = String::from_utf8_lossy(&out).trim().to_string();
	if login.is_empty() {
		return None;
	}
	LOGINS.lock().insert(ctx.repo.host.clone(), login.clone());
	Some(login)
}

// ── status ─────────────────────────────────────────────────────────────────

fn status_with(state: GhState, message: Option<String>) -> GhStatus {
	GhStatus {
		state,
		binary_path: None,
		version: None,
		login: None,
		repo: None,
		repo_root: None,
		message,
	}
}

/// Can this project show pull requests, and if not, why not. **Never fails**:
/// each problem is a state, so the panel has one empty state to draw.
///
/// The cheap local questions come first (is there a repository, an `origin`, is
/// it GitHub), then the install and version of `gh`, then the one network call —
/// who is signed in. `refresh` re-probes `gh` and forgets the cached login, for
/// the panel's refresh button after the user installs or signs in.
pub async fn status(project_path: &str, refresh: bool) -> GhStatus {
	let path = project_path.to_string();
	let Ok(resolved) = blocking(move || repo::resolve(&path)).await else {
		return status_with(
			GhState::NotRepository,
			Some("Could not inspect this folder.".to_string()),
		);
	};
	let resolved = match resolved {
		Ok(resolved) => resolved,
		Err(repo::Unresolved::NotRepository) => {
			return status_with(
				GhState::NotRepository,
				Some("This folder is not inside a git repository.".to_string()),
			)
		}
		Err(repo::Unresolved::NoOrigin) => return status_with(
			GhState::NoOrigin,
			Some(
				"This repository has no `origin` remote, so there is no pull-request list to read."
					.to_string(),
			),
		),
		Err(repo::Unresolved::NotGitHub(url)) => {
			return status_with(
				GhState::NotGitHub,
				Some(format!("`origin` ({url}) is not a GitHub repository.")),
			)
		}
	};

	let mut status = status_with(GhState::NotInstalled, None);
	status.repo_root = Some(resolved.root.to_string_lossy().into_owned());
	status.repo = Some(resolved.repo.clone());

	let Ok(binary) = blocking(move || run::binary(refresh)).await else {
		return status;
	};
	let Some(binary) = binary else {
		status.message = Some(
			"Install the GitHub CLI (https://cli.github.com), then sign in with `gh auth login`."
				.to_string(),
		);
		return status;
	};
	status.binary_path = Some(binary.to_string_lossy().into_owned());

	let ctx = Ctx { binary, repo: resolved.repo, root: resolved.root };
	let version = match run::cached_version() {
		Some(version) => Some(version),
		None => probe_version(&ctx).await,
	};
	status.version = version.clone();
	if version.as_deref().and_then(parse::parse_version).is_some_and(|v| v < MIN_VERSION) {
		let (major, minor, _) = MIN_VERSION;
		status.state = GhState::TooOld;
		status.message = Some(format!(
			"gh {} is too old — {major}.{minor} or newer is needed. Update it, then refresh.",
			version.unwrap_or_default()
		));
		return status;
	}

	if refresh {
		LOGINS.lock().remove(&ctx.repo.host);
	}
	if let Some(login) = LOGINS.lock().get(&ctx.repo.host) {
		status.state = GhState::Ready;
		status.login = Some(login.clone());
		return status;
	}
	let output =
		match run::run(&ctx.binary, call(args::viewer(&ctx.repo.host), STATUS_TIMEOUT)).await {
			Ok(output) => output,
			Err(error) => {
				status.state = GhState::Unreachable;
				status.message = Some(error.to_string());
				return status;
			}
		};
	if output.succeeded() {
		let login = String::from_utf8_lossy(&output.stdout).trim().to_string();
		LOGINS.lock().insert(ctx.repo.host.clone(), login.clone());
		status.state = GhState::Ready;
		status.login = Some(login);
		return status;
	}
	match parse::classify(output.code, &output.stderr) {
		parse::GhFailure::AuthRequired => {
			status.state = GhState::NotAuthenticated;
			status.message = Some(format!(
				"gh is not signed in to {}. Run `gh auth login --hostname {}`, then refresh.",
				ctx.repo.host, ctx.repo.host
			));
		}
		other => {
			status.state = GhState::Unreachable;
			status.message = Some(match other {
				parse::GhFailure::Network => format!("Could not reach {}.", ctx.repo.host),
				other => describe(&other),
			});
		}
	}
	status
}

/// `gh --version`, remembered with the binary it came from.
async fn probe_version(ctx: &Ctx) -> Option<String> {
	let output = run::run(&ctx.binary, call(args::version(), STATUS_TIMEOUT)).await.ok()?;
	if !output.succeeded() {
		return None;
	}
	let version = parse::parse_version_line(&String::from_utf8_lossy(&output.stdout))?;
	run::remember_version(&version);
	Some(version)
}

/// A failure as one sentence, for the status message.
fn describe(failure: &parse::GhFailure) -> String {
	match failure.clone().into_app_error("the request") {
		AppError::Remote(message)
		| AppError::NotFound(message)
		| AppError::Unauthenticated(message) => message,
		other => other.to_string(),
	}
}

// ── reads ──────────────────────────────────────────────────────────────────

/// The newest pull requests of `origin`, 50 at most.
pub async fn pr_list(project_path: &str, state: PrStateFilter) -> AppResult<Vec<PrSummary>> {
	let ctx = context(project_path).await?;
	let (out, login) = tokio::join!(
		read(&ctx, args::pr_list(&ctx.repo, state), READ_TIMEOUT, "the pull request list"),
		viewer_login(&ctx),
	);
	let raw = parse::parse_pr_list(&out?).map_err(|e| unreadable("a pull request list", e))?;
	Ok(raw.iter().map(|pr| parse::summary(pr, login.as_deref())).collect())
}

/// One pull request in full, with the merge base its diff is taken against.
pub async fn pr_detail(project_path: &str, number: u64) -> AppResult<PrDetail> {
	let number = validate_number(number)?;
	let ctx = context(project_path).await?;
	let what = format!("pull request #{number}");
	let (out, login) = tokio::join!(
		read(&ctx, args::pr_view(&ctx.repo, number), READ_TIMEOUT, &what),
		viewer_login(&ctx)
	);
	let raw = parse::parse_pr(&out?).map_err(|e| unreadable(&what, e))?;

	let base = raw.base_oid().to_string();
	let head = raw.head_oid().to_string();
	validate_sha(&base)
		.and(validate_sha(&head))
		.map_err(|_| AppError::Remote(format!("GitHub did not report the commits of {what}")))?;
	let merge_base = merge_base(&ctx, project_path, &base, &head).await?;
	Ok(parse::detail(&raw, login.as_deref(), merge_base))
}

/// The files of one pull request, with the lines each can be commented on.
pub async fn pr_files(project_path: &str, number: u64) -> AppResult<PrFiles> {
	let number = validate_number(number)?;
	let ctx = context(project_path).await?;
	let what = format!("the files of pull request #{number}");
	let (files, total) = tokio::try_join!(
		read(&ctx, args::pr_files(&ctx.repo, number), READ_TIMEOUT, &what),
		read(&ctx, args::pr_changed_files(&ctx.repo, number), READ_TIMEOUT, &what),
	)?;
	let raw: Vec<parse::RawFile> = parse::parse_pages(&files).map_err(|e| unreadable(&what, e))?;
	let files: Vec<_> = raw.iter().map(|f| parse::map_file(f, &ctx.root)).collect();
	let total = String::from_utf8_lossy(&total)
		.trim()
		.parse::<u64>()
		.unwrap_or(files.len() as u64)
		.max(files.len() as u64);
	// GitHub stops listing at 3000 files while the pull request still counts
	// them all, so "the count is larger than the list" is the whole test.
	let truncated = total > files.len() as u64;
	Ok(PrFiles { files, total, truncated })
}

/// The inline review comments already on a pull request, oldest first as GitHub
/// lists them.
pub async fn pr_comments(project_path: &str, number: u64) -> AppResult<Vec<PrComment>> {
	let number = validate_number(number)?;
	let ctx = context(project_path).await?;
	let what = format!("the review comments of pull request #{number}");
	let out = read(&ctx, args::pr_comments(&ctx.repo, number), READ_TIMEOUT, &what).await?;
	let raw: Vec<parse::RawComment> = parse::parse_pages(&out).map_err(|e| unreadable(&what, e))?;
	Ok(raw.iter().map(parse::map_comment).collect())
}

/// Publish a review. **The one write to GitHub** (ADR-0077): validated here,
/// sent as JSON on stdin, never retried — a second POST would be a second
/// review.
pub async fn submit_review(
	project_path: &str,
	number: u64,
	review: &ReviewSubmission,
) -> AppResult<SubmittedReview> {
	let number = validate_number(number)?;
	review::validate(review)?;
	let body = review::payload(review)?;
	let ctx = context(project_path).await?;
	let what = format!("pull request #{number}");
	let call = Call {
		args: args::submit_review(&ctx.repo, number),
		stdin: Some(body),
		stdout_cap: Some(REVIEW_OUTPUT_CAP),
		timeout: Some(SUBMIT_TIMEOUT),
	};
	let output = run::run(&ctx.binary, call).await.map_err(|e| match e {
		// The request may have reached GitHub before the wait ran out.
		AppError::Remote(message) => AppError::Remote(format!(
			"{message}; the review may have been published, so check the pull request before submitting again"
		)),
		other => other,
	})?;
	if output.overflowed {
		return Err(unreadable("the review's answer", "an answer far larger than expected"));
	}
	if !output.succeeded() {
		let mut failure = parse::classify(output.code, &output.stderr);
		// `gh` prints GitHub's reasons to stdout; add the ones stderr left out.
		if let parse::GhFailure::Validation(message) = &mut failure {
			if let Some(detail) = review::validation_detail(&output.stdout) {
				if !message.contains(&detail) {
					*message = format!("{message} — {detail}");
				}
			}
		}
		return Err(failure.into_app_error(&what));
	}
	parse::parse_submitted(&output.stdout).map_err(|e| unreadable("the published review", e))
}

/// One file at one commit of a pull request: from the clone when the commit is
/// there, from GitHub when it is not (a fork's head). `None` when the file does
/// not exist at that commit.
pub async fn pr_blob(
	project_path: &str,
	rel_path: &str,
	sha: &str,
	max_bytes: Option<usize>,
) -> AppResult<Option<FileContents>> {
	validate_rel_path(rel_path)?;
	validate_sha(sha)?;

	let (path, rel, commit) = (project_path.to_string(), rel_path.to_string(), sha.to_string());
	match blocking(move || blob::local(&path, &rel, &commit, max_bytes)).await? {
		blob::Local::Found(contents) => return Ok(Some(contents)),
		blob::Local::Absent => return Ok(None),
		blob::Local::NoCommit => {}
	}

	let ctx = context(project_path).await?;
	let cap = max_bytes.unwrap_or(crate::services::files::DEFAULT_MAX_BYTES);
	let call = Call {
		args: blob::contents_args(&ctx.repo, rel_path, sha),
		stdout_cap: Some(cap),
		timeout: Some(BLOB_TIMEOUT),
		..Call::default()
	};
	let output = run::run(&ctx.binary, call).await?;
	blob::from_remote(rel_path, &output, max_bytes)
}

// ── merge base ─────────────────────────────────────────────────────────────

/// The merge base of a pull request's base and head.
///
/// From the clone when both commits are in it (free, offline); from GitHub's
/// compare endpoint otherwise. Remembered either way.
async fn merge_base(ctx: &Ctx, project_path: &str, base: &str, head: &str) -> AppResult<String> {
	let key = (ctx.repo.slug.clone(), base.to_string(), head.to_string());
	if let Some(found) = MERGE_BASES.lock().get(&key) {
		return Ok(found.clone());
	}

	let (path, b, h) = (project_path.to_string(), base.to_string(), head.to_string());
	let found = match blocking(move || local_merge_base(&path, &b, &h)).await? {
		Some(oid) => oid,
		None => {
			let what = "the merge base of this pull request";
			let out = read(ctx, args::compare(&ctx.repo, base, head), READ_TIMEOUT, what).await?;
			let oid = String::from_utf8_lossy(&out).trim().to_string();
			validate_sha(&oid).map_err(|_| unreadable(what, "an unexpected answer"))?;
			oid
		}
	};
	MERGE_BASES.lock().insert(key, found.clone());
	Ok(found)
}

fn local_merge_base(project_path: &str, base: &str, head: &str) -> Option<String> {
	let repository = crate::services::git::discover(project_path)?;
	let base = git2::Oid::from_str(base).ok()?;
	let head = git2::Oid::from_str(head).ok()?;
	// Both commits must be here: `merge_base` on a missing one is an error, and
	// a shallow clone can hold both yet lack their common ancestor.
	repository.find_commit(base).ok()?;
	repository.find_commit(head).ok()?;
	repository.merge_base(base, head).ok().map(|oid| oid.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn inputs_are_validated_before_they_reach_an_argv() {
		assert!(validate_number(1).is_ok());
		assert!(validate_number(0).is_err());

		let sha = "0123456789abcdef0123456789abcdef01234567";
		assert!(validate_sha(sha).is_ok());
		for bad in [
			"",
			"main",
			"0123456",
			&sha.to_uppercase(),
			&format!("{sha}0"),
			"0123456789abcdef0123456789abcdef0123456g",
			"HEAD~1                                  ",
		] {
			assert!(validate_sha(bad).is_err(), "{bad:?}");
		}

		for good in ["README.md", "src/a/b.ts", "dir with space/f g.txt", "a..b/c", ".github/x.yml"]
		{
			assert!(validate_rel_path(good).is_ok(), "{good:?}");
		}
		for bad in
			["", "/etc/passwd", "\\windows", "C:/x", "../x", "a/../b", "a\\..\\b", "a/..", "a\0b"]
		{
			assert!(validate_rel_path(bad).is_err(), "{bad:?}");
		}
	}

	#[tokio::test]
	async fn status_names_the_local_problems_without_touching_gh() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().to_string_lossy().into_owned();
		assert_eq!(status(&path, false).await.state, GhState::NotRepository);

		let repository = git2::Repository::init(dir.path()).unwrap();
		let no_origin = status(&path, false).await;
		assert_eq!(no_origin.state, GhState::NoOrigin);
		assert!(no_origin.repo.is_none());

		repository.remote("origin", "https://gitlab.com/o/r.git").unwrap();
		let gitlab = status(&path, false).await;
		assert_eq!(gitlab.state, GhState::NotGitHub);
		assert!(gitlab.message.unwrap().contains("gitlab.com"));
	}

	#[tokio::test]
	async fn commands_refuse_a_folder_that_is_not_a_github_repository() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().to_string_lossy().into_owned();
		assert!(matches!(
			pr_list(&path, PrStateFilter::Open).await,
			Err(AppError::InvalidInput(_))
		));
		assert!(matches!(pr_detail(&path, 0).await, Err(AppError::InvalidInput(_))));
		assert!(matches!(pr_files(&path, 1).await, Err(AppError::InvalidInput(_))));
		assert!(matches!(pr_comments(&path, 1).await, Err(AppError::InvalidInput(_))));
		let review = ReviewSubmission {
			commit_oid: "0123456789abcdef0123456789abcdef01234567".into(),
			event: crate::models::ReviewEvent::Approve,
			body: String::new(),
			comments: vec![],
		};
		assert!(matches!(submit_review(&path, 1, &review).await, Err(AppError::InvalidInput(_))));
		// Refused by validation before any repository is looked at.
		let empty = ReviewSubmission { event: crate::models::ReviewEvent::Comment, ..review };
		let Err(AppError::InvalidInput(message)) = submit_review(&path, 1, &empty).await else {
			panic!("expected a refusal");
		};
		assert!(message.contains("only an approval may be empty"), "{message}");
	}

	#[test]
	fn a_merge_base_is_found_in_the_clone() {
		let dir = tempfile::tempdir().unwrap();
		let repository = git2::Repository::init(dir.path()).unwrap();
		let sig = git2::Signature::now("t", "t@example.com").unwrap();
		let tree =
			repository.find_tree(repository.treebuilder(None).unwrap().write().unwrap()).unwrap();
		let root = repository.commit(None, &sig, &sig, "root", &tree, &[]).unwrap();
		let root_commit = repository.find_commit(root).unwrap();
		let a = repository.commit(None, &sig, &sig, "a", &tree, &[&root_commit]).unwrap();
		let b = repository.commit(None, &sig, &sig, "b", &tree, &[&root_commit]).unwrap();

		let path = dir.path().to_string_lossy().into_owned();
		assert_eq!(
			local_merge_base(&path, &a.to_string(), &b.to_string()).as_deref(),
			Some(root.to_string().as_str())
		);
		// One side missing: the network has to answer.
		let missing = "0123456789abcdef0123456789abcdef01234567";
		assert_eq!(local_merge_base(&path, &a.to_string(), missing), None);
	}
}
