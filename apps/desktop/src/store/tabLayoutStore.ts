import { deferredLocalStorage } from '@lib/persistStorage';
import { isPrKeyOfProject } from '@lib/tabGroups';
import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface TabLayoutState {
	/** Project ids, in the order their groups sit on the strip. */
	groupOrder: string[];
	/** Tab keys (`sessionTabKey`, `prTabKey`), in the order they sit within their
	 *  group. Only the order: what is open is `terminalStore.tabs` and
	 *  `prTabStore.tabs`. */
	itemOrder: string[];
	/** Project ids whose group is folded down to its chip. */
	collapsed: string[];
	setGroupOrder: (ids: string[]) => void;
	setItemOrder: (keys: string[]) => void;
	toggleCollapsed: (projectId: string) => void;
	/** The project was removed from the workspace. */
	forgetProject: (projectId: string) => void;
}

/**
 * How the top-bar tabs are arranged (specs/05-features.md F16, ADR-0080).
 *
 * Orders only. Keys of tabs that have since closed stay behind and are ignored
 * by `groupTabs`; the next reorder writes the full visible order back, which
 * prunes them.
 */
export const useTabLayoutStore = create<TabLayoutState>()(
	persist(
		(set) => ({
			groupOrder: [],
			itemOrder: [],
			collapsed: [],
			setGroupOrder: (groupOrder) => set({ groupOrder }),
			setItemOrder: (itemOrder) => set({ itemOrder }),
			toggleCollapsed: (projectId) =>
				set((s) => ({
					collapsed: s.collapsed.includes(projectId)
						? s.collapsed.filter((id) => id !== projectId)
						: [...s.collapsed, projectId],
				})),
			forgetProject: (projectId) =>
				set((s) => ({
					groupOrder: s.groupOrder.filter((id) => id !== projectId),
					collapsed: s.collapsed.filter((id) => id !== projectId),
					itemOrder: s.itemOrder.filter((k) => !isPrKeyOfProject(k, projectId)),
				})),
		}),
		{
			name: 'factorai.tabLayout',
			storage: deferredLocalStorage(),
			version: 1,
			partialize: (s) => ({
				groupOrder: s.groupOrder,
				itemOrder: s.itemOrder,
				collapsed: s.collapsed,
			}),
		},
	),
);
