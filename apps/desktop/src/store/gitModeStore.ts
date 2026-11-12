import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

export type AppMode = 'sessions' | 'git';

/** One Git tab: a checkout directory of a project (F35). */
export interface GitTab {
	projectId: string;
	dir: string;
}

interface GitModeState {
	mode: AppMode;
	tabs: GitTab[];
	activeDir: string | null;
	setMode: (mode: AppMode) => void;
	/** Dedupes by `dir` and activates. */
	openTab: (projectId: string, dir: string) => void;
	closeTab: (dir: string) => void;
	activate: (dir: string) => void;
	/** HEAD before a rebase, per directory, for "Undo rebase". Memory only. */
	preRebase: Record<string, string | undefined>;
	setPreRebase: (dir: string, sha: string | null) => void;
	/** Sessions started from Git mode, for the origin strip. Memory only. */
	origin: Record<string, { dir: string; title: string } | undefined>;
	setOrigin: (sessionId: string, origin: { dir: string; title: string }) => void;
}

/**
 * The window's mode and its Git tabs (specs/05-features.md F35).
 *
 * Plain `localStorage`, written on every change: a mode flip is rare and the
 * synchronous write is what keeps the first frame from flashing the other mode
 * (ADR-0013).
 */
export const useGitModeStore = create<GitModeState>()(
	persist(
		(set) => ({
			mode: 'sessions',
			tabs: [],
			activeDir: null,
			setMode: (mode) => set({ mode }),
			openTab: (projectId, dir) =>
				set((s) => ({
					tabs: s.tabs.some((t) => t.dir === dir) ? s.tabs : [...s.tabs, { projectId, dir }],
					activeDir: dir,
				})),
			closeTab: (dir) =>
				set((s) => {
					const index = s.tabs.findIndex((t) => t.dir === dir);
					if (index < 0) return s;
					const tabs = s.tabs.filter((t) => t.dir !== dir);
					// Closing the one you are on lands on its neighbour, the way a
					// browser does; closing another tab leaves the selection alone.
					const activeDir =
						s.activeDir === dir
							? (tabs[Math.min(index, tabs.length - 1)]?.dir ?? null)
							: s.activeDir;
					return { tabs, activeDir };
				}),
			activate: (dir) => set((s) => (s.tabs.some((t) => t.dir === dir) ? { activeDir: dir } : s)),
			preRebase: {},
			setPreRebase: (dir, sha) =>
				set((s) => ({ preRebase: { ...s.preRebase, [dir]: sha ?? undefined } })),
			origin: {},
			setOrigin: (sessionId, origin) =>
				set((s) => ({ origin: { ...s.origin, [sessionId]: origin } })),
		}),
		{
			name: 'factorai.gitmode',
			version: 1,
			storage: createJSONStorage(() => localStorage),
			partialize: (s) => ({ mode: s.mode, tabs: s.tabs, activeDir: s.activeDir }),
		},
	),
);
