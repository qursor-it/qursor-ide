import type { AgentId } from '@factorai/types';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

/** The open canvas, and the agent panel's geometry (specs/05-features.md F44). */
interface CanvasState {
	selected: { projectId: string; path: string } | null;
	select: (sel: { projectId: string; path: string } | null) => void;
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
			select: (selected) => set({ selected }),
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
