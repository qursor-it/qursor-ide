import { ProjectIcon } from '@components/layout/ProjectIcon';
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
	IconButton,
	Input,
} from '@factorai/ui';
import { queryKeys } from '@lib/queryKeys';
import { cmd, pickFolder } from '@lib/tauri';
import { type GitTab, isLooseRepo, looseRepoId, useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import { GitBranch, Plus, X } from 'lucide-react';
import { useState } from 'react';

/**
 * The open repositories, at the top of Git mode's sidebar (specs/05-features.md
 * F35, ADR-0095): one row per Git tab — the project's tile and name, the branch
 * with ahead/behind, a close × — and the + menu that opens another. Until
 * ADR-0095 this was a strip in the top bar's second row.
 */
export function GitTabs() {
	const tabs = useGitModeStore((s) => s.tabs);
	const activeDir = useGitModeStore((s) => s.activeDir);
	const openTab = useGitModeStore((s) => s.openTab);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const projects = projectsQ.data ?? [];
	const nameOf = (id: string) =>
		isLooseRepo(id)
			? (id.split(/[\\/]/).filter(Boolean).pop() ?? id)
			: (projects.find((p) => p.id === id)?.displayName ?? id);
	// Groups in first-seen order, tabs keeping theirs.
	const groups = [...new Set(tabs.map((t) => t.projectId))].map((projectId) => ({
		projectId,
		tabs: tabs.filter((t) => t.projectId === projectId),
	}));
	const [filter, setFilter] = useState('');
	const [menuOpen, setMenuOpen] = useState(false);
	const needle = filter.trim().toLowerCase();
	const shown = projects.filter(
		(p) =>
			!needle ||
			p.displayName.toLowerCase().includes(needle) ||
			p.realPath.toLowerCase().includes(needle),
	);

	return (
		<div
			role="tablist"
			aria-label="Git tabs"
			aria-orientation="vertical"
			data-testid="git-tabs"
			className="flex shrink-0 flex-col gap-0.5"
		>
			<div className="flex items-center px-2.5 pt-1 pb-1 font-medium text-muted-foreground text-xs uppercase tracking-wider">
				<span className="flex-1">Repositories</span>
				<DropdownMenu
					open={menuOpen}
					onOpenChange={(open) => {
						setMenuOpen(open);
						if (!open) setFilter('');
					}}
				>
					<DropdownMenuTrigger asChild>
						<IconButton size="md" aria-label="Open Git tab" title="Open a Git tab">
							<Plus />
						</IconButton>
					</DropdownMenuTrigger>
					<DropdownMenuContent align="start">
						<DropdownMenuItem
							data-testid="git-open-repository"
							onSelect={() =>
								void pickFolder('Open repository').then((dir) => {
									if (dir) openTab(looseRepoId(dir), dir);
								})
							}
						>
							Open repository…
						</DropdownMenuItem>
						{projects.length > 0 && (
							<>
								<DropdownMenuSeparator />
								<div className="px-1 py-1">
									<Input
										autoFocus
										value={filter}
										onChange={(e) => setFilter(e.target.value)}
										// The menu's typeahead would steal every letter; only the keys
										// that move into the list or close it reach it.
										onKeyDown={(e) => {
											if (e.key !== 'ArrowDown' && e.key !== 'Escape') e.stopPropagation();
											if (e.key === 'Enter' && shown[0]) {
												openTab(shown[0].id, shown[0].realPath);
												setMenuOpen(false);
												setFilter('');
											}
										}}
										placeholder="Filter projects…"
										aria-label="Filter projects"
										data-testid="git-open-filter"
										className="h-7 text-sm"
									/>
								</div>
								{shown.length === 0 && (
									<p className="px-2 py-1.5 text-muted-foreground text-xs">No project matches.</p>
								)}
							</>
						)}
						{shown.map((p) => (
							<DropdownMenuItem key={p.id} onSelect={() => openTab(p.id, p.realPath)}>
								{p.displayName}
							</DropdownMenuItem>
						))}
					</DropdownMenuContent>
				</DropdownMenu>
			</div>
			{groups.flatMap((g) =>
				g.tabs.map((tab) => (
					<GitTabItem
						key={tab.dir}
						tab={tab}
						name={nameOf(g.projectId)}
						active={tab.dir === activeDir}
					/>
				)),
			)}
		</div>
	);
}

function GitTabItem({ tab, name, active }: { tab: GitTab; name: string; active: boolean }) {
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
			className={`group flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 ${
				active ? 'bg-background shadow-sm' : 'hover:bg-secondary/60'
			}`}
			onClick={() => activate(tab.dir)}
			onKeyDown={(e) => {
				if (e.key === 'Enter' || e.key === ' ') {
					e.preventDefault();
					activate(tab.dir);
				}
			}}
		>
			<ProjectIcon name={name} size={28} />
			<span className="flex min-w-0 flex-1 flex-col">
				<span data-testid="tab-group-chip" className="truncate font-medium text-foreground text-sm">
					{name}
				</span>
				<span className="flex min-w-0 items-center gap-1 text-muted-foreground text-xs">
					<GitBranch className="size-3 shrink-0" aria-hidden />
					<span className="truncate">{label}</span>
					{current && (current.ahead > 0 || current.behind > 0) && (
						<span className="shrink-0 tabular-nums">
							{current.ahead > 0 && `↑${current.ahead}`}
							{current.behind > 0 && ` ↓${current.behind}`}
						</span>
					)}
				</span>
			</span>
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
