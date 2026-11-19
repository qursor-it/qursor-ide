import { useActiveProject } from '@hooks/useActiveProject';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { type AppMode, useGitModeStore } from '@store/gitModeStore';
import { useAttentionStore } from '@store/attentionStore';
import { useServerStore } from '@store/serverStore';
import { useSidebarStore } from '@store/sidebarStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQueries, useQuery } from '@tanstack/react-query';
import { AgentMascot } from '@components/brand/AgentMascot';
import { GitBranch, Server, Shapes } from 'lucide-react';
import { type ReactNode, useMemo } from 'react';

/** Conflicts appear when a merge or rebase stops, not per keystroke. */
const REPO_STATE_POLL_MS = 10_000;

/**
 * The `Sessions | Git` switch (specs/05-features.md F35). Each half carries the
 * other mode's news: sessions working, files in conflict.
 */
export function ModeSwitch() {
	const mode = useGitModeStore((s) => s.mode);
	const collapsed = useSidebarStore((s) => s.collapsed);
	const toggleCollapsed = useSidebarStore((s) => s.toggleCollapsed);
	const setMode = useGitModeStore((s) => s.setMode);
	const tabs = useGitModeStore((s) => s.tabs);
	const openTab = useGitModeStore((s) => s.openTab);
	const ensureTabs = useGitModeStore((s) => s.ensureTabs);
	const { project, root } = useActiveProject();
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const sessionTabs = useTerminalStore((s) => s.tabs);
	const sessionProjectIds = useMemo(
		() => [...new Set(sessionTabs.map((t) => t.projectId))],
		[sessionTabs],
	);

	// Sessions that need you, waiting or unseen: the Sessions icon's badge.
	const unseen = useAttentionStore((s) => s.unseen);
	const needsYou = useTerminalStore(
		(s) =>
			Object.entries(s.bySession).filter(([id, t]) => t.status === 'waiting_input' || unseen[id])
				.length,
	);
	const conflicts = useQueries({
		queries: tabs.map((t) => ({
			queryKey: queryKeys.gitRepoState(t.dir),
			queryFn: () => cmd.gitRepoState(t.dir),
			refetchInterval: REPO_STATE_POLL_MS,
		})),
		combine: (results) => results.reduce((n, r) => n + (r.data?.conflicted.length ?? 0), 0),
	});

	// Agent commands on servers waiting for a yes or no (ADR-0108).
	const confirmsWaiting = useServerStore((s) => s.confirms.length);

	const pick = (next: AppMode) => {
		if (next === 'git') {
			// Git opens on every project you have a session tab in, so the two strips
			// list the same projects (F35). Closed Git tabs come back on the next switch.
			const byId = new Map((projectsQ.data ?? []).map((p) => [p.id, p.realPath]));
			const fromSessions = sessionProjectIds.flatMap((id) => {
				const dir = byId.get(id);
				return dir ? [{ projectId: id, dir }] : [];
			});
			if (tabs.length === 0 && project && root) openTab(project.id, root);
			ensureTabs(fromSessions);
		}
		// The Sessions icon is also the way back to a collapsed project tree.
		else if (mode === 'sessions' && collapsed) toggleCollapsed();
		setMode(next);
	};

	// Icons in the app rail since ADR-0093; the accessible names stay `Sessions`
	// and `Git`, which is what the tab role is for. The badge is the other mode's
	// news: sessions working, files in conflict.
	const half = (value: AppMode, label: string, count: number, icon: ReactNode) => (
		<button
			type="button"
			role="tab"
			aria-selected={mode === value}
			aria-label={label}
			title={label}
			data-testid={`mode-${value}`}
			className={`relative flex size-9 items-center justify-center rounded-lg [&_svg]:size-4.5 ${
				mode === value
					? 'bg-background text-foreground shadow-sm'
					: 'text-muted-foreground hover:bg-secondary/60 hover:text-foreground'
			}`}
			onClick={() => pick(value)}
		>
			{icon}
			{count > 0 && (
				<span
					data-testid={`mode-${value}-count`}
					className="absolute -top-0.5 -right-0.5 min-w-4 rounded-full bg-primary px-1 text-[10px] font-semibold leading-4 text-primary-foreground tabular-nums"
				>
					{count}
				</span>
			)}
		</button>
	);

	return (
		<div
			role="tablist"
			aria-label="Mode"
			aria-orientation="vertical"
			className="flex flex-col items-center gap-1"
		>
			{half('sessions', 'Sessions', needsYou, <AgentMascot />)}
			{half('git', 'Git', conflicts, <GitBranch />)}
			{half('canvas', 'Canvas', 0, <Shapes />)}
			{half('servers', 'Servers', confirmsWaiting, <Server />)}
		</div>
	);
}
