import type { AgentId } from '@factorai/types';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

/** The open canvas, and the agent panel's geometry (specs/05-features.md F44). */
interface CanvasState {
	selected: { projectId: string; path: string } | null;
	select: (sel: { projectId: string; path: string } | null) => void;
	/** The open canvases, in the order they were opened: the top bar's tab strip. */
	tabs: { projectId: string; path: string }[];
	/** Closes a tab; the selected one hands the board to its right neighbour, else its left. */
	closeTab: (path: string) => void;
	panelWidth: number;
	setPanelWidth: (width: number) => void;
	panelOpen: boolean;
	togglePanel: () => void;
	/** The agent a canvas's first message starts; the last pick is remembered. */
	agent: AgentId;
	setAgent: (agent: AgentId) => void;
}

export const useCanvasStore = create<CanvasState>()(
	persist(
		(set) => ({
			selected: null,
			select: (selected) =>
				set((s) => ({
					selected,
					tabs:
						!selected || s.tabs.some((t) => t.path === selected.path)
							? s.tabs
							: [...s.tabs, selected],
				})),
			tabs: [],
			closeTab: (path) =>
				set((s) => {
					const i = s.tabs.findIndex((t) => t.path === path);
					if (i < 0) return s;
					const tabs = s.tabs.filter((t) => t.path !== path);
					if (s.selected?.path !== path) return { tabs };
					return { tabs, selected: tabs[i] ?? tabs[i - 1] ?? null };
				}),
			panelWidth: 380,
			setPanelWidth: (panelWidth) => set({ panelWidth }),
			panelOpen: true,
			togglePanel: () => set((s) => ({ panelOpen: !s.panelOpen })),
			agent: 'claude',
			setAgent: (agent) => set({ agent }),
		}),
		{
			name: 'factorai.canvas',
			version: 1,
			storage: createJSONStorage(() => localStorage),
		},
	),
);
