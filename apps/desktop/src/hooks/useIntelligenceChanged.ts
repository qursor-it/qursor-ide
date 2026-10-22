import { queryKeys } from '@lib/queryKeys';
import { events } from '@lib/tauri';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';

/**
 * Keep a project's suggestions honest when the backend changes them
 * (specs/05-features.md F33): a run saved its result, or a decision was
 * recorded.
 *
 * Mounted once at the root for the reason `useRoutinesChanged` is: a run
 * finishes for a project whatever page is showing, and a cache invalidated while
 * nobody is looking costs nothing. The events carry no excerpts, hashes or file
 * text (F33), so this only says *which project to refetch* and the queries do
 * the reading.
 */
export function useIntelligenceChanged(): void {
	const queryClient = useQueryClient();

	useEffect(() => {
		const refetch = (projectId: string) => {
			void queryClient.invalidateQueries({ queryKey: queryKeys.improvements(projectId) });
			void queryClient.invalidateQueries({ queryKey: queryKeys.intelligenceRun(projectId) });
		};
		const changed = events.onIntelligenceChanged(({ projectId }) => refetch(projectId));
		const finished = events.onIntelligenceRunFinished(({ projectId }) => refetch(projectId));
		return () => {
			void changed.then((un) => un());
			void finished.then((un) => un());
		};
	}, [queryClient]);
}
