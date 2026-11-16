import { CommitSession } from '@components/gitmode/CommitSession';
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
import { reachableFrom } from '@lib/reachable';
import { sessionDuring } from '@lib/sessionDuring';
import { clampGitDetailHeight, isLooseRepo, useGitModeStore } from '@store/gitModeStore';
import { cmd, copyText } from '@lib/tauri';
import { useQuery } from '@tanstack/react-query';
import { type ReactNode, useCallback, useMemo, useState } from 'react';
import { GitCommitPane } from './GitCommitPane';
import { IntegrateDialog, type IntegrateRequest } from './IntegrateDialog';
import { PromptDialog } from './PromptDialog';
import { useGitOp } from './useGitOp';
import { useQuickAgent } from './useQuickAgent';

/** The width the rail and ref chips are laid out for; the table is wider than
 *  the panel the graph was designed in, but its rows are still one line. */
const LAYOUT_WIDTH = 600;
/** Dense, as desktop Git clients list commits. */
const TABLE_ROW_HEIGHT = 24;

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
export function GitHistory({ dir, projectId }: { dir: string; projectId: string }) {
	const { commits, laneCount, graph, isPending, hasMore, isLoadingMore, loadMore } =
		useGitHistory(dir);
	// Which session was running in this checkout when each commit was made
	// (F41), for the table's Session column. The list the project page reads.
	const sessionsQ = useQuery({
		queryKey: queryKeys.sessions(projectId),
		queryFn: () => cmd.listSessions(projectId),
		enabled: !isLooseRepo(projectId),
	});
	const sessionOf = useMemo(() => {
		const map = new Map<string, string>();
		for (const c of commits) {
			const s = sessionDuring(c.commitTime, sessionsQ.data ?? [], dir);
			if (s) map.set(c.sha, s.title.trim() || 'Untitled session');
		}
		return map;
	}, [commits, sessionsQ.data, dir]);
	const [selected, select] = useState<string | null>(null);
	const detailHeight = useGitModeStore((s) => s.detailHeight);
	const setDetailHeight = useGitModeStore((s) => s.setDetailHeight);
	const quick = useQuickAgent(dir);
	const { run, running } = useGitOp(dir);
	const refsQ = useQuery({ queryKey: queryKeys.gitRefs(dir), queryFn: () => cmd.gitRefs(dir) });
	const current = refsQ.data?.head ?? 'HEAD';
	const headSha = refsQ.data?.headSha ?? null;
	// What HEAD contains, so the rest can be dimmed (F35).
	const onHead = useMemo(() => reachableFrom(headSha, commits), [headSha, commits]);
	const [naming, setNaming] = useState<{ kind: 'branch' | 'tag'; sha: string } | null>(null);
	const [integrate, setIntegrate] = useState<IntegrateRequest | null>(null);
	// The row the context menu was opened on, found from the event target.
	const [menuSha, setMenuSha] = useState<string | null>(null);
	const onSelect = useCallback((sha: string) => select(sha), []);

	const short = menuSha?.slice(0, 8) ?? '';
	const selectedMerge = (commits.find((c) => c.sha === selected)?.parents.length ?? 0) > 1;
	// A merge commit is picked or reverted against its first parent.
	const mergeCommit = (commits.find((c) => c.sha === menuSha)?.parents.length ?? 0) > 1;

	if (isPending) return <PanelEmpty>Loading…</PanelEmpty>;
	if (!graph?.repoRoot) return <PanelEmpty>Not a git repository.</PanelEmpty>;
	if (commits.length === 0) return <PanelEmpty>No commits yet.</PanelEmpty>;

	const pitch = lanePitch(laneCount, LAYOUT_WIDTH);
	// At least as wide as its column head.
	const rail = Math.max(48, railWidth(laneCount, pitch));
	const textWidth = Math.max(0, LAYOUT_WIDTH - rail - ROW_PAD_LEFT - 24);

	return (
		// The commit list, and the selected commit docked under it as desktop Git
		// clients have it (F35, ADR-0099).
		<div data-testid="git-history" className="flex min-h-0 min-w-0 flex-1 flex-col">
			<div className="@container min-h-0 min-w-0 flex-1 overflow-auto pb-1">
				{/* The table's column heads, as in the mock-up (F35, ADR-0095). */}
				<div
					aria-hidden
					style={{ paddingLeft: ROW_PAD_LEFT }}
					className="sticky top-0 z-10 flex h-8 items-center gap-1.5 border-border border-b bg-secondary/60 pr-2 font-medium text-muted-foreground text-xs uppercase tracking-wider backdrop-blur"
				>
					<span className="shrink-0" style={{ width: rail }}>
						Graph
					</span>
					<span className="min-w-0 flex-1">Subject</span>
					{sessionOf.size > 0 && <span className="hidden w-40 shrink-0 @4xl:block">Session</span>}
					<span className="hidden w-28 shrink-0 @2xl:block">Author</span>
					<span className="hidden w-16 shrink-0 @lg:block">SHA</span>
					<span className="hidden w-32 shrink-0 @xl:block">Date</span>
				</div>
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
									// The column only when some commit has a session to show.
									session={sessionOf.size > 0 ? (sessionOf.get(commit.sha) ?? null) : undefined}
									rowHeight={TABLE_ROW_HEIGHT}
									head={commit.sha === headSha}
									away={onHead.size > 0 && !onHead.has(commit.sha)}
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
						clamp={clampGitDetailHeight}
					/>
					<div
						data-testid="commit-detail"
						style={{ height: detailHeight }}
						className="@container flex shrink-0 overflow-y-auto"
					>
						<div className="flex min-w-0 flex-1 flex-col @3xl:flex-row">
							<div className="min-w-0 flex-1 @3xl:overflow-y-auto">
								<div className="pt-3 empty:hidden">
									<CommitSession projectId={projectId} dir={dir} sha={selected} />
								</div>
								<GitCommitPane
									dir={dir}
									sha={selected}
									refs={commits.find((c) => c.sha === selected)?.refs ?? []}
									onSelectSha={select}
								/>
							</div>
							<aside className="border-border border-t @3xl:w-72 @3xl:shrink-0 @3xl:overflow-y-auto @3xl:border-t-0 @3xl:border-l">
								{/* The context menu's ✦ questions and its actions, laid out in the
					    detail as the mock-up has them (F35, ADR-0095). */}
								{quick.enabled && (
									<DetailSection title="Ask about this commit">
										<div data-testid="commit-ask" className="flex flex-col gap-1.5">
											{(
												[
													['Explain this commit', { kind: 'explainCommit', sha: selected }],
													['Find bugs introduced here', { kind: 'findBugs', sha: selected }],
													['Write a release note', { kind: 'releaseNote', sha: selected }],
												] as const
											).map(([label, task]) => (
												<Button
													key={label}
													variant="outline"
													disabled={quick.busy}
													onClick={() => void quick.show(label, task)}
													className="justify-start gap-2 font-normal"
												>
													<span className="text-primary">✦</span> {label}
												</Button>
											))}
											<Button
												variant="outline"
												disabled={quick.busy}
												onClick={() =>
													void quick.show(
														'Undo this commit safely',
														{ kind: 'undoCommit', sha: selected },
														{
															request: `Undo commit ${selected} safely, without rewriting shared history.`,
														},
													)
												}
												className="justify-start gap-2 font-normal"
											>
												<span className="text-primary">✦</span> Undo this commit safely
											</Button>
										</div>
									</DetailSection>
								)}
								<DetailSection title="Actions">
									<div data-testid="commit-actions" className="flex flex-wrap gap-1.5">
										<Button
											size="sm"
											variant="secondary"
											disabled={running}
											onClick={() =>
												void run({ kind: 'cherryPick', sha: selected, mergeCommit: selectedMerge })
											}
										>
											Cherry-pick
										</Button>
										<Button
											size="sm"
											variant="secondary"
											disabled={running}
											onClick={() =>
												void run({ kind: 'revert', sha: selected, mergeCommit: selectedMerge })
											}
										>
											Revert
										</Button>
										<Button
											size="sm"
											variant="secondary"
											disabled={running}
											onClick={() => setNaming({ kind: 'branch', sha: selected })}
										>
											New branch here
										</Button>
										<Button
											size="sm"
											variant="secondary"
											disabled={running}
											onClick={() => setNaming({ kind: 'tag', sha: selected })}
										>
											Tag
										</Button>
										<Button
											size="sm"
											variant="secondary"
											disabled={running}
											onClick={() =>
												void run(
													{ kind: 'checkoutCommit', sha: selected },
													{
														confirm: `Check out ${selected.slice(0, 8)}? HEAD will be detached: new commits will not be on any branch.`,
													},
												)
											}
										>
											Checkout
										</Button>
									</div>
								</DetailSection>
							</aside>
						</div>
					</div>
				</>
			)}
		</div>
	);
}

function DetailSection({ title, children }: { title: string; children: ReactNode }) {
	return (
		<section className="border-border border-t px-4 py-3">
			<h3 className="mb-2 font-medium text-muted-foreground text-xs uppercase tracking-wider">
				{title}
			</h3>
			{children}
		</section>
	);
}
