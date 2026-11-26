import type { Delegation, DelegationList, TerminalStatus } from '@factorai/types';
import { Button, cn } from '@factorai/ui';
import { ChevronDown, ChevronRight, CornerDownRight } from 'lucide-react';
import { useState } from 'react';
import { AgentMark } from '@components/layout/AgentMark';
import { StatusDot } from '@components/layout/StatusDot';
import { useSessionTitles } from '@hooks/useSessionTitles';
import { agentName } from '@lib/agents';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useTerminalStore } from '@store/terminalStore';
import { Link, useNavigate } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';

const NONE: DelegationList = { active: [], retired: [] };

/** A project's live delegations and retired delegates (F50), refreshed on
 *  `delegations:changed`. */
export function useDelegations(projectId: string): DelegationList {
	const { data } = useQuery({
		queryKey: queryKeys.delegations(projectId),
		queryFn: () => cmd.listDelegations(projectId),
	});
	return data ?? NONE;
}

/** How many live delegates close with this session (F50, ADR-0126). */
export function liveDelegates(list: DelegationList | undefined, sessionId: string): number {
	return (list?.active ?? []).filter(
		(d) => d.parentSessionId === sessionId && d.status !== 'pendingApproval',
	).length;
}

/** The words and dot for a delegation: its own state, or once it runs, its PTY's. */
export function describeDelegation(
	status: Delegation['status'],
	live: TerminalStatus | undefined,
): { label: string; dot: TerminalStatus; tone: string } {
	if (status === 'pendingApproval')
		return { label: 'Awaiting your approval', dot: 'waiting_input', tone: 'text-primary' };
	if (status === 'starting' || live === undefined)
		return { label: 'Starting', dot: 'unknown', tone: 'text-muted-foreground' };
	if (live === 'waiting_input')
		return { label: 'Needs you', dot: 'waiting_input', tone: 'text-primary' };
	if (live === 'working') return { label: 'Working', dot: 'working', tone: 'text-status-working' };
	return { label: 'Running', dot: live, tone: 'text-muted-foreground' };
}

function useDelegationState(d: Delegation) {
	const live = useTerminalStore((s) => s.bySession[d.sessionId]?.status);
	return describeDelegation(d.status, live);
}

/** What a delegate is doing: the title its agent gave the session, else its brief. */
function useDoing(d: Delegation): string {
	const titles = useSessionTitles([{ projectId: d.projectId }]);
	const title = titles.get(d.sessionId);
	return title && title !== d.prompt ? title : d.prompt;
}

/**
 * Under the header of a session that delegated work (F50): one row per live
 * child, saying which agent, where it is and what it is doing. A child that
 * finishes leaves the strip, and the strip goes with the last one.
 */
export function DelegatesStrip({ sessionId, projectId }: { sessionId: string; projectId: string }) {
	const mine = useDelegations(projectId).active.filter((d) => d.parentSessionId === sessionId);
	const [open, setOpen] = useState(true);
	if (mine.length === 0) return null;
	const waiting = mine.filter((d) => d.status === 'pendingApproval').length;
	const running = mine.length - waiting;
	const counts = [
		running > 0 && `${running} running`,
		waiting > 0 && `${waiting} awaiting approval`,
	].filter(Boolean);
	return (
		<section
			data-testid="delegates-strip"
			className="shrink-0 border-b border-border bg-card text-xs"
		>
			<Button
				variant="ghost"
				onClick={() => setOpen((v) => !v)}
				aria-expanded={open}
				className="flex h-8 w-full justify-start gap-2 rounded-none px-4 font-normal text-xs hover:bg-secondary/50"
			>
				{open ? (
					<ChevronDown className="size-3.5 text-muted-foreground" />
				) : (
					<ChevronRight className="size-3.5 text-muted-foreground" />
				)}
				<span className="font-medium text-foreground">
					Delegated to {mine.length} {mine.length === 1 ? 'agent' : 'agents'}
				</span>
				<span className="text-muted-foreground">{counts.join(' · ')}</span>
			</Button>
			{open && (
				<ul className="pb-1">
					{mine.map((d) => (
						<DelegateRow key={d.id} d={d} projectId={projectId} />
					))}
				</ul>
			)}
		</section>
	);
}

