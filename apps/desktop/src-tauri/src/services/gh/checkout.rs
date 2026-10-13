//! Checking a pull request out into a new worktree (F31 P4, ADR-0079).
//!
//! The one place the app runs `git`, and the one place it writes to a
//! repository. The guarantee is narrow and is what the argument lists below are
//! held to: the app fetches one ref, then adds **one new linked worktree**, and
//! may create **one new branch** `pr/<n>`. It never runs `checkout`, `reset`,
//! `switch` or `stash`, never forces anything, and every command names the main
//! checkout with `-C`, so the current checkout, its index and its HEAD, and every
//! other worktree, are untouched.
//!
//! `git` rather than libgit2 because the fetch needs the user's own credential
//! helpers and SSH setup, and libgit2 is linked without a transport (ADR-0009).
//! `GIT_TERMINAL_PROMPT=0` keeps a missing credential an error instead of a hang
//! on a prompt nobody can answer; the helpers themselves keep working.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::models::{GitWorktree, PrCheckoutPlan};
use crate::services::git;

use super::run::is_native;
use super::{args, blocking, context, parse, read, validate_number, validate_sha, READ_TIMEOUT};

const FETCH_TIMEOUT: Duration = Duration::from_secs(120);
const ADD_TIMEOUT: Duration = Duration::from_secs(60);

/// Git's stderr is for a banner; the rest would only be memory.
const STDERR_CAP: usize = 64 * 1024;

static GIT: Mutex<Option<PathBuf>> = Mutex::new(None);

// ── argument lists ─────────────────────────────────────────────────────────

/// How the worktree's HEAD is set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddMode {
	/// A new branch `pr/<n>` at the head commit.
	NewBranch,
	/// The existing `pr/<n>`, which already points at the head commit.
	ExistingBranch,
	/// No branch: the head commit, detached.
	Detached,
}

pub fn branch_name(number: u64) -> String {
	format!("pr/{number}")
}

/// A remote name that is safe as a bare git argument: not empty, no leading `-`
/// (it would read as an option), no whitespace or control characters. Names come
/// from the repository's own config, so this is a second line of defence.
pub fn validate_remote(remote: &str) -> AppResult<&str> {
	if remote.is_empty()
		|| remote.starts_with('-')
		|| remote.chars().any(|c| c.is_whitespace() || c.is_control())
	{
		return Err(AppError::InvalidInput(format!("`{remote}` is not a usable remote name")));
	}
	Ok(remote)
}

/// Fetch the pull request's head from `remote` (the one the repository was read
/// from, see [`super::repo`]). No tags, no `FETCH_HEAD`, and no destination ref:
/// the objects arrive and nothing local is renamed or moved.
pub fn fetch_args(main: &Path, remote: &str, number: u64) -> Vec<String> {
	vec![
		"-C".into(),
		main.to_string_lossy().into_owned(),
		"fetch".into(),
		"--no-tags".into(),
		"--no-write-fetch-head".into(),
		remote.into(),
		format!("refs/pull/{number}/head"),
	]
}

pub fn worktree_add_args(
	main: &Path,
	number: u64,
	head_oid: &str,
	path: &Path,
	mode: &AddMode,
) -> Vec<String> {
	let mut out = vec![
		"-C".to_string(),
		main.to_string_lossy().into_owned(),
		"worktree".into(),
		"add".into(),
	];
	let path = path.to_string_lossy().into_owned();
	match mode {
		AddMode::NewBranch => out.extend(["-b".into(), branch_name(number), path, head_oid.into()]),
		AddMode::ExistingBranch => out.extend([path, branch_name(number)]),
		AddMode::Detached => out.extend(["--detach".into(), path, head_oid.into()]),
	}
	out
}

// ── where the worktree goes ────────────────────────────────────────────────

/// `<parent>/<main dir>-pr-<n>`, then `-2`, `-3`… until `occupied` says free.
pub fn pick_path(main: &Path, number: u64, occupied: impl Fn(&Path) -> bool) -> AppResult<PathBuf> {
	let (Some(parent), Some(name)) = (main.parent(), main.file_name()) else {
		return Err(AppError::InvalidInput(
			"the main checkout has no parent folder to put a worktree in".into(),
		));
	};
	let base = format!("{}-pr-{number}", name.to_string_lossy());
	let mut candidate = parent.join(&base);
	let mut n = 2;
	while occupied(&candidate) {
		candidate = parent.join(format!("{base}-{n}"));
		n += 1;
	}
	Ok(candidate)
}

