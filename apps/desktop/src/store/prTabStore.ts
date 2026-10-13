import { deferredLocalStorage } from '@lib/persistStorage';
import { usePrefsStore } from '@store/prefsStore';
import { create } from 'zustand';
import { persist } from 'zustand/middleware';

/** A pull request open as a tab in the top strip (F31). Carries its project
 *  because the number alone names nothing, and the title only so the strip can
 *  paint before the detail has been read; it is refreshed from the list and the
 *  detail while they are loaded. */
interface PrTab {
	projectId: string;
	number: number;
	title?: string;
	/** The changed file last selected on the page (repository-relative), so
	 *  switching back to the tab shows the diff you left. */
	file?: string;
}

interface PrTabState {
	tabs: PrTab[];
	/** Open a pull request as a tab: appended when new, left where it is when it
	 *  is already open. A later title fills a missing one but never blanks it. */
	open: (projectId: string, number: number, title?: string) => void;
	setTitle: (projectId: string, number: number, title: string) => void;
	setFile: (projectId: string, number: number, file: string) => void;
	close: (projectId: string, number: number) => void;
	/** Move a tab to `toIndex` within the pull request tabs. */
	reorder: (projectId: string, number: number, toIndex: number) => void;
	closeProject: (projectId: string) => void;
}

const same = (t: PrTab, projectId: string, number: number) =>
	t.projectId === projectId && t.number === number;

/** Hydration: the persisted tabs, unless the restore switch is off. Exported for
 *  its own test, which has no `localStorage` to hydrate from. */
export function mergePrTabs(persisted: unknown, current: PrTabState): PrTabState {
	const state = { ...current, ...(persisted as Partial<PrTabState>) };
	if (usePrefsStore.getState().restoreTabs) return state;
	return { ...state, tabs: [] };
}

/**
 * The open pull request tabs (specs/05-features.md F31, specs/04-frontend.md).
 *
 * Persisted like the session tabs and honouring the same restore switch at
 * hydration. Unlike the old in-panel selection this may name a pull request that
 * has since been merged: that is fine, a tab is a document you opened and the
 * detail reads whatever state it is in now.
 */
export const usePrTabStore = create<PrTabState>()(
	persist(
		(set) => ({
			tabs: [],
			open: (projectId, number, title) =>
				set((s) => {
					const i = s.tabs.findIndex((t) => same(t, projectId, number));
					if (i < 0) return { tabs: [...s.tabs, { projectId, number, title }] };
					if (title && !s.tabs[i].title) {
						const next = [...s.tabs];
						next[i] = { ...next[i], title };
						return { tabs: next };
					}
					return s;
				}),
			setTitle: (projectId, number, title) =>
				set((s) => {
					const i = s.tabs.findIndex((t) => same(t, projectId, number));
					if (i < 0 || s.tabs[i].title === title) return s;
					const next = [...s.tabs];
					next[i] = { ...next[i], title };
					return { tabs: next };
				}),
			setFile: (projectId, number, file) =>
				set((s) => {
					const i = s.tabs.findIndex((t) => same(t, projectId, number));
					if (i < 0 || s.tabs[i].file === file) return s;
					const next = [...s.tabs];
					next[i] = { ...next[i], file };
					return { tabs: next };
				}),
			close: (projectId, number) =>
				set((s) => ({ tabs: s.tabs.filter((t) => !same(t, projectId, number)) })),
			reorder: (projectId, number, toIndex) =>
				set((s) => {
					const from = s.tabs.findIndex((t) => same(t, projectId, number));
					if (from < 0) return s;
					const next = [...s.tabs];
					const [moved] = next.splice(from, 1);
					next.splice(Math.max(0, Math.min(next.length, toIndex)), 0, moved);
					return { tabs: next };
				}),
			closeProject: (projectId) =>
				set((s) => ({ tabs: s.tabs.filter((t) => t.projectId !== projectId) })),
		}),
		{
			name: 'factorai.prTabs',
			storage: deferredLocalStorage(),
			version: 1,
			partialize: (s) => ({ tabs: s.tabs }),
			// The restore switch is honoured at hydration, as for the session tabs
			// (F11, F16): off means the strip starts without them.
			merge: mergePrTabs,
		},
	),
);
