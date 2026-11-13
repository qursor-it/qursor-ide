import { GitTabs } from '@components/layout/GitTabs';
import { PanelEmpty } from '@components/layout/PanelEmpty';
import { useGitModeStore } from '@store/gitModeStore';
import { useState } from 'react';
import { AgentPopover } from './AgentPopover';
import { ConflictBanner } from './ConflictBanner';
import { GitActionBar } from './GitActionBar';
import { GitErrorHost } from './GitErrorDialog';
import { GitHistory } from './GitHistory';
import { type GitSection, GitSidebar } from './GitSidebar';
import { LocalChanges } from './LocalChanges';
import { GitConfirmHost, OpLog } from './OpLog';

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
					<div className="flex w-60 shrink-0 flex-col px-1.5 py-1">
						<GitTabs />
					</div>
					<div className="flex min-w-0 flex-1 flex-col rounded-xl border border-border bg-background shadow-card">
						<PanelEmpty>Open a repository with + beside Repositories.</PanelEmpty>
					</div>
				</div>
			)}
			<GitConfirmHost />
			<GitErrorHost />
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
				<OpLog dir={dir} />
			</div>
		</div>
	);
}
