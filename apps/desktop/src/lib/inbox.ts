import type { TerminalStatus } from '@factorai/types';

export interface InboxEntry {
	sessionId: string;
	projectId: string;
	status: TerminalStatus;
}

/**
 * The sidebar's Inbox (specs/05-features.md F36): every session that wants you,
 * then every session working, across all projects.
 *
 * Open tabs come first, in tab order, then live sessions with no tab (a
 * routine's, F22). A session wants you while it is `waiting_input` or carries an
 * unseen mark (F32) — a `stopped` one you have seen asks nothing and stays out.
 *
 * Structural parameters, like the rest of `lib/`: no store imports.
 */
export function inboxSections(
	tabs: ReadonlyArray<{ sessionId: string; projectId: string }>,
	bySession: Record<string, { projectId: string; status: TerminalStatus }>,
	unseen: Record<string, true>,
): { needsYou: InboxEntry[]; working: InboxEntry[] } {
	const seen = new Set<string>();
	const all: InboxEntry[] = [];
	const add = (sessionId: string, projectId: string) => {
		if (seen.has(sessionId)) return;
		seen.add(sessionId);
		all.push({ sessionId, projectId, status: bySession[sessionId]?.status ?? 'stopped' });
	};
	for (const t of tabs) add(t.sessionId, t.projectId);
	for (const [sessionId, live] of Object.entries(bySession)) add(sessionId, live.projectId);

	const wants = (e: InboxEntry) => e.status === 'waiting_input' || unseen[e.sessionId] === true;
	return {
		needsYou: all.filter(wants),
		working: all.filter((e) => e.status === 'working' && !wants(e)),
	};
}
