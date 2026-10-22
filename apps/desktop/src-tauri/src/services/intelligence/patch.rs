//! Planning a change to a project's instruction file - **and never making it**.
//!
//! Nothing in this module writes. It reads the file, decides where a line goes,
//! and returns the proposed text, its hash and a unified diff for a person to
//! review. The only writer is the one the viewer already has, driven by a Save
//! the person presses (ADR-0039). What this module adds is the half of that
//! bargain that makes Save trustworthy: the target is resolved inside the
//! project, the hashes let the backend check afterwards that what was saved is
//! what was reviewed, and the inverse of a change is exactly the text that was
//! added.
//!
//! **Target.** `AGENTS.md` if it exists, else `CLAUDE.md`, else a proposal to
//! create `AGENTS.md`. A `CLAUDE.md` that is a link to `AGENTS.md` is one file,
//! not two. A name that is a link pointing out of the project, a directory, a
//! dangling link, or anything that is not UTF-8 text is refused: this is a
//! project instruction file or it is nothing.
//!
//! **The change is one appended block**, `- <recommendation>`, preceded by the
//! blank line the file needs to keep it a separate item. No rewrite of the
//! document, so the diff is a handful of lines and removing it is removing
//! those lines.

use std::path::{Path, PathBuf};

use super::dedupe;
use super::{err, sha256_hex, ErrorCode};
use crate::error::{AppError, AppResult};
use crate::models::intelligence::DuplicateHint;

/// An instruction file larger than this is not edited by a feature that appends
/// a line.
pub const MAX_INSTRUCTION_FILE_BYTES: u64 = 256 * 1024;
/// How much of the existing file the model is shown.
pub const PROMPT_INSTRUCTION_BYTES: usize = 6000;

const NAMES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

#[derive(Debug, Clone)]
pub struct Target {
	/// Canonical and absolute; for a file that does not exist yet, its canonical
	/// parent plus the name.
	pub path: PathBuf,
	/// As the project spells it: `AGENTS.md` or `CLAUDE.md`.
	pub relative: String,
	pub exists: bool,
	pub note: Option<String>,
	/// Other instruction files that are separate from the target, checked for an
	/// equivalent line but never changed.
	pub also_read: Vec<(String, PathBuf)>,
}

/// The file `name` in `root`, canonicalised and confined to it.
fn inspect(root: &Path, name: &str) -> AppResult<Option<PathBuf>> {
	let candidate = root.join(name);
	match std::fs::symlink_metadata(&candidate) {
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(e) => Err(AppError::Io(format!("{name}: {e}"))),
		Ok(_) => {
			let canonical = std::fs::canonicalize(&candidate).map_err(|e| {
				err(
					ErrorCode::UnsafeTarget,
					format!("{name} cannot be resolved ({e}); is it a broken link?"),
				)
			})?;
			if !canonical.starts_with(root) {
				return Err(err(
					ErrorCode::UnsafeTarget,
					format!("{name} points outside the project folder, so it will not be edited"),
				));
			}
			if !canonical.is_file() {
				return Err(err(ErrorCode::UnsafeTarget, format!("{name} is not a regular file")));
			}
			Ok(Some(canonical))
		}
	}
}

/// Where an instruction goes. `root` must already be canonical.
pub fn resolve_target(root: &Path) -> AppResult<Target> {
	let agents = inspect(root, NAMES[0])?;
	let claude = inspect(root, NAMES[1])?;
	Ok(match (agents, claude) {
		(Some(a), c) => {
			let (note, also_read) = match c {
				Some(c) if c == a => {
					(Some("CLAUDE.md is the same file as AGENTS.md.".to_string()), vec![])
				}
				Some(c) => (
					Some(
						"CLAUDE.md also exists as a separate file and is not changed.".to_string(),
					),
					vec![(NAMES[1].to_string(), c)],
				),
				None => (None, vec![]),
			};
			Target { path: a, relative: NAMES[0].into(), exists: true, note, also_read }
		}
		(None, Some(c)) => Target {
			path: c,
			relative: NAMES[1].into(),
			exists: true,
			note: None,
			also_read: vec![],
		},
		(None, None) => Target {
			path: root.join(NAMES[0]),
			relative: NAMES[0].into(),
			exists: false,
			note: Some(
				"The project has no instruction file yet; saving creates AGENTS.md. Claude Code \
				 reads CLAUDE.md, so it needs a CLAUDE.md that imports it (`@AGENTS.md`) to see this."
					.to_string(),
			),
			also_read: vec![],
		},
	})
}

