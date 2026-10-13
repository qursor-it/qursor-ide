//! Submitting a review: what is checked before `gh` is spawned, and the JSON
//! body that goes to it on stdin (F31 P3, ADR-0077).
//!
//! GitHub answers a bad review with a 422 and a sentence, but only after a round
//! trip, and a review is the one thing the app writes. So everything checkable
//! locally is refused here with a sentence of our own, and the request body is
//! built from validated parts only.

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::models::{DiffSide, DraftComment, ReviewEvent, ReviewSubmission};

use super::{validate_rel_path, validate_sha};

/// GitHub's limit on one body, in characters.
pub const MAX_BODY_CHARS: usize = 65_536;
/// More inline comments than this in one review is a mistake, not a review.
pub const MAX_COMMENTS: usize = 500;

fn invalid(message: impl Into<String>) -> AppError {
	AppError::InvalidInput(message.into())
}

fn side_word(side: DiffSide) -> &'static str {
	match side {
		DiffSide::Left => "LEFT",
		DiffSide::Right => "RIGHT",
	}
}

fn event_word(event: ReviewEvent) -> &'static str {
	match event {
		ReviewEvent::Approve => "APPROVE",
		ReviewEvent::RequestChanges => "REQUEST_CHANGES",
		ReviewEvent::Comment => "COMMENT",
	}
}

fn validate_comment(index: usize, comment: &DraftComment) -> AppResult<()> {
	let n = index + 1;
	validate_rel_path(&comment.path)
		.map_err(|_| invalid(format!("comment {n} is on a path outside the repository")))?;
	if comment.line == 0 {
		return Err(invalid(format!("comment {n} is on line 0; lines start at 1")));
	}
	match (comment.start_line, comment.start_side) {
		(None, None) => {}
		(Some(start), Some(_)) => {
			if start == 0 || start >= comment.line {
				return Err(invalid(format!(
					"comment {n} starts at line {start}, which is not before its last line {}",
					comment.line
				)));
			}
		}
		_ => {
			return Err(invalid(format!(
				"comment {n} must give both the start line and the start side, or neither"
			)))
		}
	}
	if comment.body.trim().is_empty() {
		return Err(invalid(format!("comment {n} is empty")));
	}
	if comment.body.chars().count() > MAX_BODY_CHARS {
		return Err(invalid(format!("comment {n} is longer than {MAX_BODY_CHARS} characters")));
	}
	Ok(())
}

/// Refuse a review GitHub would refuse, or that the renderer should never send.
pub fn validate(review: &ReviewSubmission) -> AppResult<()> {
	validate_sha(&review.commit_oid)?;
	if review.body.chars().count() > MAX_BODY_CHARS {
		return Err(invalid(format!("the review text is longer than {MAX_BODY_CHARS} characters")));
	}
	if review.comments.len() > MAX_COMMENTS {
		return Err(invalid(format!(
			"a review holds at most {MAX_COMMENTS} line comments; this one has {}",
			review.comments.len()
		)));
	}
	for (index, comment) in review.comments.iter().enumerate() {
		validate_comment(index, comment)?;
	}
	// GitHub answers a COMMENT or REQUEST_CHANGES review with nothing to say
	// with a 422; an approval may be bare.
	let says_nothing = review.body.trim().is_empty() && review.comments.is_empty();
	if says_nothing && review.event != ReviewEvent::Approve {
		return Err(invalid(
			"write a comment on the review or on a line before submitting; only an approval may be empty",
		));
	}
	Ok(())
}

#[derive(Serialize)]
struct WireComment<'a> {
	path: &'a str,
	line: u32,
	side: &'static str,
	#[serde(skip_serializing_if = "Option::is_none")]
	start_line: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	start_side: Option<&'static str>,
	body: &'a str,
}

#[derive(Serialize)]
struct WireReview<'a> {
	commit_id: &'a str,
	event: &'static str,
	body: &'a str,
	comments: Vec<WireComment<'a>>,
}

/// The request body, in GitHub's snake_case. `start_*` are left out for a
/// single-line comment: GitHub rejects `null` there.
pub fn payload(review: &ReviewSubmission) -> AppResult<Vec<u8>> {
	let wire = WireReview {
		commit_id: &review.commit_oid,
		event: event_word(review.event),
		body: &review.body,
		comments: review
			.comments
			.iter()
			.map(|c| WireComment {
				path: &c.path,
				line: c.line,
				side: side_word(c.side),
				start_line: c.start_line,
				start_side: c.start_line.and(c.start_side).map(side_word),
				body: &c.body,
			})
			.collect(),
	};
	serde_json::to_vec(&wire)
		.map_err(|e| AppError::Process(format!("could not build the review: {e}")))
}

