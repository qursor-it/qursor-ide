import { PanelEmpty } from '@components/layout/PanelEmpty';
import type { GitChange } from '@factorai/types';
import { Button } from '@factorai/ui';
import { useFileViewer } from '@hooks/useFileViewer';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import { FileIcon } from '@components/files/FileIcon';
import { Minus, Plus, Undo2 } from 'lucide-react';
import { type MouseEvent, useState } from 'react';
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
	// The rows picked in one list (F35): click picks one, Ctrl/Cmd+click adds or
	// removes, Shift+click takes the range from the last plain click. The diff
	// shows the row clicked last.
	const [picked, setPicked] = useState<{ staged: boolean; paths: string[] }>({
		staged: false,
		paths: [],
	});
	const [anchor, setAnchor] = useState<string | null>(null);
	const [selected, select] = useState<{ path: string; staged: boolean } | null>(null);
	const changes = statusQ.data?.changes ?? [];
	// Conflicted rows are the banner's business, not stageable here.
	const staged = changes.filter((c) => c.group === 'staged');
	const unstaged = changes.filter((c) => c.group === 'unstaged');

	const pick = (e: MouseEvent, rows: GitChange[], c: GitChange, isStaged: boolean) => {
		const same = picked.staged === isStaged;
		let paths: string[];
		if (e.shiftKey && same && anchor) {
			const a = rows.findIndex((r) => r.path === anchor);
			const b = rows.findIndex((r) => r.path === c.path);
			paths = a < 0 ? [c.path] : rows.slice(Math.min(a, b), Math.max(a, b) + 1).map((r) => r.path);
		} else if ((e.ctrlKey || e.metaKey) && same) {
			paths = picked.paths.includes(c.path)
				? picked.paths.filter((p) => p !== c.path)
				: [...picked.paths, c.path];
			setAnchor(c.path);
		} else {
			paths = [c.path];
			setAnchor(c.path);
		}
		setPicked({ staged: isStaged, paths });
		select({ path: c.path, staged: isStaged });
	};

	const section = (label: string, rows: GitChange[], isStaged: boolean) => {
		const chosen =
			picked.staged === isStaged ? picked.paths.filter((p) => rows.some((r) => r.path === p)) : [];
		const targets = chosen.length > 0 ? chosen : rows.map((c) => c.path);
		const which = chosen.length > 0 ? `${chosen.length}` : 'all';
		return (
			<section data-testid={isStaged ? 'staged-list' : 'unstaged-list'}>
				<h3 className="flex h-8 items-center gap-1.5 px-3 font-medium text-muted-foreground text-xs uppercase tracking-wider">
					{label}
					<span className="rounded-full bg-secondary px-1.5 tabular-nums">{rows.length}</span>
					<span className="flex-1" />
					{rows.length > 0 && (
						<>
							{!isStaged && (
								<Button
									variant="quiet"
									size="sm"
									className={`${SMALL} normal-case tracking-normal hover:text-destructive`}
									disabled={running}
									onClick={() =>
										run(
											{ kind: 'discard', paths: targets },
											{
												confirm:
													targets.length === 1
														? `Discard changes to ${rows.find((r) => r.path === targets[0])?.relPath}? They cannot be recovered.`
														: `Discard changes to ${targets.length} files? They cannot be recovered.`,
											},
										)
									}
								>
									<Undo2 /> Discard {which}
								</Button>
							)}
							<Button
								variant="quiet"
								size="sm"
								className={`${SMALL} normal-case tracking-normal text-primary`}
								disabled={running}
								onClick={() => run({ kind: isStaged ? 'unstage' : 'stage', paths: targets })}
							>
								{isStaged ? <Minus /> : <Plus />}
								{isStaged ? 'Unstage' : 'Stage'} {which}
							</Button>
						</>
					)}
				</h3>
				<ul className="px-1.5">
					{rows.map((c) => {
						const on = chosen.includes(c.path);
						const slash = c.relPath.lastIndexOf('/');
						return (
							<li key={c.path} data-testid="change-row">
								<Button
									variant="ghost"
									aria-pressed={on}
									title={`${c.relPath} · double-click to ${isStaged ? 'unstage' : 'stage'}`}
									className={`h-7 w-full justify-start gap-1.5 px-2 font-normal text-sm ${on ? 'bg-primary/10 text-foreground hover:bg-primary/15 hover:text-foreground' : 'text-secondary-foreground hover:bg-secondary/60 hover:text-foreground'}`}
									onClick={(e) => pick(e, rows, c, isStaged)}
									onDoubleClick={() =>
										run({ kind: isStaged ? 'unstage' : 'stage', paths: [c.path] })
									}
								>
									<FileIcon fileName={c.relPath.slice(slash + 1)} className="size-3.5 shrink-0" />
									<span className="min-w-0 flex-1 truncate text-left">
										<span className="text-muted-foreground">{c.relPath.slice(0, slash + 1)}</span>
										{c.relPath.slice(slash + 1)}
									</span>
									<span className="shrink-0 text-xs tabular-nums">
										{c.additions != null && <span className="text-git-added">+{c.additions}</span>}{' '}
										{c.deletions != null && (
											<span className="text-git-deleted">−{c.deletions}</span>
										)}
									</span>
								</Button>
							</li>
						);
					})}
				</ul>
			</section>
		);
	};

	return (
		<div data-testid="local-changes" className="flex min-h-0 flex-1">
			<div className="flex w-80 shrink-0 flex-col border-r border-border">
				<div className="min-h-0 flex-1 overflow-y-auto py-1">
					{section('Staged', staged, true)}
					{section('Unstaged', unstaged, false)}
				</div>
				<CommitBox dir={dir} branch={statusQ.data?.branch ?? null} staged={staged.length} />
			</div>
			<div className="flex min-w-0 flex-1 flex-col">
				{selected ? (
					<>
						<div className="flex h-9 shrink-0 items-center gap-2 border-b border-border px-3 text-xs">
							<span className="min-w-0 truncate text-foreground">{selected.path}</span>
							<span className="rounded-full bg-primary/15 px-2 py-0.5 text-primary">
								{selected.staged ? 'staged' : 'unstaged'}
							</span>
							<span className="flex-1" />
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
