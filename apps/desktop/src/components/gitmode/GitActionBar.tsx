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
import { useState } from 'react';
import { AskAgentField } from './AskAgentField';
import { IntegrateDialog, type IntegrateRequest } from './IntegrateDialog';
import { PromptDialog } from './PromptDialog';
import { useGitOp } from './useGitOp';

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

	const branchMenu = (label: string, pick: (name: string) => void) => (
		<DropdownMenu>
			<DropdownMenuTrigger asChild>
				<Button variant="outline" size="sm" disabled={running || others.length === 0}>
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
			className="flex h-9 shrink-0 items-center gap-1.5 border-b border-border px-3"
		>
			<Button variant="outline" size="sm" disabled={running} onClick={() => run({ kind: 'fetch' })}>
				Fetch
			</Button>
			<Button variant="outline" size="sm" disabled={running} onClick={() => run({ kind: 'pull' })}>
				Pull
			</Button>
			<Button
				variant="outline"
				size="sm"
				disabled={running}
				// A branch with no upstream publishes itself on the first push.
				onClick={() =>
					run({ kind: 'push', setUpstream: current ? current.upstream === null : false })
				}
			>
				Push
				{current && current.ahead > 0 && (
					<span data-testid="push-ahead" className="tabular-nums">
						↑{current.ahead}
					</span>
				)}
			</Button>
			<Button variant="outline" size="sm" disabled={running} onClick={() => setNaming(true)}>
				Branch
			</Button>
			{branchMenu('Merge…', (target) => setIntegrate({ kind: 'merge', target }))}
			{branchMenu('Rebase…', (target) => setIntegrate({ kind: 'rebase', target }))}
			<Button
				variant="outline"
				size="sm"
				disabled={running}
				onClick={() => run({ kind: 'stashPush', message: null })}
			>
				Stash
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
					void run({ kind: 'createBranch', name, at: 'HEAD', checkout: true });
				}}
			/>
		</div>
	);
}
