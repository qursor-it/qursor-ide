import type { AgentSessionStartEvent } from '@factorai/types';
import {
	Button,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from '@factorai/ui';
import { disposeTerminal, startAgentSession } from '@components/terminal/Terminal';
import { useSessionTitles } from '@hooks/useSessionTitles';
import { agentName } from '@lib/agents';
import { queryKeys } from '@lib/queryKeys';
import { showErrorNotice } from '@lib/errorNotice';
import { formatError } from '@lib/errors';
import { cmd, events } from '@lib/tauri';
import { useTerminalStore } from '@store/terminalStore';
import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from '@tanstack/react-router';
import { useEffect, useState } from 'react';

/** F50 session starts and delegation approval. */
export function AgentSessionStarts() {
	const queryClient = useQueryClient();
	const router = useRouter();
	const [requests, setRequests] = useState<AgentSessionStartEvent[]>([]);
	const [busy, setBusy] = useState(false);
	const request = requests[0];

	useEffect(() => {
		const listener = events.onAgentSessionStart((event) => {
			if (event.delegationId) {
				if (event.autoApproved) {
					startAgentSession(event.delegationId, event);
					void queryClient.invalidateQueries({ queryKey: queryKeys.sessions(event.projectId) });
				} else {
					setRequests((items) => [...items, event]);
				}
				return;
			}
			void cmd
				.startSession(event.projectId, event.agent, true)
				.then((id) => {
					startAgentSession(id, event);
					void queryClient.invalidateQueries({ queryKey: queryKeys.sessions(event.projectId) });
				})
				.catch((error) =>
					showErrorNotice(`Could not start the agent session: ${formatError(error)}`),
				);
		});
		// A finished delegate is disposable (F50): Rust has stopped its PTY, so its
		// tab closes here, and a view of it moves to the session that delegated.
		const changes = events.onDelegationsChanged(({ projectId }) => {
			void queryClient
				.fetchQuery({
					queryKey: queryKeys.delegations(projectId),
					queryFn: () => cmd.listDelegations(projectId),
					staleTime: 0,
				})
				.then(({ retired }) => {
					const terminals = useTerminalStore.getState();
					const here = router.state.location.pathname;
					for (const { sessionId, parentSessionId } of retired) {
						if (!terminals.tabs.some((t) => t.sessionId === sessionId)) continue;
						terminals.detach(sessionId);
						disposeTerminal(sessionId);
						if (here.endsWith(`/sessions/${sessionId}`)) {
							void router.navigate({
								to: '/projects/$projectId/sessions/$sessionId',
								params: { projectId, sessionId: parentSessionId },
							});
						}
					}
				})
				.catch((error) => console.error('list_delegations failed', error));
		});
		return () => {
			void listener.then((off) => off());
			void changes.then((off) => off());
		};
	}, [queryClient, router]);

	async function decide(approve: boolean) {
		const id = request?.delegationId;
		if (!id || busy) return;
		setBusy(true);
		try {
			const launch = await cmd.resolveDelegation(id, approve);
			if (launch) startAgentSession(id, launch);
		} catch (error) {
			showErrorNotice(`Could not resolve the delegation: ${formatError(error)}`);
		} finally {
			setRequests((items) => items.filter((item) => item.delegationId !== id));
			setBusy(false);
		}
	}

	const titles = useSessionTitles(request ? [{ projectId: request.projectId }] : []);
	const parent = request?.parentSessionId;
	const requestedBy = parent ? (titles.get(parent) ?? parent.slice(0, 8)) : null;

	if (!request) return null;
	return (
		<Dialog
			open
			onOpenChange={(open) => {
				if (!open) void decide(false);
			}}
		>
			<DialogContent>
				<DialogHeader>
					<DialogTitle>Delegate to {agentName(request.agent)}</DialogTitle>
					<DialogDescription className="break-all">
						{requestedBy && (
							<>
								Requested by <span className="text-foreground">{requestedBy}</span>
								{' · '}
							</>
						)}
						{request.cwd}
					</DialogDescription>
				</DialogHeader>
				<div className="max-h-64 overflow-y-auto whitespace-pre-wrap break-words rounded-md bg-muted p-3 text-sm">
					{request.prompt}
				</div>
				<DialogFooter>
					<Button variant="outline" disabled={busy} onClick={() => void decide(false)}>
						Decline
					</Button>
					<Button disabled={busy} onClick={() => void decide(true)}>
						Start session
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
