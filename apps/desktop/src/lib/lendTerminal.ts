import type { AgentId } from '@factorai/types';
import { cmd } from '@lib/tauri';
import { type ServerTab, useServerStore } from '@store/serverStore';

/**
 * Lend an SSH tab's terminal to a session that is already open (F46). The tab
 * shows the lend at once and takes it back when the backend refuses. The
 * session is drawn in Sessions mode, so the tab keeps no pane for it.
 */
export async function lendTerminal(
	tab: ServerTab,
	session: { sessionId: string; projectId: string; agent: AgentId },
): Promise<void> {
	if (!tab.terminalId) return;
	const store = useServerStore.getState();
	store.setAgent(tab.key, { ...session, projectCwd: '', mode: 'shared', borrowed: true });
	try {
		await cmd.agentLinkTerminal(tab.terminalId, session.sessionId, 'shared');
	} catch (e) {
		useServerStore.getState().setAgent(tab.key, null);
		throw e;
	}
}

/** Give the terminal back to the human. */
export function takeBackTerminal(tab: ServerTab): void {
	if (tab.terminalId) cmd.agentUnlinkTerminal(tab.terminalId).catch(() => {});
	useServerStore.getState().setAgent(tab.key, null);
}
