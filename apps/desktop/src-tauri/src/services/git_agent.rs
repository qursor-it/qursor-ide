//! Quick `✦` actions: one-shot `claude -p` runs that answer a question about
//! the repository (F35 § Agents "Quick", ADR-0089).
//!
//! The process has no tools, no MCP, no hooks and no session transcript; the
//! prompt goes in on stdin. The agent only ever *proposes* git lines — they run
//! through `git_proposed`, from a click.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use dashmap::DashMap;
use git2::{Diff, DiffFormat, DiffOptions, Repository};
use serde::Deserialize;

use crate::agents;
use crate::commands::off_main;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::services::intelligence::provider::{
	base_command, run_bounded, ProviderError, RunControl,
};
use crate::services::{agent_cli, git, profiles, settings};

const TIMEOUT: Duration = Duration::from_secs(60);
const OUTPUT_CAP: usize = 64 * 1024;
/// Most bytes of diff (or conflicted file) that go into a prompt.
const DIFF_CAP: usize = 200 * 1024;

/// Runs in flight, by checkout directory, so `cancel` can reach the process.
static RUNS: LazyLock<DashMap<String, Arc<RunControl>>> = LazyLock::new(DashMap::new);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum GitAgentTask {
	CommitMessage,
	ExplainCommit { sha: String },
	FindBugs { sha: String },
	ReleaseNote { sha: String },
	UndoCommit { sha: String },
	SummariseBranch { branch: String, against: String },
	ExplainConflict { path: String },
	Request { text: String },
}

pub fn cancel(dir: &str) {
	if let Some(control) = RUNS.get(dir) {
		control.cancel();
	}
}

pub async fn ask(db: &Db, project_id: &str, dir: &str, task: GitAgentTask) -> AppResult<String> {
	let agent = profiles::agent_for_spawn(db, project_id, None);
	if agent.as_deref().is_some_and(|a| a != agents::CLAUDE) {
		return Err(AppError::InvalidInput("quick actions need a Claude project".into()));
	}
	let descriptor = agents::descriptor(agents::CLAUDE)
		.ok_or_else(|| AppError::Process("the Claude agent is not registered".into()))?;
	let status = agent_cli::check_agent_cli(
		descriptor,
		settings::binary_override(db, agents::CLAUDE).as_deref(),
	);
	let binary = status
		.binary_path
		.filter(|_| status.installed)
		.map(PathBuf::from)
		.ok_or_else(|| AppError::NotFound("the claude command was not found".into()))?;
	let config_dir = profiles::config_dir_for_spawn(db, agents::CLAUDE, project_id, None);
	let is_message = task == GitAgentTask::CommitMessage;
	let dir_owned = dir.to_string();
	let control = Arc::new(RunControl::default());
	if RUNS.insert(dir.to_string(), control.clone()).is_some() {
		return Err(AppError::InvalidInput("an agent question is already running here".into()));
	}
	let result = off_main(move || {
		let prompt = build_prompt(&dir_owned, &task)?;
		let args: Vec<OsString> = [
			"-p",
			"--output-format",
			"text",
			"--tools",
			"",
			"--strict-mcp-config",
			"--disable-slash-commands",
			"--no-session-persistence",
			"--permission-mode",
			"dontAsk",
		]
		.into_iter()
		.map(OsString::from)
		.collect();
		let command = base_command(&binary, &args, Path::new(&dir_owned), config_dir.as_deref())?;
		let out = run_bounded(command, prompt.into_bytes(), &control, TIMEOUT, OUTPUT_CAP)
			.map_err(|e| match e {
				ProviderError::Timeout => {
					AppError::Process("the agent did not answer in 60 s".into())
				}
				ProviderError::Cancelled => AppError::Process("cancelled".into()),
				ProviderError::OutputTooLarge => {
					AppError::Process("the answer was too long".into())
				}
				other => AppError::Process(format!("{other:?}")),
			})?;
		Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
	})
	.await;
	RUNS.remove(dir);
	let text = result?;
	if text.is_empty() {
		return Err(AppError::Process("the agent returned nothing".into()));
	}
	Ok(if is_message { strip_attribution(&text) } else { text })
}

