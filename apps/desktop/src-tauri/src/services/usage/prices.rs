//! API list prices, for the status bar's estimated session cost.
//!
//! USD per million tokens, standard (non-batch, non-fast) short-context rates.
//! Sources, read on 2026-10-19:
//! - Anthropic: https://docs.claude.com/en/docs/about-claude/pricing
//!   (`cache_write` is the 5-minute write price — a 1-hour write is twice the
//!   input price, applied in `session_cost`; Haiku 5.5 is the up-to-100k
//!   prompt tier).
//! - OpenAI: https://platform.openai.com/docs/pricing (`cache_read` is the
//!   "Cached input" column). The page lists no cache-write price for
//!   `gpt-5.3-codex`, so its `cache_write` is its input price.
//!
//! This is an estimate at list price: subscriptions are not billed this way.

/// Prices of one model, USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
	pub input: f64,
	pub output: f64,
	pub cache_read: f64,
	pub cache_write: f64,
}

const fn p(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Price {
	Price { input, output, cache_read, cache_write }
}

/// Model id prefix → price. The longest matching prefix wins, so
/// `claude-opus-4-1` is not read as `claude-opus-4`.
const TABLE: &[(&str, Price)] = &[
	// Anthropic.
	("claude-fable-5-1", p(10.0, 50.0, 0.25, 12.5)),
	("claude-fable-5", p(10.0, 50.0, 1.0, 12.5)),
	("claude-mythos-5-1", p(10.0, 50.0, 0.25, 12.5)),
	("claude-mythos-5", p(10.0, 50.0, 1.0, 12.5)),
	("claude-opus-5-5", p(4.0, 20.0, 0.20, 5.0)),
	("claude-opus-5", p(5.0, 25.0, 0.50, 6.25)),
	("claude-opus-4-8", p(5.0, 25.0, 0.50, 6.25)),
	("claude-opus-4-7", p(5.0, 25.0, 0.50, 6.25)),
	("claude-opus-4-6", p(5.0, 25.0, 0.50, 6.25)),
	("claude-opus-4-5", p(5.0, 25.0, 0.50, 6.25)),
	("claude-opus-4-1", p(15.0, 75.0, 1.50, 18.75)),
	("claude-opus-4", p(15.0, 75.0, 1.50, 18.75)),
	("claude-sonnet-5-5", p(2.0, 10.0, 0.10, 2.5)),
	("claude-sonnet-5", p(2.0, 10.0, 0.20, 2.5)),
	("claude-sonnet-4-6", p(3.0, 15.0, 0.30, 3.75)),
	("claude-sonnet-4-5", p(3.0, 15.0, 0.30, 3.75)),
	("claude-sonnet-4", p(3.0, 15.0, 0.30, 3.75)),
	("claude-haiku-5-5", p(0.10, 0.50, 0.01, 0.125)),
	("claude-haiku-4-5", p(1.0, 5.0, 0.10, 1.25)),
	// OpenAI (Codex).
	("gpt-5.3-codex", p(1.75, 14.0, 0.175, 1.75)),
	("gpt-5.6-sol", p(4.0, 20.0, 0.40, 5.0)),
	("gpt-5.6-cyber", p(12.5, 75.0, 1.25, 15.625)),
	("gpt-6-astra", p(10.0, 50.0, 1.0, 12.5)),
	("gpt-6.1-sol", p(2.0, 10.0, 0.10, 2.5)),
	("gpt-6-luna", p(0.10, 0.50, 0.01, 0.125)),
];

/// Whether `rest` (what follows a matched prefix) is a boundary: nothing, a
/// `@`/`[` suffix, or a `-YYYYMMDD` date. So an unlisted `claude-opus-4-9` does
/// not silently take `claude-opus-4`'s price.
fn at_boundary(rest: &str) -> bool {
	match rest.as_bytes().first() {
		None | Some(b'@') | Some(b'[') => true,
		Some(b'-') => {
			rest.as_bytes()[1..].iter().take(8).filter(|b| b.is_ascii_digit()).count() == 8
		}
		_ => false,
	}
}

/// The price of `model`, matching on the longest table prefix so dated ids
/// (`claude-sonnet-4-5-20250929`) resolve. `None` for a model not listed.
pub fn price_for(model: &str) -> Option<Price> {
	let model = model.to_ascii_lowercase();
	TABLE
		.iter()
		.filter(|(prefix, _)| model.strip_prefix(*prefix).is_some_and(at_boundary))
		.max_by_key(|(prefix, _)| prefix.len())
		.map(|(_, price)| *price)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn price_prefix_matches_dated_ids() {
		let s = price_for("claude-sonnet-4-5-20250929").unwrap();
		assert_eq!((s.input, s.output, s.cache_read, s.cache_write), (3.0, 15.0, 0.30, 3.75));
		// The longest prefix wins: 4.1 is not 4.
		assert_eq!(price_for("claude-opus-4-1-20250805").unwrap().input, 15.0);
		assert_eq!(price_for("claude-opus-4-20250514").unwrap().input, 15.0);
		assert_eq!(price_for("claude-opus-4-8").unwrap().input, 5.0);
		assert_eq!(price_for("claude-opus-5-5").unwrap().output, 20.0);
		assert_eq!(price_for("gpt-5.6-sol").unwrap().cache_read, 0.40);
		assert_eq!(price_for("claude-opus-4-9"), None);
		assert_eq!(price_for("claude-opus-4-8[1m]").unwrap().input, 5.0);
		assert_eq!(price_for("gpt-9-unknown"), None);
		assert_eq!(price_for(""), None);
	}
}