/// A linked worktree that is this pull request's: on `pr/<n>`, or sitting at its
/// head commit. The renderer's `findPrWorktree` applies the same test.
pub fn find_existing(
	worktrees: &[GitWorktree],
	number: u64,
	head_oid: &str,
) -> Option<GitWorktree> {
	let branch = branch_name(number);
	worktrees
		.iter()
		.find(|w| {
			!w.is_main
				&& w.exists && (w.branch.as_deref() == Some(branch.as_str())
				|| w.head.as_deref() == Some(head_oid))
		})
		.cloned()
}

/// What the repository on disk says, before anything is fetched.
struct Local {
	/// The remote the pull requests belong to.
	remote: String,
	main: PathBuf,
	worktrees: Vec<GitWorktree>,
	path: PathBuf,
}

fn inspect(project_path: &str, number: u64) -> AppResult<Local> {
	let remote = super::repo::resolve(project_path).map_err(super::unresolved_error)?.repo.remote;
	let worktrees = git::worktrees(project_path)?;
	let Some(main) = worktrees.iter().find(|w| w.is_main) else {
		return Err(AppError::InvalidInput(
			"this repository has no main checkout to add a worktree beside".into(),
		));
	};
	let main = PathBuf::from(&main.path);
	let taken: HashSet<PathBuf> = worktrees.iter().map(|w| PathBuf::from(&w.path)).collect();
	let path = pick_path(&main, number, |p| p.exists() || taken.contains(p))?;
	Ok(Local { remote, main, worktrees, path })
}

/// The commit a local branch points at.
fn branch_target(main: &Path, branch: &str) -> Option<String> {
	let repository = git2::Repository::open(main).ok()?;
	let found = repository.find_branch(branch, git2::BranchType::Local).ok()?;
	found.get().peel_to_commit().ok().map(|c| c.id().to_string())
}

// ── the plan ───────────────────────────────────────────────────────────────

/// What a checkout would do. Reads the pull request from GitHub and the
/// worktree list from disk; writes nothing.
pub async fn plan(project_path: &str, number: u64) -> AppResult<PrCheckoutPlan> {
	let number = validate_number(number)?;
	let ctx = context(project_path).await?;
	let what = format!("pull request #{number}");
	let out = read(&ctx, args::pr_view(&ctx.repo, number), READ_TIMEOUT, &what).await?;
	let raw = parse::parse_pr(&out).map_err(|e| super::unreadable(&what, e))?;
	let summary = parse::summary(&raw, None);
	validate_sha(&summary.head_ref_oid)
		.map_err(|_| AppError::Remote(format!("GitHub did not report the head of {what}")))?;

	let path = project_path.to_string();
	blocking(move || -> AppResult<PrCheckoutPlan> {
		let local = inspect(&path, number)?;
		let branch = branch_name(number);
		let branch_at = branch_target(&local.main, &branch);
		Ok(PrCheckoutPlan {
			main_path: local.main.to_string_lossy().into_owned(),
			worktree_path: local.path.to_string_lossy().into_owned(),
			existing: find_existing(&local.worktrees, number, &summary.head_ref_oid),
			branch_exists: branch_at.is_some(),
			branch_at,
			branch,
			from_fork: summary.is_cross_repository,
			head_owner: summary.head_owner.clone(),
			head_oid: summary.head_ref_oid.clone(),
		})
	})
	.await?
}

// ── the checkout ───────────────────────────────────────────────────────────

