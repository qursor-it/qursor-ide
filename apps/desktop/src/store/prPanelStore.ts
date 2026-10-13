import type { PrStateFilter } from '@factorai/types';
import { create } from 'zustand';

/** What the PRs tab opens on: the pull requests that still need somebody
 *  (F31). Closed and merged are one Select away. */
export const DEFAULT_PR_FILTER: PrStateFilter = 'open';

interface PrPanelState {
	/** The list's state filter. One for the app rather than per project: it is a
	 *  habit about what you look for, like the panel's own tab. */
	filter: PrStateFilter;

	setFilter: (filter: PrStateFilter) => void;
}

/**
 * The PRs tab's list state, and nothing else (F31, specs/04-frontend.md). Which
 * pull request is open is `prTabStore` and the route; this is only the filter.
 *
 * **Deliberately not persisted**: it is a habit about what you look for, and the
 * list is cheap to reopen on the default. The data itself lives in TanStack Query.
 */
export const usePrPanelStore = create<PrPanelState>()((set) => ({
	filter: DEFAULT_PR_FILTER,

	setFilter: (filter) => set({ filter }),
}));
