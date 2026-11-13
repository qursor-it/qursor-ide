import { ProjectIcon } from '@components/layout/ProjectIcon';
import { StatusDot } from '@components/layout/StatusDot';
import { restartSession } from '@components/terminal/Terminal';
import { Button } from '@factorai/ui';
import { useSessionTitles } from '@hooks/useSessionTitles';
import { type InboxEntry, inboxSections } from '@lib/inbox';
import { queryKeys } from '@lib/queryKeys';
import { tabsInKnownProjects } from '@lib/sessionGroups';
import { cmd } from '@lib/tauri';
import { useAttentionStore } from '@store/attentionStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQuery } from '@tanstack/react-query';
import { useNavigate, useParams } from '@tanstack/react-router';
import { useMemo } from 'react';

/**
 * The Inbox (specs/05-features.md F36): the sessions that want you, then the
 * ones working, from every project, above the project tree. It answers the
 * product's first question — which agent needs me next — without opening a
 * project. Renders nothing while nothing is live or waiting.
 */
export function Inbox() {
	const tabs = useTerminalStore((s) => s.tabs);
	const bySession = useTerminalStore((s) => s.bySession);
	const unseen = useAttentionStore((s) => s.unseen);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const { sessionId: activeId } = useParams({ strict: false }) as { sessionId?: string };
	const navigate = useNavigate();

	const { needsYou, working } = useMemo(() => {
		const known = tabsInKnownProjects(tabs, projectsQ.data);
		const live = Object.fromEntries(
			tabsInKnownProjects(
				Object.entries(bySession).map(([sessionId, t]) => ({ ...t, sessionId })),
				projectsQ.data,
			).map((t) => [t.sessionId, t]),
		);
		return inboxSections(known, live, unseen);
	}, [tabs, bySession, unseen, projectsQ.data]);

	const tabIds = useMemo(() => new Set(tabs.map((t) => t.sessionId)), [tabs]);
	const titles = useSessionTitles(useMemo(() => [...needsYou, ...working], [needsYou, working]));
	const projectName = useMemo(
		() => new Map((projectsQ.data ?? []).map((p) => [p.id, p.displayName])),
		[projectsQ.data],
	);

	if (needsYou.length === 0 && working.length === 0) return null;

	const open = (e: InboxEntry) => {
		// A session that stopped while you were away is a restart, as a stopped
		// tab is (F16).
		if (!useTerminalStore.getState().bySession[e.sessionId]) restartSession(e.sessionId);
		void navigate({
			to: '/projects/$projectId/sessions/$sessionId',
			params: { projectId: e.projectId, sessionId: e.sessionId },
		});
	};

	const section = (label: string, entries: InboxEntry[], testId: string) =>
		entries.length > 0 && (
			<section aria-label={label} data-testid={testId}>
				<h2 className="flex items-center gap-1.5 px-3 pt-2 pb-1 font-medium text-muted-foreground text-xs uppercase tracking-wider">
					{label}
					<span className="rounded bg-secondary px-1.5 tabular-nums tracking-normal">
						{entries.length}
					</span>
				</h2>
				<ul className="space-y-0.5 px-1.5">
					{entries.map((e) => {
						const name = projectName.get(e.projectId) ?? '';
						const wants = testId === 'inbox-needs-you';
						return (
							<li key={e.sessionId}>
								<Button
									variant="secondary"
									data-testid={`inbox-row-${e.sessionId}`}
									aria-current={e.sessionId === activeId ? 'page' : undefined}
									onClick={() => open(e)}
									className={`h-auto w-full flex-col items-stretch gap-0.5 rounded-lg px-2.5 py-1.5 text-left font-normal ${
										e.sessionId === activeId
											? 'bg-background shadow-sm hover:bg-background'
											: 'bg-transparent hover:bg-secondary/60'
									} ${wants ? 'shadow-[inset_3px_0_0_var(--primary)]' : ''}`}
								>
									<span className="flex min-w-0 items-center gap-2">
										<StatusDot
											status={e.status}
											background={!tabIds.has(e.sessionId)}
											unseen={unseen[e.sessionId] === true}
										/>
										<span className="truncate text-foreground">
											{titles.get(e.sessionId) ?? 'New session'}
										</span>
									</span>
									<span className="flex min-w-0 items-center gap-1.5 pl-4 text-muted-foreground text-xs">
										<ProjectIcon name={name} size={14} />
										<span className="truncate">{name}</span>
									</span>
								</Button>
							</li>
						);
					})}
				</ul>
			</section>
		);

	return (
		<div data-testid="inbox" className="max-h-[45%] shrink-0 overflow-y-auto pb-1">
			{section('Needs you', needsYou, 'inbox-needs-you')}
			{section('Working', working, 'inbox-working')}
		</div>
	);
}