/// Fetch the pull request's head and add a worktree for it. Returns the existing
/// worktree untouched when there already is one. `head_oid` is the commit the
/// user was shown, and is what gets checked out even if the pull request has
/// moved since.
pub async fn checkout(
	project_path: &str,
	number: u64,
	head_oid: &str,
	detached: bool,
) -> AppResult<GitWorktree> {
	let number = validate_number(number)?;
	validate_sha(head_oid)?;

	let (p, oid) = (project_path.to_string(), head_oid.to_string());
	let local = blocking(move || {
		let local = inspect(&p, number)?;
		let existing = find_existing(&local.worktrees, number, &oid);
		Ok::<_, AppError>((local.remote, local.main, local.path, existing))
	})
	.await??;
	let (remote, main, path, existing) = local;
	if let Some(existing) = existing {
		return Ok(existing);
	}

	let git = git_binary().await?;
	checkout_worktree(&git, &main, &remote, number, head_oid, detached, &path).await?;

	let (p, wanted) = (project_path.to_string(), path.clone());
	blocking(move || git::worktrees(&p))
		.await??
		.into_iter()
		.find(|w| Path::new(&w.path) == wanted || same_dir(Path::new(&w.path), &wanted))
		.ok_or_else(|| {
			AppError::Process("git added the worktree but it is not in the worktree list".into())
		})
}

fn same_dir(a: &Path, b: &Path) -> bool {
	match (dunce::canonicalize(a), dunce::canonicalize(b)) {
		(Ok(a), Ok(b)) => a == b,
		_ => false,
	}
}

/// The git half, testable without `gh`: fetch from `remote`, check the commit
/// arrived, add the worktree.
pub async fn checkout_worktree(
	git: &Path,
	main: &Path,
	remote: &str,
	number: u64,
	head_oid: &str,
	detached: bool,
	path: &Path,
) -> AppResult<()> {
	let remote = validate_remote(remote)?;
	let fetched = run_git(git, fetch_args(main, remote, number), FETCH_TIMEOUT).await?;
	if !fetched.ok {
		return Err(AppError::Remote(format!(
			"git could not fetch pull request #{number}: {}",
			last_line(&fetched.stderr)
		)));
	}

	let (m, oid) = (main.to_path_buf(), head_oid.to_string());
	let branch = branch_name(number);
	let (have_commit, branch_at) = blocking(move || {
		let have = git2::Repository::open(&m)
			.ok()
			.zip(git2::Oid::from_str(&oid).ok())
			.is_some_and(|(repo, id)| repo.find_commit(id).is_ok());
		(have, branch_target(&m, &branch_name(number)))
	})
	.await?;
	if !have_commit {
		return Err(AppError::Remote(format!(
			"commit {} is not in pull request #{number} any more; refresh and try again",
			short(head_oid)
		)));
	}

	let mode = match branch_at.as_deref() {
		None if detached => AddMode::Detached,
		None => AddMode::NewBranch,
		Some(at) if at == head_oid && !detached => AddMode::ExistingBranch,
		Some(_) if detached => AddMode::Detached,
		Some(at) => {
			return Err(AppError::InvalidInput(format!(
				"branch {branch} already exists at {} — check out detached instead",
				short(at)
			)))
		}
	};
	let added =
		run_git(git, worktree_add_args(main, number, head_oid, path, &mode), ADD_TIMEOUT).await?;
	if !added.ok {
		return Err(AppError::Process(format!(
			"git could not add the worktree: {}",
			last_line(&added.stderr)
		)));
	}
	Ok(())
}

fn short(oid: &str) -> &str {
	oid.get(..7).unwrap_or(oid)
}

fn last_line(stderr: &str) -> String {
	stderr
		.lines()
		.map(str::trim)
		.rfind(|l| !l.is_empty())
		.unwrap_or("git gave no reason")
		.to_string()
}

// ── running git ────────────────────────────────────────────────────────────

struct GitOutput {
	ok: bool,
	stderr: String,
}

fn not_installed() -> AppError {
	AppError::NotFound("git not found on PATH".into())
}

/// A positive answer is kept; a negative one never is, so installing git needs no
/// restart.
async fn git_binary() -> AppResult<PathBuf> {
	if let Some(found) = GIT.lock().clone() {
		return Ok(found);
	}
	let found = blocking(locate).await?.ok_or_else(not_installed)?;
	*GIT.lock() = Some(found.clone());
	Ok(found)
}

fn locate() -> Option<PathBuf> {
	// A `git.cmd` shim would put the arguments through a shell (see `run::is_native`).
	let on_path = crate::services::agent_cli::find_binary_on_path("git").filter(|p| is_native(p));
	on_path.or_else(|| fallbacks().into_iter().find(|p| p.is_file()))
}