/// A file's text, if it is the kind of file this edits.
pub fn read_text(path: &Path) -> AppResult<String> {
	let meta = std::fs::metadata(path)?;
	if meta.len() > MAX_INSTRUCTION_FILE_BYTES {
		return Err(err(
			ErrorCode::TargetTooLarge,
			format!(
				"{} is {} bytes; instruction files over {} are not edited here",
				path.display(),
				meta.len(),
				MAX_INSTRUCTION_FILE_BYTES
			),
		));
	}
	let bytes = std::fs::read(path)?;
	String::from_utf8(bytes).ok().filter(|text| !text.contains('\0')).ok_or_else(|| {
		err(ErrorCode::UnsafeTarget, format!("{} is not UTF-8 text", path.display()))
	})
}

/// The bytes of a file, for hashing what is on disk.
pub fn read_bytes_bounded(path: &Path) -> AppResult<Vec<u8>> {
	let meta = std::fs::metadata(path)?;
	if meta.len() > MAX_INSTRUCTION_FILE_BYTES * 4 {
		return Err(err(
			ErrorCode::TargetTooLarge,
			format!("{} is too large to verify", path.display()),
		));
	}
	Ok(std::fs::read(path)?)
}

/// `path` as a person should see it: Windows' `\\?\` prefix removed.
pub fn display_path(path: &Path) -> String {
	crate::agents::path_identity::display_path(&path.to_string_lossy())
}

/// Whether two paths are the same file once resolved.
pub fn same_file(a: &Path, b: &Path) -> bool {
	match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
		(Ok(a), Ok(b)) => a == b,
		_ => false,
	}
}

// ── The block ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
	/// Exactly what is appended: the separating blank line and the bullet.
	pub text: String,
	/// The bullet alone, newline included.
	pub line: String,
}

/// The block that adds `recommendation` to a file whose text is `base`.
pub fn build_block(base: &str, recommendation: &str) -> Block {
	let nl = if base.contains("\r\n") { "\r\n" } else { "\n" };
	let lead = if base.is_empty() || base.ends_with(&format!("{nl}{nl}")) {
		String::new()
	} else if base.ends_with('\n') {
		nl.to_string()
	} else {
		format!("{nl}{nl}")
	};
	let line = format!("- {recommendation}{nl}");
	Block { text: format!("{lead}{line}"), line }
}

/// `content` with `block` taken out again, or `None` if it is not there.
///
/// The common case is the block at the very end, exactly as it was added. If the
/// file has grown since, the bullet is removed from where it stands, together
/// with the blank line this feature put in front of it when that is still there.
pub fn remove_block(content: &str, block: &Block) -> Option<String> {
	if let Some(rest) = content.strip_suffix(block.text.as_str()) {
		return Some(rest.to_string());
	}
	let lead = block.text.strip_suffix(block.line.as_str()).unwrap_or_default();
	let mut search_end = content.len();
	while let Some(at) = content[..search_end].rfind(block.line.as_str()) {
		let on_a_line_of_its_own = at == 0 || content[..at].ends_with('\n');
		if on_a_line_of_its_own {
			let start = if !lead.is_empty() && content[..at].ends_with(lead) {
				at - lead.len()
			} else {
				at
			};
			return Some(format!("{}{}", &content[..start], &content[at + block.line.len()..]));
		}
		search_end = at;
	}
	None
}

// ── Duplicates ─────────────────────────────────────────────────────────────

