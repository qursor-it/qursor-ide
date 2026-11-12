import { Button } from '@factorai/ui';
import { useStartGitAgentSession } from '@hooks/useStartGitAgentSession';
import { resolveConflictsPrompt } from '@lib/gitPrompts';
import { relativeToRoot } from '@lib/paths';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useRef, useState } from 'react';
import { useGitOp } from './useGitOp';

const SMALL = 'h-6 px-2 text-xs';

/**
 * In-progress merge or rebase: red while files conflict, green once they are
 * resolved, and after a finished rebase an `Undo rebase` (specs/05-features.md
 * F35). The agent only ever starts from the button here.
 */
export function ConflictBanner({ dir, projectId }: { dir: string; projectId: string }) {
	const { run, running } = useGitOp(dir);
	const startAgent = useStartGitAgentSession();
	const navigate = useNavigate();
	const preRebase = useGitModeStore((s) => s.preRebase[dir]);
	const setPreRebase = useGitModeStore((s) => s.setPreRebase);
	const sessionId = useGitModeStore(
		(s) => Object.entries(s.origin).find(([, o]) => o?.dir === dir)?.[0],
	);
	const stateQ = useQuery({
		queryKey: queryKeys.gitRepoState(dir),
		queryFn: () => cmd.gitRepoState(dir),
		refetchInterval: 3_000,
	});
	const refsQ = useQuery({ queryKey: queryKeys.gitRefs(dir), queryFn: () => cmd.gitRefs(dir) });
	const statusQ = useQuery({
		queryKey: queryKeys.gitStatus(dir),
		queryFn: () => cmd.gitStatus(dir),
	});
	// The files this operation conflicted on, kept once they stop conflicting so
	// the green banner can list what was resolved.
	const seen = useRef<{ op: string; files: string[] }>({ op: 'none', files: [] });
	const [refused, setRefused] = useState<string | null>(null);

	const state = stateQ.data;
	if (!state) return null;
	if (state.op === 'none') seen.current = { op: 'none', files: [] };
	else if (state.conflicted.length > 0) seen.current = { op: state.op, files: state.conflicted };

	if (state.op === 'merge' || state.op === 'rebase') {
		const op = state.op;
		const current = refsQ.data?.head ?? 'HEAD';
		const theirs =
			refsQ.data?.local.find((b) => b.sha === state.theirs)?.name ??
			state.theirs?.slice(0, 8) ??
			'the other branch';
		const rel = (files: string[]) => files.map((f) => relativeToRoot(f, dir));
		const abort = (
			<Button
				variant="outline"
				size="sm"
				className={SMALL}
				disabled={running}
				onClick={() =>
					run(
						{ kind: op === 'merge' ? 'mergeAbort' : 'rebaseAbort' },
						{ confirm: `Abort the ${op}? Everything it did so far is undone.` },
					)
				}
			>
				Abort {op}
			</Button>
		);

		if (state.conflicted.length > 0) {
			return (
				<div
					data-testid="conflict-banner"
					className="shrink-0 border-b border-border bg-destructive/10 px-3 py-1.5 text-sm"
				>
					<div className="flex items-center gap-2">
						<span className="min-w-0 flex-1 text-destructive">
							{op === 'merge' ? 'Merge' : 'Rebase'} stopped: {state.conflicted.length} file
							{state.conflicted.length === 1 ? '' : 's'} conflict.
						</span>
						<Button
							size="sm"
							className={SMALL}
							onClick={() =>
								void startAgent(
									projectId,
									dir,
									`Resolve ${op} conflicts`,
									resolveConflictsPrompt({ op, current, theirs, files: rel(state.conflicted) }),
								)
							}
						>
							✦ Resolve with agent
						</Button>
						{abort}
					</div>
					<ul className="mt-1">
						{state.conflicted.map((f) => (
							<li key={f} className="flex items-center gap-1.5 text-xs">
								<span className="min-w-0 flex-1 truncate font-mono text-secondary-foreground">
									{relativeToRoot(f, dir)}
								</span>
								{/* In a rebase git's "ours" is the branch being rebased onto, so
								    "mine" is git's --theirs there. */}
								<Button
									variant="ghost"
									size="sm"
									className={SMALL}
									disabled={running}
									onClick={() => run({ kind: 'checkoutSide', path: f, theirs: op === 'rebase' })}
								>
									Use mine
								</Button>
								<Button
									variant="ghost"
									size="sm"
									className={SMALL}
									disabled={running}
									onClick={() => run({ kind: 'checkoutSide', path: f, theirs: op === 'merge' })}
								>
									Use theirs
								</Button>
							</li>
						))}
					</ul>
				</div>
			);
		}

		return (
			<div
				data-testid="resolved-banner"
				className="flex shrink-0 items-center gap-2 border-b border-border bg-emerald-500/10 px-3 py-1.5 text-sm"
			>
				<span className="min-w-0 flex-1 truncate text-emerald-300">
					{seen.current.files.length > 0
						? `✦ Resolved: ${rel(seen.current.files).join(', ')}`
						: 'No conflicts left.'}
				</span>
				{op === 'merge' ? (
					<Button
						size="sm"
						className={SMALL}
						disabled={running}
						onClick={() => run({ kind: 'commit', message: '', amend: false })}
					>
						Confirm merge commit
					</Button>
				) : (
					<Button
						size="sm"
						className={SMALL}
						disabled={running}
						onClick={() => run({ kind: 'rebaseContinue' })}
					>
						Continue rebase
					</Button>
				)}
				{abort}
				{sessionId && (
					<Button
						variant="outline"
						size="sm"
						className={SMALL}
						onClick={() => {
							useGitModeStore.getState().setMode('sessions');
							void navigate({
								to: '/projects/$projectId/sessions/$sessionId',
								params: { projectId, sessionId },
							});
						}}
					>
						Open the session
					</Button>
				)}
			</div>
		);
	}

	// A rebase that ran to the end: say how to take it back.
	if (state.op === 'none' && preRebase && refsQ.data?.headSha && refsQ.data.headSha !== preRebase) {
		return (
			<div
				data-testid="rebase-done-banner"
				className="flex shrink-0 items-center gap-2 border-b border-border bg-emerald-500/10 px-3 py-1.5 text-sm"
			>
				<span className="min-w-0 flex-1 truncate text-emerald-300">
					{refused ?? 'Rebase finished.'}
				</span>
				<Button
					variant="outline"
					size="sm"
					className={SMALL}
					disabled={running}
					onClick={async () => {
						if ((statusQ.data?.changes.length ?? 0) > 0) {
							setRefused('Undo rebase refused: commit or stash your changes first.');
							return;
						}
						setRefused(null);
						const result = await run(
							{ kind: 'resetHardTo', sha: preRebase },
							{ confirm: 'Undo the rebase? The branch goes back to where it was before it.' },
						);
						if (result?.ok) setPreRebase(dir, null);
					}}
				>
					Undo rebase
				</Button>
				<Button
					variant="ghost"
					size="sm"
					className={SMALL}
					onClick={() => {
						setRefused(null);
						setPreRebase(dir, null);
					}}
				>
					Dismiss
				</Button>
			</div>
		);
	}
	return null;
}