fn fallbacks() -> Vec<PathBuf> {
	let mut found: Vec<PathBuf> = Vec::new();
	#[cfg(windows)]
	{
		for var in ["ProgramFiles", "ProgramFiles(x86)"] {
			if let Some(base) = std::env::var_os(var) {
				found.push(PathBuf::from(base).join("Git").join("cmd").join("git.exe"));
			}
		}
	}
	#[cfg(not(windows))]
	{
		for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
			found.push(Path::new(dir).join("git"));
		}
	}
	found
}

fn command(git: &Path, args: &[String]) -> tokio::process::Command {
	let mut cmd = std::process::Command::new(git);
	cmd.args(args.iter().map(OsString::from));
	crate::services::child_env::changes_for_current_env().apply_to_command(&mut cmd);
	// Never block on a username prompt; credential helpers still answer.
	cmd.env("GIT_TERMINAL_PROMPT", "0");
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
	}
	let mut cmd = tokio::process::Command::from(cmd);
	// A timeout drops the future, which kills the child: no orphan git.
	cmd.kill_on_drop(true);
	cmd
}

async fn run_git(git: &Path, args: Vec<String>, timeout: Duration) -> AppResult<GitOutput> {
	let mut cmd = command(git, &args);
	cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
	let child = cmd
		.spawn()
		.map_err(|e| AppError::Process(format!("could not run {}: {e}", git.display())))?;
	match tokio::time::timeout(timeout, child.wait_with_output()).await {
		Ok(Ok(out)) => {
			let tail = out.stderr.len().saturating_sub(STDERR_CAP);
			Ok(GitOutput {
				ok: out.status.success(),
				stderr: String::from_utf8_lossy(&out.stderr[tail..]).into_owned(),
			})
		}
		Ok(Err(e)) => Err(AppError::Process(format!("git did not finish: {e}"))),
		Err(_) => Err(AppError::Remote(format!(
			"git did not finish within {} seconds",
			timeout.as_secs()
		))),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const OID: &str = "0123456789abcdef0123456789abcdef01234567";

	fn all_argv(main: &Path) -> Vec<Vec<String>> {
		let path = Path::new("/work/app-pr-7");
		vec![
			fetch_args(main, "origin", 7),
			worktree_add_args(main, 7, OID, path, &AddMode::NewBranch),
			worktree_add_args(main, 7, OID, path, &AddMode::ExistingBranch),
			worktree_add_args(main, 7, OID, path, &AddMode::Detached),
		]
	}

	#[test]
	fn every_git_call_names_the_main_checkout_and_never_rewrites_one() {
		let main = Path::new("/work/app");
		for argv in all_argv(main) {
			assert_eq!(argv[0], "-C", "{argv:?}");
			assert_eq!(argv[1], main.to_string_lossy(), "{argv:?}");
			for banned in [
				"checkout", "reset", "switch", "stash", "push", "restore", "clean", "--force", "-f",
			] {
				assert!(!argv.iter().any(|a| a == banned), "{banned} in {argv:?}");
			}
		}
	}

	#[test]
	fn the_argument_lists_have_the_expected_shape() {
		let main = Path::new("/work/app");
		let path = Path::new("/work/app-pr-7");
		assert_eq!(
			fetch_args(main, "origin", 7)[2..],
			["fetch", "--no-tags", "--no-write-fetch-head", "origin", "refs/pull/7/head"]
		);
		assert_eq!(fetch_args(main, "xplorer", 7)[5], "xplorer");
		assert_eq!(
			worktree_add_args(main, 7, OID, path, &AddMode::NewBranch)[2..],
			["worktree", "add", "-b", "pr/7", "/work/app-pr-7", OID]
		);
		assert_eq!(
			worktree_add_args(main, 7, OID, path, &AddMode::ExistingBranch)[2..],
			["worktree", "add", "/work/app-pr-7", "pr/7"]
		);
		assert_eq!(
			worktree_add_args(main, 7, OID, path, &AddMode::Detached)[2..],
			["worktree", "add", "--detach", "/work/app-pr-7", OID]
		);
	}

	#[test]
	fn a_remote_name_must_not_read_as_an_option() {
		for good in ["origin", "xplorer", "my-fork", "a.b/c"] {
			assert!(validate_remote(good).is_ok(), "{good:?}");
		}
		for bad in ["", "-u", "--upload-pack=x", "a b", "a\tb", "a\nb"] {
			assert!(validate_remote(bad).is_err(), "{bad:?}");
		}
	}

	#[test]
	fn the_path_is_suffixed_until_it_is_free() {
		let main = Path::new("/work/app");
		let none = |_: &Path| false;
		assert_eq!(pick_path(main, 7, none).unwrap(), Path::new("/work/app-pr-7"));

		let taken: HashSet<PathBuf> =
			["/work/app-pr-7", "/work/app-pr-7-2"].iter().map(PathBuf::from).collect();
		assert_eq!(
			pick_path(main, 7, |p| taken.contains(p)).unwrap(),
			Path::new("/work/app-pr-7-3")
		);
		assert!(pick_path(Path::new("/"), 7, none).is_err());
	}

	fn wt(
		path: &str,
		branch: Option<&str>,
		head: &str,
		is_main: bool,
		exists: bool,
	) -> GitWorktree {
		GitWorktree {
			path: path.into(),
			name: None,
			branch: branch.map(str::to_string),
			head: Some(head.into()),
			is_main,
			locked: false,
			prunable: false,
			exists,
		}
	}

	#[test]
	fn an_existing_worktree_is_found_by_branch_or_head() {
		let other = "f".repeat(40);
		let list = vec![
			wt("/w/app", Some("pr/7"), &other, true, true),
			wt("/w/gone", Some("pr/7"), &other, false, false),
			wt("/w/by-branch", Some("pr/7"), &other, false, true),
		];
		assert_eq!(find_existing(&list, 7, OID).unwrap().path, "/w/by-branch");
		assert!(find_existing(&list, 8, OID).is_none());

		let list = vec![wt("/w/by-head", None, OID, false, true)];
		assert_eq!(find_existing(&list, 7, OID).unwrap().path, "/w/by-head");
	}

	#[test]
	fn the_last_line_of_stderr_is_the_reason() {
		assert_eq!(
			last_line("remote: x\nfatal: could not read Username\n\n"),
			"fatal: could not read Username"
		);
		assert_eq!(last_line(""), "git gave no reason");
	}

	// ── against a real git ────────────────────────────────────────────────

	struct Fixture {
		_dir: tempfile::TempDir,
		main: PathBuf,
		pr_oid: String,
		start_oid: String,
		git: PathBuf,
	}

	fn commit(repo: &git2::Repository, message: &str, parents: &[&git2::Commit]) -> git2::Oid {
		let sig = git2::Signature::now("t", "t@example.com").unwrap();
		let tree = repo.find_tree(repo.treebuilder(None).unwrap().write().unwrap()).unwrap();
		repo.commit(None, &sig, &sig, message, &tree, parents).unwrap()
	}

	/// A bare `origin` holding a commit at `refs/pull/7/head`, and a clone-like
	/// repository with that origin and one commit of its own. `None` without git.
	fn fixture() -> Option<Fixture> {
		let git = crate::services::agent_cli::find_binary_on_path("git")?;
		let dir = tempfile::tempdir().unwrap();
		let origin_path = dir.path().join("origin.git");
		let origin = git2::Repository::init_bare(&origin_path).unwrap();
		let pr = commit(&origin, "pull request head", &[]);
		origin.reference("refs/pull/7/head", pr, true, "test").unwrap();

		let main = dir.path().join("app");
		let repo = git2::Repository::init(&main).unwrap();
		let start = commit(&repo, "start", &[]);
		repo.reference("refs/heads/main", start, true, "test").unwrap();
		repo.set_head("refs/heads/main").unwrap();
		repo.remote("origin", &origin_path.to_string_lossy()).unwrap();
		Some(Fixture { main, pr_oid: pr.to_string(), start_oid: start.to_string(), git, _dir: dir })
	}

	fn head_of(main: &Path) -> (Option<String>, String) {
		let repo = git2::Repository::open(main).unwrap();
		let head = repo.head().unwrap();
		let oid = head.peel_to_commit().unwrap().id().to_string();
		(head.shorthand().ok().map(str::to_string), oid)
	}

	#[tokio::test]
	async fn the_pull_request_lands_in_a_new_worktree_and_the_checkout_is_untouched() {
		let Some(fx) = fixture() else {
			eprintln!("skipped: git is not on PATH");
			return;
		};
		let before = head_of(&fx.main);
		let path = pick_path(&fx.main, 7, |p| p.exists()).unwrap();
		checkout_worktree(&fx.git, &fx.main, "origin", 7, &fx.pr_oid, false, &path).await.unwrap();

		assert!(path.join(".git").exists());
		let list = git::worktrees(&fx.main.to_string_lossy()).unwrap();
		let added = list.iter().find(|w| !w.is_main).expect("a linked worktree");
		assert_eq!(added.branch.as_deref(), Some("pr/7"));
		assert_eq!(added.head.as_deref(), Some(fx.pr_oid.as_str()));
		assert!(same_dir(Path::new(&added.path), &path));
		assert_eq!(find_existing(&list, 7, &fx.pr_oid).unwrap().path, added.path);
		assert_eq!(head_of(&fx.main), before);
	}

	#[tokio::test]
	async fn the_fetch_goes_to_the_named_remote_not_to_origin() {
		let Some(fx) = fixture() else {
			eprintln!("skipped: git is not on PATH");
			return;
		};
		let repo = git2::Repository::open(&fx.main).unwrap();
		let url = repo.find_remote("origin").unwrap().url().unwrap().to_string();
		repo.remote_delete("origin").unwrap();
		repo.remote("xplorer", &url).unwrap();
		let path = pick_path(&fx.main, 7, |p| p.exists()).unwrap();

		let err = checkout_worktree(&fx.git, &fx.main, "origin", 7, &fx.pr_oid, false, &path)
			.await
			.unwrap_err();
		assert!(matches!(err, AppError::Remote(_)), "{err:?}");

		checkout_worktree(&fx.git, &fx.main, "xplorer", 7, &fx.pr_oid, false, &path).await.unwrap();
		assert!(path.join(".git").exists());
	}

	#[tokio::test]
	async fn a_branch_at_another_commit_needs_the_detached_choice() {
		let Some(fx) = fixture() else {
			eprintln!("skipped: git is not on PATH");
			return;
		};
		let repo = git2::Repository::open(&fx.main).unwrap();
		let start = repo.find_commit(git2::Oid::from_str(&fx.start_oid).unwrap()).unwrap();
		repo.branch("pr/7", &start, false).unwrap();

		let path = pick_path(&fx.main, 7, |p| p.exists()).unwrap();
		let err = checkout_worktree(&fx.git, &fx.main, "origin", 7, &fx.pr_oid, false, &path)
			.await
			.unwrap_err();
		let AppError::InvalidInput(message) = err else { panic!("{err:?}") };
		assert!(message.contains("detached"), "{message}");
		assert!(!path.exists());

		checkout_worktree(&fx.git, &fx.main, "origin", 7, &fx.pr_oid, true, &path).await.unwrap();
		let list = git::worktrees(&fx.main.to_string_lossy()).unwrap();
		let added = list.iter().find(|w| !w.is_main).unwrap();
		assert_eq!(added.branch, None);
		assert_eq!(added.head.as_deref(), Some(fx.pr_oid.as_str()));
		// The pre-existing branch was not moved.
		let repo = git2::Repository::open(&fx.main).unwrap();
		let branch = repo.find_branch("pr/7", git2::BranchType::Local).unwrap();
		assert_eq!(branch.get().target().unwrap().to_string(), fx.start_oid);
	}

	#[tokio::test]
	async fn a_failed_fetch_is_a_remote_error_and_adds_nothing() {
		let Some(fx) = fixture() else {
			eprintln!("skipped: git is not on PATH");
			return;
		};
		let path = pick_path(&fx.main, 8, |p| p.exists()).unwrap();
		// The origin has no pull request 8.
		let err = checkout_worktree(&fx.git, &fx.main, "origin", 8, &fx.pr_oid, false, &path)
			.await
			.unwrap_err();
		assert!(matches!(err, AppError::Remote(_)), "{err:?}");
		assert!(!path.exists());
	}
}