/// A line in `files` that already says what `recommendation` says.
///
/// **Conservative**: most of the recommendation's words must be in the line, and
/// the line must not be so much longer that sharing them means nothing. A false
/// "duplicate" would withhold a patch the person wanted; the person can still
/// override it, but the default should rarely be wrong.
pub fn find_duplicate(files: &[(String, String)], recommendation: &str) -> Option<DuplicateHint> {
	let wanted = dedupe::tokens(recommendation);
	if wanted.len() < 2 {
		return None;
	}
	let mut best: Option<(f64, DuplicateHint)> = None;
	for (relative, content) in files {
		for (index, raw) in content.lines().enumerate() {
			let text = strip_marker(raw);
			if text.starts_with('#') || text.starts_with("```") {
				continue;
			}
			let have = dedupe::tokens(text);
			if have.len() < 2 {
				continue;
			}
			let shared = wanted.intersection(&have).count() as f64;
			let coverage = shared / wanted.len() as f64;
			let precision = shared / have.len() as f64;
			if coverage >= 0.8 && precision >= 0.3 {
				let score = coverage + precision;
				if best.as_ref().is_none_or(|(s, _)| score > *s) {
					best = Some((
						score,
						DuplicateHint {
							path: relative.clone(),
							line: (index + 1) as u32,
							text: super::redact::truncate_chars(text.trim(), 200),
						},
					));
				}
			}
		}
	}
	best.map(|(_, hint)| hint)
}

/// A line without its list marker: `- x`, `* x`, `1. x`.
fn strip_marker(line: &str) -> &str {
	let t = line.trim_start();
	let t = t.trim_start_matches(['-', '*', '+']).trim_start();
	let digits = t.chars().take_while(char::is_ascii_digit).count();
	if digits > 0 && t[digits..].starts_with(['.', ')']) {
		return t[digits + 1..].trim_start();
	}
	t
}

// ── The plan ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Ready {
	pub target: Target,
	pub base_hash: Option<String>,
	pub block: Block,
	pub proposed: String,
	pub proposed_hash: String,
	pub diff: String,
	pub duplicate: Option<DuplicateHint>,
}

#[derive(Debug)]
pub enum Plan {
	/// The file already says this; there is no patch.
	Duplicate {
		target: Target,
		hint: DuplicateHint,
	},
	Ready(Box<Ready>),
}

/// Plan adding `recommendation` under `root` (canonical). With
/// `allow_duplicate` a duplicate is still returned as a patch, carrying the hint.
pub fn plan_instruction(
	root: &Path,
	recommendation: &str,
	allow_duplicate: bool,
) -> AppResult<Plan> {
	let target = resolve_target(root)?;
	let base = if target.exists { read_text(&target.path)? } else { String::new() };
	let mut files = vec![(target.relative.clone(), base.clone())];
	for (relative, path) in &target.also_read {
		if let Ok(text) = read_text(path) {
			files.push((relative.clone(), text));
		}
	}
	let duplicate = find_duplicate(&files, recommendation);
	if let (Some(hint), false) = (&duplicate, allow_duplicate) {
		return Ok(Plan::Duplicate { target, hint: hint.clone() });
	}
	let block = build_block(&base, recommendation);
	let proposed = format!("{base}{}", block.text);
	let diff = unified_diff(&base, &proposed, &target.relative, target.exists);
	Ok(Plan::Ready(Box::new(Ready {
		base_hash: target.exists.then(|| sha256_hex(base.as_bytes())),
		proposed_hash: sha256_hex(proposed.as_bytes()),
		target,
		block,
		proposed,
		diff,
		duplicate,
	})))
}

// ── Unified diff ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
	Equal(usize, usize),
	Delete(usize),
	Insert(usize),
}

