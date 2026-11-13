import { CommitDetail } from '@components/graph/CommitDetail';
import { CommitRow } from '@components/graph/CommitRow';
import { PanelEmpty } from '@components/layout/PanelEmpty';
import { PanelResizer } from '@components/layout/PanelResizer';
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
import { useGitHistory } from '@hooks/useGitHistory';
import { lanePitch, railWidth, ROW_PAD_LEFT } from '@lib/gitGraph';
import { queryKeys } from '@lib/queryKeys';
import { cmd, copyText } from '@lib/tauri';
import { clampDetailHeight, usePanelStore } from '@store/panelStore';
import { useQuery } from '@tanstack/react-query';
import { useCallback, useState } from 'react';
import { IntegrateDialog, type IntegrateRequest } from './IntegrateDialog';
import { PromptDialog } from './PromptDialog';
import { useGitOp } from './useGitOp';
import { useQuickAgent } from './useQuickAgent';

/** The width the rail and ref chips are laid out for; the table is wider than
 *  the panel the graph was designed in, but its rows are still one line. */
const LAYOUT_WIDTH = 600;

const noCard = () => {};

const RESET_MODES = [
	{
		mode: 'soft',
		label: 'Soft — keep changes staged',
		warning: 'The branch moves; later commits stay staged.',
	},
	{
		mode: 'mixed',
		label: 'Mixed — keep changes unstaged',
		warning: 'The branch moves; later commits stay as unstaged changes.',
	},
	{
		mode: 'hard',
		label: 'Hard — discard changes…',
		warning: 'Uncommitted changes and the commits after it are lost from the branch.',
	},
] as const;

/**
 * The F18 graph at full width, with the selected commit below it (F35). Reuses
 * `CommitRow` (with its author, date and sha columns on) and `CommitDetail`.
 */
