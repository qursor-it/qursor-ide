//! Best-effort secret redaction, and the shaping every excerpt goes through.
//!
//! **This is not data-loss prevention.** It catches the shapes secrets usually
//! have — provider key prefixes, bearer tokens, `password=` assignments, private
//! key blocks, credentials in URLs, long opaque tokens — and replaces them with
//! a placeholder before text is stored, shown in the preview or sent anywhere.
//! A secret that looks like prose gets through; the preview exists so a person
//! can see what would be sent and remove it (ADR-0084).
//!
//! Over-redaction is the accepted failure direction: `token: string` in a
//! TypeScript type loses the word `string`, and a 40-hex commit id is replaced.

/// The placeholder for a value that was removed.
pub const REDACTED: &str = "[REDACTED]";

/// Token prefixes with the shortest tail that makes a match credible. A prefix
/// only counts at the start of a word, so `task-` is not `sk-`.
const PREFIXES: &[(&str, usize, TailKind, &str)] = &[
	("sk-", 16, TailKind::Token, "api-key"),
	("sk_live_", 16, TailKind::Token, "api-key"),
	("sk_test_", 16, TailKind::Token, "api-key"),
	("pk_live_", 16, TailKind::Token, "api-key"),
	("rk_live_", 16, TailKind::Token, "api-key"),
	("ghp_", 20, TailKind::Token, "github-token"),
	("gho_", 20, TailKind::Token, "github-token"),
	("ghu_", 20, TailKind::Token, "github-token"),
	("ghs_", 20, TailKind::Token, "github-token"),
	("ghr_", 20, TailKind::Token, "github-token"),
	("github_pat_", 20, TailKind::Token, "github-token"),
	("glpat-", 16, TailKind::Token, "gitlab-token"),
	("xoxb-", 10, TailKind::Token, "slack-token"),
	("xoxp-", 10, TailKind::Token, "slack-token"),
	("xoxa-", 10, TailKind::Token, "slack-token"),
	("xoxr-", 10, TailKind::Token, "slack-token"),
	("xoxs-", 10, TailKind::Token, "slack-token"),
	("AKIA", 16, TailKind::Upper, "aws-key"),
	("ASIA", 16, TailKind::Upper, "aws-key"),
	("AIza", 30, TailKind::Token, "google-key"),
	("npm_", 30, TailKind::Token, "npm-token"),
	("hf_", 30, TailKind::Token, "hf-token"),
	("pypi-", 30, TailKind::Token, "pypi-token"),
	("eyJ", 30, TailKind::Jwt, "jwt"),
];

#[derive(Clone, Copy)]
enum TailKind {
	/// `[A-Za-z0-9_-]`
	Token,
	/// `[A-Z0-9]`
	Upper,
	/// `[A-Za-z0-9_.-]`
	Jwt,
}

impl TailKind {
	fn accepts(self, c: char) -> bool {
		match self {
			Self::Token => c.is_ascii_alphanumeric() || c == '_' || c == '-',
			Self::Upper => c.is_ascii_uppercase() || c.is_ascii_digit(),
			Self::Jwt => c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'),
		}
	}
}

/// Words that make the value after `=` / `:` a secret.
const SENSITIVE_KEYS: &[&str] = &[
	"password",
	"passwd",
	"pwd",
	"passphrase",
	"secret",
	"token",
	"apikey",
	"api_key",
	"api-key",
	"access_key",
	"accesskey",
	"private_key",
	"privatekey",
	"credential",
	"authorization",
];

/// Remove secret-shaped substrings. Pure, and idempotent on its own output.
pub fn redact(text: &str) -> String {
	let mut out = redact_private_keys(text);
	out = redact_url_credentials(&out);
	out = redact_bearer(&out);
	for (prefix, min_tail, tail, label) in PREFIXES {
		out = redact_prefixed(&out, prefix, *min_tail, *tail, label);
	}
	out = redact_assignments(&out);
	out = redact_flags(&out);
	redact_long_tokens(&out)
}

