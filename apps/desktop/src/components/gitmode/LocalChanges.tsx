import { PanelEmpty } from '@components/layout/PanelEmpty';
import type { GitChange } from '@factorai/types';
import { Button } from '@factorai/ui';
import { useFileViewer } from '@hooks/useFileViewer';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { CommitBox } from './CommitBox';
import { HunkDiff } from './HunkDiff';
import { useGitOp } from './useGitOp';

const SMALL = 'h-5 px-1.5 text-xs';

/** Staged and unstaged files, the selected file's hunks and the commit box (F35). */
export function LocalChanges({ dir }: { dir: string }) {
	const { run, running } = useGitOp(dir);
	const { open } = useFileViewer();
	const setMode = useGitModeStore((s) => s.setMode);
	const statusQ = useQuery({
		queryKey: queryKeys.gitStatus(dir),
		queryFn: () => cmd.gitStatus(dir),
		refetchInterval: 5_000,
	});
	const [selected, select] = useState<{ path: string; staged: boolean } | null>(null);
	const changes = statusQ.data?.changes ?? [];
	// Conflicted rows are the banner's business, not stageable here.
	const staged = changes.filter((c) => c.group === 'staged');
	const unstaged = changes.filter((c) => c.group === 'unstaged');

	const section = (label: string, rows: GitChange[], isStaged: boolean) => (
		<section data-testid={isStaged ? 'staged-list' : 'unstaged-list'}>
			<h3 className="flex items-center gap-1.5 px-3 py-1 font-medium text-muted-foreground text-xs uppercase">
				{label}
				<span className="tabular-nums">{rows.length}</span>
				{rows.length > 0 && (
					<Button
						variant="ghost"
						size="sm"
						className={`${SMALL} ml-auto normal-case`}
						disabled={running}
						onClick={() =>
							run({ kind: isStaged ? 'unstage' : 'stage', paths: rows.map((c) => c.path) })
						}
					>
						{isStaged ? 'Unstage all' : 'Stage all'}
					</Button>
				)}
			</h3>
			<ul>
				{rows.map((c) => {
					const on = selected?.path === c.path && selected.staged === isStaged;
					return (
						<li
							key={c.path}
							data-testid="change-row"
							className={`flex h-6.5 items-center gap-1 pr-2 pl-3 text-sm ${on ? 'bg-secondary' : 'hover:bg-secondary/50'}`}
						>
							<button
								type="button"
								className="min-w-0 flex-1 truncate text-left text-secondary-foreground"
								title={c.relPath}
								onClick={() => select({ path: c.path, staged: isStaged })}
							>
								{c.relPath}
							</button>
							<Button
								variant="ghost"
								size="sm"
								className={SMALL}
								disabled={running}
								onClick={() => run({ kind: isStaged ? 'unstage' : 'stage', paths: [c.path] })}
							>
								{isStaged ? 'Unstage' : 'Stage'}
							</Button>
							{!isStaged && (
								<Button
									variant="ghost"
									size="sm"
									className={SMALL}
									disabled={running}
									onClick={() =>
										run(
											{ kind: 'discard', paths: [c.path] },
											{ confirm: `Discard changes to ${c.relPath}? They cannot be recovered.` },
										)
									}
								>
									Discard
								</Button>
							)}
						</li>
					);
				})}
			</ul>
		</section>
	);

	return (
		<div data-testid="local-changes" className="flex min-h-0 flex-1">
			<div className="flex w-80 shrink-0 flex-col border-r border-border">
				<div className="min-h-0 flex-1 overflow-y-auto py-1">
					{section('Unstaged', unstaged, false)}
					{section('Staged', staged, true)}
				</div>
				<CommitBox dir={dir} branch={statusQ.data?.branch ?? null} staged={staged.length} />
			</div>
			<div className="flex min-w-0 flex-1 flex-col">
				{selected ? (
					<>
						<div className="flex h-7 shrink-0 items-center gap-2 border-b border-border px-3 text-xs">
							<span className="min-w-0 flex-1 truncate text-muted-foreground">{selected.path}</span>
							<Button
								variant="ghost"
								size="sm"
								className={SMALL}
								onClick={() => {
									setMode('sessions');
									open(selected.path, { diff: selected.staged ? 'staged' : 'unstaged' });
								}}
							>
								Open side by side
							</Button>
						</div>
						<HunkDiff key={`${selected.path}:${selected.staged}`} dir={dir} {...selected} />
					</>
				) : (
					<PanelEmpty>{changes.length === 0 ? 'No changes.' : 'Select a file.'}</PanelEmpty>
				)}
			</div>
		</div>
	);
}
