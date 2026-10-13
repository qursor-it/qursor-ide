import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import type { ReviewLayerInput } from '@components/viewer/prReviewZones';
import { type DiffMode, parsePrRange } from '@hooks/useFileViewer';
import { useGhStatus } from '@hooks/useGhStatus';
import { draftKey } from '@lib/reviewAnchors';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';

/** Comments already on GitHub change while a diff is open, but not by the
 *  second: a short stale time refreshes them on each open without polling. */
const COMMENTS_STALE_MS = 30_000;

/**
 * What the diff's review layer needs for one file of a pull request
 * (specs/05-features.md F31, P3), or `null` when `mode` is not a pull request's
 * or its file list has not arrived.
 *
 * Everything here is a cache hit when the pull request's detail pane is open:
 * the status and the file list are the queries it already ran for this head.
 * The comments are the one new read, and a failure of it only means the diff
 * shows no existing conversation — drafting does not need them.
 */
export function usePrReviewInput(
	mode: DiffMode,
	path: string,
	root: string | null,
): ReviewLayerInput | null {
	const pr = parsePrRange(mode);
	const status = useGhStatus(pr ? root : null);
	const slug = status.data?.repo?.slug ?? null;

	const files = useQuery({
		queryKey: queryKeys.ghPrFiles(root ?? '', pr?.number ?? 0, pr?.head ?? ''),
		queryFn: () => cmd.ghPrFiles(root ?? '', pr?.number ?? 0),
		staleTime: Number.POSITIVE_INFINITY,
		retry: false,
		enabled: Boolean(pr && root),
	});
	const comments = useQuery({
		queryKey: queryKeys.ghPrComments(slug ?? '', pr?.number ?? 0),
		queryFn: () => cmd.ghPrComments(root ?? '', pr?.number ?? 0),
		staleTime: COMMENTS_STALE_MS,
		retry: false,
		enabled: Boolean(pr && root && slug),
	});

	const file = files.data?.files.find((f) => f.path === path);
	const list = comments.data;
	const number = pr?.number;
	const head = pr?.head;
	return useMemo(() => {
		if (!slug || !file || number === undefined || !head) return null;
		return {
			key: draftKey(slug, number),
			headOid: head,
			file: { relPath: file.relPath, noPatch: file.noPatch, commentable: file.commentable },
			comments: list ?? [],
		};
	}, [slug, file, number, head, list]);
}
