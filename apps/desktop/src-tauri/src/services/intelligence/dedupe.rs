//! Candidate identity: a key that does not depend on the model's exact words,
//! and a conservative rule for when two candidates are the same one.
//!
//! The key is `sha256(project | problem kind | action | tokens)`, where the
//! tokens are the recommendation lowercased, stripped of punctuation and
//! stopwords, lightly de-pluralised, sorted and de-duplicated. Re-running on the
//! same events therefore lands on the same row whenever the model words the
//! recommendation the same way, and on the same *evidence* whenever it does not.
//!
//! **Negations and polarity words are deliberately not stopwords.** "Always use
//! X" and "never use X" must not merge.
//!
//! Nothing here compares candidates across projects: a key includes the project,
//! and the merge search is handed one project's rows.

use std::collections::{BTreeSet, HashSet};

use super::sha256_hex;
use crate::models::intelligence::{ActionKind, ProblemKind};

/// Function words that carry no meaning for "is this the same advice". English
/// and Italian, since corrections arrive in both. `not`, `no`, `never`,
/// `always`, `non`, `mai`, `sempre` are absent on purpose.
const STOPWORDS: &[&str] = &[
	"a", "an", "the", "and", "or", "of", "to", "in", "on", "for", "with", "is", "are", "be",
	"this", "that", "it", "its", "as", "at", "by", "from", "before", "after", "when", "while",
	"then", "you", "your", "we", "our", "i", "me", "my", "so", "if", "should", "must", "can",
	"will", "please", "make", "sure", "ensure", "every", "each", "any", "all", "do", "does", "did",
	"have", "has", "had", "was", "were", "been", "being", "than", "into", "per", "via", "also",
	"just", "only", "up", "out", "about", "over", "prior", "il", "lo", "la", "i", "gli", "le",
	"un", "uno", "una", "di", "da", "in", "con", "su", "per", "tra", "fra", "e", "o", "che", "si",
	"del", "della", "dei", "delle", "al", "alla", "ai", "alle", "nel", "nella", "prima", "dopo",
	"quando", "devi", "deve", "ogni",
];

/// The normalised token set of a piece of advice.
pub fn tokens(text: &str) -> BTreeSet<String> {
	let mut out = BTreeSet::new();
	let mut word = String::new();
	let mut flush = |word: &mut String| {
		if word.is_empty() {
			return;
		}
		let token = fold_plural(&std::mem::take(word));
		if token.chars().count() >= 2 && !STOPWORDS.contains(&token.as_str()) {
			out.insert(token);
		}
	};
	for c in text.chars().flat_map(char::to_lowercase) {
		// Characters that make up a command or a path stay in the word: `pnpm`,
		// `cargo-fmt`, `a.rs`, `src/lib`, but not the sentence punctuation
		// around them.
		if c.is_alphanumeric() || matches!(c, '_' | '-' | '/' | '.' | ':' | '\'') {
			word.push(c);
		} else {
			flush(&mut word);
		}
	}
	flush(&mut word);
	out.into_iter()
		.map(|t| t.trim_matches(|c: char| matches!(c, '.' | ':' | '-' | '\'' | '/')).to_string())
		.filter(|t| t.chars().count() >= 2)
		.collect()
}

/// `tests` -> `test`. `committing` is left alone, since a stemmer would merge
/// more than this wants to, and so is anything that is not a plain word: `a.rs`
/// is a file, not a plural.
fn fold_plural(word: &str) -> String {
	let word = word.trim_matches(|c: char| matches!(c, '.' | ':' | '\''));
	let plain = word.chars().all(char::is_alphabetic);
	// Words that end in `s` without being plurals. `always` has to stay `always`:
	// it is the one that tells "always use X" from "never use X".
	const NOT_PLURALS: &[&str] = &["always", "sometimes", "perhaps", "unless", "does", "goes"];
	let singular_looking = word.ends_with("ss")
		|| word.ends_with("us")
		|| word.ends_with("is")
		|| NOT_PLURALS.contains(&word);
	if plain && word.chars().count() > 3 && word.ends_with('s') && !singular_looking {
		word[..word.len() - 1].to_string()
	} else {
		word.to_string()
	}
}

