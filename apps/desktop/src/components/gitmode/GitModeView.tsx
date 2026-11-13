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
import { GitConfirmHost } from './OpLog';

/** The full-window view of the selected Git tab (specs/05-features.md F35). */
export function GitModeView() {
	const tabs = useGitModeStore((s) => s.tabs);
	const activeDir = useGitModeStore((s) => s.activeDir);
	const tab = tabs.find((t) => t.dir === activeDir);

	return (
		<div data-testid="git-mode-view" className="flex min-h-0 flex-1 flex-col">
			{tab ? (
				// Keyed on the directory so selection and paging belong to one checkout.
				<GitTabBody key={tab.dir} projectId={tab.projectId} dir={tab.dir} />
			) : (
				<PanelEmpty>Open a Git tab with +.</PanelEmpty>
			)}
			<GitConfirmHost />
			<GitErrorHost />
		</div>
	);
}

function GitTabBody({ projectId, dir }: { projectId: string; dir: string }) {
	const [section, setSection] = useState<GitSection>('history');
	return (
		<>
			<GitActionBar dir={dir} />
			<AgentPopover dir={dir} projectId={projectId} />
			<ConflictBanner dir={dir} projectId={projectId} />
			<div className="flex min-h-0 flex-1">
				<GitSidebar dir={dir} projectId={projectId} section={section} onSection={setSection} />
				<main className="flex min-w-0 flex-1 flex-col">
					{section === 'history' ? <GitHistory dir={dir} /> : <LocalChanges dir={dir} />}
				</main>
			</div>
		</>
	);
}
