import { PanelEmpty } from '@components/layout/PanelEmpty';
import { ProjectIcon } from '@components/layout/ProjectIcon';
import { Button } from '@factorai/ui';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useQuery } from '@tanstack/react-query';
import { useGitModeStore } from '@store/gitModeStore';
import { useState } from 'react';
import { AgentPopover } from './AgentPopover';
import { ConflictBanner } from './ConflictBanner';
import { GitActionBar } from './GitActionBar';
import { GitHistory } from './GitHistory';
import { type GitSection, GitSidebar } from './GitSidebar';
import { LocalChanges } from './LocalChanges';

/** The full-window view of the selected Git tab (specs/05-features.md F35). */
export function GitModeView() {
	const tabs = useGitModeStore((s) => s.tabs);
	const activeDir = useGitModeStore((s) => s.activeDir);
	const tab = tabs.find((t) => t.dir === activeDir);

	return (
		<div data-testid="git-mode-view" className="flex min-h-0 min-w-0 flex-1 flex-col">
			{tab ? (
				// Keyed on the directory so selection and paging belong to one checkout.
				<GitTabBody key={tab.dir} projectId={tab.projectId} dir={tab.dir} />
			) : (
				<div className="flex min-h-0 min-w-0 flex-1 pr-2 pb-2">
					<div className="flex min-w-0 flex-1 flex-col rounded-xl border border-border bg-background shadow-card">
						<OpenAProject />
					</div>
				</div>
			)}
		</div>
	);
}

function GitTabBody({ projectId, dir }: { projectId: string; dir: string }) {
	const [section, setSection] = useState<GitSection>('history');
	return (
		// The sidebar on the chrome, everything about the checkout in one card on
		// it, as the session column is (ADR-0090).
		<div className="flex min-h-0 min-w-0 flex-1 pr-2 pb-2">
			<GitSidebar dir={dir} projectId={projectId} section={section} onSection={setSection} />
			<div className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-xl border border-border bg-background shadow-card">
				<GitActionBar dir={dir} />
				<AgentPopover dir={dir} projectId={projectId} />
				<ConflictBanner dir={dir} projectId={projectId} />
				<main className="flex min-h-0 min-w-0 flex-1 flex-col">
					{section === 'history' ? (
						<GitHistory dir={dir} projectId={projectId} />
					) : (
						<LocalChanges dir={dir} />
					)}
				</main>
			</div>
		</div>
	);
}

/** With no Git tab open: the projects, one click each from their history, and
 *  the tab bar's Open project for anything else (F35, ADR-0100). */
function OpenAProject() {
	const openTab = useGitModeStore((s) => s.openTab);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const projects = projectsQ.data ?? [];
	if (projects.length === 0)
		return <PanelEmpty>Open a repository with Open project in the tab bar.</PanelEmpty>;
	return (
		<div data-testid="git-pick-project" className="mx-auto w-full max-w-md p-8">
			<h2 className="mb-1 font-semibold text-base">Open a project&apos;s repository</h2>
			<p className="mb-4 text-muted-foreground text-sm">
				Each opens as a tab above; Open project there adds more, or any folder.
			</p>
			<ul className="flex flex-col gap-1">
				{projects.map((p) => (
					<li key={p.id}>
						<Button
							variant="outline"
							className="w-full justify-start gap-2"
							onClick={() => openTab(p.id, p.realPath)}
						>
							<ProjectIcon name={p.displayName} size={18} />
							<span className="truncate">{p.displayName}</span>
						</Button>
					</li>
				))}
			</ul>
		</div>
	);
}