/// The stable key for one candidate.
pub fn dedupe_key(
	project_id: &str,
	kind: ProblemKind,
	action: ActionKind,
	recommendation: &str,
) -> String {
	let joined = tokens(recommendation).into_iter().collect::<Vec<_>>().join(" ");
	sha256_hex(format!("v1|{project_id}|{}|{}|{joined}", kind.as_str(), action.as_str()).as_bytes())
}

/// What the merge search needs to know about a candidate already stored.
#[derive(Debug, Clone)]
pub struct Existing {
	pub id: String,
	pub action: ActionKind,
	pub tokens: BTreeSet<String>,
	pub evidence: HashSet<String>,
}

/// Words that turn advice around. Two pieces of advice that differ in whether
/// they contain one are opposites, however many other words they share.
const NEGATIVE: &[&str] = &[
	"not",
	"no",
	"never",
	"non",
	"mai",
	"dont",
	"don't",
	"avoid",
	"without",
	"stop",
	"cannot",
	"can't",
	"shouldn't",
	"mustn't",
	"doesn't",
	"isn't",
	"evita",
	"senza",
];

fn negated(tokens: &BTreeSet<String>) -> bool {
	tokens.iter().any(|t| NEGATIVE.contains(&t.as_str()))
}

/// Jaccard similarity of two token sets.
pub fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
	if a.is_empty() && b.is_empty() {
		return 1.0;
	}
	let inter = a.intersection(b).count() as f64;
	let union = a.union(b).count() as f64;
	inter / union
}

/// Share of the smaller evidence set that also appears in the larger one.
fn overlap_coefficient(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
	let smaller = a.len().min(b.len());
	if smaller == 0 {
		return 0.0;
	}
	a.intersection(b).count() as f64 / smaller as f64
}

