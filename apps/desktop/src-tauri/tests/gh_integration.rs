//! Pull requests through `gh` (F31): the wire shape the renderer reads, and one
//! live run against a real `gh`.
//!
//! The wire-shape tests assert literal strings for the same reason
//! `wire_shape.rs` does: the TS types in `packages/types` are hand-mirrored, and
//! `tsc` cannot see across the bridge. `PrDetail` is the one to watch — it
//! flattens `PrSummary` so the TS `extends` is true on the wire — and
//! `CommentableLines` crosses as arrays of `[start, end]` pairs.
//!
//! The live test is `#[ignore]`d: it needs a signed-in `gh` and the network, and
//! is run by hand (`cargo test --test gh_integration -- --ignored --nocapture`).
//! Every call in it is a read.

use factorai_lib::error::AppError;
use factorai_lib::models::{
	CommentableLines, DiffSide, DraftComment, GhRepo, GhState, GhStatus, PrComment, PrState,
	PrStateFilter, ReviewEvent, ReviewSubmission,
};
use factorai_lib::services::gh;

#[test]
fn the_new_error_kinds_cross_as_a_tagged_union() {
	let json = serde_json::to_value(AppError::Unauthenticated("run gh auth login".into())).unwrap();
	assert_eq!(
		json,
		serde_json::json!({"kind": "Unauthenticated", "message": "run gh auth login"})
	);
	let json = serde_json::to_value(AppError::Remote("rate limited".into())).unwrap();
	assert_eq!(json, serde_json::json!({"kind": "Remote", "message": "rate limited"}));
}

#[test]
fn a_status_is_camel_case_with_camel_case_states() {
	let status = GhStatus {
		state: GhState::NotAuthenticated,
		binary_path: Some("C:\\gh.exe".into()),
		version: Some("2.101.0".into()),
		login: None,
		repo: Some(GhRepo {
			host: "github.com".into(),
			owner: "o".into(),
			name: "r".into(),
			slug: "github.com/o/r".into(),
			web_url: "https://github.com/o/r".into(),
		}),
		repo_root: Some("C:\\code\\r".into()),
		message: None,
	};
	let json = serde_json::to_value(&status).unwrap();
	assert_eq!(json["state"], "notAuthenticated");
	assert_eq!(json["binaryPath"], "C:\\gh.exe");
	assert_eq!(json["repoRoot"], "C:\\code\\r");
	assert_eq!(json["repo"]["webUrl"], "https://github.com/o/r");
	assert!(json["login"].is_null());
	for (state, word) in [
		(GhState::Ready, "ready"),
		(GhState::NotInstalled, "notInstalled"),
		(GhState::TooOld, "tooOld"),
		(GhState::NotRepository, "notRepository"),
		(GhState::NoOrigin, "noOrigin"),
		(GhState::NotGitHub, "notGitHub"),
		(GhState::Unreachable, "unreachable"),
	] {
		assert_eq!(serde_json::to_value(state).unwrap(), word);
	}
}

#[test]
fn a_filter_is_read_from_the_renderers_word() {
	for (word, want) in [
		("open", PrStateFilter::Open),
		("closed", PrStateFilter::Closed),
		("merged", PrStateFilter::Merged),
		("all", PrStateFilter::All),
	] {
		assert_eq!(serde_json::from_value::<PrStateFilter>(word.into()).unwrap(), want);
	}
	assert!(serde_json::from_value::<PrStateFilter>("everything".into()).is_err());
}

#[test]
fn commentable_ranges_cross_as_pairs() {
	let lines = CommentableLines { left: vec![(1, 3)], right: vec![(1, 4), (20, 22)] };
	assert_eq!(
		serde_json::to_value(lines).unwrap(),
		serde_json::json!({"left": [[1, 3]], "right": [[1, 4], [20, 22]]})
	);
}

#[test]
fn a_review_arrives_camel_case_with_screaming_events_and_upper_sides() {
	let review: ReviewSubmission = serde_json::from_value(serde_json::json!({
		"commitOid": "0123456789abcdef0123456789abcdef01234567",
		"event": "REQUEST_CHANGES",
		"body": "b",
		"comments": [{"path": "a.ts", "line": 4, "side": "RIGHT", "startLine": 2, "startSide": "RIGHT", "body": "c"}]
	}))
	.unwrap();
	assert_eq!(review.event, ReviewEvent::RequestChanges);
	assert_eq!(review.comments[0].start_line, Some(2));
	assert_eq!(review.comments[0].side, DiffSide::Right);
	let single: DraftComment = serde_json::from_value(
		serde_json::json!({"path": "a.ts", "line": 4, "side": "LEFT", "startLine": null, "startSide": null, "body": "c"}),
	)
	.unwrap();
	assert_eq!(single.side, DiffSide::Left);
	assert_eq!(single.start_line, None);
}