export function GitHistory({ dir }: { dir: string }) {
	const { commits, laneCount, graph, isPending, hasMore, isLoadingMore, loadMore } =
		useGitHistory(dir);
	const detailHeight = usePanelStore((s) => s.detailHeight);
	const setDetailHeight = usePanelStore((s) => s.setDetailHeight);
	const [selected, select] = useState<string | null>(null);
	const quick = useQuickAgent(dir);
	const { run, running } = useGitOp(dir);
	const refsQ = useQuery({ queryKey: queryKeys.gitRefs(dir), queryFn: () => cmd.gitRefs(dir) });
	const current = refsQ.data?.head ?? 'HEAD';
	const [naming, setNaming] = useState<{ kind: 'branch' | 'tag'; sha: string } | null>(null);
	const [integrate, setIntegrate] = useState<IntegrateRequest | null>(null);
	// The row the context menu was opened on, found from the event target.
	const [menuSha, setMenuSha] = useState<string | null>(null);
	const onSelect = useCallback((sha: string) => select(sha), []);

	const short = menuSha?.slice(0, 8) ?? '';
	// A merge commit is picked or reverted against its first parent.
	const mergeCommit = (commits.find((c) => c.sha === menuSha)?.parents.length ?? 0) > 1;

	if (isPending) return <PanelEmpty>Loading…</PanelEmpty>;
	if (!graph?.repoRoot) return <PanelEmpty>Not a git repository.</PanelEmpty>;
	if (commits.length === 0) return <PanelEmpty>No commits yet.</PanelEmpty>;

	const pitch = lanePitch(laneCount, LAYOUT_WIDTH);
	const rail = railWidth(laneCount, pitch);
	const textWidth = Math.max(0, LAYOUT_WIDTH - rail - ROW_PAD_LEFT - 24);

	return (
		<div data-testid="git-history" className="flex min-h-0 flex-1 flex-col">
			<div className="min-h-0 flex-1 overflow-auto py-1">
				<ContextMenu>
					<ContextMenuTrigger asChild>
						<ul
							aria-label="Commits"
							onContextMenu={(e) =>
								setMenuSha(
									(e.target as HTMLElement).closest<HTMLElement>('[data-sha]')?.dataset.sha ?? null,
								)
							}
						>
							{commits.map((commit, index) => (
								<CommitRow
									key={commit.sha}
									commit={commit}
									pitch={pitch}
									railWidth={rail}
									textWidth={textWidth}
									selected={commit.sha === selected}
									tabbable={selected ? commit.sha === selected : index === 0}
									remoteHost={graph.remoteHost}
									dirty={false}
									// No hover card here: the selected commit's detail is below.
									cardOpen={false}
									onCardOpen={noCard}
									onSelect={onSelect}
									columns
								/>
							))}
						</ul>
					</ContextMenuTrigger>
					<ContextMenuContent>
						{menuSha && (
							<>
								<ContextMenuItem
									disabled={running}
									onSelect={() =>
										void run(
											{ kind: 'checkoutCommit', sha: menuSha },
											{
												confirm: `Check out ${short}? HEAD will be detached: new commits will not be on any branch.`,
											},
										)
									}
								>
									Checkout this commit…
								</ContextMenuItem>
								<ContextMenuItem
									disabled={running}
									onSelect={() => setNaming({ kind: 'branch', sha: menuSha })}
								>
									New branch here…
								</ContextMenuItem>
								<ContextMenuItem
									disabled={running}
									onSelect={() => setNaming({ kind: 'tag', sha: menuSha })}
								>
									New tag here…
								</ContextMenuItem>
								<ContextMenuSeparator />
								<ContextMenuItem
									disabled={running}
									onSelect={() => void run({ kind: 'cherryPick', sha: menuSha, mergeCommit })}
								>
									Cherry-pick into {current}
								</ContextMenuItem>
								<ContextMenuItem
									disabled={running}
									onSelect={() => void run({ kind: 'revert', sha: menuSha, mergeCommit })}
								>
									Revert this commit
								</ContextMenuItem>
								<ContextMenuItem
									disabled={running}
									onSelect={() => setIntegrate({ kind: 'merge', target: menuSha })}
								>
									Merge into {current}…
								</ContextMenuItem>
								<ContextMenuItem
									disabled={running}
									onSelect={() => setIntegrate({ kind: 'rebase', target: menuSha })}
								>
									Rebase {current} onto here…
								</ContextMenuItem>
								<ContextMenuSub>
									<ContextMenuSubTrigger disabled={running}>
										Reset {current} to here
									</ContextMenuSubTrigger>
									<ContextMenuSubContent>
										{RESET_MODES.map(({ mode, label, warning }) => (
											<ContextMenuItem
												key={mode}
												onSelect={() =>
													void run(
														{ kind: 'resetTo', sha: menuSha, mode },
														{ confirm: `Reset ${current} to ${short}? ${warning}` },
													)
												}
											>
												{label}
											</ContextMenuItem>
										))}
									</ContextMenuSubContent>
								</ContextMenuSub>
								<ContextMenuSeparator />
								<ContextMenuItem onSelect={() => void copyText(menuSha)}>Copy SHA</ContextMenuItem>
								<ContextMenuItem
									onSelect={() =>
										void cmd.gitCommit(dir, menuSha).then((c) => {
											if (c) void copyText(c.body ? `${c.subject}\n\n${c.body}` : c.subject);
										})
									}
								>
									Copy message
								</ContextMenuItem>
								{quick.enabled && (
									<>
										<ContextMenuSeparator />
										<ContextMenuItem
											disabled={quick.busy}
											onSelect={() =>
												void quick.show('Explain this commit', {
													kind: 'explainCommit',
													sha: menuSha,
												})
											}
										>
											✦ Explain this commit
										</ContextMenuItem>
										<ContextMenuItem
											disabled={quick.busy}
											onSelect={() =>
												void quick.show('Find bugs introduced here', {
													kind: 'findBugs',
													sha: menuSha,
												})
											}
										>
											✦ Find bugs introduced here
										</ContextMenuItem>
										<ContextMenuItem
											disabled={quick.busy}
											onSelect={() =>
												void quick.show('Write a release note', {
													kind: 'releaseNote',
													sha: menuSha,
												})
											}
										>
											✦ Write a release note
										</ContextMenuItem>
										<ContextMenuSeparator />
										<ContextMenuItem
											disabled={quick.busy}
											onSelect={() =>
												void quick.show(
													'Undo this commit safely',
													{ kind: 'undoCommit', sha: menuSha },
													{
														request: `Undo commit ${menuSha} safely, without rewriting shared history.`,
													},
												)
											}
										>
											✦ Undo this commit safely
										</ContextMenuItem>
									</>
								)}
							</>
						)}
					</ContextMenuContent>
				</ContextMenu>
				{hasMore && (
					<div className="px-3 py-2">
						<Button variant="secondary" size="sm" disabled={isLoadingMore} onClick={loadMore}>
							{isLoadingMore ? 'Loading…' : 'Load more'}
						</Button>
					</div>
				)}
				<IntegrateDialog dir={dir} request={integrate} onClose={() => setIntegrate(null)} />
				<PromptDialog
					title={naming?.kind === 'tag' ? 'New tag here' : 'New branch here'}
					initial={naming ? '' : null}
					confirmLabel={naming?.kind === 'tag' ? 'Create tag' : 'Create branch'}
					onCancel={() => setNaming(null)}
					onSubmit={(name) => {
						const at = naming?.sha;
						const kind = naming?.kind;
						setNaming(null);
						if (!at) return;
						void run(
							kind === 'tag'
								? { kind: 'createTag', name, at }
								: { kind: 'createBranch', name, at, checkout: true },
						);
					}}
				/>
			</div>
			{selected && (
				<>
					<PanelResizer
						size={detailHeight}
						onSize={setDetailHeight}
						edge="top"
						label="Resize commit detail"
						clamp={clampDetailHeight}
					/>
					<div
						data-testid="commit-detail"
						style={{ height: detailHeight }}
						className="shrink-0 overflow-hidden"
					>
						<CommitDetail projectPath={dir} sha={selected} onSelectSha={select} />
					</div>
				</>
			)}
		</div>
	);
}
