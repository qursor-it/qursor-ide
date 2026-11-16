import { restartSession } from '@components/terminal/Terminal';
import { useTerminalStore } from '@store/terminalStore';
import { useNavigate } from '@tanstack/react-router';
import { useCallback } from 'react';

/** Open a session from a list (the Inbox, the palette, the Board): one that
 *  stopped is restarted first, exactly as a click on a stopped tab is (F16). */
export function useOpenSession(): (sessionId: string, projectId: string) => void {
	const navigate = useNavigate();
	return useCallback(
		(sessionId, projectId) => {
			if (!useTerminalStore.getState().bySession[sessionId]) restartSession(sessionId);
			void navigate({
				to: '/projects/$projectId/sessions/$sessionId',
				params: { projectId, sessionId },
			});
		},
		[navigate],
	);
}
