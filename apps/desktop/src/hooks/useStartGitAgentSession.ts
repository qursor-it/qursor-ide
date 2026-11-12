import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useTerminalStore } from '@store/terminalStore';
import { useNavigate } from '@tanstack/react-router';
import { useCallback } from 'react';

/**
 * "Resolve with agent" (specs/05-features.md F35): a brand-new session whose
 * first message is `prompt`, run in the checkout `dir`, remembered as started
 * from Git so the session pane can say so and lead back. The same flow as
 * `useStartReviewSession`; failures propagate to the caller.
 */
export function useStartGitAgentSession(): (
	projectId: string,
	dir: string,
	title: string,
	prompt: string,
) => Promise<void> {
	const navigate = useNavigate();
	return useCallback(
		async (projectId, dir, title, prompt) => {
			const sessionId = await cmd.startSession(projectId, undefined, true);
			useTerminalStore.getState().setLaunch(sessionId, { prompt, cwd: dir });
			const git = useGitModeStore.getState();
			git.setOrigin(sessionId, { dir, title });
			git.setMode('sessions');
			await navigate({
				to: '/projects/$projectId/sessions/$sessionId',
				params: { projectId, sessionId },
			});
		},
		[navigate],
	);
}
