//! What `gh` prints, and what it means when it fails (F31).
//!
//! Three jobs, all pure functions over text so they are tested against captured
//! output rather than against a network:
//!
//! - [`classify`] turns an exit code and stderr into a [`GhFailure`] the caller
//!   maps onto `AppError`. **The exit code classifies where it can, the text
//!   where it must** — `gh` documents exit 4 for "authentication required" and
//!   nothing else of use, so the rest is read from the words, which are
//!   GitHub's and `gh`'s and stable enough for a banner.
//! - The serde structs mirror `gh pr … --json` and the REST `pulls/N/files`
//!   payload, and are lenient: a missing field is a default, because a field
//!   `gh` adds or drops between releases must not blank the panel.
//! - The mappers produce the wire models in `models/mod.rs`.

use std::path::Path;

use serde::Deserialize;

use crate::error::AppError;
use crate::models::{
	CheckState, GitChangeKind, PrCheck, PrChecksState, PrChecksSummary, PrDetail, PrFile,
	PrFileStatus, PrMergeable, PrReview, PrReviewDecision, PrReviewState, PrState, PrSummary,
};

/// Why a `gh` call failed, in the terms the panel acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhFailure {
	/// No login (exit 4, a 401, or `gh` telling the user to run `gh auth login`).
	AuthRequired,
	/// The repository, pull request or file does not exist — or is not visible
	/// to this login, which GitHub reports identically.
	NotFound,
	RateLimited,
	Network,
	/// An organisation's SAML enforcement refused the token.
	Forbidden,
	/// A 422. Carries GitHub's own sentence.
	Validation(String),
	/// Anything else, as the first line `gh` printed.
	Other(String),
}

/// The longest stderr line kept for a banner.
const MESSAGE_CAP: usize = 400;

pub fn classify(exit_code: Option<i32>, stderr: &str) -> GhFailure {
	if exit_code == Some(4) {
		return GhFailure::AuthRequired;
	}
	let lower = stderr.to_ascii_lowercase();
	let has = |needle: &str| lower.contains(needle);
	// SAML first: its 403 also mentions the token and sometimes a rate limit.
	if has("saml") {
		GhFailure::Forbidden
	} else if has("http 401") || has("bad credentials") || has("gh auth login") {
		GhFailure::AuthRequired
	} else if has("rate limit") || has("http 429") {
		GhFailure::RateLimited
	} else if has("http 404") || has("could not resolve to a") {
		GhFailure::NotFound
	} else if has("http 422") {
		GhFailure::Validation(first_lines(stderr, 3))
	} else if has("error connecting to")
		|| has("no such host")
		|| has("dial tcp")
		|| has("timeout")
		|| has("timed out")
		|| has("i/o timeout")
		|| has("connection refused")
		|| has("network is unreachable")
	{
		GhFailure::Network
	} else {
		GhFailure::Other(first_line(stderr))
	}
}

/// Up to `count` non-empty lines on one line. A 422 puts the reason ("line must
/// be part of the diff") after the headline, and the reason is the useful half.
fn first_lines(stderr: &str, count: usize) -> String {
	let lines: Vec<&str> = stderr
		.lines()
		.map(str::trim)
		.filter(|l| !l.is_empty())
		.take(count)
		.map(|l| l.strip_prefix("gh: ").unwrap_or(l))
		.collect();
	if lines.is_empty() {
		return "gh failed".to_string();
	}
	let joined = lines.join(" — ");
	if joined.chars().count() <= MESSAGE_CAP {
		return joined;
	}
	let cut: String = joined.chars().take(MESSAGE_CAP).collect();
	format!("{cut}…")
}

/// The first non-empty line, without `gh`'s `gh: ` prefix, capped for a banner.
fn first_line(stderr: &str) -> String {
	let line = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("gh failed");
	let line = line.strip_prefix("gh: ").unwrap_or(line);
	if line.chars().count() <= MESSAGE_CAP {
		return line.to_string();
	}
	let cut: String = line.chars().take(MESSAGE_CAP).collect();
	format!("{cut}…")
}

impl GhFailure {
	/// The error the command returns. `what` names the thing asked for, so a 404
	/// reads "pull request #7 was not found" and not "not found".
	pub fn into_app_error(self, what: &str) -> AppError {
		match self {
			GhFailure::AuthRequired => AppError::Unauthenticated(
				"gh has no usable login for this host — run `gh auth login`".into(),
			),
			GhFailure::NotFound => AppError::NotFound(format!(
				"{what} was not found on GitHub, or this login cannot see it"
			)),
			GhFailure::RateLimited => {
				AppError::Remote("GitHub's rate limit was reached — try again in a few minutes".into())
			}
			GhFailure::Network => AppError::Remote("could not reach GitHub".into()),
			GhFailure::Forbidden => AppError::Remote(
				"the organisation requires SAML single sign-on for this token — authorise it with `gh auth refresh`"
					.into(),
			),
			GhFailure::Validation(message) => AppError::Remote(message),
			GhFailure::Other(message) => AppError::Remote(message),
		}
	}
}

