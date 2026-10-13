//! One file's contents at a commit of a pull request (F31).
//!
//! **Local first.** The commit is usually already in the clone — the base, and
//! the head of any branch the user fetched — and reading it is free and works
//! offline. Only when the commit object is absent (a fork's head, which `origin`
//! never had) does this ask GitHub, and then through `gh api` against the
//! **base** repository with `?ref=<sha>`: GitHub serves a fork PR's head commit
//! from the base repo's object store, so the fork need not be named, be public,
//! or still exist.
//!
//! Nothing is fetched into the repository. `git fetch` is a write to `.git`,
//! and this phase writes nothing (ADR-0077).

use crate::error::{AppError, AppResult};
use crate::models::{FileContents, GhRepo};
use crate::services::files;
use crate::services::git;

/// What the local object store knew.
pub enum Local {
	Found(FileContents),
	/// The commit is here and the path is not in its tree: the file does not
	/// exist at that commit. Authoritative — the network would say the same.
	Absent,
	/// The commit is not in this clone (or the repository could not be opened).
	NoCommit,
}

/// Read `rel_path` at `sha` from the clone (blocking: libgit2).
pub fn local(project_path: &str, rel_path: &str, sha: &str, max_bytes: Option<usize>) -> Local {
	let Some(repo) = git::discover(project_path) else {
		return Local::NoCommit;
	};
	let Ok(oid) = git2::Oid::from_str(sha) else {
		return Local::NoCommit;
	};
	let Ok(commit) = repo.find_commit(oid) else {
		return Local::NoCommit;
	};
	let Ok(tree) = commit.tree() else {
		return Local::NoCommit;
	};
	let Ok(entry) = tree.get_path(std::path::Path::new(rel_path)) else {
		return Local::Absent;
	};
	// A submodule or a nested tree is not a file we can show.
	let Ok(blob) = repo.find_blob(entry.id()) else {
		return Local::Absent;
	};
	let cap = max_bytes.unwrap_or(files::DEFAULT_MAX_BYTES);
	Local::Found(files::contents_from_bytes(rel_path, blob.content(), blob.size() as u64, cap))
}

/// Percent-encode each `/`-separated segment of a repository path for the URL
/// of the contents endpoint. Unreserved characters stay; every other byte of the
/// UTF-8 form becomes `%XX`, so a space, `#`, `?` or a non-ASCII name cannot
/// change what the URL means.
pub fn encode_path(rel_path: &str) -> String {
	rel_path
		.split('/')
		.map(|segment| {
			let mut out = String::with_capacity(segment.len());
			for byte in segment.bytes() {
				if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
					out.push(byte as char);
				} else {
					out.push_str(&format!("%{byte:02X}"));
				}
			}
			out
		})
		.collect::<Vec<_>>()
		.join("/")
}

/// `gh api` arguments for the raw bytes of a file at a commit.
pub fn contents_args(repo: &GhRepo, rel_path: &str, sha: &str) -> Vec<String> {
	vec![
		"api".into(),
		"--hostname".into(),
		repo.host.clone(),
		"-H".into(),
		"Accept: application/vnd.github.raw+json".into(),
		format!("repos/{}/{}/contents/{}?ref={sha}", repo.owner, repo.name, encode_path(rel_path)),
	]
}

/// Turn what `gh api` returned into the viewer's shape.
///
/// A 404 is `Ok(None)` — the file is not at that commit — like `git_blob_at`.
/// Output cut at the byte cap is `truncated`; its `size` is then the bytes read,
/// a lower bound, since stopping early is the point and the true length is
/// unknown to us.
pub fn from_remote(
	rel_path: &str,
	output: &super::run::Output,
	max_bytes: Option<usize>,
) -> AppResult<Option<FileContents>> {
	if !output.succeeded() {
		return match super::parse::classify(output.code, &output.stderr) {
			super::parse::GhFailure::NotFound => Ok(None),
			other => Err(other.into_app_error(&format!("{rel_path} at that commit"))),
		};
	}
	let cap = max_bytes.unwrap_or(files::DEFAULT_MAX_BYTES);
	if output.stdout.starts_with(b"[") && looks_like_listing(&output.stdout) {
		// The path is a directory at that commit: the API answers with a JSON
		// array of its entries even for the raw media type.
		return Err(AppError::InvalidInput(format!("{rel_path} is a directory at that commit")));
	}
	Ok(Some(files::contents_from_bytes(rel_path, &output.stdout, output.stdout.len() as u64, cap)))
}

