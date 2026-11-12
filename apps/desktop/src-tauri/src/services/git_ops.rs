//! Git mode's writes, each one a typed op run through the system `git`
//! (F35, ADR-0089). `git2` stays read-only; after a write the view re-reads.
//!
//! The guarantees held by `args_for`: every pathspec comes after `--`, and a
//! branch or ref name can never be read as an option.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::services::git_cli::{git_binary, run_git_stdin};

const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
/// Proposed commands may fetch or push, so they get the network budget.
pub const PROPOSED_TIMEOUT: Duration = NETWORK_TIMEOUT;
const LOCAL_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum GitOp {
	Stage {
		paths: Vec<String>,
	},
	Unstage {
		paths: Vec<String>,
	},
	Discard {
		paths: Vec<String>,
	},
	/// Hunk stage / unstage / discard: a patch built from the diff the view shows.
	ApplyPatch {
		patch: String,
		cached: bool,
		reverse: bool,
	},
	/// An empty message concludes a merge with git's own prepared message.
	Commit {
		message: String,
		amend: bool,
	},
	Checkout {
		branch: String,
	},
	CreateBranch {
		name: String,
		at: String,
		checkout: bool,
	},
	RenameBranch {
		from: String,
		to: String,
	},
	DeleteBranch {
		name: String,
	},
	Merge {
		branch: String,
	},
	Rebase {
		onto: String,
	},
	MergeAbort,
	RebaseAbort,
	RebaseContinue,
	/// Only for "Undo rebase", which knows the full SHA it came from.
	ResetHardTo {
		sha: String,
	},
	/// Takes one side of a conflicted file, then stages it.
	CheckoutSide {
		path: String,
		theirs: bool,
	},
	Fetch,
	Pull,
	Push {
		set_upstream: bool,
	},
	StashPush {
		message: Option<String>,
	},
	StashApply {
		index: usize,
	},
	StashPop {
		index: usize,
	},
	StashDrop {
		index: usize,
	},
}

/// A non-zero exit is a result, not an error: a conflict is a state to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOpResult {
	/// The command line as run, without the `-C <project>` the app adds.
	pub command: String,
	pub ok: bool,
	pub stdout: String,
	pub stderr: String,
}

pub fn validate_ref_name(name: &str) -> AppResult<()> {
	let bad = name.is_empty()
		|| name.starts_with('-')
		|| name.contains("..")
		|| name.contains("@{")
		|| name.chars().any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c));
	if bad {
		return Err(AppError::InvalidInput(format!("not a usable branch or ref name: {name:?}")));
	}
	Ok(())
}

fn strings(parts: &[&str]) -> Vec<String> {
	parts.iter().map(|s| (*s).to_string()).collect()
}

fn with_paths(head: &[&str], paths: &[String]) -> AppResult<Vec<String>> {
	if paths.is_empty() {
		return Err(AppError::InvalidInput("no paths given".into()));
	}
	let mut argv = strings(head);
	argv.push("--".into());
	argv.extend(paths.iter().cloned());
	Ok(argv)
}

fn with_ref(head: &[&str], name: &str) -> AppResult<Vec<String>> {
	validate_ref_name(name)?;
	let mut argv = strings(head);
	argv.push(name.to_string());
	Ok(argv)
}

