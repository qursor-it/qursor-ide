import type { GitWorktree } from '@factorai/types';
import { useNavigate } from '@tanstack/react-router';
import { useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useTerminalStore } from '@store/terminalStore';

/**
 * Start a brand-new session that runs in a given checkout, pinned to it, and go
 * to it (specs/05-features.md F31 P4, F21). What "Open checkout" and the
 * checkout toast's "Start session there" do.
 *
 * `fresh` because a reused idle session already has a cwd of its own. The cwd is
 * parked in the store for `Terminal`'s spawn, as `useStartReviewSession` does,
 * and the pin follows `pickWorktree`'s order: the store first, then the row.
 */
export function useStartWorktreeSession(
	projectId: string,
	root: string | null,
): (worktree: GitWorktree) => Promise<void> {
	const navigate = useNavigate();
	const queryClient = useQueryClient();

	return useCallback(
		async (worktree) => {
			const sessionId = await cmd.startSession(projectId, undefined, true);
			const store = useTerminalStore.getState();
			store.setLaunch(sessionId, { cwd: worktree.path });
			store.pinWorktree(sessionId, worktree.path, worktree.branch);
			if (root) {
				void cmd
					.setSessionWorktree(sessionId, root, worktree.path)
					.then(() => queryClient.invalidateQueries({ queryKey: queryKeys.sessions(projectId) }))
					.catch((e) => console.error('set_session_worktree failed', e));
			}
			await navigate({
				to: '/projects/$projectId/sessions/$sessionId',
				params: { projectId, sessionId },
			});
		},
		[navigate, queryClient, projectId, root],
	);
}
