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
import { startAgentSession } from '@components/terminal/Terminal';
import { agentName } from '@lib/agents';
import { queryKeys } from '@lib/queryKeys';
import { showErrorNotice } from '@lib/errorNotice';
import { formatError } from '@lib/errors';
import { cmd, events } from '@lib/tauri';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';

/** F50 session starts and delegation approval. */
export function AgentSessionStarts() {
	const queryClient = useQueryClient();
	const [requests, setRequests] = useState<AgentSessionStartEvent[]>([]);
	const [busy, setBusy] = useState(false);
	const request = requests[0];

	useEffect(() => {
		const listener = events.onAgentSessionStart((event) => {
			if (event.delegationId) {
				if (event.autoApproved) {
					startAgentSession(event.delegationId, event);
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
		return () => {
			void listener.then((off) => off());
		};
	}, [queryClient]);

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
					<DialogDescription className="break-all">{request.cwd}</DialogDescription>
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
