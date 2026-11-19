import type { AppError } from '@factorai/types';
import { useEffect, useRef } from 'react';
import {
	attachStream,
	disposeTerminal,
	fitToHost,
	getOrCreateTerm,
	type PooledTerm,
	showOnly,
} from '@components/terminal/Terminal';
import { isAppError } from '@lib/errors';
import { cmd } from '@lib/tauri';
import { type ServerTab, useServerStore } from '@store/serverStore';

/** Answers to host-key prompts, keyed by tab: `true` trusts the fingerprint. */
const hostKeyAnswers = new Map<string, (trust: boolean) => void>();

/** Called by the host-key dialog with the human's decision. */
export function answerHostKey(tabKey: string, trust: boolean): void {
	hostKeyAnswers.get(tabKey)?.(trust);
	hostKeyAnswers.delete(tabKey);
	useServerStore.getState().setPendingHostKey(null);
}

function isHostKey(e: unknown): e is AppError & { kind: 'HostKey' } {
	return isAppError(e) && e.kind === 'HostKey';
}

/**
 * The tab's PTY: the live one if a reload left it running, else a new login.
 * An unknown host key waits for the human; nothing is trusted on their behalf.
 */
async function ensureShell(
	tabKey: string,
	serverId: string,
	cols: number,
	rows: number,
): Promise<string> {
	const live = useServerStore.getState().tabs.find((t) => t.key === tabKey)?.terminalId;
	if (live) return live;
	let accept: string | undefined;
	for (;;) {
		try {
			const id = await cmd.serverOpenShell(serverId, tabKey, cols, rows, accept);
			useServerStore.getState().attach(tabKey, id);
			// A tab that already has its agent (an agent's own session, or one
			// restored after a restart) lends the new PTY to it straight away.
			const agent = useServerStore.getState().tabs.find((t) => t.key === tabKey)?.agent;
			if (agent) await cmd.agentLinkTerminal(id, agent.sessionId, agent.mode);
			return id;
		} catch (e) {
			if (!isHostKey(e) || accept) throw e;
			const fingerprint = e.message;
			const trust = await new Promise<boolean>((resolve) => {
				hostKeyAnswers.set(tabKey, resolve);
				useServerStore.getState().setPendingHostKey({ tabKey, serverId, fingerprint });
			});
			if (!trust) throw new Error('host key not trusted, nothing was sent to the server');
			accept = fingerprint;
		}
	}
}

/**
 * The active SSH session's terminal, filling the work card (F44).
 *
 * The same pooled xterm and byte stream as every other terminal: switching
 * tabs only shows another pooled host, so a session's scrollback survives it,
 * and a dead tab's terminal is thrown away and logs in again when revived.
 */
export function ServerTerminal({ tab }: { tab: ServerTab }) {
	const containerRef = useRef<HTMLDivElement>(null);
	const entryRef = useRef<PooledTerm | null>(null);
	const { key, dead, serverId } = tab;

	useEffect(() => {
		const container = containerRef.current;
		if (!container || dead) return;
		const entry = getOrCreateTerm(
			key,
			container,
			() => useServerStore.getState().tabs.find((t) => t.key === key)?.terminalId ?? undefined,
		);
		entryRef.current = entry;
		if (entry.host.parentElement !== container) container.appendChild(entry.host);
		showOnly(container, entry);
		fitToHost(entry);
		requestAnimationFrame(() => fitToHost(entry));
		attachStream(
			entry,
			(cols, rows) => ensureShell(key, serverId, cols, rows),
			'Could not open the SSH session',
			() => {
				const id = useServerStore.getState().tabs.find((t) => t.key === key)?.terminalId;
				if (id) useServerStore.getState().markDead(id);
			},
		);
		entry.term.focus();
		const ro = new ResizeObserver(() => fitToHost(entry));
		ro.observe(container);
		return () => ro.disconnect();
		// Keyed on primitives: the tab object changes on every attach.
	}, [key, dead, serverId]);

	return (
		<div className="relative h-full w-full bg-terminal p-2">
			<div ref={containerRef} className="relative h-full w-full" data-testid="server-terminal" />
		</div>
	);
}

/** Throw a dead tab's terminal away so the next render logs in again. */
export function reconnectTab(key: string): void {
	disposeTerminal(key);
	useServerStore.getState().revive(key);
}
