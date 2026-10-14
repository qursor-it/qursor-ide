import type { AgentId, GhRepo, GitWorktree, PrDetail } from '@factorai/types';
import { useNavigate } from '@tanstack/react-router';
import { useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { useWorktrees } from '@hooks/useWorktrees';
import { buildPrReviewPrompt } from '@lib/prReviewPrompt';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { usePrefsStore } from '@store/prefsStore';
import { useTerminalStore } from '@store/terminalStore';

/**
 * The linked checkout that has this pull request's head out, if any: the branch
 * `gh pr checkout` names (`pr/<n>`), or a HEAD at exactly the head commit. The
 * main checkout is never it — a review must not run where the human works.
 */
export function findPrWorktree(
	worktrees: readonly GitWorktree[],
	pr: Pick<PrDetail, 'number' | 'headRefOid'>,
): GitWorktree | undefined {
	return worktrees.find(
		(w) =>
			!w.isMain &&
			w.exists &&
			(w.branch === `pr/${pr.number}` || (w.head !== null && w.head === pr.headRefOid)),
	);
}

/**
 * "Review with agent" (specs/05-features.md F31, ADR-0078): start a brand-new
 * session whose first message is the review prompt, and go to it.
 *
 * `fresh` because a reused idle session is already running and would drop the
 * prompt. The prompt and cwd are parked in the store for `Terminal`'s spawn, the
 * way `useStartSession` parks the agent. When a worktree has the head out, the
 * session runs there and is pinned to it (F21), as `pickWorktree` does.
 *
 * Failures propagate to the caller, which says them.
 */
export function useStartReviewSession(
	projectId: string,
	root: string | null,
): (args: { pr: PrDetail; repo: GhRepo; agent?: AgentId }) => Promise<void> {
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const worktrees = useWorktrees(root);

	return useCallback(
		async ({ pr, repo, agent }) => {
			const worktree = findPrWorktree(worktrees, pr);
			const cwd = worktree?.path;
			const prompt = buildPrReviewPrompt({
				pr,
				repo,
				checkedOut: Boolean(cwd),
				instructions: usePrefsStore.getState().prReviewInstructions,
			});
			const sessionId = await cmd.startSession(projectId, agent, true);
			const store = useTerminalStore.getState();
			if (agent) store.setLaunchAgent(sessionId, agent);
			store.setLaunch(sessionId, { prompt, cwd });
			if (worktree && cwd) {
				store.pinWorktree(sessionId, cwd, worktree.branch);
				if (root) {
					void cmd
						.setSessionWorktree(sessionId, root, cwd)
						.then(() => queryClient.invalidateQueries({ queryKey: queryKeys.sessions(projectId) }))
						.catch((e) => console.error('set_session_worktree failed', e));
				}
			}
			await navigate({
				to: '/projects/$projectId/sessions/$sessionId',
				params: { projectId, sessionId },
			});
		},
		[navigate, queryClient, worktrees, projectId, root],
	);
}