/// What GitHub put in the `errors` of a 422 body, which `gh` prints to stdout.
/// The sentence that says *why* ("line must be part of the diff") is often only
/// there.
pub fn validation_detail(stdout: &[u8]) -> Option<String> {
	let value: serde_json::Value = serde_json::from_slice(stdout).ok()?;
	let errors = value.get("errors")?.as_array()?;
	let parts: Vec<String> = errors
		.iter()
		.filter_map(|e| match e {
			serde_json::Value::String(s) => Some(s.clone()),
			other => other.get("message").and_then(|m| m.as_str()).map(String::from),
		})
		.collect();
	if parts.is_empty() {
		None
	} else {
		Some(parts.join("; "))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

	fn line_comment() -> DraftComment {
		DraftComment {
			path: "src/a.ts".into(),
			line: 12,
			side: DiffSide::Right,
			start_line: None,
			start_side: None,
			body: "nit".into(),
		}
	}

	fn review(event: ReviewEvent, body: &str, comments: Vec<DraftComment>) -> ReviewSubmission {
		ReviewSubmission { commit_oid: SHA.into(), event, body: body.into(), comments }
	}

	#[test]
	fn the_payload_is_githubs_snake_case_without_null_ranges() {
		let mut multi = line_comment();
		multi.start_line = Some(10);
		multi.side = DiffSide::Left;
		multi.start_side = Some(DiffSide::Left);
		let r = review(ReviewEvent::RequestChanges, "please fix", vec![line_comment(), multi]);
		let value: serde_json::Value = serde_json::from_slice(&payload(&r).unwrap()).unwrap();
		assert_eq!(value["commit_id"], SHA);
		assert_eq!(value["event"], "REQUEST_CHANGES");
		assert_eq!(value["body"], "please fix");
		let single = &value["comments"][0];
		assert_eq!(single["path"], "src/a.ts");
		assert_eq!(single["line"], 12);
		assert_eq!(single["side"], "RIGHT");
		assert!(single.get("start_line").is_none() && single.get("start_side").is_none());
		let range = &value["comments"][1];
		assert_eq!(range["start_line"], 10);
		assert_eq!(range["start_side"], "LEFT");
		assert_eq!(range["side"], "LEFT");
	}

	#[test]
	fn the_wire_words_match_the_renderers() {
		assert_eq!(serde_json::to_value(ReviewEvent::RequestChanges).unwrap(), "REQUEST_CHANGES");
		assert_eq!(serde_json::to_value(ReviewEvent::Approve).unwrap(), "APPROVE");
		assert_eq!(serde_json::to_value(DiffSide::Left).unwrap(), "LEFT");
		assert!(serde_json::from_value::<ReviewEvent>("MERGE".into()).is_err());
	}

	#[test]
	fn a_valid_review_passes() {
		assert!(validate(&review(ReviewEvent::Approve, "", vec![])).is_ok());
		assert!(validate(&review(ReviewEvent::Comment, "hi", vec![])).is_ok());
		assert!(validate(&review(ReviewEvent::RequestChanges, "", vec![line_comment()])).is_ok());
		let mut multi = line_comment();
		multi.start_line = Some(11);
		multi.start_side = Some(DiffSide::Right);
		assert!(validate(&review(ReviewEvent::Comment, "", vec![multi])).is_ok());
	}

	#[test]
	fn a_bad_review_is_refused_before_gh_runs() {
		let with = |edit: &dyn Fn(&mut DraftComment)| {
			let mut c = line_comment();
			edit(&mut c);
			review(ReviewEvent::Comment, "", vec![c])
		};
		let mut bad_sha = review(ReviewEvent::Approve, "", vec![]);
		bad_sha.commit_oid = "main".into();
		let table: Vec<(&str, ReviewSubmission)> = vec![
			("short commit", bad_sha),
			("empty comment review", review(ReviewEvent::Comment, "  \n", vec![])),
			("empty change request", review(ReviewEvent::RequestChanges, "", vec![])),
			("path up", with(&|c| c.path = "../x".into())),
			("absolute path", with(&|c| c.path = "/etc/passwd".into())),
			("empty path", with(&|c| c.path = String::new())),
			("line zero", with(&|c| c.line = 0)),
			(
				"start after line",
				with(&|c| {
					c.start_line = Some(13);
					c.start_side = Some(DiffSide::Right);
				}),
			),
			(
				"start equals line",
				with(&|c| {
					c.start_line = Some(12);
					c.start_side = Some(DiffSide::Right);
				}),
			),
			("start without side", with(&|c| c.start_line = Some(3))),
			("side without start", with(&|c| c.start_side = Some(DiffSide::Right))),
			("blank comment", with(&|c| c.body = " \t".into())),
			("huge comment", with(&|c| c.body = "x".repeat(MAX_BODY_CHARS + 1))),
			("huge body", review(ReviewEvent::Comment, &"x".repeat(MAX_BODY_CHARS + 1), vec![])),
			(
				"too many comments",
				review(ReviewEvent::Comment, "", vec![line_comment(); MAX_COMMENTS + 1]),
			),
		];
		for (name, r) in table {
			assert!(matches!(validate(&r), Err(AppError::InvalidInput(_))), "{name}");
		}
		let edge = review(ReviewEvent::Comment, "", vec![line_comment(); MAX_COMMENTS]);
		assert!(validate(&edge).is_ok());
	}

	#[test]
	fn the_empty_review_sentence_says_what_to_do() {
		let Err(AppError::InvalidInput(message)) =
			validate(&review(ReviewEvent::Comment, "", vec![]))
		else {
			panic!("expected a refusal");
		};
		assert!(message.contains("only an approval may be empty"), "{message}");
	}

	#[test]
	fn a_422_body_gives_up_its_reasons() {
		let body = br#"{"message":"Unprocessable Entity","errors":["Pull request review thread line must be part of the diff",{"message":"path is invalid"}]}"#;
		assert_eq!(
			validation_detail(body).as_deref(),
			Some("Pull request review thread line must be part of the diff; path is invalid")
		);
		assert_eq!(validation_detail(br#"{"message":"x"}"#), None);
		assert_eq!(validation_detail(b"not json"), None);
	}
}
