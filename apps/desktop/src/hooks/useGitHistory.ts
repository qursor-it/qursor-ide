import type { GitGraph, GitGraphCommit } from '@factorai/types';
import { useQueries, useQueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useState } from 'react';
import { stitchPages } from '@lib/gitGraph';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';

/** Commits per page, matching `GRAPH_PAGE` in `services/git.rs`. */
const PAGE = 300;
const POLL_MS = 30_000;

/**
 * The commit graph of one directory, paged, for Git mode (F35). `useGitGraph`
 * is the panel's and follows the active project and the Graph tab; this one
 * follows the Git tab it is mounted for, so the caller keys it on `dir`.
 */
export function useGitHistory(dir: string): {
	commits: GitGraphCommit[];
	laneCount: number;
	graph: GitGraph | undefined;
	isPending: boolean;
	hasMore: boolean;
	isLoadingMore: boolean;
	loadMore: () => void;
} {
	const queryClient = useQueryClient();
	const [pageCount, setPageCount] = useState(1);
	const pages = useQueries({
		queries: Array.from({ length: pageCount }, (_unused, page) => ({
			queryKey: queryKeys.gitGraph(dir, page),
			queryFn: () => cmd.gitGraph(dir, page * PAGE, PAGE),
			refetchInterval: POLL_MS,
			refetchOnWindowFocus: true,
		})),
	});
	const { commits, laneCount, hasMore, stale } = stitchPages(pages.map((p) => p.data));

	// Refs moved mid-paging: back to one page, as the panel's graph does.
	useEffect(() => {
		if (!stale) return;
		setPageCount(1);
		void queryClient.invalidateQueries({ queryKey: ['git-graph', dir] });
	}, [stale, dir, queryClient]);

	const loadMore = useCallback(() => setPageCount((n) => n + 1), []);
	return {
		commits,
		laneCount,
		graph: pages[0]?.data,
		isPending: pages[0]?.isPending ?? true,
		hasMore,
		isLoadingMore: pages.length > 1 && pages[pages.length - 1].isPending,
		loadMore,
	};
}