/// The argv after `git -C <dir>`. Pure, so the argv rules are unit-testable.
pub fn args_for(op: &GitOp) -> AppResult<Vec<String>> {
	match op {
		GitOp::Stage { paths } => with_paths(&["add"], paths),
		GitOp::Unstage { paths } => with_paths(&["restore", "--staged"], paths),
		GitOp::Discard { paths } => with_paths(&["restore"], paths),
		GitOp::ApplyPatch { cached, reverse, .. } => {
			let mut argv = strings(&["apply"]);
			if *cached {
				argv.push("--cached".into());
			}
			if *reverse {
				argv.push("--reverse".into());
			}
			argv.push("-".into());
			Ok(argv)
		}
		GitOp::Commit { message, amend } => {
			let mut argv = strings(&["commit"]);
			if *amend {
				argv.push("--amend".into());
			}
			if message.is_empty() {
				argv.push("--no-edit".into());
			} else {
				argv.extend(strings(&["-F", "-"]));
			}
			Ok(argv)
		}
		GitOp::Checkout { branch } => with_ref(&["switch"], branch),
		GitOp::CreateBranch { name, at, checkout } => {
			validate_ref_name(name)?;
			validate_ref_name(at)?;
			let head: &[&str] = if *checkout { &["switch", "-c"] } else { &["branch"] };
			let mut argv = strings(head);
			argv.extend([name.clone(), at.clone()]);
			Ok(argv)
		}
		GitOp::RenameBranch { from, to } => {
			validate_ref_name(from)?;
			validate_ref_name(to)?;
			Ok(strings(&["branch", "-m", from, to]))
		}
		GitOp::DeleteBranch { name } => with_ref(&["branch", "-d"], name),
		GitOp::Merge { branch } => with_ref(&["merge", "--no-edit"], branch),
		GitOp::Rebase { onto } => with_ref(&["rebase"], onto),
		GitOp::MergeAbort => Ok(strings(&["merge", "--abort"])),
		GitOp::RebaseAbort => Ok(strings(&["rebase", "--abort"])),
		// No editor may open: nobody could answer it.
		GitOp::RebaseContinue => Ok(strings(&["-c", "core.editor=true", "rebase", "--continue"])),
		GitOp::ResetHardTo { sha } => {
			if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
				return Err(AppError::InvalidInput("a full 40-character SHA is required".into()));
			}
			Ok(strings(&["reset", "--hard", sha]))
		}
		GitOp::CheckoutSide { path, theirs } => {
			let side = if *theirs { "--theirs" } else { "--ours" };
			Ok(strings(&["checkout", side, "--", path]))
		}
		GitOp::Fetch => Ok(strings(&["fetch", "--all"])),
		GitOp::Pull => Ok(strings(&["pull", "--no-edit"])),
		GitOp::Push { set_upstream } => Ok(if *set_upstream {
			strings(&["push", "-u", "origin", "HEAD"])
		} else {
			strings(&["push"])
		}),
		GitOp::StashPush { message } => {
			let mut argv = strings(&["stash", "push"]);
			if let Some(m) = message.as_deref().filter(|m| !m.is_empty()) {
				argv.extend(["-m".to_string(), m.to_string()]);
			}
			Ok(argv)
		}
		GitOp::StashApply { index } => Ok(strings(&["stash", "apply", &stash_ref(*index)])),
		GitOp::StashPop { index } => Ok(strings(&["stash", "pop", &stash_ref(*index)])),
		GitOp::StashDrop { index } => Ok(strings(&["stash", "drop", &stash_ref(*index)])),
	}
}

fn stash_ref(index: usize) -> String {
	format!("stash@{{{index}}}")
}

fn stdin_for(op: &GitOp) -> Option<String> {
	match op {
		GitOp::Commit { message, .. } if !message.is_empty() => Some(message.clone()),
		GitOp::ApplyPatch { patch, .. } => Some(patch.clone()),
		_ => None,
	}
}

fn timeout_for(op: &GitOp) -> Duration {
	match op {
		GitOp::Fetch | GitOp::Pull | GitOp::Push { .. } => NETWORK_TIMEOUT,
		_ => LOCAL_TIMEOUT,
	}
}

/// `git` plus the argv, quoting what has a space so the line can be pasted.
pub fn display(argv: &[String]) -> String {
	let mut line = String::from("git");
	for a in argv {
		line.push(' ');
		if a.is_empty() || a.contains(|c: char| c.is_whitespace() || c == '"') {
			line.push_str(&format!("{a:?}"));
		} else {
			line.push_str(a);
		}
	}
	line
}

