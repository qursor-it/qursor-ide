//! The argument lists of every `gh` call, in one place so one test can hold
//! them all to the same rule (F31, ADR-0077).
//!
//! **Every call names its repository.** `gh`, given nothing, resolves one from
//! the working directory's remotes — and in a fork's clone that is `upstream`,
//! the repository the user did *not* choose. So pull-request commands carry
//! `--repo HOST/OWNER/NAME` and API calls carry `--hostname HOST` plus a
//! `repos/OWNER/NAME/…` path. `gh`'s `{owner}` / `{repo}` placeholders are
//! never used: they are filled from the same remote resolution.

use crate::models::{GhRepo, PrStateFilter};

use super::parse::{LIST_FIELDS, VIEW_FIELDS};

/// How many pull requests the list asks for. A page the panel can render
/// without virtualising, and one request.
pub const LIST_LIMIT: u32 = 50;

pub fn version() -> Vec<String> {
	vec!["--version".into()]
}

/// The signed-in login on `host`. Also the auth check: a host with no login
/// exits 4. This reads `/user` through `gh`; it never asks for the token.
pub fn viewer(host: &str) -> Vec<String> {
	["api", "--hostname", host, "user", "--jq", ".login"].map(String::from).to_vec()
}

fn state_word(state: PrStateFilter) -> &'static str {
	match state {
		PrStateFilter::Open => "open",
		PrStateFilter::Closed => "closed",
		PrStateFilter::Merged => "merged",
		PrStateFilter::All => "all",
	}
}

pub fn pr_list(repo: &GhRepo, state: PrStateFilter) -> Vec<String> {
	vec![
		"pr".into(),
		"list".into(),
		"--repo".into(),
		repo.slug.clone(),
		"--state".into(),
		state_word(state).into(),
		"--limit".into(),
		LIST_LIMIT.to_string(),
		"--json".into(),
		LIST_FIELDS.into(),
	]
}

pub fn pr_view(repo: &GhRepo, number: u64) -> Vec<String> {
	vec![
		"pr".into(),
		"view".into(),
		number.to_string(),
		"--repo".into(),
		repo.slug.clone(),
		"--json".into(),
		VIEW_FIELDS.into(),
	]
}

/// Every changed file, 100 a page. GitHub stops at 3000 files; the caller
/// compares against the pull request's own count.
pub fn pr_files(repo: &GhRepo, number: u64) -> Vec<String> {
	vec![
		"api".into(),
		"--hostname".into(),
		repo.host.clone(),
		"--paginate".into(),
		format!("repos/{}/{}/pulls/{number}/files?per_page=100", repo.owner, repo.name),
	]
}

/// Every inline review comment on the pull request, 100 a page.
pub fn pr_comments(repo: &GhRepo, number: u64) -> Vec<String> {
	vec![
		"api".into(),
		"--hostname".into(),
		repo.host.clone(),
		"--paginate".into(),
		format!("repos/{}/{}/pulls/{number}/comments?per_page=100", repo.owner, repo.name),
	]
}

/// Submit a review. **The one write the app makes to GitHub** (ADR-0077): the
/// JSON body, review text included, arrives on stdin (`--input -`) and never in
/// this list, which any process on the machine can read.
pub fn submit_review(repo: &GhRepo, number: u64) -> Vec<String> {
	vec![
		"api".into(),
		"--hostname".into(),
		repo.host.clone(),
		"-X".into(),
		"POST".into(),
		format!("repos/{}/{}/pulls/{number}/reviews", repo.owner, repo.name),
		"--input".into(),
		"-".into(),
	]
}

/// The pull request's own file count, which `pulls/N/files` does not report.
pub fn pr_changed_files(repo: &GhRepo, number: u64) -> Vec<String> {
	vec![
		"api".into(),
		"--hostname".into(),
		repo.host.clone(),
		format!("repos/{}/{}/pulls/{number}", repo.owner, repo.name),
		"--jq".into(),
		".changed_files".into(),
	]
}

