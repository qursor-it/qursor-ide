import {
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuSeparator,
	ContextMenuTrigger,
} from '@factorai/ui';
import { useWorktrees } from '@hooks/useWorktrees';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { useState } from 'react';
import { IntegrateDialog, type IntegrateRequest } from './IntegrateDialog';
import { PromptDialog } from './PromptDialog';
import { useGitOp } from './useGitOp';

export type GitSection = 'history' | 'changes';

interface GitSidebarProps {
	dir: string;
	projectId: string;
	section: GitSection;
	onSection: (section: GitSection) => void;
}

/** Local changes, All commits, then the repository's refs (specs/05-features.md F35). */
export function GitSidebar({ dir, projectId, section, onSection }: GitSidebarProps) {
	const { run } = useGitOp(dir);
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
	const [integrate, setIntegrate] = useState<IntegrateRequest | null>(null);

	const row = (label: ReactNode, selected: boolean, onClick: () => void, testid: string) => (
		<button
			type="button"
			data-testid={testid}
			aria-current={selected}
			className={`flex w-full items-center gap-1.5 px-3 py-1 text-left text-sm ${
				selected
					? 'bg-secondary text-foreground'
					: 'text-secondary-foreground hover:bg-secondary/50'
			}`}
			onClick={onClick}
		>
			{label}
		</button>
	);

	return (
		<nav
			data-testid="git-sidebar"
			aria-label="Repository"
			className="flex w-60 shrink-0 flex-col overflow-y-auto border-r border-border py-1"
		>
			{row(
				<>
					Local changes
					<span className="ml-auto tabular-nums text-muted-foreground">{changes}</span>
				</>,
				section === 'changes',
				() => onSection('changes'),
				'git-local-changes',
			)}
			{row('All commits', section === 'history', () => onSection('history'), 'git-all-commits')}

			<Heading>Branches</Heading>
			{refs?.local.map((b) => (
				<ContextMenu key={b.name}>
					<ContextMenuTrigger asChild>
						<button
							type="button"
							data-testid="git-branch"
							data-branch={b.name}
							title={b.upstream ? `${b.name} · tracks ${b.upstream}` : b.name}
							className="flex w-full items-center gap-1.5 px-3 py-1 text-left text-sm text-secondary-foreground hover:bg-secondary/50"
							onDoubleClick={() => run({ kind: 'checkout', branch: b.name })}
						>
							<span
								className={`min-w-0 flex-1 truncate ${b.current ? 'font-medium text-foreground' : ''}`}
							>
								{b.current ? '● ' : ''}
								{b.name}
							</span>
							{(b.ahead > 0 || b.behind > 0) && (
								<span className="shrink-0 text-xs tabular-nums text-muted-foreground">
									{b.ahead > 0 && `↑${b.ahead}`}
									{b.behind > 0 && ` ↓${b.behind}`}
								</span>
							)}
						</button>
					</ContextMenuTrigger>
					<ContextMenuContent>
						<ContextMenuItem onSelect={() => run({ kind: 'checkout', branch: b.name })}>
							Checkout
						</ContextMenuItem>
						<ContextMenuItem
							disabled={b.current}
							onSelect={() => setIntegrate({ kind: 'merge', target: b.name })}
						>
							Merge into current
						</ContextMenuItem>
						<ContextMenuItem
							disabled={b.current}
							onSelect={() => setIntegrate({ kind: 'rebase', target: b.name })}
						>
							Rebase current onto it
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
					</ContextMenuContent>
				</ContextMenu>
			))}

			{refs?.remotes.map((r) => (
				<div key={r.remote}>
					<Heading>{r.remote}</Heading>
					{r.branches.map((name) => (
						<div key={name} className="truncate px-3 py-1 text-sm text-muted-foreground">
							{name}
						</div>
					))}
				</div>
			))}

			{refs && refs.tags.length > 0 && (
				<>
					<Heading>Tags</Heading>
					{refs.tags.map((t) => (
						<div key={t} className="truncate px-3 py-1 text-sm text-muted-foreground">
							{t}
						</div>
					))}
				</>
			)}

			{refs && refs.stashes.length > 0 && (
				<>
					<Heading>Stashes</Heading>
					{refs.stashes.map((s) => (
						<ContextMenu key={s.index}>
							<ContextMenuTrigger asChild>
								<div
									data-testid="git-stash"
									className="truncate px-3 py-1 text-sm text-secondary-foreground hover:bg-secondary/50"
								>
									{s.message}
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
					))}
				</>
			)}

			{worktrees.length > 1 && (
				<>
					<Heading>Worktrees</Heading>
					{worktrees.map((w) => (
						<button
							key={w.path}
							type="button"
							data-testid="git-worktree"
							title={w.path}
							className="truncate px-3 py-1 text-left text-sm text-secondary-foreground hover:bg-secondary/50"
							onClick={() => openTab(projectId, w.path)}
						>
							{w.name ?? w.branch ?? w.path}
						</button>
					))}
				</>
			)}

			<IntegrateDialog dir={dir} request={integrate} onClose={() => setIntegrate(null)} />
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

function Heading({ children }: { children: ReactNode }) {
	return (
		<div className="px-3 pt-3 pb-1 text-xs font-medium uppercase text-muted-foreground">
			{children}
		</div>
	);
}