/// Runs one argv under `git -C <dir>`; a non-zero exit is `Ok` with `ok: false`.
pub async fn run_argv(
	dir: &str,
	argv: Vec<String>,
	stdin: Option<String>,
	timeout: Duration,
) -> AppResult<GitOpResult> {
	let git = git_binary().await?;
	let mut full = vec!["-C".to_string(), dir.to_string()];
	full.extend(argv.iter().cloned());
	let out = run_git_stdin(&git, full, stdin, timeout).await?;
	Ok(GitOpResult { command: display(&argv), ok: out.ok, stdout: out.stdout, stderr: out.stderr })
}

pub async fn run_op(dir: &str, op: GitOp) -> AppResult<GitOpResult> {
	let argv = args_for(&op)?;
	let mut result = run_argv(dir, argv, stdin_for(&op), timeout_for(&op)).await?;
	if let GitOp::CheckoutSide { path, .. } = &op {
		if result.ok {
			let add = with_paths(&["add"], std::slice::from_ref(path))?;
			let add = run_argv(dir, add, None, LOCAL_TIMEOUT).await?;
			result.command = format!("{} && {}", result.command, add.command);
			result.ok = add.ok;
			result.stderr = add.stderr;
		}
	}
	Ok(result)
}

#[cfg(test)]
mod tests {
	use super::*;
	use git2::{Repository, RepositoryState, Signature};
	use std::fs;
	use std::path::Path;

	fn s(parts: &[&str]) -> Vec<String> {
		strings(parts)
	}

	#[test]
	fn stage_puts_paths_after_double_dash() {
		let op = GitOp::Stage { paths: s(&["-rf", "a b"]) };
		assert_eq!(args_for(&op).unwrap(), s(&["add", "--", "-rf", "a b"]));
	}

	#[test]
	fn branch_names_starting_with_dash_are_refused() {
		let err = args_for(&GitOp::Checkout { branch: "-f".into() }).unwrap_err();
		assert!(matches!(err, AppError::InvalidInput(_)));
		for bad in ["", "a b", "a..b", "a\nb", "x@{1}"] {
			assert!(validate_ref_name(bad).is_err(), "{bad:?}");
		}
		validate_ref_name("feat/git-mode").unwrap();
	}

	#[test]
	fn commit_reads_message_from_stdin() {
		let commit = |message: &str, amend| {
			args_for(&GitOp::Commit { message: message.into(), amend }).unwrap()
		};
		assert_eq!(commit("m", false), s(&["commit", "-F", "-"]));
		assert_eq!(commit("m", true), s(&["commit", "--amend", "-F", "-"]));
		assert_eq!(commit("", false), s(&["commit", "--no-edit"]));
	}

	#[test]
	fn push_with_upstream() {
		assert_eq!(
			args_for(&GitOp::Push { set_upstream: true }).unwrap(),
			s(&["push", "-u", "origin", "HEAD"])
		);
	}

	#[test]
	fn undo_rebase_requires_full_sha() {
		assert!(args_for(&GitOp::ResetHardTo { sha: "abc".into() }).is_err());
		let sha = "0123456789abcdef0123456789abcdef01234567";
		assert_eq!(
			args_for(&GitOp::ResetHardTo { sha: sha.into() }).unwrap(),
			s(&["reset", "--hard", sha])
		);
	}

	#[test]
	fn the_command_line_quotes_arguments_with_spaces() {
		assert_eq!(display(&s(&["add", "--", "a b"])), "git add -- \"a b\"");
	}

	/// A repository with identity configured and `a.txt` committed, or `None`
	/// without git on PATH.
	fn fixture(content: &str) -> Option<(tempfile::TempDir, Repository)> {
		crate::services::agent_cli::find_binary_on_path("git")?;
		let dir = tempfile::tempdir().unwrap();
		let repo = Repository::init(dir.path()).unwrap();
		let mut cfg = repo.config().unwrap();
		cfg.set_str("user.name", "factorai tests").unwrap();
		cfg.set_str("user.email", "tests@example.invalid").unwrap();
		cfg.set_bool("commit.gpgsign", false).unwrap();
		repo.set_head("refs/heads/main").unwrap();
		fs::write(dir.path().join("a.txt"), content).unwrap();
		commit_all(&repo, "base");
		Some((dir, repo))
	}

