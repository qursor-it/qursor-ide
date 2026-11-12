import { CommitDetail } from '@components/graph/CommitDetail';
import { CommitRow } from '@components/graph/CommitRow';
import { PanelEmpty } from '@components/layout/PanelEmpty';
import { PanelResizer } from '@components/layout/PanelResizer';
import { Button } from '@factorai/ui';
import { useGitHistory } from '@hooks/useGitHistory';
import { lanePitch, railWidth, ROW_PAD_LEFT } from '@lib/gitGraph';
import { clampDetailHeight, usePanelStore } from '@store/panelStore';
import { useCallback, useState } from 'react';

/** The width the rail and ref chips are laid out for; the table is wider than
 *  the panel the graph was designed in, but its rows are still one line. */
const LAYOUT_WIDTH = 600;

/**
 * The F18 graph at full width, with the selected commit below it (F35). Reuses
 * `CommitRow` and `CommitDetail`; author, date and sha are on the row's hover
 * card rather than in columns of their own.
 */
export function GitHistory({ dir }: { dir: string }) {
	const { commits, laneCount, graph, isPending, hasMore, isLoadingMore, loadMore } =
		useGitHistory(dir);
	const detailHeight = usePanelStore((s) => s.detailHeight);
	const setDetailHeight = usePanelStore((s) => s.setDetailHeight);
	const [selected, select] = useState<string | null>(null);
	const [carded, setCarded] = useState<string | null>(null);
	const onCardOpen = useCallback((sha: string, open: boolean) => {
		setCarded((current) => (open ? sha : current === sha ? null : current));
	}, []);
	const onSelect = useCallback((sha: string) => select(sha), []);

	if (isPending) return <PanelEmpty>Loading…</PanelEmpty>;
	if (!graph?.repoRoot) return <PanelEmpty>Not a git repository.</PanelEmpty>;
	if (commits.length === 0) return <PanelEmpty>No commits yet.</PanelEmpty>;

	const pitch = lanePitch(laneCount, LAYOUT_WIDTH);
	const rail = railWidth(laneCount, pitch);
	const textWidth = Math.max(0, LAYOUT_WIDTH - rail - ROW_PAD_LEFT - 24);

	return (
		<div data-testid="git-history" className="flex min-h-0 flex-1 flex-col">
			<div className="min-h-0 flex-1 overflow-auto py-1">
				<ul aria-label="Commits">
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
							cardOpen={commit.sha === carded}
							onCardOpen={onCardOpen}
							onSelect={onSelect}
						/>
					))}
				</ul>
				{hasMore && (
					<div className="px-3 py-2">
						<Button variant="secondary" size="sm" disabled={isLoadingMore} onClick={loadMore}>
							{isLoadingMore ? 'Loading…' : 'Load more'}
						</Button>
					</div>
				)}
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
