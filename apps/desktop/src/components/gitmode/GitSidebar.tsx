import {
	LiveRow,
	LiveSection,
	NeedsYouPill,
	SidebarSectionHeader,
	SidebarToolbar,
} from '@components/layout/SidebarParts';
import {
	Button,
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuSeparator,
	ContextMenuSub,
	ContextMenuSubContent,
	ContextMenuSubTrigger,
	ContextMenuTrigger,
} from '@factorai/ui';
import { useWorktrees } from '@hooks/useWorktrees';
import { type BranchNode, branchTree } from '@lib/branchTree';
import { queryKeys } from '@lib/queryKeys';
import { cmd, copyText } from '@lib/tauri';
import { isOpen, useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { useState } from 'react';
import {
	Archive,
	Check,
	ChevronRight,
	CircleAlert,
	FilePen,
	Folder,
	FolderGit2,
	FolderOpen,
	GitBranch,
	GitCommitVertical,
	GitMerge,
	Globe,
	Tag,
} from 'lucide-react';
import { IntegrateDialog, type IntegrateRequest } from './IntegrateDialog';
import { PromptDialog } from './PromptDialog';
import { useGitOp } from './useGitOp';
import { useQuickAgent } from './useQuickAgent';

import type { GitRepoOp } from '@factorai/types';
import type { GitSection } from '@store/gitModeStore';

interface GitSidebarProps {
	dir: string;
	projectId: string;
	section: GitSection;
	onSection: (section: GitSection) => void;
}

/** Local changes, All commits, then the repository's refs (specs/05-features.md F35). */
export function GitSidebar({ dir, projectId, section, onSection }: GitSidebarProps) {
	const { run } = useGitOp(dir);
	const quick = useQuickAgent(dir);
	const openTab = useGitModeStore((s) => s.openTab);
	const refsQ = useQuery({ queryKey: queryKeys.gitRefs(dir), queryFn: () => cmd.gitRefs(dir) });
	const statusQ = useQuery({
		queryKey: queryKeys.gitStatus(dir),
		queryFn: () => cmd.gitStatus(dir),
		refetchInterval: 5_000,
	});
	const worktrees = useWorktrees(dir);
	const refs = refsQ.data;
	const changes = statusQ.data?.changes.length ?? 0;
	const [renaming, setRenaming] = useState<string | null>(null);
	const [naming, setNaming] = useState<{ kind: 'branch' | 'tag'; at: string } | null>(null);
	const head = refs?.head ?? null;
	const [integrate, setIntegrate] = useState<IntegrateRequest | null>(null);

	// Polled by the rail's conflict badge, which shares the cache entry.
	const repoState = useQuery({
		queryKey: queryKeys.gitRepoState(dir),
		queryFn: () => cmd.gitRepoState(dir),
	}).data;
	const conflicts = repoState?.conflicted.length ?? 0;

	const [term, setTerm] = useState('');
	const q = term.trim().toLowerCase();
	const matches = (name: string) => !q || name.toLowerCase().includes(q);

	const toggled = useGitModeStore((s) => s.toggled);
	const toggle = useGitModeStore((s) => s.toggle);
	// A filter opens everything, so a match inside a closed folder still shows.
	const open = (key: string, byDefault = true) => !!q || isOpen(toggled, key, byDefault);

	const nav = (
		label: ReactNode,
		icon: ReactNode,
		selected: boolean,
		onClick: () => void,
		testid: string,
	) => (
		<Button
			variant="ghost"
			data-testid={testid}
			aria-current={selected}
			className={`${ROW} ${selected ? 'bg-background text-foreground shadow-sm' : 'text-secondary-foreground'}`}
			onClick={onClick}
		>
			{icon}
			{label}
		</Button>
	);

	const branchNodes = (nodes: BranchNode[], depth: number, scope: string): ReactNode =>
		nodes.map((node) =>
			node.kind === 'folder' ? (
				<div key={node.path}>
					<TreeToggle
						depth={depth}
						open={open(`${scope}/${node.path}`)}
						onToggle={() => toggle(`${scope}/${node.path}`)}
						icon={open(`${scope}/${node.path}`) ? <FolderOpen /> : <Folder />}
					>
						{node.name}
					</TreeToggle>
					{open(`${scope}/${node.path}`) && branchNodes(node.children, depth + 1, scope)}
				</div>
			) : scope === 'local' ? (
				localBranch(node.full, node.name, depth)
			) : (
				<div
					key={node.full}
					title={node.full}
					style={{ paddingLeft: indent(depth) }}
					className="flex h-7 items-center gap-1.5 truncate pr-2 text-muted-foreground text-sm [&_svg]:size-3.5 [&_svg]:shrink-0"
				>
					<GitBranch />
					<span className="truncate">{node.name}</span>
				</div>
			),
		);

	const localBranch = (name: string, label: string, depth: number) => {
		const b = refs?.local.find((x) => x.name === name);
		if (!b) return null;
		return (
			<ContextMenu key={name}>
				<ContextMenuTrigger asChild>
					<Button
						variant="ghost"
						data-testid="git-branch"
						data-branch={b.name}
						title={b.upstream ? `${b.name} · tracks ${b.upstream}` : b.name}
						style={{ paddingLeft: indent(depth) }}
						className={`${ROW} text-secondary-foreground`}
						onDoubleClick={() => run({ kind: 'checkout', branch: b.name })}
					>
						{b.current ? <Check className="text-foreground" /> : <GitBranch />}
						<span
							className={`min-w-0 flex-1 truncate text-left ${b.current ? 'font-semibold text-foreground' : ''}`}
						>
							{label}
						</span>
						{(b.ahead > 0 || b.behind > 0) && (
							<span className="shrink-0 text-xs tabular-nums text-muted-foreground">
								{b.ahead > 0 && `↑${b.ahead}`}
								{b.behind > 0 && ` ↓${b.behind}`}
							</span>
						)}
					</Button>
				</ContextMenuTrigger>
				{/* The branch menu, as desktop Git clients have it (F35): checkout,
				    sync with the remote, integrate with the current branch, new refs
				    here, then naming and deleting. */}
				<ContextMenuContent>
					<ContextMenuItem
						disabled={b.current}
						onSelect={() => run({ kind: 'checkout', branch: b.name })}
					>
						Checkout…
					</ContextMenuItem>
					{b.current && (
						<>
							<ContextMenuSeparator />
							{b.upstream && (
								<ContextMenuItem onSelect={() => run({ kind: 'pull' })}>
									Pull from '{b.upstream}'
								</ContextMenuItem>
							)}
							<ContextMenuItem
								onSelect={() => run({ kind: 'push', setUpstream: b.upstream === null })}
							>
								Push to '{b.upstream?.split('/')[0] ?? 'origin'}'
							</ContextMenuItem>
						</>
					)}
					{!b.current && head && (
						<>
							<ContextMenuSeparator />
							<ContextMenuItem onSelect={() => setIntegrate({ kind: 'merge', target: b.name })}>
								Merge '{b.name}' into '{head}'…
							</ContextMenuItem>
							<ContextMenuItem onSelect={() => setIntegrate({ kind: 'rebase', target: b.name })}>
								Rebase '{head}' on '{b.name}'…
							</ContextMenuItem>
						</>
					)}
					<ContextMenuSeparator />
					<ContextMenuItem onSelect={() => setNaming({ kind: 'branch', at: b.name })}>
						New Branch…
					</ContextMenuItem>
					<ContextMenuItem onSelect={() => setNaming({ kind: 'tag', at: b.name })}>
						New Tag…
					</ContextMenuItem>
					<ContextMenuSeparator />
					<ContextMenuItem onSelect={() => setRenaming(b.name)}>Rename…</ContextMenuItem>
					<ContextMenuItem
						disabled={b.current}
						onSelect={() =>
							run(
								{ kind: 'deleteBranch', name: b.name },
								{ confirm: `Delete branch ${b.name}? Commits only on it become unreachable.` },
							)
						}
					>
						Delete…
					</ContextMenuItem>
					{quick.enabled && !b.current && head && (
						<>
							<ContextMenuSeparator />
							<ContextMenuSub>
								<ContextMenuSubTrigger>AI</ContextMenuSubTrigger>
								<ContextMenuSubContent>
									<ContextMenuItem
										disabled={quick.busy}
										onSelect={() =>
											void quick.show(`Summarise ${b.name}`, {
												kind: 'summariseBranch',
												branch: b.name,
												against: head,
											})
										}
									>
										✦ Summarise '{b.name}' against '{head}'
									</ContextMenuItem>
								</ContextMenuSubContent>
							</ContextMenuSub>
						</>
					)}
					<ContextMenuSeparator />
					<ContextMenuItem onSelect={() => void copyText(b.name)}>Copy Branch Name</ContextMenuItem>
				</ContextMenuContent>
			</ContextMenu>
		);
	};

	const group = (key: string, title: string, children: ReactNode) => (
		<section data-testid={`git-section-${key}`} className="mt-2">
			<SidebarSectionHeader
				title={title}
				className="px-1.5"
				open={open(`section/${key}`)}
				onToggle={() => toggle(`section/${key}`)}
			/>
			{open(`section/${key}`) && children}
		</section>
	);

	const localNames = refs?.local.map((b) => b.name).filter(matches) ?? [];
	const remotes = (refs?.remotes ?? [])
		.map((r) => ({ ...r, branches: r.branches.filter(matches) }))
		.filter((r) => !q || r.branches.length > 0);
	const tags = refs?.tags.filter(matches) ?? [];
	const stashes = refs?.stashes.filter((x) => matches(x.message)) ?? [];

	const live = [
		...(conflicts > 0
			? [
					<LiveRow
						key="conflicts"
						testId="live-git-conflicts"
						mark={<CircleAlert className="text-destructive" />}
						label="Conflicts"
						meta={<NeedsYouPill n={conflicts} />}
						onClick={() => onSection('changes')}
					/>,
				]
			: []),
		...(repoState && repoState.op !== 'none'
			? [
					<LiveRow
						key="op"
						testId="live-git-op"
						mark={<GitMerge />}
						label={`${OP_LABEL[repoState.op]} in progress`}
						onClick={() => onSection('changes')}
					/>,
				]
			: []),
	];

	return (
		<nav data-testid="git-sidebar" aria-label="Repository" className="flex min-h-0 flex-1 flex-col">
			<SidebarToolbar
				value={term}
				onChange={setTerm}
				placeholder="Filter branches and tags…"
				action={{
					label: head ? `New branch from ${head}` : 'New branch',
					testId: 'git-new-branch',
					disabled: !head,
					onClick: () => head && setNaming({ kind: 'branch', at: head }),
				}}
			/>
			<LiveSection rows={live} />
			<div className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-1.5 pb-1">
				<SidebarSectionHeader title="Views" className="mt-2 px-1.5" />
				{nav(
					<>
						Local changes
						<span className="ml-auto tabular-nums text-muted-foreground">{changes}</span>
					</>,
					<FilePen />,
					section === 'changes',
					() => onSection('changes'),
					'git-local-changes',
				)}
				{nav(
					'All commits',
					<GitCommitVertical />,
					section === 'history',
					() => onSection('history'),
					'git-all-commits',
				)}

				{refs &&
					(localNames.length > 0 || !q) &&
					group('branches', 'Branches', branchNodes(branchTree(localNames), 1, 'local'))}

				{remotes.length > 0 &&
					group(
						'remotes',
						'Remotes',
						remotes.map((r) => (
							<div key={r.remote}>
								<TreeToggle
									depth={1}
									open={open(`remote/${r.remote}`, false)}
									onToggle={() => toggle(`remote/${r.remote}`)}
									icon={<Globe />}
								>
									{r.remote}
								</TreeToggle>
								{open(`remote/${r.remote}`, false) &&
									branchNodes(
										branchTree(r.branches.map((n) => n.replace(`${r.remote}/`, ''))),
										2,
										`remote:${r.remote}`,
									)}
							</div>
						)),
					)}

				{tags.length > 0 &&
					group(
						'tags',
						'Tags',
						tags.map((t) => (
							<div
								key={t}
								style={{ paddingLeft: indent(1) }}
								className="flex h-7 items-center gap-1.5 truncate pr-2 text-muted-foreground text-sm [&_svg]:size-3.5 [&_svg]:shrink-0"
							>
								<Tag />
								<span className="truncate">{t}</span>
							</div>
						)),
					)}

				{stashes.length > 0 &&
					group(
						'stashes',
						'Stashes',
						stashes.map((s) => (
							<ContextMenu key={s.index}>
								<ContextMenuTrigger asChild>
									<div
										data-testid="git-stash"
										title={s.message}
										style={{ paddingLeft: indent(1) }}
										className="flex h-7 items-center gap-1.5 rounded-md pr-2 text-secondary-foreground text-sm hover:bg-secondary/50 [&_svg]:size-3.5 [&_svg]:shrink-0"
									>
										<Archive />
										<span className="truncate">{s.message}</span>
									</div>
								</ContextMenuTrigger>
								<ContextMenuContent>
									<ContextMenuItem onSelect={() => run({ kind: 'stashApply', index: s.index })}>
										Apply
									</ContextMenuItem>
									<ContextMenuItem onSelect={() => run({ kind: 'stashPop', index: s.index })}>
										Pop
									</ContextMenuItem>
									<ContextMenuItem
										onSelect={() =>
											run(
												{ kind: 'stashDrop', index: s.index },
												{ confirm: `Drop stash "${s.message}"? It cannot be recovered.` },
											)
										}
									>
										Drop…
									</ContextMenuItem>
								</ContextMenuContent>
							</ContextMenu>
						)),
					)}

				{worktrees.length > 1 &&
					group(
						'worktrees',
						'Worktrees',
						worktrees.map((w) => (
							<Button
								key={w.path}
								variant="ghost"
								data-testid="git-worktree"
								title={w.path}
								style={{ paddingLeft: indent(1) }}
								className={`${ROW} text-secondary-foreground`}
								onClick={() => openTab(projectId, w.path)}
							>
								<FolderGit2 />
								<span className="truncate">{w.name ?? w.branch ?? w.path}</span>
							</Button>
						)),
					)}
			</div>

			<IntegrateDialog dir={dir} request={integrate} onClose={() => setIntegrate(null)} />
			<PromptDialog
				title={
					naming?.kind === 'tag' ? `New tag at ${naming.at}` : `New branch from ${naming?.at ?? ''}`
				}
				initial={naming ? '' : null}
				confirmLabel={naming?.kind === 'tag' ? 'Create tag' : 'Create branch'}
				onCancel={() => setNaming(null)}
				onSubmit={(name) => {
					const n = naming;
					setNaming(null);
					if (!n) return;
					void run(
						n.kind === 'tag'
							? { kind: 'createTag', name, at: n.at }
							: { kind: 'createBranch', name, at: n.at, checkout: true },
					);
				}}
			/>
			<PromptDialog
				title="Rename branch"
				initial={renaming}
				confirmLabel="Rename"
				onCancel={() => setRenaming(null)}
				onSubmit={(to) => {
					const from = renaming;
					setRenaming(null);
					if (from && to !== from) void run({ kind: 'renameBranch', from, to });
				}}
			/>
		</nav>
	);
}

const ROW =
	'h-7 w-full justify-start gap-1.5 px-2 font-normal text-sm hover:bg-secondary/60 hover:text-foreground';
const OP_LABEL: Record<Exclude<GitRepoOp, 'none'>, string> = {
	merge: 'Merge',
	rebase: 'Rebase',
	cherryPick: 'Cherry-pick',
	revert: 'Revert',
};
const indent = (depth: number) => `${0.5 + depth * 0.875}rem`;

/** A collapsible row: a branch folder or a remote (F35). */
function TreeToggle({
	depth,
	open,
	onToggle,
	icon,
	children,
}: {
	depth: number;
	open: boolean;
	onToggle: () => void;
	icon?: ReactNode;
	children: ReactNode;
}) {
	return (
		<Button
			variant="ghost"
			aria-expanded={open}
			onClick={onToggle}
			style={{ paddingLeft: indent(depth) }}
			className={`${ROW} text-secondary-foreground`}
		>
			<ChevronRight className={`-ml-1 transition-transform ${open ? 'rotate-90' : ''}`} />
			{icon}
			<span className="truncate">{children}</span>
		</Button>
	);
}
