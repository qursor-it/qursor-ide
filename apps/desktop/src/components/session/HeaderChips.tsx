import type { PlanItem, TerminalStatus } from '@factorai/types';
import { Button } from '@factorai/ui';
import { Check, Circle, CircleDot } from 'lucide-react';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { usePanelStore } from '@store/panelStore';
import { useQuery } from '@tanstack/react-query';

const PILL: Record<TerminalStatus, { label: string; className: string }> = {
	working: { label: 'Working', className: 'bg-status-working/15 text-status-working' },
	waiting_input: { label: 'Needs you', className: 'bg-primary/15 text-primary' },
	stopped: { label: 'Stopped', className: 'bg-secondary text-muted-foreground' },
	unknown: { label: 'Live', className: 'bg-secondary text-muted-foreground' },
};

/** The session's state in words beside its title (F3). No `title` attribute:
 *  the dot before the title already carries one, and one hover is enough. */
export function StatusPill({ status }: { status: TerminalStatus }) {
	const { label, className } = PILL[status];
	return (
		<span
			data-testid="session-status-pill"
			className={`shrink-0 rounded-full px-2 py-0.5 font-medium text-xs ${className}`}
		>
			{label}
		</span>
	);
}

/**
 * What the agent has changed in this checkout, as a door onto the Changes panel
 * (F3, F13). No poll of its own: it reads every time a session is shown (the
 * header keys it by session) and when the window regains focus, and shares the panel's query, so with the panel open
 * it follows the panel's 3s poll. Absent outside a repository and on a clean tree.
 */
export function ChangesChip({ root }: { root: string | null }) {
	const setOpen = usePanelStore((s) => s.setOpen);
	const setTab = usePanelStore((s) => s.setTab);
	const { data } = useQuery({
		queryKey: queryKeys.gitStatus(root ?? ''),
		queryFn: () => cmd.gitStatus(root ?? ''),
		enabled: Boolean(root),
		refetchOnMount: 'always',
	});
	const changes = data?.repoRoot ? data.changes : [];
	if (changes.length === 0) return null;
	const add = changes.reduce((n, c) => n + (c.additions ?? 0), 0);
	const del = changes.reduce((n, c) => n + (c.deletions ?? 0), 0);
	const files = data?.truncated ? data.total : changes.length;
	return (
		<Button
			variant="secondary"
			size="sm"
			data-testid="session-changes"
			title="Review the changes in this checkout"
			onClick={() => {
				setTab('changes');
				setOpen(true);
			}}
			className="h-6 shrink-0 gap-1.5 rounded-full px-2.5 font-normal text-xs"
		>
			<span>
				{files} {files === 1 ? 'file' : 'files'}
			</span>
			{add > 0 && <span className="text-git-added">+{add}</span>}
			{del > 0 && <span className="text-git-deleted">−{del}</span>}
		</Button>
	);
}

/** The agent's plan as a list of steps (F40): a check for done (struck
 *  through), the accent's dot for in progress, an empty circle for to do. */
export function PlanList({ plan }: { plan: PlanItem[] }) {
	return (
		<ol data-testid="session-plan-list" className="space-y-1.5 text-sm">
			{plan.map((p) => (
				<li key={p.content} className="flex items-start gap-2">
					{p.status === 'completed' ? (
						<Check className="mt-0.5 size-3.5 shrink-0 text-status-working" aria-label="Done" />
					) : p.status === 'in_progress' ? (
						<CircleDot className="mt-0.5 size-3.5 shrink-0 text-primary" aria-label="In progress" />
					) : (
						<Circle className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-label="To do" />
					)}
					<span
						className={
							p.status === 'completed'
								? 'text-muted-foreground line-through'
								: p.status === 'in_progress'
									? 'font-medium'
									: undefined
						}
					>
						{p.content}
					</span>
				</li>
			))}
		</ol>
	);
}