/// Drops `Co-Authored-By:` and `Generated with` lines, whatever the prompt said.
pub fn strip_attribution(message: &str) -> String {
	let kept: Vec<&str> = message
		.lines()
		.filter(|l| {
			let l = l.trim().to_lowercase();
			!l.starts_with("co-authored-by:") && !l.contains("generated with")
		})
		.collect();
	kept.join("\n").trim().to_string()
}

const PROPOSE_RULES: &str = "Answer with exactly one line starting with `# ` that explains the plan, \
followed by one line per command, each starting with `git `. No code fences, no other text, no shell syntax.";

fn build_prompt(dir: &str, task: &GitAgentTask) -> AppResult<String> {
	let repo =
		git::discover(dir).ok_or_else(|| AppError::InvalidInput("not a git repository".into()))?;
	let (instruction, label, body) = match task {
		GitAgentTask::CommitMessage => (
			"Write a commit message for the staged change below. Output only the commit message: \
			 a subject line of at most 72 characters, a blank line, then an optional body. \
			 No trailers, no Co-Authored-By, no code fences."
				.to_string(),
			"Staged diff",
			staged_diff(&repo)?,
		),
		GitAgentTask::ExplainCommit { sha } => (
			"Explain what this commit changes and why, concisely.".into(),
			"Commit diff",
			commit_diff(&repo, sha)?,
		),
		GitAgentTask::FindBugs { sha } => (
			"Review this commit for bugs it introduces. List concrete problems, or say none found."
				.into(),
			"Commit diff",
			commit_diff(&repo, sha)?,
		),
		GitAgentTask::ReleaseNote { sha } => (
			"Write a short user-facing release note for this commit.".into(),
			"Commit diff",
			commit_diff(&repo, sha)?,
		),
		GitAgentTask::UndoCommit { sha } => (
			format!(
				"Propose the safest way to undo commit {sha} without rewriting shared history. {PROPOSE_RULES}"
			),
			"Commit diff",
			commit_diff(&repo, sha)?,
		),
		GitAgentTask::SummariseBranch { branch, against } => (
			format!("Summarise what branch `{branch}` changes compared with `{against}`."),
			"Branch diff",
			branch_diff(&repo, branch, against)?,
		),
		GitAgentTask::ExplainConflict { path } => (
			format!(
				"Explain the merge conflict in `{path}`: what each side intends and how they clash."
			),
			"File with conflict markers",
			conflicted_file(&repo, path)?,
		),
		GitAgentTask::Request { text } => (
			format!("The user asks, about this git repository: {text}\n{PROPOSE_RULES}"),
			"",
			String::new(),
		),
	};
	Ok(if body.is_empty() { instruction } else { format!("{instruction}\n\n{label}:\n{body}") })
}

fn git_err(e: git2::Error) -> AppError {
	AppError::Io(format!("git: {}", e.message()))
}

/// The patch text of `diff`, stopping once `DIFF_CAP` bytes are collected.
fn print_capped(diff: &Diff<'_>) -> String {
	let mut out = String::new();
	// Returning false aborts the walk with an error; the text so far is what we want.
	let _ = diff.print(DiffFormat::Patch, |_, _, line| {
		if out.len() >= DIFF_CAP {
			return false;
		}
		if matches!(line.origin(), '+' | '-' | ' ') {
			out.push(line.origin());
		}
		out.push_str(&String::from_utf8_lossy(line.content()));
		true
	});
	cap(out)
}

fn cap(mut text: String) -> String {
	if text.len() > DIFF_CAP {
		let mut end = DIFF_CAP;
		while !text.is_char_boundary(end) {
			end -= 1;
		}
		text.truncate(end);
	}
	text
}

fn staged_diff(repo: &Repository) -> AppResult<String> {
	let head = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
	let diff = repo
		.diff_tree_to_index(head.as_ref(), None, Some(&mut DiffOptions::new()))
		.map_err(git_err)?;
	Ok(print_capped(&diff))
}

fn commit_of<'r>(repo: &'r Repository, rev: &str) -> AppResult<git2::Commit<'r>> {
	repo.revparse_single(rev).and_then(|o| o.peel_to_commit()).map_err(git_err)
}

