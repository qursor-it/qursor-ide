import { PanelEmpty } from '@components/layout/PanelEmpty';
import { ProjectIcon } from '@components/layout/ProjectIcon';
import { StatusDot } from '@components/layout/StatusDot';
import { Button } from '@factorai/ui';
import { useOpenSession } from '@hooks/useOpenSession';
import { useSessionTitles } from '@hooks/useSessionTitles';
import { type InboxEntry, inboxSections } from '@lib/inbox';
import { queryKeys } from '@lib/queryKeys';
import { tabsInKnownProjects } from '@lib/sessionGroups';
import { cmd } from '@lib/tauri';
import { useAttentionStore } from '@store/attentionStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQuery } from '@tanstack/react-query';
import { createRoute } from '@tanstack/react-router';
import { useMemo } from 'react';
import { rootRoute } from './__root';

/**
 * The Board (specs/05-features.md F38): every open or live session, in a
 * column per state — needs you, working, stopped — across all projects. The
 * Inbox's question asked of a whole screen, for when several agents run at once.
 */
function BoardView() {
	const tabs = useTerminalStore((s) => s.tabs);
	const bySession = useTerminalStore((s) => s.bySession);
	const unseen = useAttentionStore((s) => s.unseen);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const openSession = useOpenSession();

	const columns = useMemo(() => {
		const live = Object.fromEntries(
			tabsInKnownProjects(
				Object.entries(bySession).map(([sessionId, t]) => ({ ...t, sessionId })),
				projectsQ.data,
			).map((t) => [t.sessionId, t]),
		);
		return inboxSections(tabsInKnownProjects(tabs, projectsQ.data), live, unseen);
	}, [tabs, bySession, unseen, projectsQ.data]);
	const all = useMemo(
		() => [...columns.needsYou, ...columns.working, ...columns.stopped],
		[columns],
	);
	const titles = useSessionTitles(all);
	const nameOf = useMemo(
		() => new Map((projectsQ.data ?? []).map((p) => [p.id, p.displayName])),
		[projectsQ.data],
	);

	const column = (label: string, entries: InboxEntry[], testId: string, tint: string) => (
		<section data-testid={testId} aria-label={label} className="flex min-h-0 flex-col">
			<h2 className="flex items-center gap-2 px-1 pb-2 font-medium text-secondary-foreground text-sm">
				{label}
				<span className={`rounded-full px-2 text-xs tabular-nums ${tint}`}>{entries.length}</span>
			</h2>
			<ul className="flex min-h-0 flex-col gap-2 overflow-y-auto pb-1">
				{entries.map((e) => {
					const name = nameOf.get(e.projectId) ?? '';
					return (
						<li key={e.sessionId}>
							<Button
								variant="secondary"
								data-testid={`board-card-${e.sessionId}`}
								onClick={() => openSession(e.sessionId, e.projectId)}
								className="h-auto w-full flex-col items-stretch gap-1.5 whitespace-normal rounded-xl border border-border bg-card px-3 py-2.5 text-left font-normal shadow-sm hover:bg-card hover:shadow-md"
							>
								<span className="flex min-w-0 items-center gap-1.5 text-muted-foreground text-xs">
									<ProjectIcon name={name} size={14} />
									<span className="truncate">{name}</span>
									<span className="ml-auto">
										<StatusDot status={e.status} unseen={unseen[e.sessionId] === true} />
									</span>
								</span>
								<span className="line-clamp-2 font-medium text-foreground text-sm">
									{titles.get(e.sessionId) ?? 'New session'}
								</span>
							</Button>
						</li>
					);
				})}
			</ul>
		</section>
	);

	return (
		<main className="flex h-full flex-col">
			<header className="flex shrink-0 items-center gap-3 border-border border-b px-6 py-4">
				<h1 className="font-semibold text-base">All sessions</h1>
				<span className="text-muted-foreground text-xs">
					Open and running, across every project
				</span>
			</header>
			{all.length === 0 ? (
				<PanelEmpty>No session is open or running. Start one from a project.</PanelEmpty>
			) : (
				<div className="grid min-h-0 flex-1 grid-cols-3 gap-4 bg-muted/40 px-6 py-4">
					{column('Needs you', columns.needsYou, 'board-needs-you', 'bg-primary/15 text-primary')}
					{column(
						'Working',
						columns.working,
						'board-working',
						'bg-status-working/15 text-status-working',
					)}
					{column(
						'Stopped',
						columns.stopped,
						'board-stopped',
						'bg-secondary text-muted-foreground',
					)}
				</div>
			)}
		</main>
	);
}

export const boardRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: '/board',
	component: BoardView,
});
