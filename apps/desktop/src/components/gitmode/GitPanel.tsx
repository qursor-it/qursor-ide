import { PanelEmpty } from '@components/layout/PanelEmpty';
import { Button } from '@factorai/ui';
import { useActiveCheckout } from '@hooks/useActiveCheckout';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import { ExternalLink, GitBranch } from 'lucide-react';
import { useState } from 'react';
import { GitActionBar } from './GitActionBar';
import { GitHistory } from './GitHistory';
import type { GitSection } from '@store/gitModeStore';
import { LocalChanges } from './LocalChanges';

/**
 * The right panel's Graph tab (F18, F35): Git mode for the session's checkout,
 * in the panel — the action bar, the history table with the commit docked under
 * it, and Local changes with the commit box — so git's quick jobs need no mode
 * switch. Widen the panel by its edge and the columns come back.
 */
export function GitPanel() {
	const { projectId, root } = useActiveCheckout();
	const openTab = useGitModeStore((s) => s.openTab);
	const setMode = useGitModeStore((s) => s.setMode);
	const [section, setSection] = useState<GitSection>('history');
	const statusQ = useQuery({
		queryKey: queryKeys.gitStatus(root ?? ''),
		queryFn: () => cmd.gitStatus(root ?? ''),
		enabled: Boolean(root),
		refetchOnMount: 'always',
	});
	const refsQ = useQuery({
		queryKey: queryKeys.gitRefs(root ?? ''),
		queryFn: () => cmd.gitRefs(root ?? ''),
		enabled: Boolean(root),
	});
	if (!root || !projectId) return <PanelEmpty>Select a project to see its history.</PanelEmpty>;
	if (statusQ.data && !statusQ.data.repoRoot) return <PanelEmpty>Not a git repository.</PanelEmpty>;
	const current = refsQ.data?.local.find((b) => b.current);
	const changes = statusQ.data?.changes.length ?? 0;

	const pick = (value: GitSection, label: string, count?: number) => (
		<Button
			variant="ghost"
			size="sm"
			aria-pressed={section === value}
			data-testid={`git-panel-${value}`}
			onClick={() => setSection(value)}
			className={`h-7 gap-1.5 px-2 ${
				section === value
					? 'bg-background text-foreground shadow-sm ring-1 ring-border hover:bg-background'
					: 'text-muted-foreground hover:bg-secondary hover:text-foreground'
			}`}
		>
			{label}
			{count !== undefined && <span className="text-muted-foreground tabular-nums">{count}</span>}
		</Button>
	);

	return (
		// Keyed on the checkout so selection and paging belong to one repository.
		<div key={root} data-testid="git-panel" className="flex min-h-0 min-w-0 flex-1 flex-col">
			<div className="flex shrink-0 items-center gap-1.5 border-border border-b px-3 py-1.5 text-sm">
				<GitBranch className="size-4 shrink-0 text-muted-foreground" aria-hidden />
				<span className="min-w-0 truncate font-semibold">
					{statusQ.data?.branch ?? refsQ.data?.head ?? 'detached'}
				</span>
				{current && (current.ahead > 0 || current.behind > 0) && (
					<span className="shrink-0 text-muted-foreground text-xs tabular-nums">
						{current.ahead > 0 && `↑${current.ahead}`}
						{current.behind > 0 && ` ↓${current.behind}`}
					</span>
				)}
				<span className="flex-1" />
				{pick('history', 'Commits')}
				{pick('changes', 'Changes', changes)}
				<Button
					variant="quiet"
					size="sm"
					className="h-7 px-1.5"
					title="Open in Git mode"
					aria-label="Open in Git mode"
					onClick={() => {
						openTab(projectId, root);
						setMode('git');
					}}
				>
					<ExternalLink />
				</Button>
			</div>
			<div className="border-border border-b">
				<GitActionBar dir={root} compact />
			</div>
			<main className="flex min-h-0 min-w-0 flex-1 flex-col">
				{section === 'history' ? (
					<GitHistory dir={root} projectId={projectId} />
				) : (
					<LocalChanges dir={root} />
				)}
			</main>
		</div>
	);
}
