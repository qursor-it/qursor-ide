import type { GitOp, GitOpResult } from '@factorai/types';

export type GitErrorAction =
	| { kind: 'retry' }
	| { kind: 'pullThenPush' }
	| { kind: 'stashAndRetry' }
	| { kind: 'pushSetUpstream' }
	| { kind: 'askAgent' }
	| { kind: 'copy' };

export interface GitErrorInfo {
	title: string;
	actions: GitErrorAction[];
}

const has = (text: string, needles: string[]) => needles.some((n) => text.includes(n));

/** "commit" -> "Commit", "createBranch" -> "Create branch". */
function opLabel(kind: GitOp['kind']): string {
	const words = kind.replace(/([A-Z])/g, ' $1').toLowerCase();
	return words.charAt(0).toUpperCase() + words.slice(1);
}

/**
 * What the error dialog says about a failed op, or null when none should open
 * (success, or a conflict: that is the ConflictBanner's job).
 */
export function explainGitError(op: GitOp, result: GitOpResult): GitErrorInfo | null {
	if (result.ok) return null;
	const text = `${result.stderr}\n${result.stdout}`.toLowerCase();
	// Only these leave a merge or rebase in progress, which the banner shows; a
	// conflicted stash pop has no banner, so it gets the dialog.
	const banner = ['merge', 'rebase', 'rebaseContinue', 'pull'].includes(op.kind);
	if (banner && has(text, ['conflict', 'automatic merge failed', 'could not apply'])) return null;
	const copy: GitErrorAction = { kind: 'copy' };
	if (has(text, ['[rejected]', 'non-fast-forward', 'fetch first', 'updates were rejected']))
		return {
			title: "Push rejected: the remote has commits you don't have",
			actions: [{ kind: 'pullThenPush' }, { kind: 'askAgent' }, copy],
		};
	if (
		has(text, [
			'would be overwritten',
			'please commit your changes or stash them',
			'cannot pull with rebase: you have unstaged changes',
			'your local changes',
		])
	)
		return { title: 'Local changes are in the way', actions: [{ kind: 'stashAndRetry' }, copy] };
	if (has(text, ['has no upstream branch', 'no tracking information']))
		return {
			title: 'This branch has no upstream',
			actions:
				op.kind === 'push' || op.kind === 'pull' ? [{ kind: 'pushSetUpstream' }, copy] : [copy],
		};
	if (has(text, ['index.lock', 'another git process']))
		return {
			title: 'The repository is locked by another git process',
			actions: [{ kind: 'retry' }, copy],
		};
	if (
		has(text, [
			'authentication failed',
			'permission denied',
			'could not read from remote',
			'could not resolve host',
		])
	)
		return { title: 'Could not reach the remote', actions: [{ kind: 'retry' }, copy] };
	if (text.includes('git not found on path'))
		return { title: 'git is not installed or not on PATH', actions: [copy] };
	return {
		title: `${opLabel(op.kind)} failed`,
		actions: [{ kind: 'retry' }, { kind: 'askAgent' }, copy],
	};
}