// ── versions ───────────────────────────────────────────────────────────────

/// `gh version 2.101.0 (2026-09-15)` → `2.101.0`.
pub fn parse_version_line(stdout: &str) -> Option<String> {
	let first = stdout.lines().find(|l| !l.trim().is_empty())?;
	let rest = first.trim().strip_prefix("gh version ")?;
	let word = rest.split_whitespace().next()?;
	parse_version(word)?;
	Some(word.to_string())
}

/// `2.101.0` → `(2, 101, 0)`; a suffix on the patch (`0-rc1`) is dropped.
pub fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
	let mut parts = version.trim_start_matches('v').split('.');
	let major = parts.next()?.parse().ok()?;
	let minor = parts.next()?.parse().ok()?;
	let patch_word = parts.next().unwrap_or("0");
	let digits: String = patch_word.chars().take_while(char::is_ascii_digit).collect();
	Some((major, minor, digits.parse().unwrap_or(0)))
}

// ── JSON ───────────────────────────────────────────────────────────────────

/// Every element of `--paginate` output.
///
/// `gh api --paginate` prints one JSON array per page and does not join them
/// (`--slurp` would, from 2.48 on, which is above the floor), so the stream is
/// `[…][…]`. A streaming deserializer reads that as it is.
pub fn parse_pages<T: for<'de> Deserialize<'de>>(
	bytes: &[u8],
) -> Result<Vec<T>, serde_json::Error> {
	let mut all = Vec::new();
	for page in serde_json::Deserializer::from_slice(bytes).into_iter::<Vec<T>>() {
		all.extend(page?);
	}
	Ok(all)
}

/// `null` where a list or string is expected is "none", not an error.
fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
	D: serde::Deserializer<'de>,
	T: Default + Deserialize<'de>,
{
	Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Default, Deserialize)]
struct RawActor {
	#[serde(default)]
	login: Option<String>,
	#[serde(default)]
	is_bot: bool,
}

