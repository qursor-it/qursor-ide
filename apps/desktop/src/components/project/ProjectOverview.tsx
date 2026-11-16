import type { Project } from '@factorai/types';
import { Button } from '@factorai/ui';
import { useFileViewer } from '@hooks/useFileViewer';
import { childPath } from '@lib/paths';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useAttentionStore } from '@store/attentionStore';
import { usePanelStore } from '@store/panelStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQuery } from '@tanstack/react-query';
import { BookOpen } from 'lucide-react';
import type { ReactNode } from 'react';

function Stat({ label, testId, children }: { label: string; testId: string; children: ReactNode }) {
	return (
		<div data-testid={testId} className="min-w-0 rounded-xl bg-secondary/60 px-4 py-3">
			<div className="text-muted-foreground text-xs">{label}</div>
			<div className="mt-1 flex items-baseline gap-1.5 truncate font-semibold text-base">
				{children}
			</div>
		</div>
	);
}

/**
 * The top of a project page (F2, ADR-0090): what is live, what wants you, what
 * is uncommitted, and the rules the agent runs under (F9's `CLAUDE.md`, opened
 * in the viewer where F26 can edit it).
 */
export function ProjectOverview({ project }: { project: Project }) {
	const bySession = useTerminalStore((s) => s.bySession);
	const unseen = useAttentionStore((s) => s.unseen);
	const setTab = usePanelStore((s) => s.setTab);
	const setOpen = usePanelStore((s) => s.setOpen);
	const viewer = useFileViewer();
	const live = Object.entries(bySession).filter(([, t]) => t.projectId === project.id);
	const working = live.filter(([id, t]) => t.status === 'working' && !unseen[id]).length;
	const needsYou = live.filter(([id, t]) => t.status === 'waiting_input' || unseen[id]).length;

	const statusQ = useQuery({
		queryKey: queryKeys.gitStatus(project.realPath),
		queryFn: () => cmd.gitStatus(project.realPath),
		enabled: !project.missing,
	});
	const changes = statusQ.data?.repoRoot ? statusQ.data.changes : null;
	const claudeMd = childPath(project.realPath, 'CLAUDE.md');
	const rulesQ = useQuery({
		queryKey: queryKeys.pathKind(claudeMd),
		queryFn: () => cmd.pathKinds([claudeMd]),
		enabled: !project.missing,
	});
	const hasRules = rulesQ.data?.[0] === 'file';

	return (
		<div className="flex shrink-0 flex-col gap-3 px-6 pb-4">
			<div className="grid grid-cols-3 gap-3">
				<Stat label="Working now" testId="project-stat-working">
					{working}
					<span className="font-normal text-muted-foreground text-xs">
						{working === 1 ? 'session' : 'sessions'}
					</span>
				</Stat>
				<Stat label="Needs you" testId="project-stat-needs-you">
					<span className={needsYou > 0 ? 'text-primary' : undefined}>{needsYou}</span>
					<span className="font-normal text-muted-foreground text-xs">
						{needsYou === 1 ? 'session' : 'sessions'}
					</span>
				</Stat>
				<Stat label="Uncommitted" testId="project-stat-uncommitted">
					{changes === null ? (
						<span className="font-normal text-muted-foreground text-sm">Not a repository</span>
					) : (
						<>
							{changes.length}
							<span className="font-normal text-muted-foreground text-xs">
								{changes.length === 1 ? 'file' : 'files'}
							</span>
							{changes.length > 0 && (
								<Button
									variant="link"
									size="sm"
									className="ml-auto h-auto p-0 font-normal text-xs"
									onClick={() => {
										setTab('changes');
										setOpen(true);
									}}
								>
									Review
								</Button>
							)}
						</>
					)}
				</Stat>
			</div>
			<div
				data-testid="project-rules"
				className="flex items-center gap-3 rounded-xl border border-border border-dashed px-4 py-3"
			>
				<BookOpen className="size-4 shrink-0 text-primary" aria-hidden />
				<div className="min-w-0 flex-1">
					<div className="font-medium text-sm">Rules the agent runs under</div>
					<div className="truncate text-muted-foreground text-xs">
						{hasRules
							? 'CLAUDE.md in the project folder'
							: 'No CLAUDE.md in the project folder yet — the agent runs on its defaults'}
					</div>
				</div>
				{hasRules && (
					<Button size="sm" variant="outline" onClick={() => viewer.open(claudeMd)}>
						Open CLAUDE.md
					</Button>
				)}
			</div>
		</div>
	);
}
