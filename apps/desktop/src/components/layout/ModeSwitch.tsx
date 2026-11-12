import { useActiveProject } from '@hooks/useActiveProject';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { type AppMode, useGitModeStore } from '@store/gitModeStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQueries } from '@tanstack/react-query';

/** Conflicts appear when a merge or rebase stops, not per keystroke. */
const REPO_STATE_POLL_MS = 10_000;

/**
 * The `Sessions | Git` switch (specs/05-features.md F35). Each half carries the
 * other mode's news: sessions working, files in conflict.
 */
export function ModeSwitch() {
	const mode = useGitModeStore((s) => s.mode);
	const setMode = useGitModeStore((s) => s.setMode);
	const tabs = useGitModeStore((s) => s.tabs);
	const openTab = useGitModeStore((s) => s.openTab);
	const { project, root } = useActiveProject();

	const working = useTerminalStore(
		(s) => Object.values(s.bySession).filter((t) => t.status === 'working').length,
	);
	const conflicts = useQueries({
		queries: tabs.map((t) => ({
			queryKey: queryKeys.gitRepoState(t.dir),
			queryFn: () => cmd.gitRepoState(t.dir),
			refetchInterval: REPO_STATE_POLL_MS,
		})),
		combine: (results) => results.reduce((n, r) => n + (r.data?.conflicted.length ?? 0), 0),
	});

	const pick = (next: AppMode) => {
		// The first visit has nothing to show, so it opens the project in view.
		if (next === 'git' && tabs.length === 0 && project && root) openTab(project.id, root);
		setMode(next);
	};

	const half = (value: AppMode, label: string, count: number) => (
		<button
			type="button"
			role="tab"
			aria-selected={mode === value}
			data-testid={`mode-${value}`}
			className={`flex h-6 items-center gap-1.5 rounded px-2.5 text-sm ${
				mode === value
					? 'bg-secondary text-foreground'
					: 'text-muted-foreground hover:bg-secondary/50 hover:text-foreground'
			}`}
			onClick={() => pick(value)}
		>
			{label}
			{count > 0 && (
				<span data-testid={`mode-${value}-count`} className="tabular-nums text-xs opacity-80">
					{count}
				</span>
			)}
		</button>
	);

	return (
		<div role="tablist" aria-label="Mode" className="flex shrink-0 items-center gap-0.5">
			{half('sessions', 'Sessions', working)}
			{half('git', 'Git', conflicts)}
		</div>
	);
}
