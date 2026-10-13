//! Which lines of a pull request's diff a review comment may be left on (F31).
//!
//! GitHub accepts an inline comment only on a line that appears in the diff — a
//! changed line or its surrounding context — and answers anything else with a
//! 422. The renderer therefore needs the set, per side, before it offers a "+"
//! on a line; asking GitHub to find out costs a failed request per mistake.
//!
//! The set is derived from the `patch` of each file in `pulls/N/files`. The
//! patch text stays here: only the ranges cross the bridge.

use crate::models::CommentableLines;

/// `@@ -a[,b] +c[,d] @@` → `(a, c)`: where each side starts. Omitted counts
/// (`@@ -1 +1 @@`, a single-line hunk) need no handling because the counts are
/// not read at all.
fn parse_header(line: &str) -> Option<(u32, u32)> {
	let rest = line.strip_prefix("@@ -")?;
	let (old, rest) = rest.split_once(" +")?;
	let (new, _) = rest.split_once(" @@").or_else(|| rest.split_once(" @"))?;
	let old_start = old.split(',').next()?.parse().ok()?;
	let new_start = new.split(',').next()?.parse().ok()?;
	// Only the starts are used; counts are re-derived from the body, so a hunk
	// whose header disagrees with its lines (a truncated patch) is still read as
	// what it contains.
	Some((old_start, new_start))
}

/// Extend `ranges` by one line: inside the current hunk's run when contiguous.
fn push(ranges: &mut Vec<(u32, u32)>, run_open: &mut bool, line: u32) {
	match ranges.last_mut() {
		Some(last) if *run_open && last.1 + 1 == line => last.1 = line,
		_ => {
			ranges.push((line, line));
			*run_open = true;
		}
	}
}

/// Inclusive ranges of old-file (`left`) and new-file (`right`) line numbers per
/// hunk.
///
/// Walks the body rather than trusting the header's counts. A context line
/// advances both sides, `-` only the old, `+` only the new, and
/// `\ No newline at end of file` neither — it annotates the line before it.
pub fn commentable_lines(patch: &str) -> CommentableLines {
	let mut out = CommentableLines::default();
	let (mut old, mut new) = (0u32, 0u32);
	let mut in_hunk = false;
	let (mut left_open, mut right_open) = (false, false);

	for line in patch.lines() {
		if line.starts_with("@@") {
			match parse_header(line) {
				Some((o, n)) => {
					old = o;
					new = n;
					in_hunk = true;
					// A new hunk starts new ranges even if numbering happens to touch.
					left_open = false;
					right_open = false;
				}
				// A header we cannot read ends the hunk: reading on would number
				// lines from the previous one.
				None => in_hunk = false,
			}
			continue;
		}
		if !in_hunk {
			continue;
		}
		match line.as_bytes().first() {
			Some(b'\\') => {}
			Some(b'-') => {
				push(&mut out.left, &mut left_open, old);
				old += 1;
			}
			Some(b'+') => {
				push(&mut out.right, &mut right_open, new);
				new += 1;
			}
			// A context line starts with a space; an editor that trimmed trailing
			// whitespace leaves it empty, and it is still a context line.
			_ => {
				push(&mut out.left, &mut left_open, old);
				push(&mut out.right, &mut right_open, new);
				old += 1;
				new += 1;
			}
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn lines(patch: &str, left: Vec<(u32, u32)>, right: Vec<(u32, u32)>) -> bool {
		commentable_lines(patch) == CommentableLines { left, right }
	}

	#[test]
	fn patches_become_commentable_ranges() {
		let table = [
			(
				"modified lines with context",
				"@@ -10,5 +10,6 @@ fn x\n ctx\n ctx\n-old\n+new\n+added\n ctx\n ctx",
				vec![(10, 14)],
				vec![(10, 15)],
			),
			(
				"a new file has no old side",
				"@@ -0,0 +1,3 @@\n+a\n+b\n+c",
				vec![],
				vec![(1, 3)],
			),
			(
				"a deleted file has no new side",
				"@@ -1,2 +0,0 @@\n-a\n-b",
				vec![(1, 2)],
				vec![],
			),
			(
				"omitted counts mean one line",
				"@@ -1 +1 @@\n-before\n+after",
				vec![(1, 1)],
				vec![(1, 1)],
			),
			(
				"the no-newline marker is not a line",
				"@@ -1 +1 @@\n-before\n\\ No newline at end of file\n+after\n\\ No newline at end of file",
				vec![(1, 1)],
				vec![(1, 1)],
			),
			(
				"each hunk is its own range",
				"@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n@@ -20,2 +20,3 @@\n x\n+y\n z",
				vec![(1, 3), (20, 21)],
				vec![(1, 3), (20, 22)],
			),
			(
				"a content line that looks like a header or a file marker",
				"@@ -1,2 +1,2 @@\n--- old.txt\n+++ new.txt",
				vec![(1, 1)],
				vec![(1, 1)],
			),
			(
				"an empty context line (trailing space stripped) counts on both sides",
				"@@ -4,3 +4,3 @@\n a\n\n-b\n+c",
				vec![(4, 6)],
				vec![(4, 6)],
			),
		];
		for (name, patch, left, right) in table {
			assert!(lines(patch, left, right), "{name}: {:?}", commentable_lines(patch));
		}
	}

	#[test]
	fn nothing_is_commentable_without_a_hunk() {
		assert!(lines("", vec![], vec![]));
		assert!(lines("Binary files differ", vec![], vec![]));
		// An unreadable header ends the hunk instead of numbering from garbage.
		assert!(lines("@@ nonsense @@\n+a", vec![], vec![]));
	}

	#[test]
	fn a_header_with_trailing_context_text_reads() {
		assert_eq!(parse_header("@@ -12,7 +12,8 @@ impl Foo {"), Some((12, 12)));
		assert_eq!(parse_header("@@ -1 +1 @@"), Some((1, 1)));
		assert_eq!(parse_header("@@ -0,0 +1,3 @@"), Some((0, 1)));
		assert_eq!(parse_header("not a header"), None);
	}

	/// The captured pull request: a real added file's patch is one range on the
	/// right and none on the left.
	#[test]
	fn a_captured_added_file() {
		let files = include_str!(concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/../../../tests/fixtures/gh/pr-files-paginated.json"
		));
		let first: Vec<serde_json::Value> =
			serde_json::Deserializer::from_str(files).into_iter().next().unwrap().unwrap();
		let patch = first[0]["patch"].as_str().unwrap();
		let additions = first[0]["additions"].as_u64().unwrap() as u32;
		let c = commentable_lines(patch);
		assert!(c.left.is_empty());
		assert_eq!(c.right, vec![(1, additions)]);
	}
}
