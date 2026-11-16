import type { GitOp } from '@factorai/types';
import {
	Button,
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
} from '@factorai/ui';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useQuery } from '@tanstack/react-query';
import {
	Archive,
	ArrowDownToLine,
	ArrowUpFromLine,
	CloudDownload,
	GitBranchPlus,
	GitMerge,
	GitPullRequestArrow,
	LoaderCircle,
} from 'lucide-react';
import { type ReactNode, useState } from 'react';
import { AskAgentField } from './AskAgentField';
import { IntegrateDialog, type IntegrateRequest } from './IntegrateDialog';
import { PromptDialog } from './PromptDialog';
import { useGitOp } from './useGitOp';

/** The bar's buttons hover like the rows beside them, not in the accent. */
const BAR = 'hover:bg-secondary hover:text-foreground';

/**
 * Fetch, Pull, Push, Branch, Merge, Rebase, Stash (specs/05-features.md F35).
 * Every button is disabled while a write runs: one in flight per repository.
 */
export function GitActionBar({ dir }: { dir: string }) {
	const { run, running } = useGitOp(dir);
	const refsQ = useQuery({ queryKey: queryKeys.gitRefs(dir), queryFn: () => cmd.gitRefs(dir) });
	const local = refsQ.data?.local ?? [];
	const current = local.find((b) => b.current);
	const others = local.filter((b) => !b.current);
	const [naming, setNaming] = useState(false);
	const [integrate, setIntegrate] = useState<IntegrateRequest | null>(null);
	// Which button's operation is running, so its glyph turns into a spinner
	// while git works (F35). Not behind `motion-safe:`: it is a state, not
	// decoration.
	const [busy, setBusy] = useState<string | null>(null);
	const go = (key: string, op: GitOp) => {
		setBusy(key);
		void run(op).finally(() => setBusy(null));
	};
	const icon = (key: string, glyph: ReactNode) =>
		busy === key ? (
			<LoaderCircle className="animate-spin" data-testid={`git-busy-${key}`} />
		) : (
			glyph
		);

	const branchMenu = (label: string, icon: ReactNode, pick: (name: string) => void) => (
		<DropdownMenu>
			<DropdownMenuTrigger asChild>
				<Button variant="ghost" size="sm" className={BAR} disabled={running || others.length === 0}>
					{icon}
					{label}
				</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent align="start">
				{others.map((b) => (
					<DropdownMenuItem key={b.name} onSelect={() => pick(b.name)}>
						{b.name}
					</DropdownMenuItem>
				))}
			</DropdownMenuContent>
		</DropdownMenu>
	);

	return (
		<div
			data-testid="git-action-bar"
			className="flex h-12 shrink-0 items-center gap-1 border-b border-border px-3"
		>
			<Button
				variant="ghost"
				size="sm"
				className={BAR}
				disabled={running}
				onClick={() => go('fetch', { kind: 'fetch' })}
			>
				{icon('fetch', <CloudDownload />)} Fetch
			</Button>
			<Button
				variant="ghost"
				size="sm"
				className={BAR}
				disabled={running}
				onClick={() => go('pull', { kind: 'pull' })}
			>
				{icon('pull', <ArrowDownToLine />)} Pull
			</Button>
			<Button
				variant="outline"
				size="sm"
				disabled={running}
				// A branch with no upstream publishes itself on the first push.
				onClick={() =>
					go('push', { kind: 'push', setUpstream: current ? current.upstream === null : false })
				}
			>
				{icon('push', <ArrowUpFromLine />)} Push
				{current && current.ahead > 0 && (
					<span
						data-testid="push-ahead"
						className="rounded-full bg-primary/15 px-1.5 text-primary text-xs tabular-nums"
					>
						↑{current.ahead}
					</span>
				)}
			</Button>
			<span className="mx-1 h-5 w-px bg-border" aria-hidden />
			<Button
				variant="ghost"
				size="sm"
				className={BAR}
				disabled={running}
				onClick={() => setNaming(true)}
			>
				{icon('branch', <GitBranchPlus />)} Branch
			</Button>
			{branchMenu('Merge…', <GitMerge />, (target) => setIntegrate({ kind: 'merge', target }))}
			{branchMenu('Rebase…', <GitPullRequestArrow />, (target) =>
				setIntegrate({ kind: 'rebase', target }),
			)}
			<Button
				variant="ghost"
				className={BAR}
				size="sm"
				disabled={running}
				onClick={() => go('stash', { kind: 'stashPush', message: null })}
			>
				{icon('stash', <Archive />)} Stash
			</Button>
			<AskAgentField dir={dir} />
			<IntegrateDialog dir={dir} request={integrate} onClose={() => setIntegrate(null)} />
			<PromptDialog
				title="New branch from HEAD"
				initial={naming ? '' : null}
				confirmLabel="Create branch"
				onCancel={() => setNaming(false)}
				onSubmit={(name) => {
					setNaming(false);
					go('branch', { kind: 'createBranch', name, at: 'HEAD', checkout: true });
				}}
			/>
		</div>
	);
}
