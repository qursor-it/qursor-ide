import type { PrStateFilter } from '@factorai/types';
import { create } from 'zustand';

/** What the PRs tab opens on: the pull requests that still need somebody
 *  (F31). Closed and merged are one Select away. */
export const DEFAULT_PR_FILTER: PrStateFilter = 'open';

interface PrPanelState {
	/** The pull request open in the detail view, **per project**, or null for the
	 *  list. Keyed by project id so switching project and back returns to where
	 *  you were, and never shows project A's #11 under project B's repository. */
	selected: Record<string, number | null>;
	/** The list's state filter. One for the app rather than per project: it is a
	 *  habit about what you look for, like the panel's own tab. */
	filter: PrStateFilter;

	select: (projectId: string, number: number | null) => void;
	setFilter: (filter: PrStateFilter) => void;
}

/**
 * Where the PRs tab is, and nothing else (F31, specs/04-frontend.md).
 *
 * **Deliberately not persisted.** A pull request number from last session may be
 * merged, closed or on a repository whose `origin` has since changed, and
 * reopening the app on a stale detail view is worse than opening on the list.
 * The data itself lives in TanStack Query; this holds only the navigation.
 */
export const usePrPanelStore = create<PrPanelState>()((set) => ({
	selected: {},
	filter: DEFAULT_PR_FILTER,

	select: (projectId, number) => set((s) => ({ selected: { ...s.selected, [projectId]: number } })),
	setFilter: (filter) => set({ filter }),
}));
