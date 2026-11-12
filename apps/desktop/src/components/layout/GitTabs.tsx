import { TabGroup } from '@components/layout/TabGroup';
import { DndContext } from '@dnd-kit/core';
import { SortableContext, horizontalListSortingStrategy } from '@dnd-kit/sortable';
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
	IconButton,
} from '@factorai/ui';
import { queryKeys } from '@lib/queryKeys';
import { groupSortId } from '@lib/tabGroups';
import { cmd } from '@lib/tauri';
import { type GitTab, useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import { GitBranch, Plus, X } from 'lucide-react';
import { useState } from 'react';

/**
 * The Git tab strip, row 2 of the top bar in Git mode (specs/05-features.md
 * F35): the session strip's shape, grouped under the same project chips. No
 * reordering — tabs stay in the order they were opened.
 */
export function GitTabs() {
	const tabs = useGitModeStore((s) => s.tabs);
	const activeDir = useGitModeStore((s) => s.activeDir);
	const openTab = useGitModeStore((s) => s.openTab);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const projects = projectsQ.data ?? [];
	const nameOf = (id: string) => projects.find((p) => p.id === id)?.displayName ?? id;
	// Groups in first-seen order, tabs keeping theirs.
	const groups = [...new Set(tabs.map((t) => t.projectId))].map((projectId) => ({
		projectId,
		tabs: tabs.filter((t) => t.projectId === projectId),
	}));
	const [folded, setFolded] = useState<Set<string>>(new Set());
	const toggle = (id: string) =>
		setFolded((c) => {
			const next = new Set(c);
			if (!next.delete(id)) next.add(id);
			return next;
		});

	return (
		<div
			role="tablist"
			aria-label="Git tabs"
			data-testid="git-tabs"
			className="scrollbar-none flex min-w-0 flex-1 items-center gap-2 overflow-x-auto py-0.5"
		>
			{/* `TabGroup` is sortable, so it needs the contexts even with nothing to
			    move. */}
			<DndContext>
				<SortableContext
					items={groups.map((g) => groupSortId(g.projectId))}
					strategy={horizontalListSortingStrategy}
				>
					{groups.map((g) => {
						// The active tab stays drawn when its group is folded, as in the
						// session strip.
						const collapsed = folded.has(g.projectId) && !g.tabs.some((t) => t.dir === activeDir);
						return (
							<TabGroup
								key={g.projectId}
								projectId={g.projectId}
								name={nameOf(g.projectId)}
								collapsed={collapsed}
								count={g.tabs.length}
								onToggle={toggle}
								onNudge={() => {}}
							>
								{!collapsed &&
									g.tabs.map((tab) => (
										<GitTabItem key={tab.dir} tab={tab} active={tab.dir === activeDir} />
									))}
							</TabGroup>
						);
					})}
				</SortableContext>
			</DndContext>
			<DropdownMenu>
				<DropdownMenuTrigger asChild>
					<IconButton size="md" aria-label="Open Git tab" title="Open a project's Git tab">
						<Plus />
					</IconButton>
				</DropdownMenuTrigger>
				<DropdownMenuContent align="start">
					{projects.map((p) => (
						<DropdownMenuItem key={p.id} onSelect={() => openTab(p.id, p.realPath)}>
							{p.displayName}
						</DropdownMenuItem>
					))}
				</DropdownMenuContent>
			</DropdownMenu>
		</div>
	);
}

function GitTabItem({ tab, active }: { tab: GitTab; active: boolean }) {
	const activate = useGitModeStore((s) => s.activate);
	const closeTab = useGitModeStore((s) => s.closeTab);
	const refsQ = useQuery({
		queryKey: queryKeys.gitRefs(tab.dir),
		queryFn: () => cmd.gitRefs(tab.dir),
	});
	const refs = refsQ.data;
	const current = refs?.local.find((b) => b.current);
	const label =
		refs?.head ?? (refs?.headSha ? 'detached' : (tab.dir.split(/[\\/]/).pop() ?? tab.dir));

	return (
		<div
			role="tab"
			aria-selected={active}
			tabIndex={0}
			data-testid="git-tab"
			data-dir={tab.dir}
			title={tab.dir}
			className={`group flex h-7.5 max-w-60 shrink-0 cursor-pointer items-center gap-1.5 rounded px-2 text-sm ${
				active
					? 'bg-secondary text-foreground'
					: 'text-muted-foreground transition-colors hover:bg-secondary/50 hover:text-foreground'
			}`}
			onClick={() => activate(tab.dir)}
			onKeyDown={(e) => {
				if (e.key === 'Enter' || e.key === ' ') {
					e.preventDefault();
					activate(tab.dir);
				}
			}}
		>
			<GitBranch className="size-3.5 shrink-0" aria-hidden />
			<span className="min-w-0 flex-1 truncate">{label}</span>
			{current && (current.ahead > 0 || current.behind > 0) && (
				<span className="shrink-0 text-xs tabular-nums opacity-80">
					{current.ahead > 0 && `↑${current.ahead}`}
					{current.behind > 0 && ` ↓${current.behind}`}
				</span>
			)}
			<button
				type="button"
				aria-label={`Close Git tab ${label}`}
				className={`-mr-1 rounded p-0.5 text-muted-foreground/70 hover:text-primary focus-visible:opacity-100 ${
					active ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
				}`}
				onClick={(e) => {
					e.stopPropagation();
					closeTab(tab.dir);
				}}
			>
				<X className="size-3" />
			</button>
		</div>
	);
}