/// One redacted, single-line, bounded string — what an excerpt is.
///
/// Redaction runs before truncation so a secret cut in half by the limit cannot
/// leave its first half behind. The input is pre-cut to a generous multiple of
/// the limit first, only so a megabyte tool result is not scanned to keep 280
/// characters of it.
pub fn excerpt(text: &str, max_chars: usize) -> String {
	let head: String = text.chars().take(max_chars.saturating_mul(8).max(512)).collect();
	let redacted = redact(&strip_ansi(&head));
	let flat: String = redacted.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
	let single = flat.split_whitespace().collect::<Vec<_>>().join(" ");
	truncate_chars(&single, max_chars)
}

/// Terminal colour and cursor sequences removed. A test runner's output reaches
/// a transcript exactly as it was printed, and `ESC[31m` is not part of what
/// anyone said.
pub fn strip_ansi(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut chars = text.chars().peekable();
	while let Some(c) = chars.next() {
		if c != '\u{1b}' {
			out.push(c);
			continue;
		}
		match chars.peek() {
			// CSI: parameters, then a final byte in 0x40..=0x7e.
			Some('[') => {
				chars.next();
				for next in chars.by_ref() {
					if ('\u{40}'..='\u{7e}').contains(&next) {
						break;
					}
				}
			}
			// OSC: up to BEL or ST.
			Some(']') => {
				chars.next();
				while let Some(next) = chars.next() {
					if next == '\u{7}' || (next == '\u{1b}' && chars.peek() == Some(&'\\')) {
						if next == '\u{1b}' {
							chars.next();
						}
						break;
					}
				}
			}
			Some(_) => {
				chars.next();
			}
			None => {}
		}
	}
	out
}

/// `s` cut to at most `max` characters, ending in `…` when it was cut. Never
/// splits a character.
pub fn truncate_chars(s: &str, max: usize) -> String {
	if s.chars().count() <= max {
		return s.to_string();
	}
	let keep = max.saturating_sub(1);
	let mut cut: String = s.chars().take(keep).collect();
	while cut.ends_with(char::is_whitespace) {
		cut.pop();
	}
	cut.push('…');
	cut
}

fn redact_private_keys(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut rest = text;
	loop {
		let Some(begin) = rest.find("-----BEGIN") else {
			out.push_str(rest);
			return out;
		};
		let header_window: String = rest[begin..].chars().take(80).collect();
		if !header_window.contains("PRIVATE KEY-----") {
			out.push_str(&rest[..begin + "-----BEGIN".len()]);
			rest = &rest[begin + "-----BEGIN".len()..];
			continue;
		}
		out.push_str(&rest[..begin]);
		out.push_str("[REDACTED:private-key]");
		let after_begin = &rest[begin + "-----BEGIN".len()..];
		match after_begin.find("-----END") {
			Some(end) => {
				let tail = &after_begin[end + "-----END".len()..];
				match tail.find("-----") {
					Some(close) => rest = &tail[close + "-----".len()..],
					None => return out,
				}
			}
			// A key cut off by a truncated tool result: everything after the
			// header is key material.
			None => return out,
		}
	}
}

/// `scheme://user:pass@host` loses `user:pass`.
fn redact_url_credentials(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(at) = rest.find("://") {
		let authority_start = at + 3;
		out.push_str(&rest[..authority_start]);
		rest = &rest[authority_start..];
		let end = rest
			.find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace() || c == '"')
			.unwrap_or(rest.len());
		let authority = &rest[..end];
		match authority.rfind('@') {
			Some(sep) if authority[..sep].contains(':') => {
				out.push_str(REDACTED);
				out.push_str(&authority[sep..]);
			}
			_ => out.push_str(authority),
		}
		rest = &rest[end..];
	}
	out.push_str(rest);
	out
}

/// `Bearer <token>` in any case.
fn redact_bearer(text: &str) -> String {
	let lower = text.to_ascii_lowercase();
	let mut out = String::with_capacity(text.len());
	let mut cursor = 0;
	let mut search = 0;
	while let Some(rel) = lower[search..].find("bearer ") {
		let start = search + rel;
		let boundary = text[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
		let value_start = start + "bearer ".len();
		let value_len: usize = text[value_start..]
			.chars()
			.take_while(|c| {
				c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '~' | '+' | '/' | '=' | '-')
			})
			.map(char::len_utf8)
			.sum();
		if boundary && value_len >= 8 {
			out.push_str(&text[cursor..value_start]);
			out.push_str(REDACTED);
			cursor = value_start + value_len;
			search = cursor;
		} else {
			search = value_start;
		}
	}
	out.push_str(&text[cursor..]);
	out
}

