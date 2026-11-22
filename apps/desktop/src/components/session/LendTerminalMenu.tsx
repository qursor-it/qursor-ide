import type { AgentId } from '@factorai/types';
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuLabel,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
	IconButton,
	toast,
} from '@factorai/ui';
import { useQuery } from '@tanstack/react-query';
import { Server as ServerIcon } from 'lucide-react';
import { formatError } from '@lib/errors';
import { lendTerminal, takeBackTerminal } from '@lib/lendTerminal';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { tabLabel, useServerStore } from '@store/serverStore';

/**
 * Lend an open SSH terminal to this running session, or take one back (F46).
 * The terminal stays in the Servers mode; the session reaches it through its
 * terminal tools.
 */
export function LendTerminalMenu({
	sessionId,
	projectId,
	agent,
}: {
	sessionId: string;
	projectId: string;
	agent: AgentId;
}) {
	const tabs = useServerStore((s) => s.tabs);
	const book = useQuery({ queryKey: queryKeys.servers(), queryFn: () => cmd.listServers() });
	const nameOf = (id: string) => book.data?.servers.find((s) => s.id === id)?.name ?? 'Server';
	const label = (t: (typeof tabs)[number]) =>
		`${nameOf(t.serverId)} ${tabLabel(tabs, t, '')}`.trim();
	const lent = tabs.filter((t) => t.agent?.sessionId === sessionId);
	const free = tabs.filter((t) => t.terminalId && !t.dead && !t.agent);
	const lend = (t: (typeof tabs)[number]) =>
		lendTerminal(t, { sessionId, projectId, agent }).catch((e) =>
			toast.error(`Could not lend ${label(t)}: ${formatError(e)}`),
		);
	return (
		<DropdownMenu>
			<DropdownMenuTrigger asChild>
				<IconButton
					data-testid="session-lend-terminal"
					aria-label="Lend a server terminal"
					title="Lend a server terminal"
				>
					<ServerIcon aria-hidden />
				</IconButton>
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end" className="w-72">
				{lent.length > 0 && (
					<>
						<DropdownMenuLabel>Lent to this session</DropdownMenuLabel>
						{lent.map((t) => (
							<DropdownMenuItem
								key={t.key}
								onSelect={() => takeBackTerminal(t)}
								data-testid={`take-back-${t.key}`}
							>
								Take back {label(t)}
							</DropdownMenuItem>
						))}
						<DropdownMenuSeparator />
					</>
				)}
				<DropdownMenuLabel>Share a terminal</DropdownMenuLabel>
				{free.map((t) => (
					<DropdownMenuItem
						key={t.key}
						onSelect={() => void lend(t)}
						data-testid={`lend-tab-${t.key}`}
					>
						{label(t)}
					</DropdownMenuItem>
				))}
				{free.length === 0 && (
					<DropdownMenuItem
						onSelect={() => useGitModeStore.getState().setMode('servers')}
						data-testid="lend-connect-server"
					>
						Connect a server in Servers first
					</DropdownMenuItem>
				)}
			</DropdownMenuContent>
		</DropdownMenu>
	);
}