#[derive(Debug, Default, Deserialize)]
struct RawOwner {
	#[serde(default)]
	login: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRequest {
	#[serde(default, rename = "__typename")]
	typename: String,
	#[serde(default)]
	login: Option<String>,
	#[serde(default)]
	slug: Option<String>,
	#[serde(default)]
	name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawCheck {
	#[serde(default, rename = "__typename")]
	typename: String,
	// CheckRun
	#[serde(default)]
	name: Option<String>,
	#[serde(default)]
	workflow_name: Option<String>,
	#[serde(default)]
	status: Option<String>,
	#[serde(default)]
	conclusion: Option<String>,
	#[serde(default)]
	details_url: Option<String>,
	#[serde(default)]
	completed_at: Option<String>,
	// StatusContext
	#[serde(default)]
	context: Option<String>,
	#[serde(default)]
	state: Option<String>,
	#[serde(default)]
	target_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawCommit {
	#[serde(default)]
	oid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawReview {
	#[serde(default)]
	author: Option<RawActor>,
	#[serde(default)]
	state: String,
	#[serde(default)]
	submitted_at: Option<String>,
	#[serde(default)]
	body: Option<String>,
	#[serde(default)]
	commit: Option<RawCommit>,
}

#[derive(Debug, Default, Deserialize)]
struct RawLabel {
	#[serde(default)]
	name: String,
}

/// `gh pr list --json …` and `gh pr view --json …`: one shape, the detail asking
/// for more fields.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawPr {
	number: u64,
	#[serde(default)]
	title: String,
	/// `null` for a deleted account on some `gh` versions; `ghost` on others.
	#[serde(default)]
	author: Option<RawActor>,
	#[serde(default)]
	state: String,
	#[serde(default)]
	is_draft: bool,
	#[serde(default)]
	base_ref_name: String,
	#[serde(default)]
	head_ref_name: String,
	#[serde(default)]
	head_ref_oid: String,
	#[serde(default)]
	is_cross_repository: bool,
	/// `null` when the fork the branch lived in was deleted.
	#[serde(default)]
	head_repository_owner: Option<RawOwner>,
	#[serde(default)]
	created_at: String,
	#[serde(default)]
	updated_at: String,
	#[serde(default)]
	review_decision: Option<String>,
	#[serde(default)]
	status_check_rollup: Option<Vec<RawCheck>>,
	#[serde(default)]
	additions: u64,
	#[serde(default)]
	deletions: u64,
	#[serde(default)]
	changed_files: u64,
	#[serde(default)]
	url: String,
	#[serde(default, deserialize_with = "null_default")]
	review_requests: Vec<RawRequest>,
	// Detail only.
	#[serde(default)]
	body: Option<String>,
	#[serde(default)]
	base_ref_oid: Option<String>,
	#[serde(default)]
	mergeable: Option<String>,
	#[serde(default, deserialize_with = "null_default")]
	latest_reviews: Vec<RawReview>,
	#[serde(default, deserialize_with = "null_default")]
	labels: Vec<RawLabel>,
}

impl RawPr {
	pub fn base_oid(&self) -> &str {
		self.base_ref_oid.as_deref().unwrap_or("")
	}

	pub fn head_oid(&self) -> &str {
		&self.head_ref_oid
	}
}

/// The fields `gh pr list` is asked for.
pub const LIST_FIELDS: &str = "number,title,author,state,isDraft,baseRefName,headRefName,headRefOid,isCrossRepository,headRepositoryOwner,createdAt,updatedAt,reviewDecision,statusCheckRollup,additions,deletions,changedFiles,url,reviewRequests";
/// …and `gh pr view`, which adds what only the detail shows.
pub const VIEW_FIELDS: &str = "number,title,author,state,isDraft,baseRefName,headRefName,headRefOid,isCrossRepository,headRepositoryOwner,createdAt,updatedAt,reviewDecision,statusCheckRollup,additions,deletions,changedFiles,url,reviewRequests,body,baseRefOid,mergeable,latestReviews,labels";

pub fn parse_pr_list(bytes: &[u8]) -> Result<Vec<RawPr>, serde_json::Error> {
	serde_json::from_slice(bytes)
}

pub fn parse_pr(bytes: &[u8]) -> Result<RawPr, serde_json::Error> {
	serde_json::from_slice(bytes)
}

/// ISO 8601 → epoch ms. A value that does not parse is `0` rather than an error:
/// a row with a wrong date beats a panel that shows nothing.
pub fn epoch_ms(iso: &str) -> i64 {
	chrono::DateTime::parse_from_rfc3339(iso).map(|d| d.timestamp_millis()).unwrap_or(0)
}

fn epoch_ms_opt(iso: Option<&str>) -> Option<i64> {
	iso.filter(|s| !s.is_empty()).map(epoch_ms).filter(|ms| *ms > 0)
}

/// An app's login arrives as `app/dependabot`; the prefix is `gh`'s, not a name.
fn display_login(actor: Option<&RawActor>) -> (String, bool) {
	match actor {
		Some(RawActor { login: Some(login), is_bot }) if !login.is_empty() => {
			(login.strip_prefix("app/").unwrap_or(login).to_string(), *is_bot)
		}
		_ => ("ghost".to_string(), false),
	}
}

fn pr_state(raw: &str) -> PrState {
	match raw.to_ascii_uppercase().as_str() {
		"MERGED" => PrState::Merged,
		"CLOSED" => PrState::Closed,
		_ => PrState::Open,
	}
}

fn decision(raw: Option<&str>) -> Option<PrReviewDecision> {
	match raw?.to_ascii_uppercase().as_str() {
		"APPROVED" => Some(PrReviewDecision::Approved),
		"CHANGES_REQUESTED" => Some(PrReviewDecision::ChangesRequested),
		"REVIEW_REQUIRED" => Some(PrReviewDecision::ReviewRequired),
		_ => None,
	}
}

/// One entry of `statusCheckRollup` → a check, whichever of the two kinds it is.
///
/// **The rollup mixes two GraphQL types.** A `CheckRun` (Actions and most apps)
/// has `status` and `conclusion`; a `StatusContext` (the older commit-status API)
/// has a single `state`. Both become one `CheckState`.
fn map_check(raw: &RawCheck) -> PrCheck {
	if raw.typename == "StatusContext" || (raw.context.is_some() && raw.name.is_none()) {
		let state = match raw.state.as_deref().unwrap_or("").to_ascii_uppercase().as_str() {
			"SUCCESS" => CheckState::Success,
			"FAILURE" | "ERROR" => CheckState::Failure,
			_ => CheckState::Pending,
		};
		return PrCheck {
			name: raw.context.clone().unwrap_or_else(|| "status".to_string()),
			workflow: None,
			state,
			url: raw.target_url.clone().filter(|u| !u.is_empty()),
			completed_at: None,
		};
	}

	let completed = raw.status.as_deref().unwrap_or("").eq_ignore_ascii_case("COMPLETED");
	let state = if !completed {
		CheckState::Pending
	} else {
		match raw.conclusion.as_deref().unwrap_or("").to_ascii_uppercase().as_str() {
			"SUCCESS" => CheckState::Success,
			// `ACTION_REQUIRED` waits on a person and will not complete by itself,
			// so it blocks like a failure rather than spinning as pending.
			"FAILURE" | "TIMED_OUT" | "STARTUP_FAILURE" | "ACTION_REQUIRED" => CheckState::Failure,
			"CANCELLED" => CheckState::Cancelled,
			"SKIPPED" => CheckState::Skipped,
			"NEUTRAL" | "STALE" => CheckState::Neutral,
			_ => CheckState::Pending,
		}
	};
	PrCheck {
		name: raw.name.clone().unwrap_or_default(),
		workflow: raw.workflow_name.clone().filter(|w| !w.is_empty()),
		state,
		url: raw.details_url.clone().filter(|u| !u.is_empty()),
		completed_at: epoch_ms_opt(raw.completed_at.as_deref()),
	}
}

pub fn map_checks(raw: &[RawCheck]) -> Vec<PrCheck> {
	raw.iter().map(map_check).collect()
}

/// The list row's rollup. Pending beats nothing, failure beats pending: a red
/// check is the news even while another still runs.
pub fn summarize_checks(checks: &[PrCheck]) -> PrChecksSummary {
	let (mut passed, mut failed, mut pending) = (0u32, 0u32, 0u32);
	for check in checks {
		match check.state {
			CheckState::Success | CheckState::Skipped | CheckState::Neutral => passed += 1,
			CheckState::Failure | CheckState::Cancelled => failed += 1,
			CheckState::Pending => pending += 1,
		}
	}
	let total = checks.len() as u32;
	let state = if total == 0 {
		PrChecksState::None
	} else if failed > 0 {
		PrChecksState::Failure
	} else if pending > 0 {
		PrChecksState::Pending
	} else {
		PrChecksState::Success
	};
	PrChecksSummary { state, total, passed, failed, pending }
}

fn requested_users(requests: &[RawRequest]) -> impl Iterator<Item = &str> {
	requests
		.iter()
		.filter(|r| r.typename != "Team")
		.filter_map(|r| r.login.as_deref())
		.filter(|l| !l.is_empty())
}

pub fn summary(raw: &RawPr, viewer: Option<&str>) -> PrSummary {
	let (author, author_is_bot) = display_login(raw.author.as_ref());
	let checks = map_checks(raw.status_check_rollup.as_deref().unwrap_or(&[]));
	let requested = viewer
		.is_some_and(|v| requested_users(&raw.review_requests).any(|u| u.eq_ignore_ascii_case(v)));
	PrSummary {
		number: raw.number,
		title: raw.title.clone(),
		author,
		author_is_bot,
		state: pr_state(&raw.state),
		is_draft: raw.is_draft,
		base_ref_name: raw.base_ref_name.clone(),
		head_ref_name: raw.head_ref_name.clone(),
		head_ref_oid: raw.head_ref_oid.clone(),
		is_cross_repository: raw.is_cross_repository,
		head_owner: raw.head_repository_owner.as_ref().and_then(|o| o.login.clone()),
		created_at: epoch_ms(&raw.created_at),
		updated_at: epoch_ms(&raw.updated_at),
		review_decision: decision(raw.review_decision.as_deref()),
		checks: summarize_checks(&checks),
		additions: raw.additions,
		deletions: raw.deletions,
		changed_files: raw.changed_files,
		url: raw.url.clone(),
		review_requested_from_viewer: requested,
	}
}

fn review_state(raw: &str) -> PrReviewState {
	match raw.to_ascii_uppercase().as_str() {
		"APPROVED" => PrReviewState::Approved,
		"CHANGES_REQUESTED" => PrReviewState::ChangesRequested,
		"DISMISSED" => PrReviewState::Dismissed,
		"PENDING" => PrReviewState::Pending,
		_ => PrReviewState::Commented,
	}
}

fn mergeable(raw: Option<&str>) -> PrMergeable {
	match raw.unwrap_or("").to_ascii_uppercase().as_str() {
		"MERGEABLE" => PrMergeable::Mergeable,
		"CONFLICTING" => PrMergeable::Conflicting,
		_ => PrMergeable::Unknown,
	}
}

/// The detail, given the merge base the caller resolved separately.
pub fn detail(raw: &RawPr, viewer: Option<&str>, merge_base_oid: String) -> PrDetail {
	let (author, _) = display_login(raw.author.as_ref());
	let viewer_is_author = viewer.is_some_and(|v| author.eq_ignore_ascii_case(v));
	let mut review_requests: Vec<String> = Vec::new();
	for request in &raw.review_requests {
		if request.typename == "Team" {
			if let Some(slug) = request.slug.as_deref().or(request.name.as_deref()) {
				review_requests.push(format!("@{slug}"));
			}
		} else if let Some(login) = request.login.as_deref().filter(|l| !l.is_empty()) {
			review_requests.push(login.to_string());
		}
	}
	PrDetail {
		summary: summary(raw, viewer),
		body: raw.body.clone().unwrap_or_default(),
		base_ref_oid: raw.base_ref_oid.clone().unwrap_or_default(),
		merge_base_oid,
		mergeable: mergeable(raw.mergeable.as_deref()),
		check_runs: map_checks(raw.status_check_rollup.as_deref().unwrap_or(&[])),
		latest_reviews: raw
			.latest_reviews
			.iter()
			.map(|r| PrReview {
				author: display_login(r.author.as_ref()).0,
				state: review_state(&r.state),
				submitted_at: epoch_ms_opt(r.submitted_at.as_deref()),
				body: r.body.clone().unwrap_or_default(),
				commit_oid: r.commit.as_ref().and_then(|c| c.oid.clone()).filter(|o| !o.is_empty()),
			})
			.collect(),
		review_requests,
		labels: raw.labels.iter().map(|l| l.name.clone()).collect(),
		viewer_is_author,
	}
}

// ── files ──────────────────────────────────────────────────────────────────

/// One element of `GET /repos/{o}/{r}/pulls/{n}/files`.
#[derive(Debug, Default, Deserialize)]
pub struct RawFile {
	#[serde(default)]
	filename: String,
	#[serde(default)]
	status: String,
	#[serde(default)]
	additions: u64,
	#[serde(default)]
	deletions: u64,
	/// Absent for a binary file, and for a text file too large for GitHub to
	/// diff inline.
	#[serde(default)]
	patch: Option<String>,
	#[serde(default)]
	previous_filename: Option<String>,
}

fn file_status(raw: &str) -> PrFileStatus {
	match raw {
		"added" => PrFileStatus::Added,
		"removed" => PrFileStatus::Removed,
		"renamed" => PrFileStatus::Renamed,
		"copied" => PrFileStatus::Copied,
		"changed" => PrFileStatus::Changed,
		"unchanged" => PrFileStatus::Unchanged,
		_ => PrFileStatus::Modified,
	}
}

fn change_kind(status: PrFileStatus) -> GitChangeKind {
	match status {
		PrFileStatus::Added | PrFileStatus::Copied => GitChangeKind::Added,
		PrFileStatus::Removed => GitChangeKind::Deleted,
		PrFileStatus::Renamed => GitChangeKind::Renamed,
		PrFileStatus::Changed => GitChangeKind::Typechange,
		PrFileStatus::Modified | PrFileStatus::Unchanged => GitChangeKind::Modified,
	}
}

pub fn map_file(raw: &RawFile, repo_root: &Path) -> PrFile {
	let status = file_status(&raw.status);
	let commentable = raw.patch.as_deref().map(super::patch::commentable_lines).unwrap_or_default();
	PrFile {
		path: repo_root.join(&raw.filename).to_string_lossy().into_owned(),
		rel_path: raw.filename.clone(),
		old_rel_path: raw
			.previous_filename
			.clone()
			.filter(|old| !old.is_empty() && *old != raw.filename),
		status,
		kind: change_kind(status),
		additions: raw.additions,
		deletions: raw.deletions,
		// A pure rename has no patch because it has no changed lines; that is not
		// "too large to show".
		no_patch: raw.patch.is_none() && raw.additions + raw.deletions > 0,
		commentable,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const LIST: &str = include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../../../tests/fixtures/gh/pr-list.json"
	));
	const VIEW: &str = include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../../../tests/fixtures/gh/pr-view.json"
	));
	const FILES: &str = include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../../../tests/fixtures/gh/pr-files-paginated.json"
	));

	#[test]
	fn failures_are_classified() {
		let table = [
			(Some(4), "", GhFailure::AuthRequired),
			(Some(4), "To get started with GitHub CLI, please run:  gh auth login", GhFailure::AuthRequired),
			(Some(1), "gh: Bad credentials (HTTP 401)", GhFailure::AuthRequired),
			(Some(1), "gh: Not Found (HTTP 404)", GhFailure::NotFound),
			(
				Some(1),
				"GraphQL: Could not resolve to a PullRequest with the number of 99999. (repository.pullRequest)",
				GhFailure::NotFound,
			),
			(Some(1), "GraphQL: Could not resolve to a Repository with the name 'o/r'.", GhFailure::NotFound),
			(Some(1), "gh: API rate limit exceeded for user ID 1. (HTTP 403)", GhFailure::RateLimited),
			(
				Some(1),
				"gh: You have exceeded a secondary rate limit. Please wait a few minutes (HTTP 403)",
				GhFailure::RateLimited,
			),
			(Some(1), "gh: Too Many Requests (HTTP 429)", GhFailure::RateLimited),
			(
				Some(1),
				"error connecting to api.github.com\ncheck your internet connection or https://githubstatus.com",
				GhFailure::Network,
			),
			(
				Some(1),
				"Post \"https://api.github.com/graphql\": dial tcp: lookup api.github.com: no such host",
				GhFailure::Network,
			),
			(Some(1), "dial tcp 140.82.112.5:443: connectex: A connection attempt failed", GhFailure::Network),
			(Some(1), "context deadline exceeded (Client.Timeout exceeded while awaiting headers)", GhFailure::Network),
			(
				Some(1),
				"gh: Resource protected by organization SAML enforcement. You must grant your OAuth token access to this organization. (HTTP 403)",
				GhFailure::Forbidden,
			),
			(
				Some(1),
				"gh: Validation Failed (HTTP 422)\nline must be part of the diff",
				GhFailure::Validation("Validation Failed (HTTP 422) — line must be part of the diff".into()),
			),
			(Some(1), "gh: something new and strange", GhFailure::Other("something new and strange".into())),
			(Some(1), "\n\n  gh: indented first line  \nsecond", GhFailure::Other("indented first line".into())),
			(Some(2), "", GhFailure::Other("gh failed".into())),
			(None, "killed", GhFailure::Other("killed".into())),
		];
		for (code, stderr, want) in table {
			assert_eq!(classify(code, stderr), want, "{code:?} {stderr:?}");
		}
	}

	#[test]
	fn failures_map_onto_the_app_errors_the_renderer_switches_on() {
		assert!(matches!(
			GhFailure::AuthRequired.into_app_error("x"),
			AppError::Unauthenticated(_)
		));
		assert!(
			matches!(GhFailure::NotFound.into_app_error("pull request #7"), AppError::NotFound(m) if m.contains("#7"))
		);
		for failure in [
			GhFailure::RateLimited,
			GhFailure::Network,
			GhFailure::Forbidden,
			GhFailure::Validation("v".into()),
			GhFailure::Other("o".into()),
		] {
			assert!(matches!(failure.into_app_error("x"), AppError::Remote(_)));
		}
	}

	#[test]
	fn versions_parse_and_compare_against_the_floor() {
		assert_eq!(
			parse_version_line("gh version 2.101.0 (2026-09-15)\nhttps://github.com/cli/cli/releases/tag/v2.101.0\n")
				.as_deref(),
			Some("2.101.0")
		);
		assert_eq!(parse_version_line("gh version 2.40.0\n").as_deref(), Some("2.40.0"));
		assert_eq!(parse_version_line("").as_deref(), None);
		assert_eq!(parse_version_line("git version 2.40.0").as_deref(), None);
		assert!(parse_version("2.39.2").unwrap() < super::super::MIN_VERSION);
		assert!(parse_version("2.40.0").unwrap() >= super::super::MIN_VERSION);
		assert!(parse_version("2.101.0").unwrap() >= super::super::MIN_VERSION);
		assert!(parse_version("3.0.0").unwrap() >= super::super::MIN_VERSION);
		assert_eq!(parse_version("2.45.0-rc1"), Some((2, 45, 0)));
		assert_eq!(parse_version("nightly"), None);
	}

	#[test]
	fn concatenated_pages_are_read_as_one_list() {
		let two = br#"[{"filename":"a"},{"filename":"b"}][{"filename":"c"}]"#;
		let files: Vec<RawFile> = parse_pages(two).unwrap();
		assert_eq!(files.iter().map(|f| f.filename.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
		// Whitespace between pages, as `gh` prints it.
		let spaced = b"[{\"filename\":\"a\"}]\n[{\"filename\":\"b\"}]\n";
		assert_eq!(parse_pages::<RawFile>(spaced).unwrap().len(), 2);
		assert!(parse_pages::<RawFile>(b"").unwrap().is_empty());
		assert!(parse_pages::<RawFile>(b"[{\"filename\":").is_err());
	}

	#[test]
	fn the_captured_pages_parse() {
		let files: Vec<RawFile> = parse_pages(FILES.as_bytes()).unwrap();
		assert!(files.len() > 2, "{}", files.len());
		let mapped: Vec<PrFile> = files.iter().map(|f| map_file(f, Path::new("/repo"))).collect();
		assert!(mapped.iter().any(|f| f.status == PrFileStatus::Added));
		assert!(mapped.iter().all(|f| f.path.contains("repo")));
		let modified = mapped.iter().find(|f| f.status == PrFileStatus::Modified).unwrap();
		assert!(!modified.commentable.right.is_empty());
		assert!(!modified.no_patch);
	}

	#[test]
	fn checks_roll_up_across_both_kinds() {
		let rollup = serde_json::json!([
			{"__typename":"CheckRun","name":"build","workflowName":"CI","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://x/1","completedAt":"2026-10-12T12:10:52Z"},
			{"__typename":"CheckRun","name":"lint","workflowName":"CI","status":"COMPLETED","conclusion":"SKIPPED","detailsUrl":"","completedAt":"2026-10-12T12:10:52Z"},
			{"__typename":"CheckRun","name":"e2e","workflowName":"CI","status":"IN_PROGRESS","conclusion":"","detailsUrl":"https://x/3","completedAt":"0001-01-01T00:00:00Z"},
			{"__typename":"CheckRun","name":"slow","workflowName":"CI","status":"COMPLETED","conclusion":"TIMED_OUT"},
			{"__typename":"CheckRun","name":"gone","workflowName":"CI","status":"COMPLETED","conclusion":"CANCELLED"},
			{"__typename":"CheckRun","name":"info","status":"COMPLETED","conclusion":"NEUTRAL"},
			{"__typename":"StatusContext","context":"ci/jenkins","state":"SUCCESS","targetUrl":"https://j/1"},
			{"__typename":"StatusContext","context":"ci/legacy","state":"ERROR","targetUrl":""},
			{"__typename":"StatusContext","context":"ci/wait","state":"PENDING"},
			{"__typename":"StatusContext","context":"ci/expected","state":"EXPECTED"}
		]);
		let raw: Vec<RawCheck> = serde_json::from_value(rollup).unwrap();
		let checks = map_checks(&raw);
		let states: Vec<CheckState> = checks.iter().map(|c| c.state).collect();
		assert_eq!(
			states,
			[
				CheckState::Success,
				CheckState::Skipped,
				CheckState::Pending,
				CheckState::Failure,
				CheckState::Cancelled,
				CheckState::Neutral,
				CheckState::Success,
				CheckState::Failure,
				CheckState::Pending,
				CheckState::Pending,
			]
		);
		assert_eq!(checks[0].workflow.as_deref(), Some("CI"));
		assert_eq!(checks[0].completed_at, Some(1_791_807_052_000));
		assert_eq!(checks[1].url, None, "an empty url is no url");
		assert_eq!(checks[2].completed_at, None, "the zero date of a running check is no date");
		assert_eq!(checks[6].name, "ci/jenkins");
		assert_eq!(checks[6].workflow, None);
		assert_eq!(checks[6].url.as_deref(), Some("https://j/1"));

		let summary = summarize_checks(&checks);
		assert_eq!(
			summary,
			PrChecksSummary {
				state: PrChecksState::Failure,
				total: 10,
				passed: 4,
				failed: 3,
				pending: 3
			}
		);
	}

	#[test]
	fn the_rollup_state_follows_the_worst_check() {
		let one = |state| PrCheck {
			name: "c".into(),
			workflow: None,
			state,
			url: None,
			completed_at: None,
		};
		assert_eq!(summarize_checks(&[]).state, PrChecksState::None);
		assert_eq!(summarize_checks(&[one(CheckState::Success)]).state, PrChecksState::Success);
		assert_eq!(
			summarize_checks(&[one(CheckState::Success), one(CheckState::Pending)]).state,
			PrChecksState::Pending
		);
		assert_eq!(
			summarize_checks(&[one(CheckState::Pending), one(CheckState::Failure)]).state,
			PrChecksState::Failure
		);
		assert_eq!(summarize_checks(&[one(CheckState::Skipped)]).state, PrChecksState::Success);
	}

	#[test]
	fn the_captured_list_maps() {
		let raw = parse_pr_list(LIST.as_bytes()).unwrap();
		assert!(raw.len() >= 2);
		let rows: Vec<PrSummary> = raw.iter().map(|r| summary(r, Some("Nightbr"))).collect();
		let open = rows.iter().find(|r| r.state == PrState::Open).unwrap();
		assert_eq!(open.author, "rbideau");
		assert!(!open.author_is_bot);
		assert!(open.review_requested_from_viewer, "Nightbr is a requested reviewer");
		assert_eq!(open.head_owner.as_deref(), Some("Nightbr"));
		assert!(open.created_at > 1_700_000_000_000);
		assert!(open.checks.total > 0);
		assert_eq!(open.review_decision, None, "an empty decision is none");
		let merged = rows.iter().find(|r| r.state == PrState::Merged).unwrap();
		assert!(!merged.review_requested_from_viewer);
		// Someone else is the viewer: nothing is requested from them.
		assert!(!summary(&raw[0], Some("nobody")).review_requested_from_viewer);
		assert!(!summary(&raw[0], None).review_requested_from_viewer);
	}

	#[test]
	fn the_captured_fork_detail_maps() {
		let raw = parse_pr(VIEW.as_bytes()).unwrap();
		let detail = detail(&raw, Some("bergjs"), "abc".into());
		assert_eq!(detail.summary.number, 57);
		assert_eq!(detail.summary.state, PrState::Merged);
		assert!(detail.summary.is_cross_repository);
		assert_eq!(detail.summary.head_owner.as_deref(), Some("bergjs"));
		assert!(detail.viewer_is_author);
		assert_eq!(detail.merge_base_oid, "abc");
		assert_eq!(detail.base_ref_oid.len(), 40);
		assert_eq!(detail.mergeable, PrMergeable::Unknown);
		assert!(!detail.body.is_empty());
		assert_eq!(detail.latest_reviews.len(), 1);
		assert_eq!(detail.latest_reviews[0].author, "Nightbr");
		assert_eq!(detail.latest_reviews[0].state, PrReviewState::Approved);
		assert_eq!(detail.latest_reviews[0].commit_oid, None, "an empty commit oid is no commit");
		assert!(detail.latest_reviews[0].submitted_at.is_some());
		assert!(!detail.check_runs.is_empty());
	}

	#[test]
	fn a_deleted_fork_and_a_deleted_author_still_map() {
		let raw = parse_pr(
			br#"{"number":9,"title":"t","author":null,"state":"CLOSED","isDraft":true,
			"baseRefName":"main","headRefName":"patch-1","headRefOid":"aa","isCrossRepository":true,
			"headRepositoryOwner":null,"createdAt":"2026-01-02T03:04:05Z","updatedAt":"2026-01-02T03:04:05Z",
			"reviewDecision":"CHANGES_REQUESTED","statusCheckRollup":null,"additions":1,"deletions":2,
			"changedFiles":1,"url":"https://github.com/o/r/pull/9",
			"reviewRequests":[{"__typename":"Team","name":"core","slug":"core"},{"__typename":"User","login":"me"}],
			"latestReviews":[{"author":null,"state":"WEIRD","submittedAt":null,"body":null,"commit":null}],
			"labels":[{"name":"bug"}]}"#,
		)
		.unwrap();
		let detail = detail(&raw, Some("core"), "mb".into());
		assert_eq!(detail.summary.author, "ghost");
		assert_eq!(detail.summary.head_owner, None);
		assert_eq!(detail.summary.state, PrState::Closed);
		assert!(detail.summary.is_draft);
		assert_eq!(detail.summary.review_decision, Some(PrReviewDecision::ChangesRequested));
		assert_eq!(detail.summary.checks.state, PrChecksState::None);
		assert_eq!(detail.review_requests, ["@core", "me"]);
		assert!(!detail.summary.review_requested_from_viewer, "a team is not the viewer");
		assert_eq!(detail.labels, ["bug"]);
		assert_eq!(detail.latest_reviews[0].author, "ghost");
		assert_eq!(detail.latest_reviews[0].state, PrReviewState::Commented);
		assert_eq!(detail.latest_reviews[0].submitted_at, None);
		assert_eq!(detail.body, "");
		assert!(!detail.viewer_is_author);
	}

	#[test]
	fn a_bot_author_keeps_its_name_and_flag() {
		let raw = parse_pr(
			br#"{"number":1,"author":{"id":"x","is_bot":true,"login":"app/dependabot"},"state":"OPEN"}"#,
		)
		.unwrap();
		let row = summary(&raw, None);
		assert_eq!(row.author, "dependabot");
		assert!(row.author_is_bot);
	}

	#[test]
	fn file_rows_carry_rename_and_kind() {
		let root = Path::new("/repo");
		let renamed: RawFile = serde_json::from_str(
			r#"{"filename":"src/new.ts","status":"renamed","additions":0,"deletions":0,"previous_filename":"src/old.ts"}"#,
		)
		.unwrap();
		let row = map_file(&renamed, root);
		assert_eq!(row.old_rel_path.as_deref(), Some("src/old.ts"));
		assert_eq!(row.kind, GitChangeKind::Renamed);
		assert!(!row.no_patch, "a pure rename has no lines to show");

		let big: RawFile = serde_json::from_str(
			r#"{"filename":"dist/bundle.js","status":"modified","additions":9000,"deletions":1}"#,
		)
		.unwrap();
		let row = map_file(&big, root);
		assert!(row.no_patch);
		assert!(row.commentable.left.is_empty() && row.commentable.right.is_empty());

		let removed: RawFile = serde_json::from_str(
			r#"{"filename":"a.ts","status":"removed","additions":0,"deletions":2,"patch":"@@ -1,2 +0,0 @@\n-a\n-b"}"#,
		)
		.unwrap();
		let row = map_file(&removed, root);
		assert_eq!(row.kind, GitChangeKind::Deleted);
		assert_eq!(row.commentable.left, [(1, 2)]);
		assert!(row.commentable.right.is_empty());
	}
}
