import type { PrDetail, PrFile } from '@factorai/types';
import { useQuery } from '@tanstack/react-query';
import { useNavigate, useSearch } from '@tanstack/react-router';
import { useCallback } from 'react';
import { useResetOnUnauthenticated } from '@hooks/useGhStatus';
import { pickPrFile } from '@lib/pullRequests';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { usePrTabStore } from '@store/prTabStore';
import { prViewKey, usePrViewStore } from '@store/prViewStore';

/** The open pull request is re-read every 30s (F31): its checks finishing and a
 *  review landing are what you have it open to see. The list's minute is for a
 *  screen you are scanning, not reading. */
export const DETAIL_POLL_MS = 30_000;

/** The pull request's changed files for its current head (F31).
 *
 *  **The head is in the key**, so a force-push is a new entry and the files of a
 *  given head never expire (`queryKeys.ghPrFiles`). Shared by the page's diff
 *  and the panel's list, which are one cache entry. */
export function usePrFiles(root: string, pr: Pick<PrDetail, 'number' | 'headRefOid'> | undefined) {
	const query = useQuery({
		queryKey: queryKeys.ghPrFiles(root, pr?.number ?? 0, pr?.headRefOid ?? ''),
		queryFn: () => cmd.ghPrFiles(root, pr?.number ?? 0),
		staleTime: Number.POSITIVE_INFINITY,
		retry: false,
		enabled: Boolean(pr),
	});
	useResetOnUnauthenticated(root, query.error);
	return query;
}

/**
 * Which changed file a pull request's page shows, and how to change it
 * (specs/05-features.md F31): the `change` search param of the pull request's
 * route, replaced rather than pushed so Back is not flooded, mirrored into the
 * tab so switching away and back keeps it. Not `file`, which is the viewer's.
 */
export function usePrSelection(
	projectId: string,
	number: number,
	files: readonly PrFile[] | undefined,
) {
	const navigate = useNavigate();
	const { change } = useSearch({ strict: false }) as { change?: string };
	const remembered = usePrTabStore(
		(s) => s.tabs.find((t) => t.projectId === projectId && t.number === number)?.file,
	);
	const setFile = usePrTabStore((s) => s.setFile);
	const setView = usePrViewStore((s) => s.setTab);

	const selected = files ? pickPrFile(files, change, remembered) : null;

	const select = useCallback(
		(relPath: string) => {
			setFile(projectId, number, relPath);
			// A file picked anywhere (the panel's list, a draft comment) is for reading
			// its diff.
			setView(prViewKey(projectId, number), 'files');
			void navigate({
				to: '/projects/$projectId/prs/$number',
				params: { projectId, number: String(number) },
				search: (prev) => ({ ...prev, change: relPath }),
				replace: true,
			});
		},
		[navigate, projectId, number, setFile, setView],
	);

	return { selected, select };
}