	fn commit_all(repo: &Repository, message: &str) {
		let mut index = repo.index().unwrap();
		index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None).unwrap();
		index.write().unwrap();
		let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
		let sig = Signature::now("factorai tests", "tests@example.invalid").unwrap();
		let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
		let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
		repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents).unwrap();
	}

	fn dir_of(dir: &tempfile::TempDir) -> String {
		dir.path().to_str().unwrap().to_string()
	}

	#[tokio::test]
	async fn commit_creates_a_commit_without_trailers() {
		let Some((dir, repo)) = fixture("one\n") else {
			eprintln!("skipped: git is not on PATH");
			return;
		};
		let d = dir_of(&dir);
		fs::write(dir.path().join("b.txt"), "b\n").unwrap();
		let staged = run_op(&d, GitOp::Stage { paths: s(&["b.txt"]) }).await.unwrap();
		assert!(staged.ok, "{}", staged.stderr);
		let done = run_op(&d, GitOp::Commit { message: "msg".into(), amend: false }).await.unwrap();
		assert!(done.ok, "{}", done.stderr);
		let head = repo.head().unwrap().peel_to_commit().unwrap();
		assert_eq!(head.message().unwrap(), "msg\n");
	}

	#[tokio::test]
	async fn merge_conflict_returns_ok_false() {
		let Some((dir, repo)) = fixture("one\n") else {
			eprintln!("skipped: git is not on PATH");
			return;
		};
		let d = dir_of(&dir);
		let branch =
			GitOp::CreateBranch { name: "other".into(), at: "HEAD".into(), checkout: true };
		assert!(run_op(&d, branch).await.unwrap().ok);
		fs::write(dir.path().join("a.txt"), "other\n").unwrap();
		commit_all(&repo, "other side");
		assert!(run_op(&d, GitOp::Checkout { branch: "main".into() }).await.unwrap().ok);
		fs::write(dir.path().join("a.txt"), "main\n").unwrap();
		commit_all(&repo, "main side");

		let merged = run_op(&d, GitOp::Merge { branch: "other".into() }).await.unwrap();
		assert!(!merged.ok);
		assert_eq!(repo.state(), RepositoryState::Merge);
		let mut index = repo.index().unwrap();
		index.read(true).unwrap(); // the CLI wrote it behind libgit2's cached copy
		assert_eq!(index.conflicts().unwrap().count(), 1);
	}

	#[tokio::test]
	async fn apply_patch_cached_stages_one_hunk() {
		let base: String = (1..=30).map(|n| format!("line {n}\n")).collect();
		let Some((dir, repo)) = fixture(&base) else {
			eprintln!("skipped: git is not on PATH");
			return;
		};
		let edited = base.replace("line 2\n", "two\n").replace("line 29\n", "twenty-nine\n");
		fs::write(dir.path().join("a.txt"), edited).unwrap();
		let diff = repo.diff_index_to_workdir(None, None).unwrap();
		let mut patch = git2::Patch::from_diff(&diff, 0).unwrap().unwrap();
		let full = patch.to_buf().unwrap().as_str().unwrap().to_string();
		let first = full[..full.rfind("@@ -").unwrap()].to_string();

		let op = GitOp::ApplyPatch { patch: first, cached: true, reverse: false };
		let applied = run_op(&dir_of(&dir), op).await.unwrap();
		assert!(applied.ok, "{}", applied.stderr);

		let mut index = repo.index().unwrap();
		index.read(true).unwrap();
		let entry = index.get_path(Path::new("a.txt"), 0).unwrap();
		let staged =
			String::from_utf8(repo.find_blob(entry.id).unwrap().content().to_vec()).unwrap();
		assert!(staged.contains("two\n") && staged.contains("line 29\n"), "{staged}");
	}
}
