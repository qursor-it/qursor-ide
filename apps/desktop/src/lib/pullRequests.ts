import type { DiffMode } from '@hooks/useFileViewer';
import type {
	AppError,
	CheckState,
	GhStatus,
	PrDetail,
	PrReview,
	PrSummary,
} from '@factorai/types';

/**
 * Presentation helpers for the PRs tab (specs/05-features.md F31). Pure, and in
 * a module of their own, so the labels a reviewer reads can be tested without
 * mounting a list.
 */

/** The command that installs `gh` on this OS, or null where there is no single
 *  one worth printing (Linux package names differ per distribution). */
export function ghInstallCommand(platform: 'windows' | 'mac' | 'other'): string | null {
	if (platform === 'windows') return 'winget install GitHub.cli';
	if (platform === 'mac') return 'brew install gh';
	return null;
}

/** Whether a rejected `gh` command says the login is gone. The one error kind
 *  that is about the *panel's* state rather than about the query that raised it:
 *  the answer is the empty state with `gh auth login` in it (F31). */
export function isUnauthenticated(error: unknown): boolean {
	return (
		typeof error === 'object' &&
		error !== null &&
		(error as Partial<AppError>).kind === 'Unauthenticated'
	);
}

/** A sentence for an error from a `gh` query. Remote errors carry the remote's
 *  own words, which are the useful part ("API rate limit exceeded"). */
export function ghErrorText(error: unknown): string {
	if (typeof error === 'object' && error !== null && 'message' in error) {
		const message = (error as { message: unknown }).message;
		if (typeof message === 'string' && message) return message;
	}
	return 'Could not reach GitHub.';
}

/** The empty-state sentence for a status that is not `ready` and has no custom
 *  layout. `message` is the backend's own words and wins when present. */
export function ghStatusText(status: Pick<GhStatus, 'state' | 'message'>): string {
	if (status.message) return status.message;
	switch (status.state) {
		case 'tooOld':
			return 'Your GitHub CLI is too old for pull requests. Update it with gh upgrade or your package manager.';
		case 'notRepository':
			return 'This folder is not a git repository.';
		case 'noOrigin':
			return 'This repository has no origin remote and no single GitHub remote, so there are no pull requests to show.';
		case 'notGitHub':
			return 'The origin remote is not a GitHub repository.';
		case 'unreachable':
			return 'GitHub could not be reached. Check your connection and retry.';
		default:
			return 'Pull requests are not available for this project.';
	}
}

export function reviewDecisionLabel(decision: PrSummary['reviewDecision']): string | null {
	switch (decision) {
		case 'approved':
			return 'Approved';
		case 'changesRequested':
			return 'Changes requested';
		case 'reviewRequired':
			return 'Review required';
		default:
			return null;
	}
}

export function reviewStateLabel(state: PrReview['state']): string {
	switch (state) {
		case 'approved':
			return 'Approved';
		case 'changesRequested':
			return 'Changes requested';
		case 'commented':
			return 'Commented';
		case 'dismissed':
			return 'Dismissed';
		case 'pending':
			return 'Pending';
	}
}

export function checkStateLabel(state: CheckState): string {
	switch (state) {
		case 'success':
			return 'Passed';
		case 'failure':
			return 'Failed';
		case 'pending':
			return 'Running';
		case 'skipped':
			return 'Skipped';
		case 'neutral':
			return 'Neutral';
		case 'cancelled':
			return 'Cancelled';
	}
}

/** The list row's one-line checks tooltip: "2 of 4 passed, 2 failed". Null when
 *  the pull request has no checks. */
export function checksSummaryText(checks: PrSummary['checks']): string | null {
	if (checks.state === 'none' || checks.total === 0) return null;
	const parts = [`${checks.passed} of ${checks.total} passed`];
	if (checks.failed) parts.push(`${checks.failed} failed`);
	if (checks.pending) parts.push(`${checks.pending} running`);
	return parts.join(', ');
}

export function mergeableLabel(mergeable: PrDetail['mergeable']): string {
	switch (mergeable) {
		case 'mergeable':
			return 'No conflicts';
		case 'conflicting':
			return 'Has conflicts';
		case 'unknown':
			return 'Not computed yet';
	}
}

/** The head repository's owner for a fork, or the words for a deleted one. Null
 *  when the head is a branch of the repository itself. */
export function forkLabel(pr: Pick<PrSummary, 'isCrossRepository' | 'headOwner'>): string | null {
	if (!pr.isCrossRepository) return null;
	return pr.headOwner ?? 'deleted fork';
}

/** The diff mode that opens one file of a pull request (F31). */
export function prDiffMode(number: number, mergeBase: string, head: string): DiffMode {
	return `pr:${number}:${mergeBase}..${head}`;
}
