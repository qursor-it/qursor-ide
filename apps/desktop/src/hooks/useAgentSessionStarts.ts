import { startAgentSession } from '@components/terminal/Terminal';
import { queryKeys } from '@lib/queryKeys';
import { showErrorNotice } from '@lib/errorNotice';
import { cmd, events } from '@lib/tauri';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';

/**
 * Open the session an agent started with `startSession` over MCP.
 *
 * Mounted at the shell like {@link useRoutineFires}, and like it neither
 * navigates nor opens a tab: the session appears in the sidebar and the human
 * opens it. No drain — the event only comes from a live session, so this window
 * was already listening.
 */
export function useAgentSessionStarts(): void {
	const queryClient = useQueryClient();

	useEffect(() => {
		const pending = events.onAgentSessionStart((e) => {
			// `fresh`: the prompt is a first message, so never reuse an idle session.
			void cmd
				.startSession(e.projectId, e.agent, true)
				.then((id) => {
					startAgentSession(id, e);
					void queryClient.invalidateQueries({ queryKey: queryKeys.sessions(e.projectId) });
				})
				.catch((err) => {
					console.error('agent startSession failed', err);
					showErrorNotice(`An agent asked for a new session, but it did not start: ${String(err)}`);
				});
		});
		return () => {
			void pending.then((un) => un());
		};
	}, [queryClient]);
}
