import { create } from 'zustand';

export type PrSubTab = 'files' | 'overview' | 'review';

/** `projectId#number`: the key a pull request's page state is held under. */
export function prViewKey(projectId: string, number: number): string {
	return `${projectId}#${number}`;
}

interface PrViewState {
	/** Which sub-tab (Changes, Overview, Review) each open pull request shows.
	 *  Not persisted: a page you reopen starts on its diff. */
	tabByPr: Record<string, PrSubTab>;
	setTab: (key: string, tab: PrSubTab) => void;
}

/**
 * The sub-tab of a pull request's page (specs/05-features.md F31), in a store
 * rather than the page's state because the panel's file list is what asks for
 * Changes: picking a file there has to bring the center back to its diff, from
 * a component that is not the page's child.
 */
export const usePrViewStore = create<PrViewState>()((set) => ({
	tabByPr: {},
	setTab: (key, tab) =>
		set((s) => (s.tabByPr[key] === tab ? s : { tabByPr: { ...s.tabByPr, [key]: tab } })),
}));
