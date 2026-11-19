import { toast } from '@factorai/ui';
import { useEffect } from 'react';
import { cmd, events } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useServerStore } from '@store/serverStore';

/**
 * Agent activity in SSH terminals (F45), heard wherever the window is: which
 * command an agent is running, and the commands waiting for the human's
 * confirmation. Mounted once, in the app shell.
 */
export function useServerEvents() {
	useEffect(() => {
		const offs: (() => void)[] = [];
		let gone = false;
		const keep = (p: Promise<() => void>) =>
			p.then((off) => {
				if (gone) off();
				else offs.push(off);
			});
		keep(
			events.onAgentTerminalRun((e) =>
				useServerStore.getState().setRunning(e.terminalId, e.running ? e.command : null),
			),
		);
		keep(
			events.onAgentTerminalConfirm((c) => {
				useServerStore.getState().addConfirm(c);
				const s = useServerStore.getState();
				const visible =
					useGitModeStore.getState().mode === 'servers' &&
					s.tabs.find((t) => t.key === s.active)?.terminalId === c.terminalId;
				if (!visible) {
					toast(`An agent asks to run a command on ${c.serverName}`, {
						description: c.command,
						action: {
							label: 'Show',
							onClick: () => {
								const tab = useServerStore
									.getState()
									.tabs.find((t) => t.terminalId === c.terminalId);
								if (tab) useServerStore.getState().activate(tab.key);
								useGitModeStore.getState().setMode('servers');
							},
						},
					});
				}
			}),
		);
		return () => {
			gone = true;
			for (const off of offs) off();
		};
	}, []);
}

/** Answer a confirmation and take it off screen. */
export function answerConfirm(id: string, run: boolean, command?: string) {
	useServerStore.getState().removeConfirm(id);
	cmd.agentTerminalAnswer(id, run, command).catch(() => {});
}