#[test]
fn a_comment_crosses_as_camel_case() {
	let comment = PrComment {
		id: 1,
		author: "a".into(),
		body: "b".into(),
		path: "p".into(),
		line: Some(3),
		side: Some(DiffSide::Left),
		start_line: None,
		start_side: None,
		commit_oid: "c".into(),
		created_at: 5,
		in_reply_to: Some(2),
		url: "u".into(),
		outdated: false,
	};
	let value = serde_json::to_value(comment).unwrap();
	assert_eq!(value["side"], "LEFT");
	assert_eq!(value["inReplyTo"], 2);
	assert_eq!(value["createdAt"], 5);
	assert_eq!(value["commitOid"], "c");
}

/// Read-only run against the public upstream, through a temporary clone whose
/// `origin` names it. Prints a summary; asserts only what must hold for any
/// healthy `gh`.
#[tokio::test]
#[ignore = "needs a signed-in gh and the network"]
async fn live_read_of_a_public_pull_request() {
	let dir = tempfile::tempdir().unwrap();
	let repo = git2::Repository::init(dir.path()).unwrap();
	repo.remote("origin", "https://github.com/Nightbr/factorai.git").unwrap();
	// A second remote that must be ignored.
	repo.remote("upstream", "https://github.com/qursor-it/qursor-ide.git").unwrap();
	let project = dir.path().to_string_lossy().into_owned();

	let status = gh::status(&project, true).await;
	println!("status: {status:#?}");
	assert_eq!(status.state, GhState::Ready, "{status:?}");
	assert_eq!(status.repo.as_ref().unwrap().slug, "github.com/Nightbr/factorai");

	let list = gh::pr_list(&project, PrStateFilter::All).await.unwrap();
	println!("list: {} rows, first #{} {:?}", list.len(), list[0].number, list[0].title);
	assert!(!list.is_empty() && list.len() <= 50);

	let detail = gh::pr_detail(&project, 57).await.unwrap();
	println!(
		"detail #57: {:?} by {} state={:?} cross={} headOwner={:?} base={} head={} mergeBase={} checks={:?} runs={} reviews={}",
		detail.summary.title,
		detail.summary.author,
		detail.summary.state,
		detail.summary.is_cross_repository,
		detail.summary.head_owner,
		&detail.base_ref_oid[..8],
		&detail.summary.head_ref_oid[..8],
		&detail.merge_base_oid[..8],
		detail.summary.checks.state,
		detail.check_runs.len(),
		detail.latest_reviews.len(),
	);
	assert_eq!(detail.summary.state, PrState::Merged);
	assert!(detail.summary.is_cross_repository);
	assert_eq!(detail.merge_base_oid.len(), 40);

	let files = gh::pr_files(&project, 57).await.unwrap();
	println!(
		"files: {} of {} (truncated={}); first {} {:?} commentable right={:?}",
		files.files.len(),
		files.total,
		files.truncated,
		files.files[0].rel_path,
		files.files[0].status,
		files.files[0].commentable.right,
	);
	assert_eq!(files.files.len() as u64, files.total);

	// The head commit of a fork is not in this empty clone: the blob comes over
	// the network, from the base repository.
	let file = files.files.iter().find(|f| f.rel_path.ends_with("oklch.test.ts")).unwrap();
	let head = gh::pr_blob(&project, &file.rel_path, &detail.summary.head_ref_oid, Some(2000))
		.await
		.unwrap()
		.expect("the added file exists at the head");
	println!(
		"blob at head: {} bytes, truncated={}, first line {:?}",
		head.size,
		head.truncated,
		head.contents.lines().next()
	);
	assert!(!head.contents.is_empty());

	// An added file has no base side.
	let base = gh::pr_blob(&project, &file.rel_path, &detail.merge_base_oid, None).await.unwrap();
	println!("blob at merge base: {}", if base.is_none() { "none (added)" } else { "present" });
	assert!(base.is_none());

	// A pull request that does not exist is NotFound, not a crash.
	let missing = gh::pr_detail(&project, 999_999).await;
	println!("missing pr: {missing:?}");
	assert!(matches!(missing, Err(AppError::NotFound(_))), "{missing:?}");
}
