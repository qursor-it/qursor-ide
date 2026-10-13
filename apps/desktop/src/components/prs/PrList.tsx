import type { GhStatus, PrStateFilter, PrSummary } from '@factorai/types';
import {
	Badge,
	Button,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from '@factorai/ui';
import { useQuery } from '@tanstack/react-query';
import { GitPullRequestArrow } from 'lucide-react';
import { PanelEmpty } from '@components/layout/PanelEmpty';
import { CheckGlyph } from '@components/prs/CheckGlyph';
import { usePrsTabActive, useResetOnUnauthenticated } from '@hooks/useGhStatus';
import { useActiveCheckout } from '@hooks/useActiveCheckout';
import { useGitBranch } from '@hooks/useGitBranch';
import { formatRelative } from '@lib/format';
import { checksSummaryText, ghErrorText, reviewDecisionLabel } from '@lib/pullRequests';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { usePrPanelStore } from '@store/prPanelStore';

/** The list re-reads once a minute while the tab is showing (F31): a review
 *  request or a failing check is the news the tab exists for, and a minute is
 *  slow enough to cost the rate limit nothing. Detail polls faster, because it
 *  is the one you are looking at. */
const LIST_POLL_MS = 60_000;

const FILTERS: { value: PrStateFilter; label: string }[] = [
	{ value: 'open', label: 'Open' },
	{ value: 'closed', label: 'Closed' },
	{ value: 'merged', label: 'Merged' },
	{ value: 'all', label: 'All' },
];

interface PrListProps {
	root: string;
	status: GhStatus;
	onOpen: (number: number) => void;
}

/**
 * The pull requests of `origin` in one state (specs/05-features.md F31).
 *
 * Keyed by the repository's slug, not the path, so two checkouts of one
 * repository share the answer. Nothing here writes: a row only opens the detail.
 */
export function PrList({ root, status, onOpen }: PrListProps) {
	const filter = usePrPanelStore((s) => s.filter);
	const setFilter = usePrPanelStore((s) => s.setFilter);
	const active = usePrsTabActive();
	const slug = status.repo?.slug ?? '';
	// The branch of the checkout the panel is rooted at: what "this checkout" is
	// compared against. Shares `useGitStatus`'s cache entry.
	const branch = useGitBranch(useActiveCheckout().root);

	const query = useQuery({
		queryKey: queryKeys.ghPrs(slug, filter),
		queryFn: () => cmd.ghPrList(root, filter),
		staleTime: LIST_POLL_MS,
		// Only while this tab is the one showing; a hidden tab costs no requests.
		refetchInterval: active ? LIST_POLL_MS : false,
		retry: false,
	});
	useResetOnUnauthenticated(root, query.error);

	const repoName = status.repo ? `${status.repo.owner}/${status.repo.name}` : '';

	return (
		<div className="flex min-h-0 flex-1 flex-col" data-testid="pr-list">
			<div className="flex shrink-0 items-center gap-2 border-border border-b px-2 py-1.5">
				<span
					className="min-w-0 flex-1 truncate text-muted-foreground text-xs"
					title={status.repo?.slug}
					data-testid="pr-repo"
				>
					{repoName}
					{status.repo && status.repo.remote !== 'origin' && (
						<span data-testid="pr-remote" title={`Read from the git remote ${status.repo.remote}`}>
							{` · via remote ${status.repo.remote}`}
						</span>
					)}
				</span>
				<Select value={filter} onValueChange={(v) => setFilter(v as PrStateFilter)}>
					<SelectTrigger
						aria-label="Pull request state"
						data-testid="pr-filter"
						className="h-6 w-24 shrink-0 px-2 text-xs"
					>
						<SelectValue />
					</SelectTrigger>
					<SelectContent>
						{FILTERS.map((f) => (
							<SelectItem key={f.value} value={f.value} data-testid={`pr-filter-${f.value}`}>
								{f.label}
							</SelectItem>
						))}
					</SelectContent>
				</Select>
			</div>

			<div className="min-h-0 flex-1 overflow-auto py-1 pr-2">
				{query.isPending && (
					<div className="py-1">
						<PanelEmpty>Loading…</PanelEmpty>
					</div>
				)}
				{query.isError && <ListError error={query.error} onRetry={() => void query.refetch()} />}
				{query.data && query.data.length === 0 && (
					<div className="py-1">
						<PanelEmpty>{`No ${filter === 'all' ? '' : `${filter} `}pull requests.`}</PanelEmpty>
					</div>
				)}
				{query.data && query.data.length > 0 && (
					<ul>
						{query.data.map((pr) => (
							<PrRow
								key={pr.number}
								pr={pr}
								thisCheckout={branch !== null && pr.headRefName === branch}
								onOpen={() => onOpen(pr.number)}
							/>
						))}
					</ul>
				)}
			</div>
		</div>
	);
}

function ListError({ error, onRetry }: { error: unknown; onRetry: () => void }) {
	return (
		<div data-testid="pr-error" className="flex flex-col gap-2 px-3 py-2 text-xs">
			<p className="text-destructive">{ghErrorText(error)}</p>
			<Button variant="outline" size="sm" className="self-start" onClick={onRetry}>
				Retry
			</Button>
		</div>
	);
}

function PrRow({
	pr,
	thisCheckout,
	onOpen,
}: {
	pr: PrSummary;
	thisCheckout: boolean;
	onOpen: () => void;
}) {
	const decision = reviewDecisionLabel(pr.reviewDecision);
	return (
		<li data-testid={`pr-row-${pr.number}`}>
			<button
				type="button"
				// The row rests at `secondary-foreground` and takes `foreground` from
				// its hover (DESIGN.md, The Focus-Not-Default Rule).
				className="group flex w-full flex-col gap-0.5 px-3 py-1.5 text-left transition-colors hover:bg-secondary/50"
				title={pr.title}
				onClick={onOpen}
			>
				<span className="flex min-w-0 items-center gap-1.5 text-sm">
					{pr.reviewRequestedFromViewer && (
						// **The one blue on the row** (DESIGN.md, The One Accent Rule): a
						// review asked of *you* is the human's turn. Nothing else here is
						// blue — a checkout match or a draft is not an ask.
						<span
							role="img"
							aria-label="Review requested from you"
							title="Review requested from you"
							data-testid="pr-review-requested"
							className="size-1.5 shrink-0 rounded-full bg-primary"
						/>
					)}
					<span className="shrink-0 text-muted-foreground tabular-nums">#{pr.number}</span>
					<span className="min-w-0 flex-1 truncate text-secondary-foreground group-hover:text-foreground">
						{pr.title}
					</span>
					<ChecksMark checks={pr.checks} />
				</span>
				<span className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-muted-foreground text-xs">
					<span className="min-w-0 truncate">
						{pr.author} · {formatRelative(pr.updatedAt)}
					</span>
					{pr.isDraft && (
						<Badge
							variant="outline"
							className="px-1.5 py-0 font-normal text-muted-foreground"
							data-testid="pr-draft"
						>
							Draft
						</Badge>
					)}
					{decision && <span data-testid="pr-decision">{decision}</span>}
					{thisCheckout && (
						<span
							className="flex items-center gap-1"
							title="This checkout is on this pull request's branch"
							data-testid="pr-this-checkout"
						>
							<GitPullRequestArrow className="size-3 shrink-0" aria-hidden />
							this checkout
						</span>
					)}
				</span>
			</button>
		</li>
	);
}

/** Pass, fail or running, and nothing at all for a pull request with no checks.
 *  A spinner per row would be thirty things moving in a list the reader is
 *  trying to scan, so `CheckGlyph` holds still. */
function ChecksMark({ checks }: { checks: PrSummary['checks'] }) {
	const text = checksSummaryText(checks);
	if (!text || checks.state === 'none') return null;
	return <CheckGlyph state={checks.state} label={text} testId={`pr-checks-${checks.state}`} />;
}
