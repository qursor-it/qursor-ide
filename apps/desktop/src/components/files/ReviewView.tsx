import { ChangeRow } from '@components/files/ChangesView';
import { PlanList, StatusPill } from '@components/session/HeaderChips';
import { Button } from '@factorai/ui';
import { useActiveProject } from '@hooks/useActiveProject';
import { useGitStatus } from '@hooks/useGitStatus';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { formatCost } from '@lib/usage';
import { usePanelStore } from '@store/panelStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQuery } from '@tanstack/react-query';
import { useNavigate, useParams } from '@tanstack/react-router';
import type { ReactNode } from 'react';

function Section({
	title,
	aside,
	children,
}: {
	title: string;
	aside?: ReactNode;
	children: ReactNode;
}) {
	return (
		<section className="border-border border-b px-3 py-3 last:border-b-0">
			<h3 className="mb-2 flex items-center font-medium text-muted-foreground text-xs uppercase tracking-wider">
				<span className="flex-1">{title}</span>
				{aside}
			</h3>
			{children}
		</section>
	);
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
	return (
		<div className="flex items-baseline justify-between gap-3 py-0.5 text-sm">
			<span className="text-muted-foreground">{label}</span>
			<span className="min-w-0 truncate text-right">{children}</span>
		</div>
	);
}

/**
 * The panel's Review tab (F42, ADR-0094): what you need to check an agent's
 * work without leaving its session — the session's state, branch, turns and
 * cost, the files changed in its checkout with their counts, and the agent's own
 * plan. The first tab, and the one a new install opens on.
 */
export function ReviewView() {
	const { sessionId } = useParams({ strict: false }) as { sessionId?: string };
	const { projectId } = useActiveProject();
	const live = useTerminalStore((s) => (sessionId ? s.bySession[sessionId] : undefined));
	const setTab = usePanelStore((s) => s.setTab);
	const navigate = useNavigate();
	const { status: git } = useGitStatus();
	const sessionsQ = useQuery({
		queryKey: queryKeys.sessions(projectId ?? ''),
		queryFn: () => cmd.listSessions(projectId ?? ''),
		enabled: Boolean(projectId && sessionId),
	});
	const usageQ = useQuery({
		queryKey: queryKeys.sessionUsage(sessionId ?? ''),
		queryFn: () => cmd.sessionUsage(sessionId ?? ''),
		enabled: Boolean(sessionId),
	});
	const planQ = useQuery({
		queryKey: queryKeys.sessionPlan(sessionId ?? '', live?.status ?? 'stopped'),
		queryFn: () => cmd.sessionPlan(sessionId ?? ''),
		enabled: Boolean(sessionId),
	});
	const session = sessionsQ.data?.find((s) => s.id === sessionId);
	const changes = git?.repoRoot ? git.changes : null;
	const add = (changes ?? []).reduce((n, c) => n + (c.additions ?? 0), 0);
	const del = (changes ?? []).reduce((n, c) => n + (c.deletions ?? 0), 0);
	const plan = planQ.data ?? [];

	return (
		<div data-testid="review-view" className="min-h-0 flex-1 overflow-y-auto">
			{sessionId && (
				<Section title="This session">
					<Fact label="State">
						<StatusPill status={live?.status ?? 'stopped'} />
					</Fact>
					{git?.branch && (
						<Fact label="Branch">
							<span className="font-mono text-xs">{git.branch}</span>
						</Fact>
					)}
					{session && <Fact label="Turns">{session.turnCount}</Fact>}
					{usageQ.data?.costUsd != null && (
						<Fact label="Cost so far">
							<span data-testid="session-cost" className="tabular-nums">
								{formatCost(usageQ.data.costUsd)}
							</span>
						</Fact>
					)}
				</Section>
			)}

			<Section
				title="Changed files"
				aside={
					changes && changes.length > 0 ? (
						<span className="font-normal normal-case tracking-normal">
							<span className="text-git-added">+{add}</span>{' '}
							<span className="text-git-deleted">−{del}</span>
						</span>
					) : null
				}
			>
				{changes === null ? (
					<p className="text-muted-foreground text-sm">Not a repository.</p>
				) : changes.length === 0 ? (
					<p className="text-muted-foreground text-sm">Nothing changed in this checkout.</p>
				) : (
					<>
						<ul data-testid="review-files" className="-mx-1">
							{changes.slice(0, 8).map((c) => (
								<ChangeRow key={`${c.group}:${c.path}`} change={c} />
							))}
						</ul>
						<Button
							size="sm"
							className="mt-2 w-full"
							data-testid="review-all-changes"
							// On a session the whole-screen review (F43); elsewhere the
							// Changes tab, which lists every file.
							onClick={() =>
								sessionId && projectId
									? void navigate({
											to: '/projects/$projectId/sessions/$sessionId/review',
											params: { projectId, sessionId },
										})
									: setTab('changes')
							}
						>
							Review all changes
						</Button>
					</>
				)}
			</Section>

			{sessionId && plan.length > 0 && (
				<Section
					title="Plan"
					aside={
						<span className="font-normal normal-case tracking-normal">
							{plan.filter((p) => p.status === 'completed').length}/{plan.length}
						</span>
					}
				>
					<PlanList plan={plan} />
				</Section>
			)}
		</div>
	);
}
