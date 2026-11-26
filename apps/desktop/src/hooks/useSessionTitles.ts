import type { DelegationList, SessionSummary } from '@factorai/types';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useQueries } from '@tanstack/react-query';
import { useMemo } from 'react';

/**
 * Titles for the open sessions, looked up per project.
 *
 * One query per distinct project rather than per session — they share the cache
 * entry the sidebar and project page already fill, so an open project costs
 * nothing extra. A session too new to be indexed has no title yet and falls
 * back to its short id, matching the session header — as does a restored tab
 * whose transcript has since been deleted, since the index row goes with it.
 */
export function useSessionTitles(tabs: Array<{ projectId: string }>): Map<string, string> {
	const projectIds = useMemo(() => [...new Set(tabs.map((t) => t.projectId))].sort(), [tabs]);

	// `combine` rather than a useMemo over the results array: useQueries hands
	// back a fresh array on every render, so a memo would either lie about its
	// dependencies or recompute constantly. This is the option built for it.
	const titles = useQueries({
		queries: projectIds.map((projectId) => ({
			queryKey: queryKeys.sessions(projectId),
			queryFn: () => cmd.listSessions(projectId),
		})),
		combine: titlesOf,
	});
	// A delegate is named by its brief until its agent indexes a title (F50).
	const briefs = useQueries({
		queries: projectIds.map((projectId) => ({
			queryKey: queryKeys.delegations(projectId),
			queryFn: () => cmd.listDelegations(projectId),
		})),
		combine: briefsOf,
	});
	return useMemo(() => {
		if (briefs.size === 0) return titles;
		const merged = new Map(briefs);
		for (const [id, title] of titles) merged.set(id, title);
		return merged;
	}, [titles, briefs]);
}

function briefsOf(results: Array<{ data?: unknown }>): Map<string, string> {
	const map = new Map<string, string>();
	for (const result of results) {
		const list = result.data as DelegationList | undefined;
		for (const d of list?.active ?? []) map.set(d.sessionId, d.prompt);
	}
	return map;
}

/** Module-level so its identity is stable: useQueries re-runs an inline
 *  `combine` on every render, and this one walks every session of every open
 *  project. */
function titlesOf(results: Array<{ data?: unknown }>): Map<string, string> {
	const map = new Map<string, string>();
	for (const result of results) {
		for (const session of (result.data ?? []) as SessionSummary[]) {
			const title = session.title.trim();
			if (title) map.set(session.id, title);
		}
	}
	return map;
}
