import type { CoreEvent, Spawned, TerminalDataEvent } from '@factorai/types';
import { base64ToBytes } from '@lib/base64';
import { mockInvoke, mockListen } from '@lib/tauri';
import type { CoreClient } from './client';

// The core under the browser-only bridge: every method becomes the Tauri
// command it replaced, so fixtures and the calls smoke tests assert on
// (`terminal_write`, `terminal_spawn`, …) keep their names and arguments, and
// `__FACTORAI_EMIT__('terminal:data', { id, bytesB64 })` still reaches xterm.

/** Methods whose params are the command's arguments unchanged. */
const COMMANDS: Record<string, string> = {
	'session.start': 'start_session',
	'terminal.kill': 'terminal_kill',
	'terminal.list': 'terminal_list',
	'shell.killForProject': 'shell_kill_for_project',
	'delegations.list': 'list_delegations',
	'delegations.resolve': 'resolve_delegation',
	'ide.resync': 'ide_resync',
	'ide.mention': 'ide_mention',
	'server.endTab': 'server_end_tab',
	'agentTerminal.link': 'agent_link_terminal',
	'agentTerminal.unlink': 'agent_unlink_terminal',
	'agentTerminal.answer': 'agent_terminal_answer',
	'vault.status': 'vault_status',
	'vault.unlock': 'vault_unlock',
	'vault.lock': 'vault_lock',
	'vault.setMasterPassword': 'vault_set_master_password',
	'server.list': 'list_servers',
	'server.save': 'save_server',
	'server.delete': 'delete_server',
	'server.disconnect': 'server_disconnect',
	'tunnel.list': 'list_tunnels',
	'tunnel.save': 'save_tunnel',
	'tunnel.delete': 'delete_tunnel',
	'tunnel.start': 'tunnel_start',
	'tunnel.stop': 'tunnel_stop',
	'sftp.list': 'sftp_list',
	'sftp.download': 'sftp_download',
	'sftp.upload': 'sftp_upload',
	'sftp.mkdir': 'sftp_mkdir',
	'sftp.rename': 'sftp_rename',
	'sftp.remove': 'sftp_remove',
};

const EVENTS: Record<CoreEvent['event'], string> = {
	terminalStatus: 'terminal:status',
	terminalExit: 'terminal:exit',
	sessionAdopted: 'session:adopted',
	sessionWorktree: 'session:worktree',
	agentSessionStart: 'agent-session:start',
	delegationsChanged: 'delegations:changed',
	ideOpenFile: 'ide:open-file',
	ideStatus: 'ide:status',
	agentTerminalRun: 'agent-terminal:run',
	agentTerminalConfirm: 'agent-terminal:confirm',
	routinesChanged: 'routines:changed',
	routineFire: 'routine:fire',
	attention: 'attention',
};

export function mockCore(): CoreClient {
	const slots = new Map<string, number>();
	const outputs = new Map<string, (bytes: Uint8Array) => void>();
	const decoder = new TextDecoder();
	let listening = false;

	const slotOf = (terminalId: string) => {
		let slot = slots.get(terminalId);
		if (slot === undefined) {
			slot = slots.size + 1;
			slots.set(terminalId, slot);
		}
		return slot;
	};
	const terminalOf = (slot: number) => [...slots].find(([, s]) => s === slot)?.[0];

	const request = async (method: string, params?: unknown): Promise<unknown> => {
		const args = params as Record<string, unknown> | undefined;
		switch (method) {
			// The core takes the options bare; the commands took them as `opts`.
			case 'terminal.spawn': {
				const spawned = await mockInvoke<Omit<Spawned, 'slot'>>('terminal_spawn', { opts: params });
				return { ...spawned, slot: slotOf(spawned.id) };
			}
			case 'shell.spawn': {
				const id = await mockInvoke<string>('shell_spawn', { opts: params });
				return { id, slot: slotOf(id) };
			}
			case 'server.openShell': {
				const id = await mockInvoke<string>('server_open_shell', args);
				return { id, slot: slotOf(id) };
			}
			case 'ide.reportUi':
				return mockInvoke('ide_report_ui', { snapshot: params });
			case 'terminal.attach':
				return { slot: slotOf(String(args?.terminalId)) };
			case 'terminal.detach':
			case 'terminal.visibility':
				return null;
		}
		const command = COMMANDS[method];
		if (!command) throw new Error(`mockCore: unknown method "${method}"`);
		return mockInvoke(command, args);
	};

	return {
		request: <T>(method: string, params?: unknown) => request(method, params) as Promise<T>,
		on(event, cb) {
			const unlisten = mockListen(EVENTS[event], cb);
			return () => void unlisten.then((fn) => fn());
		},
		async attach(terminalId, onOutput) {
			if (!listening) {
				listening = true;
				void mockListen<TerminalDataEvent>('terminal:data', (e) =>
					outputs.get(e.id)?.(base64ToBytes(e.bytesB64)),
				);
			}
			outputs.set(terminalId, onOutput);
			return {
				slot: slotOf(terminalId),
				detach: () => {
					if (outputs.get(terminalId) === onOutput) outputs.delete(terminalId);
				},
			};
		},
		write(slot, bytes) {
			void mockInvoke('terminal_write', { id: terminalOf(slot), data: decoder.decode(bytes) });
		},
		resize(slot, cols, rows) {
			void mockInvoke('terminal_resize', { id: terminalOf(slot), cols, rows });
		},
		setVisible() {},
	};
}
