import { isWithin } from '@lib/paths';

/** Slack after a session's last transcript entry: the commit's own output is
 *  usually what the transcript records next, so it lands a moment later. */
const SLACK_MS = 60_000;

interface SessionLike {
	id: string;
	createdAt: number;
	updatedAt: number;
	cwd: string | null;
	worktree: string | null;
	subagentOf: string | null;
}

/**
 * The session that was running in `checkout` when a commit was made (F41), or
 * null. An inference from times and folders, not a record — which is why the UI
 * says "while … was running" rather than "made by". A session in another
 * worktree never matches; sub-agents are their parent's, so they are skipped.
 * With several candidates, the one started last wins.
 */
export function sessionDuring<T extends SessionLike>(
	time: number,
	sessions: readonly T[],
	checkout: string,
): T | null {
	let best: T | null = null;
	for (const s of sessions) {
		if (s.subagentOf) continue;
		const inCheckout = s.worktree
			? isWithin(s.worktree, checkout) && isWithin(checkout, s.worktree)
			: s.cwd !== null && isWithin(s.cwd, checkout);
		if (!inCheckout) continue;
		if (time < s.createdAt || time > s.updatedAt + SLACK_MS) continue;
		if (!best || s.createdAt > best.createdAt) best = s;
	}
	return best;
}
