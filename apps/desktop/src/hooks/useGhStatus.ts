import type { GhStatus } from '@factorai/types';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';
import { isUnauthenticated } from '@lib/pullRequests';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { usePanelStore } from '@store/panelStore';

/**
 * Whether pull requests can be shown for a checkout root, and why not
 * (specs/05-features.md F31). `gh_status` never rejects, so there is no error
 * branch: every problem is a `state`.
 *
 * A minute of `staleTime` is the "does a freshly installed `gh` show up" answer
 * short of the panel's refresh and Retry buttons, which re-probe on purpose; the
 * backend caches the probe and the login for the run on top of this.
 */
export function useGhStatus(root: string | null) {
	return useQuery({
		queryKey: queryKeys.ghStatus(root ?? ''),
		queryFn: () => cmd.ghStatus(root ?? ''),
		enabled: Boolean(root),
		staleTime: 60_000,
	});
}

/**
 * Re-probe `gh` for a root and put the answer in the status cache. What the
 * Retry button runs, and what an `Unauthenticated` rejection from any other
 * `gh` query runs: a plain invalidation would be answered from the backend's
 * cached login and come back `ready` again, so the probe is forced.
 */
export function refreshGhStatus(
	queryClient: ReturnType<typeof useQueryClient>,
	root: string,
): Promise<GhStatus> {
	return cmd.ghStatus(root, true).then((status) => {
		queryClient.setQueryData(queryKeys.ghStatus(root), status);
		return status;
	});
}

/**
 * Send the panel back to its empty state when a `gh` query says the login is
 * gone (F31). Called by every component that owns such a query, so a token that
 * expires mid-session replaces the list with "Run gh auth login" instead of an
 * error the user cannot act on.
 */
export function useResetOnUnauthenticated(root: string | null, error: unknown): void {
	const queryClient = useQueryClient();
	const unauthenticated = isUnauthenticated(error);
	useEffect(() => {
		if (!unauthenticated || !root) return;
		void refreshGhStatus(queryClient, root).catch(() => {
			// The probe never rejects in practice; if the bridge itself failed there
			// is nothing better to show than the error already on screen.
		});
	}, [unauthenticated, root, queryClient]);
}

/** Whether the PRs tab is the one the user is looking at: the gate for every
 *  poll in it, so a hidden tab costs GitHub no requests (F31). */
export function usePrsTabActive(): boolean {
	return usePanelStore((s) => s.open && s.tab === 'prs');
}
