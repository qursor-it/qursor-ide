import type { AgentId, ConfirmEvent, LinkMode } from '@factorai/types';
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
	/** Why it could not connect, when that is why it is dead. */
	error?: string | null;
	/** The agent session this SSH session is lent to (F45), if any. */
	agent?: TabAgent | null;
}

/** An agent session using a tab's terminal (F45). */
export interface TabAgent {
	sessionId: string;
	projectId: string;
	projectCwd: string;
	agent: AgentId;
	/** `shared`: the human's own shell. `own`: a session opened for the agent. */
	mode: LinkMode;
}

/** A host key waiting for the human, raised by a tab's first connect. */
interface PendingHostKey {
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
	/** Open a new SSH session on a server and activate it; returns its key. An
	 *  `agent` makes it that agent's own session (F45). */
	open: (serverId: string, agent?: TabAgent) => string;
	setAgent: (key: string, agent: TabAgent | null) => void;
	/** Commands agents are running, by terminal id (F45). Memory only. */
	running: Record<string, string>;
	setRunning: (terminalId: string, command: string | null) => void;
	/** Agent commands waiting for the human (ADR-0108). Memory only. */
	confirms: ConfirmEvent[];
	addConfirm: (c: ConfirmEvent) => void;
	removeConfirm: (id: string) => void;
	close: (key: string) => void;
	activate: (key: string) => void;
	attach: (key: string, terminalId: string) => void;
	markDead: (terminalId: string) => void;
	revive: (key: string) => void;
	/** The tab never got a shell: dead, with the reason to show. */
	fail: (key: string, error: string) => void;
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
			open: (serverId, agent) => {
				const key = `ssh:${crypto.randomUUID()}`;
				set((s) => ({
					tabs: [...s.tabs, { key, serverId, terminalId: null, dead: false, agent: agent ?? null }],
					active: key,
				}));
				return key;
			},
			setAgent: (key, agent) =>
				set((s) => ({ tabs: s.tabs.map((t) => (t.key === key ? { ...t, agent } : t)) })),
			confirms: [],
			addConfirm: (c) => set((s) => ({ confirms: [...s.confirms, c] })),
			removeConfirm: (id) => set((s) => ({ confirms: s.confirms.filter((c) => c.id !== id) })),
			running: {},
			setRunning: (terminalId, command) =>
				set((s) => {
					const running = { ...s.running };
					if (command === null) delete running[terminalId];
					else running[terminalId] = command;
					return { running };
				}),
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
				set((s) => ({
					tabs: s.tabs.map((t) => (t.key === key ? { ...t, dead: false, error: null } : t)),
				})),
			fail: (key, error) =>
				set((s) => ({
					tabs: s.tabs.map((t) =>
						t.key === key ? { ...t, terminalId: null, dead: true, error } : t,
					),
				})),
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
				tabs: s.tabs.map((t) => ({ ...t, terminalId: null, dead: false, error: null })),
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