/// A unified diff from `old` to `new`, three lines of context, labelled with
/// `label`. `old_exists` false renders the old side as `/dev/null`.
pub fn unified_diff(old: &str, new: &str, label: &str, old_exists: bool) -> String {
	let a: Vec<&str> = old.split_inclusive('\n').collect();
	let b: Vec<&str> = new.split_inclusive('\n').collect();
	let ops = edit_script(&a, &b);
	if ops.iter().all(|op| matches!(op, Op::Equal(..))) {
		return String::new();
	}
	let mut out = String::new();
	if old_exists {
		out.push_str(&format!("--- a/{label}\n"));
	} else {
		out.push_str("--- /dev/null\n");
	}
	out.push_str(&format!("+++ b/{label}\n"));

	const CONTEXT: usize = 3;
	let changed: Vec<usize> = ops
		.iter()
		.enumerate()
		.filter(|(_, op)| !matches!(op, Op::Equal(..)))
		.map(|(i, _)| i)
		.collect();
	let mut hunks: Vec<(usize, usize)> = Vec::new();
	for &i in &changed {
		let start = i.saturating_sub(CONTEXT);
		let end = (i + CONTEXT + 1).min(ops.len());
		match hunks.last_mut() {
			Some(last) if start <= last.1 => last.1 = last.1.max(end),
			_ => hunks.push((start, end)),
		}
	}
	for (start, end) in hunks {
		let slice = &ops[start..end];
		let old_before = ops[..start].iter().filter(|op| !matches!(op, Op::Insert(_))).count();
		let new_before = ops[..start].iter().filter(|op| !matches!(op, Op::Delete(_))).count();
		let old_len = slice.iter().filter(|op| !matches!(op, Op::Insert(_))).count();
		let new_len = slice.iter().filter(|op| !matches!(op, Op::Delete(_))).count();
		let old_start = if old_len == 0 { old_before } else { old_before + 1 };
		let new_start = if new_len == 0 { new_before } else { new_before + 1 };
		out.push_str(&format!("@@ -{old_start},{old_len} +{new_start},{new_len} @@\n"));
		for op in slice {
			let (sign, line) = match *op {
				Op::Equal(i, _) => (' ', a[i]),
				Op::Delete(i) => ('-', a[i]),
				Op::Insert(j) => ('+', b[j]),
			};
			out.push(sign);
			out.push_str(line.strip_suffix('\n').unwrap_or(line).trim_end_matches('\r'));
			out.push('\n');
			if !line.ends_with('\n') {
				out.push_str("\\ No newline at end of file\n");
			}
		}
	}
	out
}