/// The merge base of two commits, for when one of them is not in the clone.
/// `per_page=1` keeps the answer small: only `merge_base_commit` is wanted, not
/// the commits between.
pub fn compare(repo: &GhRepo, base: &str, head: &str) -> Vec<String> {
	vec![
		"api".into(),
		"--hostname".into(),
		repo.host.clone(),
		format!("repos/{}/{}/compare/{base}...{head}?per_page=1", repo.owner, repo.name),
		"--jq".into(),
		".merge_base_commit.sha".into(),
	]
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::services::gh::{blob, repo::parse_remote_url};

	const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

	/// This clone's own shape: the fork is `origin`, the original `upstream`. A
	/// call that fell back to `gh`'s own resolution would read the wrong one.
	fn all_calls(url: &str) -> (GhRepo, Vec<(&'static str, Vec<String>)>) {
		let repo = parse_remote_url(url).unwrap();
		let calls = vec![
			("list open", pr_list(&repo, PrStateFilter::Open)),
			("list all", pr_list(&repo, PrStateFilter::All)),
			("view", pr_view(&repo, 7)),
			("files", pr_files(&repo, 7)),
			("comments", pr_comments(&repo, 7)),
			("submit review", submit_review(&repo, 7)),
			("changed files", pr_changed_files(&repo, 7)),
			("compare", compare(&repo, SHA, SHA)),
			("contents", blob::contents_args(&repo, "src/a b.ts", SHA)),
			("viewer", viewer(&repo.host)),
		];
		(repo, calls)
	}

	#[test]
	fn every_call_names_its_repository_or_host_explicitly() {
		for url in
			["https://github.com/qursor-it/qursor-ide.git", "git@ghe.corp.example:team/app.git"]
		{
			let (repo, calls) = all_calls(url);
			for (name, args) in calls {
				let joined = args.join(" ");
				let by_flag = args.windows(2).any(|w| w[0] == "--repo" && w[1] == repo.slug);
				let by_host = args.windows(2).any(|w| w[0] == "--hostname" && w[1] == repo.host)
					&& (name == "viewer"
						|| args.iter().any(|a| {
							a.starts_with(&format!("repos/{}/{}/", repo.owner, repo.name))
						}));
				assert!(by_flag || by_host, "{name} does not name {}: {joined}", repo.slug);
				// `gh`'s own placeholders resolve from the remotes — never allowed.
				for forbidden in ["{owner}", "{repo}", ":owner", ":repo", "{branch}"] {
					assert!(!joined.contains(forbidden), "{name} uses {forbidden}: {joined}");
				}
			}
		}
	}

	#[test]
	fn pr_commands_use_repo_and_api_commands_use_hostname() {
		let (repo, _) = all_calls("https://github.com/o/r");
		let list = pr_list(&repo, PrStateFilter::Merged);
		assert_eq!(&list[..6], ["pr", "list", "--repo", "github.com/o/r", "--state", "merged"]);
		assert!(list.contains(&"50".to_string()));
		let view = pr_view(&repo, 42);
		assert_eq!(&view[..5], ["pr", "view", "42", "--repo", "github.com/o/r"]);
		assert_eq!(
			pr_files(&repo, 42),
			[
				"api",
				"--hostname",
				"github.com",
				"--paginate",
				"repos/o/r/pulls/42/files?per_page=100"
			]
		);
		assert_eq!(
			pr_comments(&repo, 42),
			[
				"api",
				"--hostname",
				"github.com",
				"--paginate",
				"repos/o/r/pulls/42/comments?per_page=100"
			]
		);
		assert_eq!(
			submit_review(&repo, 42),
			[
				"api",
				"--hostname",
				"github.com",
				"-X",
				"POST",
				"repos/o/r/pulls/42/reviews",
				"--input",
				"-"
			]
		);
		assert_eq!(
			compare(&repo, "aaa", "bbb"),
			[
				"api",
				"--hostname",
				"github.com",
				"repos/o/r/compare/aaa...bbb?per_page=1",
				"--jq",
				".merge_base_commit.sha"
			]
		);
	}

	/// The review text is data for stdin: no argument carries it, and the only
	/// write names stdin explicitly.
	#[test]
	fn a_review_body_is_never_an_argument() {
		let repo = parse_remote_url("https://github.com/o/r").unwrap();
		let args = submit_review(&repo, 7);
		assert!(args.windows(2).any(|w| w[0] == "--input" && w[1] == "-"));
		assert!(args.windows(2).any(|w| w[0] == "--hostname" && w[1] == "github.com"));
		assert!(!args.iter().any(|a| a == "-f" || a == "-F" || a.starts_with("body")));
		assert_eq!(args.iter().filter(|a| a.as_str() == "-X").count(), 1);
	}

	/// The argument list is data: nothing in it passes through a shell, and the
	/// token is never requested.
	#[test]
	fn no_call_asks_for_the_token() {
		let (_, calls) = all_calls("https://github.com/o/r");
		for (name, args) in calls {
			assert!(!args.iter().any(|a| a == "token" || a == "auth"), "{name}: {args:?}");
			// Submitting a review is the one write (ADR-0077).
			if name != "submit review" {
				assert!(
					!args.iter().any(|a| a == "-X" || a == "--method" || a == "--input"),
					"{name}"
				);
			}
		}
	}
}
