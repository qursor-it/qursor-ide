import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

/** One SSH session tab in the Server mode's top bar (F44, ADR-0105). */
export interface ServerTab {
	/** `ssh:<uuid>` — also the pooled xterm's key and the PTY's client key. */
	key: string;
	serverId: string;
	/** The live PTY; memory only, so a restarted app reconnects on activation. */
	terminalId: string | null;
	/** Its shell exited or the connection dropped; the tab stays until closed. */
	dead: boolean;
}

/** A host key waiting for the human, raised by a tab's first connect. */
export interface PendingHostKey {
	tabKey: string;
	serverId: string;
	fingerprint: string;
}

interface ServerState {
	tabs: ServerTab[];
	active: string | null;
	/** Address-book folders the user closed. */
	collapsed: string[];
	pendingHostKey: PendingHostKey | null;
	/** Open a new SSH session on a server and activate it; returns its key. */
	open: (serverId: string) => string;
	close: (key: string) => void;
	activate: (key: string) => void;
	attach: (key: string, terminalId: string) => void;
	markDead: (terminalId: string) => void;
	revive: (key: string) => void;
	toggleFolder: (id: string) => void;
	setPendingHostKey: (p: PendingHostKey | null) => void;
}

/**
 * The Server mode's tabs: one per SSH session, not per server (F44).
 *
 * Persisted without their PTYs: after a restart the tabs come back and each
 * logs in again when it is shown.
 */
export const useServerStore = create<ServerState>()(
	persist(
		(set) => ({
			tabs: [],
			active: null,
			collapsed: [],
			pendingHostKey: null,
			open: (serverId) => {
				const key = `ssh:${crypto.randomUUID()}`;
				set((s) => ({
					tabs: [...s.tabs, { key, serverId, terminalId: null, dead: false }],
					active: key,
				}));
				return key;
			},
			close: (key) =>
				set((s) => {
					const i = s.tabs.findIndex((t) => t.key === key);
					const tabs = s.tabs.filter((t) => t.key !== key);
					const active =
						s.active === key ? (tabs[Math.min(i, tabs.length - 1)]?.key ?? null) : s.active;
					return { tabs, active };
				}),
			activate: (key) => set({ active: key }),
			attach: (key, terminalId) =>
				set((s) => ({
					tabs: s.tabs.map((t) => (t.key === key ? { ...t, terminalId, dead: false } : t)),
				})),
			markDead: (terminalId) =>
				set((s) => ({
					tabs: s.tabs.map((t) =>
						t.terminalId === terminalId ? { ...t, terminalId: null, dead: true } : t,
					),
				})),
			revive: (key) =>
				set((s) => ({ tabs: s.tabs.map((t) => (t.key === key ? { ...t, dead: false } : t)) })),
			toggleFolder: (id) =>
				set((s) => ({
					collapsed: s.collapsed.includes(id)
						? s.collapsed.filter((c) => c !== id)
						: [...s.collapsed, id],
				})),
			setPendingHostKey: (pendingHostKey) => set({ pendingHostKey }),
		}),
		{
			name: 'qursor.servers',
			storage: createJSONStorage(() => localStorage),
			partialize: (s) => ({
				tabs: s.tabs.map((t) => ({ ...t, terminalId: null, dead: false })),
				active: s.active,
				collapsed: s.collapsed,
			}),
		},
	),
);

/** A tab's label: the server's name, numbered when it has siblings. */
export function tabLabel(tabs: readonly ServerTab[], tab: ServerTab, name: string): string {
	const same = tabs.filter((t) => t.serverId === tab.serverId);
	if (same.length < 2) return name;
	return `${name} · ${same.indexOf(tab) + 1}`;
}