/// Shortest edit script between two line lists. Common head and tail are peeled
/// off first, so an append or a removal at the end of a file costs nothing; the
/// middle is a longest-common-subsequence table, or a wholesale replace if it
/// is too big to be worth one.
fn edit_script(a: &[&str], b: &[&str]) -> Vec<Op> {
	let head = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
	let tail =
		a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
	let (a_mid, b_mid) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
	let mut ops: Vec<Op> = (0..head).map(|i| Op::Equal(i, i)).collect();
	let (n, m) = (a_mid.len(), b_mid.len());
	if n.saturating_mul(m) > 4_000_000 {
		ops.extend((0..n).map(|i| Op::Delete(head + i)));
		ops.extend((0..m).map(|j| Op::Insert(head + j)));
	} else {
		let mut table = vec![vec![0u32; m + 1]; n + 1];
		for i in (0..n).rev() {
			for j in (0..m).rev() {
				table[i][j] = if a_mid[i] == b_mid[j] {
					table[i + 1][j + 1] + 1
				} else {
					table[i + 1][j].max(table[i][j + 1])
				};
			}
		}
		let (mut i, mut j) = (0, 0);
		while i < n || j < m {
			if i < n && j < m && a_mid[i] == b_mid[j] {
				ops.push(Op::Equal(head + i, head + j));
				i += 1;
				j += 1;
			} else if i < n && (j == m || table[i + 1][j] >= table[i][j + 1]) {
				// Deletions first on a tie, as `diff` prints them.
				ops.push(Op::Delete(head + i));
				i += 1;
			} else {
				ops.push(Op::Insert(head + j));
				j += 1;
			}
		}
	}
	ops.extend((0..tail).map(|k| Op::Equal(a.len() - tail + k, b.len() - tail + k)));
	ops
}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::TempDir;

	fn canonical(dir: &TempDir) -> PathBuf {
		std::fs::canonicalize(dir.path()).unwrap()
	}

	// ── block ──

	#[test]
	fn the_block_keeps_a_blank_line_between_it_and_the_text_before() {
		assert_eq!(build_block("", "Use pnpm").text, "- Use pnpm\n");
		assert_eq!(build_block("# Rules\n", "Use pnpm").text, "\n- Use pnpm\n");
		assert_eq!(build_block("# Rules\n\n", "Use pnpm").text, "- Use pnpm\n");
		assert_eq!(build_block("# Rules", "Use pnpm").text, "\n\n- Use pnpm\n");
		assert_eq!(build_block("# Rules\r\n", "Use pnpm").text, "\r\n- Use pnpm\r\n");
	}

	#[test]
	fn removing_the_block_gives_back_exactly_the_original() {
		for base in ["", "# Rules\n", "# Rules\n\n- one\n", "# Rules", "a\r\nb\r\n"] {
			let block = build_block(base, "Use pnpm, never npm");
			let changed = format!("{base}{}", block.text);
			assert_eq!(remove_block(&changed, &block).as_deref(), Some(base), "base {base:?}");
		}
	}

	#[test]
	fn the_block_is_removed_from_where_it_stands_if_the_file_grew_after_it() {
		let base = "# Rules\n";
		let block = build_block(base, "Use pnpm");
		let grown = format!("{base}{}\n## Later\n- other\n", block.text);
		let back = remove_block(&grown, &block).unwrap();
		assert_eq!(back, "# Rules\n\n## Later\n- other\n");
		assert!(!back.contains("Use pnpm"));
	}

	#[test]
	fn a_block_that_is_not_there_cannot_be_removed() {
		let block = build_block("# Rules\n", "Use pnpm");
		assert!(remove_block("# Rules\n- Something else\n", &block).is_none());
		// Mid-line text is not the bullet.
		assert!(remove_block("see - Use pnpm\n for more", &block).is_none());
	}

	// ── duplicates ──

	fn file(text: &str) -> Vec<(String, String)> {
		vec![("AGENTS.md".to_string(), text.to_string())]
	}

	#[test]
	fn an_equivalent_line_is_found_whatever_its_marker_or_wording() {
		let text = "# Rules\n\n- Use pnpm, never npm, in this repository\n";
		let hint = find_duplicate(&file(text), "Always use pnpm and never npm in this repository")
			.unwrap();
		assert_eq!((hint.path.as_str(), hint.line), ("AGENTS.md", 3));
		assert!(hint.text.contains("pnpm"));
		assert!(find_duplicate(
			&file("1. Use pnpm, never npm, in this repository"),
			"use pnpm never npm in this repository"
		)
		.is_some());
	}

	#[test]
	fn an_unrelated_or_merely_similar_line_is_not_a_duplicate() {
		let text =
			"- Use pnpm for installs\n- Keep functions small\n# Use pnpm never npm repository\n";
		assert!(find_duplicate(&file(text), "Run the formatter before committing").is_none());
		// A long paragraph that happens to contain the words is not the instruction.
		let long = "Our onboarding doc explains how the team came to use pnpm instead of npm, the history of the monorepo, the release process, the CI layout, the review rota and many other things in this repository";
		assert!(find_duplicate(&file(long), "use pnpm never npm repository").is_none());
		// Headings are not instructions.
		assert!(find_duplicate(
			&file("# Use pnpm never npm in this repository"),
			"Use pnpm never npm in this repository"
		)
		.is_none());
	}

	// ── target ──

	#[test]
	fn agents_md_is_preferred_then_claude_md_then_a_new_agents_md() {
		let dir = TempDir::new().unwrap();
		let root = canonical(&dir);
		let none = resolve_target(&root).unwrap();
		assert!(!none.exists);
		assert_eq!(none.relative, "AGENTS.md");
		assert!(none.note.as_deref().unwrap().contains("CLAUDE.md"));

		std::fs::write(root.join("CLAUDE.md"), "x\n").unwrap();
		let only_claude = resolve_target(&root).unwrap();
		assert_eq!((only_claude.relative.as_str(), only_claude.exists), ("CLAUDE.md", true));

		std::fs::write(root.join("AGENTS.md"), "y\n").unwrap();
		let both = resolve_target(&root).unwrap();
		assert_eq!(both.relative, "AGENTS.md");
		assert!(both.note.as_deref().unwrap().contains("separate"));
		assert_eq!(both.also_read.len(), 1);
	}

	#[cfg(unix)]
	#[test]
	fn a_claude_md_that_links_to_agents_md_is_one_file() {
		let dir = TempDir::new().unwrap();
		let root = canonical(&dir);
		std::fs::write(root.join("AGENTS.md"), "rules\n").unwrap();
		std::os::unix::fs::symlink("AGENTS.md", root.join("CLAUDE.md")).unwrap();
		let t = resolve_target(&root).unwrap();
		assert_eq!(t.relative, "AGENTS.md");
		assert!(t.also_read.is_empty(), "the alias must not be read as a second file");
		assert!(t.note.as_deref().unwrap().contains("same file"));
	}

	#[cfg(unix)]
	#[test]
	fn a_link_that_leaves_the_project_is_refused() {
		let dir = TempDir::new().unwrap();
		let outside = TempDir::new().unwrap();
		let root = canonical(&dir);
		std::fs::write(outside.path().join("secret.md"), "outside\n").unwrap();
		std::os::unix::fs::symlink(outside.path().join("secret.md"), root.join("AGENTS.md"))
			.unwrap();
		let e = resolve_target(&root).unwrap_err();
		assert!(e.to_string().contains("outside the project"), "{e}");
	}

	#[cfg(unix)]
	#[test]
	fn a_dangling_link_and_a_directory_are_refused() {
		let dir = TempDir::new().unwrap();
		let root = canonical(&dir);
		std::os::unix::fs::symlink("nowhere.md", root.join("AGENTS.md")).unwrap();
		assert!(resolve_target(&root).is_err());
		std::fs::remove_file(root.join("AGENTS.md")).unwrap();
		std::fs::create_dir(root.join("AGENTS.md")).unwrap();
		assert!(resolve_target(&root).is_err());
	}

	#[test]
	fn text_that_is_not_utf8_or_is_huge_is_not_edited() {
		let dir = TempDir::new().unwrap();
		let root = canonical(&dir);
		std::fs::write(root.join("AGENTS.md"), [0xff, 0xfe, 0x00, 0x41]).unwrap();
		assert!(plan_instruction(&root, "Use pnpm", false).is_err());
		std::fs::write(
			root.join("AGENTS.md"),
			vec![b'a'; (MAX_INSTRUCTION_FILE_BYTES + 1) as usize],
		)
		.unwrap();
		assert!(plan_instruction(&root, "Use pnpm", false).is_err());
	}

	// ── plan ──

	#[test]
	fn planning_returns_the_patch_and_writes_nothing() {
		let dir = TempDir::new().unwrap();
		let root = canonical(&dir);
		let original = "# Rules\n\n- Keep functions small\n";
		std::fs::write(root.join("AGENTS.md"), original).unwrap();
		let Plan::Ready(ready) = plan_instruction(&root, "Use pnpm, never npm", false).unwrap()
		else {
			panic!("expected a patch");
		};
		assert_eq!(ready.proposed, format!("{original}\n- Use pnpm, never npm\n"));
		assert_eq!(ready.base_hash.as_deref(), Some(sha256_hex(original.as_bytes()).as_str()));
		assert_eq!(ready.proposed_hash, sha256_hex(ready.proposed.as_bytes()));
		assert!(ready.diff.starts_with("--- a/AGENTS.md\n+++ b/AGENTS.md\n"), "{}", ready.diff);
		assert!(ready.diff.contains("+- Use pnpm, never npm"), "{}", ready.diff);
		assert_eq!(
			std::fs::read_to_string(root.join("AGENTS.md")).unwrap(),
			original,
			"planning must not write"
		);
	}

	#[test]
	fn planning_for_a_missing_file_proposes_creating_it_without_creating_it() {
		let dir = TempDir::new().unwrap();
		let root = canonical(&dir);
		let Plan::Ready(ready) = plan_instruction(&root, "Use pnpm", false).unwrap() else {
			panic!()
		};
		assert!(!ready.target.exists);
		assert!(ready.base_hash.is_none());
		assert_eq!(ready.proposed, "- Use pnpm\n");
		assert!(ready.diff.starts_with("--- /dev/null\n+++ b/AGENTS.md\n"), "{}", ready.diff);
		assert!(!root.join("AGENTS.md").exists());
	}

	#[test]
	fn a_duplicate_gives_no_patch_unless_the_person_overrides() {
		let dir = TempDir::new().unwrap();
		let root = canonical(&dir);
		std::fs::write(root.join("AGENTS.md"), "- Use pnpm, never npm, in this repository\n")
			.unwrap();
		match plan_instruction(&root, "Use pnpm never npm in this repository", false).unwrap() {
			Plan::Duplicate { hint, .. } => assert_eq!(hint.line, 1),
			Plan::Ready(_) => panic!("a duplicate must not yield a patch"),
		}
		let Plan::Ready(forced) =
			plan_instruction(&root, "Use pnpm never npm in this repository", true).unwrap()
		else {
			panic!("override should yield the patch")
		};
		assert!(forced.duplicate.is_some());
	}

	// ── diff ──

	#[test]
	fn an_identical_pair_has_no_diff() {
		assert_eq!(unified_diff("a\nb\n", "a\nb\n", "f", true), "");
	}

	#[test]
	fn an_append_shows_its_context_and_correct_counts() {
		let old = "1\n2\n3\n4\n5\n6\n";
		let new = "1\n2\n3\n4\n5\n6\n\n- added\n";
		let diff = unified_diff(old, new, "AGENTS.md", true);
		assert_eq!(
			diff,
			"--- a/AGENTS.md\n+++ b/AGENTS.md\n@@ -4,3 +4,5 @@\n 4\n 5\n 6\n+\n+- added\n"
		);
	}

	#[test]
	fn the_inverse_diff_is_the_mirror_image() {
		let old = "1\n2\n3\n";
		let new = "1\n2\n3\n\n- added\n";
		let removal = unified_diff(new, old, "f", true);
		assert!(removal.contains("-\n-- added\n"), "{removal}");
		assert!(removal.contains("@@ -1,5 +1,3 @@"), "{removal}");
	}

	#[test]
	fn a_missing_final_newline_is_called_out() {
		let diff = unified_diff("a\nb", "a\nb\n", "f", true);
		assert!(diff.contains("-b\n\\ No newline at end of file\n+b\n"), "{diff}");
	}

	#[test]
	fn a_change_in_the_middle_and_far_apart_changes_make_separate_hunks() {
		let old: String = (1..=30).map(|n| format!("line {n}\n")).collect();
		let new =
			old.replace("line 3\n", "line three\n").replace("line 28\n", "line twenty-eight\n");
		let diff = unified_diff(&old, &new, "f", true);
		assert_eq!(diff.matches("@@ ").count(), 2, "{diff}");
		assert!(diff.contains("-line 3\n+line three\n"));
		assert!(diff.contains("-line 28\n+line twenty-eight\n"));
	}

	#[test]
	fn a_new_file_diff_starts_at_zero() {
		let diff = unified_diff("", "- Use pnpm\n", "AGENTS.md", false);
		assert!(diff.contains("@@ -0,0 +1,1 @@\n+- Use pnpm\n"), "{diff}");
	}
}
