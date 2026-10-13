//! Which GitHub repository a project is, from its remotes (F31).
//!
//! **`origin` wins.** A clone with an `upstream` remote — a fork — has two
//! candidates, and `gh`'s own resolution would pick `upstream`. The user decided
//! the panel shows the pull requests of the repository they cloned from, so
//! `origin` is read here and every `gh` call that follows names that repository
//! explicitly (see [`super::args`]). When `origin` exists it is the answer, even
//! if it is not GitHub: there is no fallback past it.
//!
//! **Without `origin`,** the one GitHub remote is used — a clone whose remote was
//! named `xplorer` is still unambiguous. `upstream` is never a candidate (by
//! convention it is the parent of a fork, not the user's repository). Two or more
//! remaining GitHub remotes are ambiguous, and the panel says which instead of
//! guessing: [`Unresolved::NoOrigin`] carries their names.
//!
//! This is a config read. `git2` has no transport linked in (ADR-0009) and
//! nothing here contacts the remote.

use std::path::PathBuf;

use crate::models::GhRepo;
use crate::services::git;

/// Why a project has no repository to show pull requests for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unresolved {
	NotRepository,
	/// No `origin`, and not exactly one other GitHub remote. Carries the sorted
	/// names of the GitHub remotes (excluding `upstream`) when there are several;
	/// empty when there are none.
	NoOrigin(Vec<String>),
	/// `origin` exists and is not a GitHub host (or not an `owner/name` URL).
	NotGitHub(String),
}

/// A project's repository, and where its working tree is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
	pub repo: GhRepo,
	pub root: PathBuf,
}

/// Open the project's repository and read its remote (blocking: libgit2).
pub fn resolve(project_path: &str) -> Result<Resolved, Unresolved> {
	let repository = git::discover(project_path).ok_or(Unresolved::NotRepository)?;
	let root = repository.workdir().map(git::workdir_path).ok_or(Unresolved::NotRepository)?;
	if let Ok(origin) = repository.find_remote("origin") {
		let url = origin
			.url()
			.ok()
			.filter(|u| !u.trim().is_empty())
			.ok_or(Unresolved::NoOrigin(Vec::new()))?;
		return match parse_remote_url("origin", url) {
			Some(repo) => Ok(Resolved { repo, root }),
			None => Err(Unresolved::NotGitHub(url.to_string())),
		};
	}

	let mut candidates: Vec<GhRepo> = Vec::new();
	if let Ok(names) = repository.remotes() {
		// `StringArray::iter()` yields `Result<Option<&str>>`: skip what cannot be read.
		for name in names.iter() {
			let Ok(Some(name)) = name else { continue };
			if name == "upstream" {
				continue;
			}
			let found = repository.find_remote(name).ok();
			let repo = found.and_then(|r| r.url().ok().and_then(|u| parse_remote_url(name, u)));
			candidates.extend(repo);
		}
	}
	match candidates.len() {
		1 => Ok(Resolved { repo: candidates.remove(0), root }),
		_ => {
			let mut names: Vec<String> = candidates.into_iter().map(|c| c.remote).collect();
			names.sort();
			Err(Unresolved::NoOrigin(names))
		}
	}
}

/// Hosts that speak the git URL shapes below and are not GitHub. A GitHub
/// Enterprise host has an arbitrary name, so the list is of what to *refuse*,
/// not of what to accept.
const OTHER_FORGES: &[&str] = &["gitlab", "bitbucket", "codeberg", "azure.com", "sr.ht", "gitea"];

/// `owner/name` out of the URL of the remote called `remote`: scp
/// (`git@host:o/r.git`), `ssh://`, `https://` (with or without credentials, a
/// port, `.git`, a trailing slash) and `git://`. `None` for anything else, including a GitLab-shaped host.
pub fn parse_remote_url(remote: &str, url: &str) -> Option<GhRepo> {
	let url = url.trim();
	let (host, path) = split_url(url)?;
	let host = host.to_ascii_lowercase();
	if !valid_host(&host) || OTHER_FORGES.iter().any(|f| host.contains(f)) {
		return None;
	}
	let path = path.split(['?', '#']).next().unwrap_or("");
	let path = path.trim_matches('/');
	let path = path.strip_suffix(".git").unwrap_or(path).trim_end_matches('/');
	let mut parts = path.split('/');
	let (owner, name) = (parts.next()?, parts.next()?);
	if parts.next().is_some() || !valid_segment(owner) || !valid_segment(name) {
		return None;
	}
	Some(GhRepo {
		slug: format!("{host}/{owner}/{name}"),
		web_url: format!("https://{host}/{owner}/{name}"),
		remote: remote.to_string(),
		host,
		owner: owner.to_string(),
		name: name.to_string(),
	})
}