fn commit_diff(repo: &Repository, sha: &str) -> AppResult<String> {
	let commit = commit_of(repo, sha)?;
	let parent = commit.parent(0).ok().map(|p| p.tree()).transpose().map_err(git_err)?;
	let tree = commit.tree().map_err(git_err)?;
	let diff = repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), None).map_err(git_err)?;
	Ok(print_capped(&diff))
}

fn branch_diff(repo: &Repository, branch: &str, against: &str) -> AppResult<String> {
	let (tip, base) = (commit_of(repo, branch)?, commit_of(repo, against)?);
	let merge_base = repo
		.merge_base(tip.id(), base.id())
		.and_then(|id| repo.find_commit(id))
		.map_err(git_err)?;
	let from = merge_base.tree().map_err(git_err)?;
	let to = tip.tree().map_err(git_err)?;
	let diff = repo.diff_tree_to_tree(Some(&from), Some(&to), None).map_err(git_err)?;
	Ok(print_capped(&diff))
}

fn conflicted_file(repo: &Repository, path: &str) -> AppResult<String> {
	let rel = Path::new(path);
	if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir)) {
		return Err(AppError::InvalidInput("path must stay inside the repository".into()));
	}
	let workdir = repo.workdir().ok_or_else(|| AppError::InvalidInput("bare repository".into()))?;
	let text = std::fs::read(workdir.join(rel))?;
	Ok(cap(String::from_utf8_lossy(&text).into_owned()))
}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::TempDir;

	fn repo_with_commit(content: &str) -> (TempDir, String) {
		let tmp = TempDir::new().unwrap();
		let repo = Repository::init(tmp.path()).unwrap();
		std::fs::write(tmp.path().join("big.txt"), content).unwrap();
		let mut index = repo.index().unwrap();
		index.add_path(Path::new("big.txt")).unwrap();
		index.write().unwrap();
		let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
		let sig = git2::Signature::now("t", "t@example.com").unwrap();
		let id = repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
		(tmp, id.to_string())
	}

	fn dir_of(tmp: &TempDir) -> String {
		tmp.path().to_string_lossy().into_owned()
	}

	#[test]
	fn commit_message_prompt_forbids_trailers() {
		let (tmp, _) = repo_with_commit("a\n");
		let p = build_prompt(&dir_of(&tmp), &GitAgentTask::CommitMessage).unwrap();
		assert!(p.contains("Output only the commit message"));
		assert!(p.contains("No trailers, no Co-Authored-By"));
	}

	#[test]
	fn request_prompt_demands_git_lines() {
		let (tmp, _) = repo_with_commit("a\n");
		let p =
			build_prompt(&dir_of(&tmp), &GitAgentTask::Request { text: "undo".into() }).unwrap();
		assert!(p.contains("starting with `# `") && p.contains("starting with `git `"));
	}

	#[test]
	fn diff_is_capped() {
		let (tmp, sha) = repo_with_commit(&"x".repeat(2 * 1024 * 1024));
		let p = build_prompt(&dir_of(&tmp), &GitAgentTask::ExplainCommit { sha }).unwrap();
		assert!(p.len() <= DIFF_CAP + 1024, "prompt was {} bytes", p.len());
		assert!(p.len() > DIFF_CAP / 2);
	}

	#[test]
	fn codex_project_is_refused() {
		let tmp = TempDir::new().unwrap();
		let db = Db::open(&tmp.path().join("data")).unwrap();
		db.with(|c| settings::set(c, crate::models::SettingKey::AgentDefault, Some(agents::CODEX)))
			.unwrap();
		let err =
			tauri::async_runtime::block_on(ask(&db, "p1", "/nowhere", GitAgentTask::CommitMessage))
				.unwrap_err();
		assert!(
			matches!(err, AppError::InvalidInput(m) if m == "quick actions need a Claude project")
		);
	}

	#[test]
	fn attribution_lines_are_stripped() {
		let raw = "feat: x\n\nbody\n\nCo-Authored-By: Bot <b@x>\nGenerated with Tool\n";
		assert_eq!(strip_attribution(raw), "feat: x\n\nbody");
	}
}