fn redact_prefixed(
	text: &str,
	prefix: &str,
	min_tail: usize,
	tail: TailKind,
	label: &str,
) -> String {
	let mut out = String::with_capacity(text.len());
	let mut cursor = 0;
	let mut search = 0;
	while let Some(rel) = text[search..].find(prefix) {
		let start = search + rel;
		let boundary = text[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
		let tail_start = start + prefix.len();
		let tail_chars = text[tail_start..].chars().take_while(|c| tail.accepts(*c));
		let (count, bytes) =
			tail_chars.fold((0usize, 0usize), |(n, b), c| (n + 1, b + c.len_utf8()));
		let jwt_shaped = !matches!(tail, TailKind::Jwt)
			|| text[tail_start..tail_start + bytes].matches('.').count() >= 2;
		if boundary && count >= min_tail && jwt_shaped {
			out.push_str(&text[cursor..start]);
			out.push_str("[REDACTED:");
			out.push_str(label);
			out.push(']');
			cursor = tail_start + bytes;
			search = cursor;
		} else {
			search = tail_start;
		}
	}
	out.push_str(&text[cursor..]);
	out
}

/// `password=hunter2`, `"api_key": "abc"`, `MY_SECRET: abc`.
fn redact_assignments(text: &str) -> String {
	let mut ranges: Vec<(usize, usize)> = Vec::new();
	let mut last_end = 0;
	for (sep, c) in text.char_indices() {
		if (c != '=' && c != ':') || sep < last_end {
			continue;
		}
		let Some(key) = key_before(text, sep) else { continue };
		let lower = key.to_ascii_lowercase();
		if !SENSITIVE_KEYS.iter().any(|k| lower.contains(k)) {
			continue;
		}
		let mut value_start = sep + 1;
		// `==`, `=>` and `:=` are operators spelled with the same character.
		while text[value_start..].starts_with(['=', '>']) {
			value_start += 1;
		}
		value_start +=
			text[value_start..].len() - text[value_start..].trim_start_matches([' ', '\t']).len();
		if text[value_start..].starts_with(['"', '\'']) {
			value_start += 1;
		}
		let to_line_end = lower.contains("authorization");
		let value_len: usize = text[value_start..]
			.chars()
			.take_while(|c| {
				if to_line_end {
					*c != '\n' && *c != '"' && *c != '\''
				} else {
					!c.is_whitespace()
						&& !matches!(c, '"' | '\'' | ',' | ';' | '&' | ')' | '}' | ']' | '<' | '`')
				}
			})
			.map(char::len_utf8)
			.sum();
		let value = &text[value_start..value_start + value_len];
		if value.is_empty() || value.starts_with("[REDACTED") {
			continue;
		}
		ranges.push((value_start, value_start + value_len));
		last_end = value_start + value_len;
	}
	apply_ranges(text, &ranges, REDACTED)
}

/// The identifier just before a separator, skipping spaces and a closing quote.
fn key_before(text: &str, sep: usize) -> Option<&str> {
	let before = text[..sep].trim_end_matches([' ', '\t']);
	let before = before.trim_end_matches(['"', '\'']);
	let start = before
		.char_indices()
		.rev()
		.take_while(|(_, c)| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
		.last()
		.map(|(i, _)| i)?;
	let key = &before[start..];
	(!key.is_empty()).then_some(key)
}

/// `--password hunter2`: the value is the next word.
fn redact_flags(text: &str) -> String {
	let mut ranges: Vec<(usize, usize)> = Vec::new();
	let mut words = word_spans(text).peekable();
	while let Some((s, e)) = words.next() {
		let word = text[s..e].to_ascii_lowercase();
		if !word.starts_with("--") || word.contains('=') {
			continue;
		}
		if !SENSITIVE_KEYS.iter().any(|k| word.contains(k)) {
			continue;
		}
		if let Some(&(ns, ne)) = words.peek() {
			let next = &text[ns..ne];
			if !next.starts_with('-') && !next.starts_with("[REDACTED") {
				ranges.push((ns, ne));
			}
		}
	}
	apply_ranges(text, &ranges, REDACTED)
}

/// A 40-plus character run of letters and digits that is not a word: an opaque
/// token, a hash, a base64 blob.
fn redact_long_tokens(text: &str) -> String {
	let mut ranges: Vec<(usize, usize)> = Vec::new();
	let mut start: Option<usize> = None;
	let is_token_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=' | '-');
	let consider = |s: usize, e: usize, ranges: &mut Vec<(usize, usize)>| {
		let run = &text[s..e];
		let long_enough = run.chars().count() >= 40;
		let has_digit = run.chars().any(|c| c.is_ascii_digit());
		let has_letter = run.chars().any(|c| c.is_ascii_alphabetic());
		if long_enough && has_digit && has_letter {
			ranges.push((s, e));
		}
	};
	for (i, c) in text.char_indices() {
		match (is_token_char(c), start) {
			(true, None) => start = Some(i),
			(false, Some(s)) => {
				consider(s, i, &mut ranges);
				start = None;
			}
			_ => {}
		}
	}
	if let Some(s) = start {
		consider(s, text.len(), &mut ranges);
	}
	apply_ranges(text, &ranges, "[REDACTED:token]")
}

fn word_spans(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
	let mut start: Option<usize> = None;
	let mut chars = text.char_indices();
	let mut done = false;
	std::iter::from_fn(move || {
		if done {
			return None;
		}
		loop {
			match chars.next() {
				Some((i, c)) if c.is_whitespace() => {
					if let Some(s) = start.take() {
						return Some((s, i));
					}
				}
				Some((i, _)) => {
					start.get_or_insert(i);
				}
				None => {
					done = true;
					return start.take().map(|s| (s, text.len()));
				}
			}
		}
	})
}

fn apply_ranges(text: &str, ranges: &[(usize, usize)], replacement: &str) -> String {
	if ranges.is_empty() {
		return text.to_string();
	}
	let mut out = String::with_capacity(text.len());
	let mut cursor = 0;
	for &(s, e) in ranges {
		if s < cursor {
			continue;
		}
		out.push_str(&text[cursor..s]);
		out.push_str(replacement);
		cursor = e;
	}
	out.push_str(&text[cursor..]);
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn gone(input: &str, secret: &str) {
		let out = redact(input);
		assert!(!out.contains(secret), "{secret:?} survived in {out:?}");
		assert!(out.contains("[REDACTED"), "no placeholder in {out:?}");
	}

	#[test]
	fn provider_key_prefixes_are_removed() {
		gone("export KEY=sk-ant-api03-abcdefghijklmnopqrstuvwx", "abcdefghijklmnop");
		gone("use sk-proj-abcdefghijklmnopqrstuv for it", "abcdefghijklmnop");
		gone("token ghp_abcdefghijklmnopqrstuvwxyz0123456789", "abcdefghijklmnop");
		gone("github_pat_11ABCDEFG0123456789_abcdefghijklmnop", "11ABCDEFG0123");
		gone("aws AKIAIOSFODNN7EXAMPLE here", "IOSFODNN7EXAMPLE");
		gone("slack xoxb-123456789012-abcdefghijkl", "123456789012");
		gone("google AIzaSyA-abcdefghijklmnopqrstuvwxyz012345", "abcdefghijklmnop");
	}

	#[test]
	fn a_prefix_inside_a_word_or_with_a_short_tail_is_left_alone() {
		let text = "install sk-learn, then ask-me-anything and task-runner";
		assert_eq!(redact(text), text);
		assert_eq!(redact("ghp_short"), "ghp_short");
	}

	#[test]
	fn bearer_tokens_in_any_case() {
		gone("Authorization: Bearer abcDEF123456.ghi-jkl_mno", "abcDEF123456");
		gone("curl -H 'authorization: bearer abcdef1234567890' x", "abcdef1234567890");
		assert_eq!(redact("a bearer of bad news"), "a bearer of bad news");
	}

	#[test]
	fn basic_authorization_loses_the_whole_value() {
		gone("Authorization: Basic dXNlcjpwYXNzd29yZA==", "dXNlcjpwYXNzd29yZA");
	}

	#[test]
	fn assignments_keep_the_name_and_lose_the_value() {
		let out = redact("DB_PASSWORD=hunter2 and api_key: abc123 then {\"secret\": \"s3cr3t\"}");
		assert!(out.contains("DB_PASSWORD=[REDACTED]"), "{out}");
		assert!(out.contains("api_key: [REDACTED]"), "{out}");
		assert!(out.contains("\"secret\": \"[REDACTED]"), "{out}");
		for leaked in ["hunter2", "abc123", "s3cr3t"] {
			assert!(!out.contains(leaked), "{leaked} survived in {out}");
		}
	}

	#[test]
	fn flags_that_take_a_secret_lose_the_next_word() {
		let out = redact("mytool --password hunter2 --verbose");
		assert!(!out.contains("hunter2"), "{out}");
		assert!(out.contains("--verbose"), "{out}");
	}

	#[test]
	fn url_credentials_are_removed_but_the_host_stays() {
		let out = redact("git clone https://user:pa55w0rd@github.com/acme/repo.git");
		assert!(!out.contains("pa55w0rd"), "{out}");
		assert!(out.contains("@github.com/acme/repo.git"), "{out}");
		assert_eq!(redact("https://user@host/x"), "https://user@host/x");
	}

	#[test]
	fn private_key_blocks_are_removed_whole() {
		let text = "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\nabcd\n-----END RSA PRIVATE KEY-----\nafter";
		let out = redact(text);
		assert!(!out.contains("MIIEow"), "{out}");
		assert!(out.contains("before") && out.contains("after"), "{out}");
		let cut = redact("x -----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBg");
		assert!(!cut.contains("MIIEvQ"), "{cut}");
	}

	#[test]
	fn a_public_certificate_block_is_not_a_private_key() {
		let text = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----";
		assert_eq!(redact(text), text);
	}

	#[test]
	fn jwts_and_long_opaque_tokens() {
		gone(
			"eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
			"dozjgNryP4J3",
		);
		gone("sha 0123456789abcdef0123456789abcdef01234567 end", "0123456789abcdef0123");
		let prose = "a_perfectly_ordinary_long_identifier_without_any_digits_at_all_here";
		assert_eq!(redact(prose), prose);
	}

	#[test]
	fn ordinary_text_is_untouched_and_redaction_is_idempotent() {
		let text = "Don't use unwrap() here; run `pnpm test` first.";
		assert_eq!(redact(text), text);
		let once = redact("password=hunter2 sk-abcdefghijklmnopqrstuv");
		assert_eq!(redact(&once), once);
	}

	#[test]
	fn terminal_escape_sequences_are_not_text() {
		assert_eq!(strip_ansi("\u{1b}[31mFAIL\u{1b}[0m src/a.ts"), "FAIL src/a.ts");
		assert_eq!(strip_ansi("a\u{1b}]0;title\u{7}b"), "ab");
		assert_eq!(excerpt("\u{1b}[1;31merror\u{1b}[0m: boom", 50), "error: boom");
	}

	#[test]
	fn an_excerpt_is_one_bounded_line() {
		let e = excerpt("line one\n\tline  two\u{7}\nline three", 280);
		assert_eq!(e, "line one line two line three");
		let long = excerpt(&"abc ".repeat(200), 50);
		assert_eq!(long.chars().count(), 50);
		assert!(long.ends_with('…'));
		let multibyte = excerpt(&"é".repeat(400), 10);
		assert_eq!(multibyte.chars().count(), 10);
	}

	#[test]
	fn a_secret_the_limit_would_cut_in_half_is_redacted_first() {
		let text = format!("{} password=hunter2hunter2", "x ".repeat(20));
		let e = excerpt(&text, 50);
		assert!(!e.contains("hunter"), "{e}");
	}
}