/// Host (without credentials or port) and path of a remote URL.
fn split_url(url: &str) -> Option<(String, &str)> {
	if let Some((scheme, rest)) = url.split_once("://") {
		if !matches!(scheme.to_ascii_lowercase().as_str(), "https" | "http" | "ssh" | "git") {
			return None;
		}
		let (authority, path) = rest.split_once('/')?;
		// `user[:password]@host[:port]` — only the host matters.
		let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
		let host = hostport.split(':').next()?;
		return Some((host.to_string(), path));
	}
	// scp-like: `[user@]host:path`, with no slash before the colon.
	let (head, path) = url.split_once(':')?;
	if head.contains('/') || head.contains('\\') {
		return None;
	}
	let host = head.rsplit_once('@').map_or(head, |(_, h)| h);
	// A one-letter "host" is a Windows drive (`C:\repo`), not a server.
	if host.len() < 2 {
		return None;
	}
	Some((host.to_string(), path))
}

fn valid_host(host: &str) -> bool {
	!host.is_empty()
		&& host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
		&& !host.starts_with(['.', '-'])
}

/// A GitHub owner or repository name. The same set the command layer enforces on
/// anything that reaches an argv.
pub fn valid_segment(segment: &str) -> bool {
	!segment.is_empty()
		&& segment != "."
		&& segment != ".."
		&& segment.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn slug(url: &str) -> Option<String> {
		parse_remote_url("origin", url).map(|r| r.slug)
	}

	#[test]
	fn remote_urls_parse() {
		let table = [
			("git@github.com:Nightbr/factorai.git", Some("github.com/Nightbr/factorai")),
			("git@github.com:Nightbr/factorai", Some("github.com/Nightbr/factorai")),
			("github.com:o/r.git", Some("github.com/o/r")),
			("ssh://git@github.com/o/r.git", Some("github.com/o/r")),
			("ssh://git@ghe.corp.example:2222/o/r", Some("ghe.corp.example/o/r")),
			(
				"https://github.com/qursor-it/qursor-ide.git",
				Some("github.com/qursor-it/qursor-ide"),
			),
			("https://github.com/o/r", Some("github.com/o/r")),
			("https://github.com/o/r/", Some("github.com/o/r")),
			("https://github.com/o/r.git/", Some("github.com/o/r")),
			("https://user@github.com/o/r.git", Some("github.com/o/r")),
			("https://user:pw@github.com/o/r.git", Some("github.com/o/r")),
			("https://x-access-token:abc@github.com:443/o/r", Some("github.com/o/r")),
			("HTTPS://GitHub.com/O/R.git", Some("github.com/O/R")),
			("git://github.com/o/r.git", Some("github.com/o/r")),
			("https://github.example.org/team/app.git", Some("github.example.org/team/app")),
			("  https://github.com/o/dotted.name.git\n", Some("github.com/o/dotted.name")),
			// Not GitHub.
			("https://gitlab.com/o/r.git", None),
			("git@gitlab.example.com:group/sub/r.git", None),
			("git@bitbucket.org:o/r.git", None),
			("https://dev.azure.com/org/project/_git/r", None),
			// Not an owner/name path.
			("https://github.com/o", None),
			("https://github.com/o/r/extra", None),
			("https://github.com/", None),
			("https://github.com/o/../x", None),
			("file:///srv/git/r.git", None),
			(r"C:\Users\me\repo", None),
			("/srv/git/repo.git", None),
			("../sibling", None),
			("", None),
		];
		for (url, want) in table {
			assert_eq!(slug(url).as_deref(), want, "{url:?}");
		}
	}

	#[test]
	fn a_repo_carries_the_pieces_the_panel_needs() {
		let repo = parse_remote_url("origin", "git@github.com:Nightbr/factorai.git").unwrap();
		assert_eq!(
			repo,
			GhRepo {
				host: "github.com".into(),
				owner: "Nightbr".into(),
				name: "factorai".into(),
				slug: "github.com/Nightbr/factorai".into(),
				web_url: "https://github.com/Nightbr/factorai".into(),
				remote: "origin".into(),
			}
		);
	}

	fn init() -> (tempfile::TempDir, git2::Repository, String) {
		let dir = tempfile::tempdir().unwrap();
		let repo = git2::Repository::init(dir.path()).unwrap();
		let path = dir.path().to_string_lossy().into_owned();
		(dir, repo, path)
	}

	/// This very clone: `origin` is the fork and `upstream` the original. `origin`
	/// is read, and `upstream` alone is never picked up.
	#[test]
	fn origin_wins_and_upstream_alone_is_never_read() {
		let (_dir, repo, path) = init();
		assert_eq!(resolve(&path), Err(Unresolved::NoOrigin(vec![])));

		repo.remote("upstream", "https://github.com/Nightbr/factorai.git").unwrap();
		assert_eq!(resolve(&path), Err(Unresolved::NoOrigin(vec![])));

		repo.remote("origin", "https://github.com/qursor-it/qursor-ide.git").unwrap();
		let resolved = resolve(&path).unwrap();
		assert_eq!(resolved.repo.slug, "github.com/qursor-it/qursor-ide");
		assert_eq!(resolved.repo.remote, "origin");
	}

	#[test]
	fn a_single_other_github_remote_is_used() {
		let (_dir, repo, path) = init();
		repo.remote("xplorer", "git@github.com:xplorer-platform/xplorer-backend.git").unwrap();
		let resolved = resolve(&path).unwrap();
		assert_eq!(resolved.repo.slug, "github.com/xplorer-platform/xplorer-backend");
		assert_eq!(resolved.repo.remote, "xplorer");
	}

	#[test]
	fn upstream_is_skipped_when_another_github_remote_exists() {
		let (_dir, repo, path) = init();
		repo.remote("upstream", "https://github.com/Nightbr/factorai.git").unwrap();
		repo.remote("mine", "https://github.com/me/factorai.git").unwrap();
		let resolved = resolve(&path).unwrap();
		assert_eq!(resolved.repo.slug, "github.com/me/factorai");
		assert_eq!(resolved.repo.remote, "mine");
	}

	#[test]
	fn several_github_remotes_without_origin_are_named_not_guessed() {
		let (_dir, repo, path) = init();
		repo.remote("zeta", "https://github.com/o/z.git").unwrap();
		repo.remote("alpha", "https://github.com/o/a.git").unwrap();
		repo.remote("upstream", "https://github.com/p/a.git").unwrap();
		assert_eq!(
			resolve(&path),
			Err(Unresolved::NoOrigin(vec!["alpha".to_string(), "zeta".to_string()]))
		);
	}

	#[test]
	fn a_gitlab_remote_does_not_count_as_a_candidate() {
		let (_dir, repo, path) = init();
		repo.remote("lab", "https://gitlab.com/o/r.git").unwrap();
		assert_eq!(resolve(&path), Err(Unresolved::NoOrigin(vec![])));
		repo.remote("hub", "https://github.com/o/r.git").unwrap();
		assert_eq!(resolve(&path).unwrap().repo.remote, "hub");
	}

	#[test]
	fn a_non_github_origin_is_final_even_with_another_github_remote() {
		let (_dir, repo, path) = init();
		repo.remote("origin", "https://gitlab.com/o/r.git").unwrap();
		repo.remote("mirror", "https://github.com/o/r.git").unwrap();
		assert_eq!(
			resolve(&path),
			Err(Unresolved::NotGitHub("https://gitlab.com/o/r.git".to_string()))
		);
	}

	#[test]
	fn a_non_github_origin_and_a_non_repository_are_named() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().to_string_lossy().into_owned();
		assert_eq!(resolve(&path), Err(Unresolved::NotRepository));

		let repo = git2::Repository::init(dir.path()).unwrap();
		repo.remote("origin", "https://gitlab.com/o/r.git").unwrap();
		assert_eq!(
			resolve(&path),
			Err(Unresolved::NotGitHub("https://gitlab.com/o/r.git".to_string()))
		);
	}
}
