//! Agent-proposed git commands (F35 § Running proposed commands, ADR-0089).
//!
//! A proposal is a line of text. It runs only if it parses into plain argv and
//! passes the allowlist below; it is then spawned directly, never via a shell.

use crate::error::{AppError, AppResult};

const ALLOWED: &[&str] = &[
	"add",
	"restore",
	"commit",
	"switch",
	"checkout",
	"branch",
	"merge",
	"rebase",
	"fetch",
	"pull",
	"push",
	"stash",
	"revert",
	"cherry-pick",
	"tag",
	"log",
	"show",
	"diff",
	"status",
];
const REFUSED: &[&str] =
	&["clean", "config", "filter-branch", "gc", "prune", "update-ref", "reflog"];
const PROGRAM_OPTIONS: &[&str] = &["--exec", "-x", "--upload-pack", "--receive-pack"];
const FORCE_OPTIONS: &[&str] = &["-f", "--force", "--force-with-lease", "--mirror", "--delete"];
const SHELL_CHARS: &str = "|&;<>$`\n()";

/// Splits on whitespace honouring `'...'` and `"..."`; a shell metacharacter
/// outside quotes is refused. Quotes have no escapes: a quote ends at the next
/// one of the same kind.
pub fn parse_proposed(line: &str) -> AppResult<Vec<String>> {
	let mut argv = Vec::new();
	let mut current = String::new();
	let mut has_word = false;
	let mut quote: Option<char> = None;
	for c in line.chars() {
		match quote {
			Some(q) if c == q => quote = None,
			Some(_) => current.push(c),
			None if c == '\'' || c == '"' => {
				quote = Some(c);
				has_word = true;
			}
			None if SHELL_CHARS.contains(c) => {
				return Err(AppError::InvalidInput(format!("{c:?} is not allowed in a command")));
			}
			None if c.is_whitespace() => {
				if has_word {
					argv.push(std::mem::take(&mut current));
					has_word = false;
				}
			}
			None => {
				current.push(c);
				has_word = true;
			}
		}
	}
	if quote.is_some() {
		return Err(AppError::InvalidInput("unterminated quote".into()));
	}
	if has_word {
		argv.push(current);
	}
	Ok(argv)
}

/// `Ok` when the argv (starting with `git`) may run; otherwise the reason.
pub fn validate_proposed(argv: &[String]) -> Result<(), String> {
	if argv.first().map(String::as_str) != Some("git") {
		return Err("only git commands can run".into());
	}
	let Some(sub) = argv.get(1).map(String::as_str) else {
		return Err("no git subcommand".into());
	};
	if sub.starts_with('-') {
		return Err(format!("global option {sub} is not allowed before the subcommand"));
	}
	if REFUSED.contains(&sub) {
		return Err(format!("git {sub} is not allowed"));
	}
	if !ALLOWED.contains(&sub) {
		return Err(format!("git {sub} is not in the allowed list"));
	}
	let rest = &argv[2..];
	if let Some(o) = rest.iter().find(|a| {
		PROGRAM_OPTIONS.contains(&a.as_str())
			|| a.starts_with("--exec=")
			|| a.starts_with("--upload-pack=")
			|| a.starts_with("--receive-pack=")
	}) {
		return Err(format!("{o} runs a program and is not allowed"));
	}
	if let Some(o) = rest.iter().find(|a| a.starts_with("--output")) {
		return Err(format!("{o} writes a file and is not allowed"));
	}
	let has = |names: &[&str]| rest.iter().any(|a| names.contains(&a.as_str()));
	// `-fu` is `-f -u`: a bundle of short flags counts as each of its letters.
	let has_short = |letter: char| {
		rest.iter().any(|a| {
			a.len() > 1 && a.starts_with('-') && !a.starts_with("--") && a.contains(letter)
		})
	};
	if sub == "rebase" && (has(&["--interactive"]) || has_short('i')) {
		return Err("interactive rebase is not allowed".into());
	}
	// `+ref` force-pushes and `:ref` deletes, without any flag.
	if sub == "push"
		&& (has(FORCE_OPTIONS)
			|| has_short('f')
			|| has_short('d')
			|| rest.iter().any(|a| {
				a.starts_with("--force-with-lease=") || a.starts_with('+') || a.contains(":")
			})) {
		return Err("force, mirror and delete pushes are not allowed".into());
	}
	Ok(())
}

/// Parse and validate one line; the argv (starting with `git`) or the reason.
pub fn check_line(line: &str) -> Result<Vec<String>, String> {
	let argv = parse_proposed(line).map_err(|e| match e {
		AppError::InvalidInput(m) => m,
		other => other.to_string(),
	})?;
	validate_proposed(&argv)?;
	Ok(argv)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn check(line: &str) -> Result<(), String> {
		validate_proposed(&parse_proposed(line).map_err(|e| e.to_string())?)
	}

	#[test]
	fn allows_plain_revert() {
		assert_eq!(check("git revert 4e1a9c2"), Ok(()));
	}

	#[test]
	fn refuses_shell_chars() {
		for line in
			["git log; rm -rf /", "git log | cat", "git log && x", "git log $(x)", "git log `x`"]
		{
			assert!(parse_proposed(line).is_err(), "{line}");
		}
		assert!(parse_proposed("git commit -m \"a;b\"").is_ok());
	}

	#[test]
	fn refuses_global_c() {
		assert!(check("git -c core.sshCommand=x fetch").is_err());
		assert!(check("git -C /tmp status").is_err());
	}

	#[test]
	fn refuses_force_push() {
		for flag in ["-f", "--force", "--force-with-lease", "--mirror", "--delete"] {
			assert!(check(&format!("git push {flag} origin main")).is_err(), "{flag}");
		}
		assert_eq!(check("git push origin main"), Ok(()));
	}

	#[test]
	fn refuses_force_and_delete_without_flags() {
		for line in [
			"git push origin +main",
			"git push origin :old",
			"git push -fu origin main",
			"git push -d origin x",
			"git rebase -ir main",
			"git log --output=/tmp/x",
		] {
			assert!(check(line).is_err(), "{line}");
		}
		assert_eq!(check("git push -u origin main"), Ok(()));
	}

	#[test]
	fn refuses_reset_hard() {
		assert!(check("git reset --hard HEAD~1").is_err());
	}

	#[test]
	fn refuses_interactive_rebase() {
		assert!(check("git rebase -i HEAD~3").is_err());
		assert!(check("git rebase --interactive main").is_err());
		assert_eq!(check("git rebase main"), Ok(()));
	}

	#[test]
	fn refuses_non_git() {
		assert!(check("rm x").is_err());
		assert!(check("").is_err());
	}

	#[test]
	fn refuses_clean_and_config() {
		for line in ["git clean -fd", "git config user.name x", "git gc", "git reflog expire --all"]
		{
			assert!(check(line).is_err(), "{line}");
		}
		assert!(check("git rebase --exec make main").is_err());
	}

	#[test]
	fn quoted_message_is_one_arg() {
		let argv = parse_proposed("git commit -m \"a b\"").unwrap();
		assert_eq!(argv, ["git", "commit", "-m", "a b"]);
	}
}