function DelegateRow({ d, projectId }: { d: Delegation; projectId: string }) {
	const navigate = useNavigate();
	const state = useDelegationState(d);
	const doing = useDoing(d);
	return (
		<li data-testid={`delegate-${d.id}`} className="flex h-7 items-center gap-2 px-4">
			<AgentMark agent={d.agent} className="size-3.5 text-muted-foreground" />
			<span className="w-20 shrink-0 truncate text-foreground">{agentName(d.agent)}</span>
			<span className={cn('flex w-40 shrink-0 items-center gap-1.5', state.tone)}>
				<StatusDot status={state.dot} className="size-1.5" />
				{state.label}
			</span>
			<span className="min-w-0 flex-1 truncate text-muted-foreground" title={`Brief · ${d.prompt}`}>
				{doing}
			</span>
			{d.status !== 'pendingApproval' && (
				<Button
					variant="outline"
					size="sm"
					className="h-6 shrink-0"
					onClick={() =>
						void navigate({
							to: '/projects/$projectId/sessions/$sessionId',
							params: { projectId, sessionId: d.sessionId },
						})
					}
				>
					Open
				</Button>
			)}
		</li>
	);
}

/** Above a delegated session (F50): who handed it the work, and the way back. */
export function DelegationOriginStrip({
	sessionId,
	projectId,
	parentTitle,
}: {
	sessionId: string;
	projectId: string;
	parentTitle: (parentId: string) => string;
}) {
	const navigate = useNavigate();
	const d = useDelegations(projectId).active.find((x) => x.sessionId === sessionId);
	const parentAgent = useTerminalStore((s) =>
		d ? s.bySession[d.parentSessionId]?.agent : undefined,
	);
	if (!d) return null;
	return (
		<div
			data-testid="delegation-origin-strip"
			className="flex h-9 shrink-0 items-center gap-2 border-b border-border bg-card px-4 text-xs"
		>
			<span className="flex shrink-0 items-center gap-1 rounded-sm bg-secondary px-1.5 py-0.5 text-secondary-foreground">
				<CornerDownRight className="size-3" />
				delegated by {parentAgent ? agentName(parentAgent) : 'a session'}
			</span>
			<span className="min-w-0 flex-1 truncate text-muted-foreground" title={d.prompt}>
				{d.prompt}
			</span>
			<Button
				variant="outline"
				size="sm"
				className="h-6 max-w-64 shrink-0"
				onClick={() =>
					void navigate({
						to: '/projects/$projectId/sessions/$sessionId',
						params: { projectId, sessionId: d.parentSessionId },
					})
				}
			>
				<span className="truncate">Back to {parentTitle(d.parentSessionId)}</span>
			</Button>
		</div>
	);
}

/** A session's live delegates in the sidebar, nested under its row (F50). */
export function SidebarDelegates({
	delegations,
	parentId,
	projectId,
	pad,
}: {
	delegations: Delegation[];
	parentId: string;
	projectId: string;
	pad: string;
}) {
	return delegations
		.filter((d) => d.parentSessionId === parentId)
		.map((d) => <SidebarDelegate key={d.id} d={d} projectId={projectId} pad={pad} />);
}

function SidebarDelegate({ d, projectId, pad }: { d: Delegation; projectId: string; pad: string }) {
	const state = useDelegationState(d);
	const doing = useDoing(d);
	return (
		<li>
			<Link
				to="/projects/$projectId/sessions/$sessionId"
				params={{ projectId, sessionId: d.sessionId }}
				title={`${agentName(d.agent)} · ${state.label} · ${doing}`}
				data-testid={`sidebar-delegate-${d.id}`}
				className={`flex items-center gap-1.5 py-1 pr-2 ${pad} text-muted-foreground text-xs transition-colors hover:bg-secondary/50 hover:text-foreground [&.active]:text-foreground`}
				activeProps={{ className: 'bg-secondary text-foreground' }}
			>
				<CornerDownRight className="ml-2 size-3 shrink-0 text-muted-foreground/60" />
				<AgentMark agent={d.agent} className="size-3" />
				<span className="min-w-0 flex-1 truncate">{doing}</span>
				<StatusDot status={state.dot} className="size-1.5" />
			</Link>
		</li>
	);
}