/// A directory listing is a JSON array of objects carrying `"type"` and `"path"`.
/// A source file that merely starts with `[` is not one.
fn looks_like_listing(bytes: &[u8]) -> bool {
	serde_json::from_slice::<Vec<serde_json::Value>>(bytes).is_ok_and(|items| {
		!items.is_empty()
			&& items.iter().all(|i| i.get("type").is_some() && i.get("path").is_some())
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::services::gh::run::Output;

	/// A repository with two commits: `a.txt` + `old name.txt`, then a rename.
	fn repo_with_history() -> (tempfile::TempDir, String, String, String) {
		let dir = tempfile::tempdir().unwrap();
		let repo = git2::Repository::init(dir.path()).unwrap();
		let sig = git2::Signature::now("t", "t@example.com").unwrap();

		let commit =
			|repo: &git2::Repository, files: &[(&str, &[u8])], parent: Option<git2::Oid>| {
				let mut builder = repo.treebuilder(None).unwrap();
				for (name, bytes) in files {
					let blob = repo.blob(bytes).unwrap();
					builder.insert(name, blob, 0o100644).unwrap();
				}
				let tree = repo.find_tree(builder.write().unwrap()).unwrap();
				let parents: Vec<git2::Commit> =
					parent.map(|p| repo.find_commit(p).unwrap()).into_iter().collect();
				let refs: Vec<&git2::Commit> = parents.iter().collect();
				repo.commit(None, &sig, &sig, "c", &tree, &refs).unwrap()
			};

		let nested = {
			// `src/lib.rs` needs a subtree.
			let blob = repo.blob(b"pub fn f() {}\n").unwrap();
			let mut inner = repo.treebuilder(None).unwrap();
			inner.insert("lib.rs", blob, 0o100644).unwrap();
			let inner = inner.write().unwrap();
			let mut outer = repo.treebuilder(None).unwrap();
			outer.insert("src", inner, 0o040000).unwrap();
			let readme = repo.blob(b"hello\n").unwrap();
			outer.insert("README.md", readme, 0o100644).unwrap();
			let bin = repo.blob(b"PK\x00\x01binary").unwrap();
			outer.insert("blob.bin", bin, 0o100644).unwrap();
			let tree = repo.find_tree(outer.write().unwrap()).unwrap();
			repo.commit(None, &sig, &sig, "nested", &tree, &[]).unwrap()
		};
		let first = commit(&repo, &[("old name.txt", b"one\ntwo\n")], None);
		let second = commit(&repo, &[("new name.txt", b"one\ntwo\n")], Some(first));
		(dir, first.to_string(), second.to_string(), nested.to_string())
	}

	#[test]
	fn a_local_commit_is_read_without_the_network() {
		let (dir, first, second, nested) = repo_with_history();
		let project = dir.path().to_string_lossy().into_owned();

		// The base side of a rename: the old path at the old commit.
		match local(&project, "old name.txt", &first, None) {
			Local::Found(c) => {
				assert_eq!(c.contents, "one\ntwo\n");
				assert_eq!(c.size, 8);
				assert_eq!(c.path, "old name.txt");
				assert!(!c.is_binary && !c.truncated);
				assert_eq!(c.line_count, 2);
			}
			_ => panic!("expected the old name at the first commit"),
		}
		// …and the new path at the new commit.
		assert!(matches!(local(&project, "new name.txt", &second, None), Local::Found(_)));

		// A path that is not in that commit's tree is absent, not "ask the network".
		assert!(matches!(local(&project, "new name.txt", &first, None), Local::Absent));
		assert!(matches!(local(&project, "old name.txt", &second, None), Local::Absent));
		assert!(matches!(local(&project, "missing.txt", &second, None), Local::Absent));

		// A nested path, a binary file and a directory.
		assert!(
			matches!(local(&project, "src/lib.rs", &nested, None), Local::Found(c) if c.contents == "pub fn f() {}\n")
		);
		assert!(
			matches!(local(&project, "blob.bin", &nested, None), Local::Found(c) if c.is_binary)
		);
		assert!(matches!(local(&project, "src", &nested, None), Local::Absent));
	}

	#[test]
	fn the_cap_truncates_like_git_blob_at() {
		let (dir, first, _, _) = repo_with_history();
		let project = dir.path().to_string_lossy().into_owned();
		match local(&project, "old name.txt", &first, Some(4)) {
			Local::Found(c) => {
				assert!(c.truncated);
				assert_eq!(c.contents, "one\n");
				assert_eq!(c.size, 8, "the true size, not the cap");
			}
			_ => panic!("expected contents"),
		}
	}

	#[test]
	fn an_unknown_commit_falls_through_to_the_network() {
		let (dir, _, _, _) = repo_with_history();
		let project = dir.path().to_string_lossy().into_owned();
		let elsewhere = "0123456789abcdef0123456789abcdef01234567";
		assert!(matches!(local(&project, "README.md", elsewhere, None), Local::NoCommit));
		assert!(matches!(local(&project, "README.md", "not a sha", None), Local::NoCommit));
		let no_repo = tempfile::tempdir().unwrap();
		assert!(matches!(
			local(&no_repo.path().to_string_lossy(), "README.md", elsewhere, None),
			Local::NoCommit
		));
	}

	#[test]
	fn paths_are_encoded_per_segment() {
		assert_eq!(encode_path("src/lib.rs"), "src/lib.rs");
		assert_eq!(encode_path("old name.txt"), "old%20name.txt");
		assert_eq!(encode_path("a#b/c?d.txt"), "a%23b/c%3Fd.txt");
		assert_eq!(encode_path("docs/ünï.md"), "docs/%C3%BCn%C3%AF.md");
		assert_eq!(encode_path("100%/x"), "100%25/x");
		assert_eq!(encode_path("a+b/c~d_e-f"), "a%2Bb/c~d_e-f");
	}

	fn repo() -> GhRepo {
		crate::services::gh::repo::parse_remote_url("https://github.com/Nightbr/factorai").unwrap()
	}

	#[test]
	fn the_contents_request_names_the_repo_host_and_commit() {
		let sha = "0123456789abcdef0123456789abcdef01234567";
		let args = contents_args(&repo(), "apps/my app/x.ts", sha);
		assert_eq!(
			args,
			[
				"api",
				"--hostname",
				"github.com",
				"-H",
				"Accept: application/vnd.github.raw+json",
				&format!("repos/Nightbr/factorai/contents/apps/my%20app/x.ts?ref={sha}"),
			]
		);
	}

	fn output(stdout: &[u8], stderr: &str, code: i32, overflowed: bool) -> Output {
		Output { stdout: stdout.to_vec(), stderr: stderr.into(), code: Some(code), overflowed }
	}

	#[test]
	fn a_remote_answer_becomes_contents_and_a_404_becomes_none() {
		let ok =
			from_remote("a.ts", &output(b"let a = 1;\n", "", 0, false), None).unwrap().unwrap();
		assert_eq!(ok.contents, "let a = 1;\n");
		assert_eq!(ok.path, "a.ts");

		let missing =
			from_remote("a.ts", &output(b"", "gh: Not Found (HTTP 404)", 1, false), None).unwrap();
		assert!(missing.is_none());

		let denied =
			from_remote("a.ts", &output(b"", "gh: Bad credentials (HTTP 401)", 1, false), None)
				.unwrap_err();
		assert!(matches!(denied, AppError::Unauthenticated(_)));

		let rate = from_remote("a.ts", &output(b"", "API rate limit exceeded", 1, false), None)
			.unwrap_err();
		assert!(matches!(rate, AppError::Remote(_)));
	}

	#[test]
	fn a_remote_read_past_the_cap_is_truncated_not_failed() {
		// The runner read cap + 1 bytes and killed the child, so the exit code is
		// whatever a killed process reports.
		let long = output(b"abcdef", "", 1, true);
		let c = from_remote("big.txt", &long, Some(5)).unwrap().unwrap();
		assert!(c.truncated);
		assert_eq!(c.contents, "abcde");
	}

	#[test]
	fn a_directory_listing_is_not_mistaken_for_a_file() {
		let listing = br#"[{"name":"a","path":"src/a","type":"file"}]"#;
		let err = from_remote("src", &output(listing, "", 0, false), None).unwrap_err();
		assert!(matches!(err, AppError::InvalidInput(_)));
		// A real file that starts with a bracket is a file.
		let json_file = b"[1, 2, 3]\n";
		let c = from_remote("list.json", &output(json_file, "", 0, false), None).unwrap().unwrap();
		assert_eq!(c.contents, "[1, 2, 3]\n");
	}
}