/// The existing candidate a new one should be merged into, if any.
///
/// **Conservative on purpose: every signal must agree.** Same action, the same
/// polarity (never merge "always" with "never"), at least half of the smaller
/// evidence set in common, *and* some real similarity in the advice itself. Two different recommendations drawn from one correction are
/// two candidates; the same recommendation reworded over the same events is one.
pub fn find_merge_target<'a>(
	existing: &'a [Existing],
	action: ActionKind,
	recommendation_tokens: &BTreeSet<String>,
	evidence: &HashSet<String>,
) -> Option<&'a Existing> {
	existing
		.iter()
		.filter(|e| e.action == action)
		.filter(|e| negated(&e.tokens) == negated(recommendation_tokens))
		.filter(|e| overlap_coefficient(&e.evidence, evidence) >= 0.5)
		.filter(|e| jaccard(&e.tokens, recommendation_tokens) >= 0.3)
		.max_by(|a, b| {
			let score = |e: &Existing| jaccard(&e.tokens, recommendation_tokens);
			score(a).partial_cmp(&score(b)).unwrap_or(std::cmp::Ordering::Equal)
		})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn set(items: &[&str]) -> HashSet<String> {
		items.iter().map(|s| s.to_string()).collect()
	}

	#[test]
	fn tokens_drop_filler_and_fold_plurals_but_keep_polarity() {
		let t = tokens("Always run the tests before committing!");
		assert!(t.contains("always") && t.contains("run") && t.contains("test"));
		assert!(!t.contains("the") && !t.contains("before"));
		assert!(tokens("never use unwrap").contains("never"));
		assert!(tokens("non usare unwrap").contains("non"));
	}

	#[test]
	fn tokens_keep_commands_and_paths_whole() {
		let t = tokens("Run `pnpm lint` and cargo-fmt on src/lib.rs.");
		assert!(t.contains("pnpm"), "{t:?}");
		assert!(t.contains("cargo-fmt"), "{t:?}");
		assert!(t.contains("src/lib.rs"), "{t:?}");
	}

	#[test]
	fn the_key_is_stable_across_wording_that_normalises_the_same() {
		let a = dedupe_key(
			"p1",
			ProblemKind::ProjectConvention,
			ActionKind::Instruction,
			"Run the linter before committing.",
		);
		let b = dedupe_key(
			"p1",
			ProblemKind::ProjectConvention,
			ActionKind::Instruction,
			"run LINTER, committing!!",
		);
		assert_eq!(a, b);
	}

	#[test]
	fn the_key_separates_projects_kinds_actions_and_polarity() {
		let base = |project, kind, action, text: &str| dedupe_key(project, kind, action, text);
		let k = base("p1", ProblemKind::WorkflowHint, ActionKind::Instruction, "always use pnpm");
		assert_ne!(
			k,
			base("p2", ProblemKind::WorkflowHint, ActionKind::Instruction, "always use pnpm")
		);
		assert_ne!(
			k,
			base("p1", ProblemKind::VerificationGap, ActionKind::Instruction, "always use pnpm")
		);
		assert_ne!(
			k,
			base("p1", ProblemKind::WorkflowHint, ActionKind::TestSuggestion, "always use pnpm")
		);
		assert_ne!(
			k,
			base("p1", ProblemKind::WorkflowHint, ActionKind::Instruction, "never use pnpm")
		);
	}

	fn existing(id: &str, action: ActionKind, rec: &str, evidence: &[&str]) -> Existing {
		Existing { id: id.into(), action, tokens: tokens(rec), evidence: set(evidence) }
	}

	#[test]
	fn a_reworded_recommendation_over_the_same_evidence_merges() {
		let stored = [existing(
			"c1",
			ActionKind::Instruction,
			"Always run pnpm lint before committing",
			&["e1", "e2"],
		)];
		let new = tokens("Run pnpm lint prior to every commit");
		let hit = find_merge_target(&stored, ActionKind::Instruction, &new, &set(&["e1", "e2"]));
		assert_eq!(hit.map(|e| e.id.as_str()), Some("c1"));
	}

	#[test]
	fn the_same_evidence_with_unrelated_advice_does_not_merge() {
		let stored = [existing("c1", ActionKind::Instruction, "Always run pnpm lint", &["e1"])];
		let new = tokens("Prefer composition over inheritance in services");
		assert!(find_merge_target(&stored, ActionKind::Instruction, &new, &set(&["e1"])).is_none());
	}

	#[test]
	fn similar_advice_over_different_evidence_does_not_merge() {
		let stored = [existing("c1", ActionKind::Instruction, "Always run pnpm lint", &["e1"])];
		let new = tokens("Always run pnpm lint");
		assert!(find_merge_target(&stored, ActionKind::Instruction, &new, &set(&["e9"])).is_none());
	}

	#[test]
	fn a_different_action_never_merges() {
		let stored = [existing("c1", ActionKind::Instruction, "Always run pnpm lint", &["e1"])];
		let new = tokens("Always run pnpm lint");
		assert!(
			find_merge_target(&stored, ActionKind::TestSuggestion, &new, &set(&["e1"])).is_none()
		);
	}

	#[test]
	fn opposite_advice_over_the_same_evidence_does_not_merge() {
		let stored =
			[existing("c1", ActionKind::Instruction, "Always use unwrap in tests", &["e1"])];
		for opposite in
			["Never use unwrap in tests", "Don't use unwrap in tests", "Avoid unwrap in tests"]
		{
			let new = tokens(opposite);
			assert!(
				jaccard(&stored[0].tokens, &new) >= 0.3,
				"the case must be one the similarity rule alone would merge: {opposite}"
			);
			assert!(
				find_merge_target(&stored, ActionKind::Instruction, &new, &set(&["e1"])).is_none(),
				"{opposite} merged into its opposite"
			);
		}
	}
}
