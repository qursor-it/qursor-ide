import type { AgentId, LinkMode, Server } from '@factorai/types';
import {
	Button,
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuLabel,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from '@factorai/ui';
import { useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { AgentMark } from '@components/layout/AgentMark';
import { Terminal } from '@components/terminal/Terminal';
import { useInstalledAgents } from '@hooks/useInstalledAgents';
import { agentName } from '@lib/agents';
import { formatError } from '@lib/errors';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { type ServerTab, type TabAgent, useServerStore } from '@store/serverStore';
import { toast } from '@factorai/ui';

/**
 * Start a session of `agent` for a server and lend it a terminal (F45): this
 * tab's own shell when `shared`, a new SSH session of the same server when
 * `own`. The session runs in the server's project, so it is also in the
 * Sessions sidebar and resumes like any other.
 */
async function startAgent(
	tab: ServerTab,
	server: Server,
	agent: AgentId,
	mode: LinkMode,
	invalidate: () => void,
): Promise<void> {
	const project = await cmd.serverAgentProject(server.id);
	invalidate();
	const sessionId = await cmd.startSession(project.id, agent, true);
	const a: TabAgent = {
		sessionId,
		projectId: project.id,
		projectCwd: project.realPath,
		agent,
		mode,
	};
	const store = useServerStore.getState();
	if (mode === 'own') {
		store.open(server.id, a);
		return;
	}
	store.setAgent(tab.key, a);
	if (tab.terminalId) await cmd.agentLinkTerminal(tab.terminalId, sessionId, 'shared');
}

/** The "Agent" control in an SSH session's header (F45, mockup 2). */
export function AgentMenu({ tab, server }: { tab: ServerTab; server: Server }) {
	const qc = useQueryClient();
	const installed = useInstalledAgents();
	const [busy, setBusy] = useState(false);
	const invalidate = () => {
		qc.invalidateQueries({ queryKey: queryKeys.projects() });
		qc.invalidateQueries({ queryKey: queryKeys.sidebar() });
	};
	const go = (agent: AgentId, mode: LinkMode) => {
		setBusy(true);
		startAgent(tab, server, agent, mode, invalidate)
			.catch((e) => toast.error(`Could not start ${agentName(agent)}: ${formatError(e)}`))
			.finally(() => setBusy(false));
	};
	const detach = () => {
		if (tab.terminalId) cmd.agentUnlinkTerminal(tab.terminalId).catch(() => {});
		useServerStore.getState().setAgent(tab.key, null);
	};
	return (
		<DropdownMenu>
			<DropdownMenuTrigger asChild>
				<Button size="sm" variant="outline" disabled={busy || tab.dead} data-testid="agent-menu">
					{tab.agent ? <AgentMark agent={tab.agent.agent} /> : null}
					{tab.agent ? agentName(tab.agent.agent) : 'Agent'}
				</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end" className="w-72">
				{tab.agent ? (
					<>
						<DropdownMenuLabel>
							{tab.agent.mode === 'shared' ? 'Uses this terminal with you' : 'Its own SSH session'}
						</DropdownMenuLabel>
						<DropdownMenuItem onSelect={detach} data-testid="agent-detach">
							Take this terminal back
						</DropdownMenuItem>
					</>
				) : installed.length === 0 ? (
					<DropdownMenuLabel>
						No agent CLI found on this computer (Settings › Agents)
					</DropdownMenuLabel>
				) : (
					installed.map((id) => (
						<div key={id}>
							<DropdownMenuLabel>{agentName(id)}</DropdownMenuLabel>
							<DropdownMenuItem
								onSelect={() => go(id, 'shared')}
								data-testid={`agent-${id}-shared`}
							>
								<AgentMark agent={id} /> New session · shares this terminal
							</DropdownMenuItem>
							<DropdownMenuItem onSelect={() => go(id, 'own')} data-testid={`agent-${id}-own`}>
								<AgentMark agent={id} /> New session · its own SSH tab
							</DropdownMenuItem>
							<DropdownMenuSeparator />
						</div>
					))
				)}
			</DropdownMenuContent>
		</DropdownMenu>
	);
}

/** The agent's own terminal, beside the SSH session it was lent (F45). */
export function AgentPane({ agent }: { agent: TabAgent }) {
	return (
		<div
			className="flex min-w-0 flex-col border-border border-l"
			style={{ width: '46%' }}
			data-testid="agent-pane"
		>
			<div className="flex h-8 shrink-0 items-center gap-2 border-border border-b px-3 text-xs text-muted-foreground">
				<AgentMark agent={agent.agent} />
				<span className="font-medium text-foreground">{agentName(agent.agent)}</span>
				<span>{agent.mode === 'shared' ? '· shares your terminal' : '· its own SSH session'}</span>
			</div>
			<div className="relative min-h-0 flex-1 bg-terminal">
				<Terminal
					sessionId={agent.sessionId}
					projectId={agent.projectId}
					projectCwd={agent.projectCwd}
					sessionCwd={null}
					surface="servers"
				/>
			</div>
		</div>
	);
}

/** "Claude is running …" over a lent terminal, with the human's way to stop it. */
export function RunningChip({ tab }: { tab: ServerTab }) {
	const command = useServerStore((s) => (tab.terminalId ? s.running[tab.terminalId] : undefined));
	if (!command || !tab.agent || !tab.terminalId) return null;
	const id = tab.terminalId;
	return (
		<div
			data-testid="agent-running"
			className="absolute right-3 bottom-3 z-10 flex max-w-[70%] items-center gap-2 rounded-lg bg-background px-2.5 py-1.5 text-xs shadow-popover"
		>
			<AgentMark agent={tab.agent.agent} />
			<span>{agentName(tab.agent.agent)} is running</span>
			<code className="truncate font-mono text-muted-foreground">{command}</code>
			<Button
				size="sm"
				variant="outline"
				className="h-6 px-2 text-xs"
				onClick={() => cmd.terminalWrite(id, '\u0003').catch(() => {})}
			>
				Ctrl+C
			</Button>
		</div>
	);
}
