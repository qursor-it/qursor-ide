import type { AgentId, GhRepo, GhStatus, PrDetail } from '@factorai/types';
import {
	Button,
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
	IconButton,
} from '@factorai/ui';
import { useQuery } from '@tanstack/react-query';
import { Bot, ChevronLeft, ExternalLink } from 'lucide-react';
import { type ReactNode, useState } from 'react';
import { FileChangeRow } from '@components/files/FileChangeRow';
import { AgentMark } from '@components/layout/AgentMark';
import { CheckGlyph } from '@components/prs/CheckGlyph';
import { CheckoutPr } from '@components/prs/CheckoutPr';
import { ReviewTab } from '@components/prs/ReviewTab';
import { usePrsTabActive, useResetOnUnauthenticated } from '@hooks/useGhStatus';
import { useFileViewer } from '@hooks/useFileViewer';
import { useInstalledAgents } from '@hooks/useInstalledAgents';
import { useStartReviewSession } from '@hooks/useStartReviewSession';
import { agentName } from '@lib/agents';
import { formatAbsolute, formatRelative } from '@lib/format';
import {
	checkStateLabel,
	forkLabel,
	ghErrorText,
	mergeableLabel,
	prDiffMode,
	reviewStateLabel,
} from '@lib/pullRequests';
import { queryKeys } from '@lib/queryKeys';
import { draftKey } from '@lib/reviewAnchors';
import { cmd, openExternally } from '@lib/tauri';
import { useReviewDraftStore } from '@store/reviewDraftStore';

/** The open pull request is re-read every 30s (F31): its checks finishing and a
 *  review landing are what you have it open to see. The list's minute is for a
 *  screen you are scanning, not reading. */
const DETAIL_POLL_MS = 30_000;

type SubTab = 'files' | 'overview' | 'review';

interface PrDetailViewProps {
	projectId: string;
	root: string;
	status: GhStatus;
	number: number;
	onBack: () => void;
}

/**
 * One pull request (specs/05-features.md F31): identity above, then Files,
 * Overview and Review as sub-tabs, in the shape of the graph's `CommitDetail`.
 *
 * **Files first**, because a reviewer opens a pull request to read its diff;
 * the description is one click away. The Review tab is where comments written in
 * the diff are published (P3): the one write the app makes to GitHub, behind a
 * confirmation (ADR-0077). Nothing here merges or closes.
 */
export function PrDetailView({ projectId, root, status, number, onBack }: PrDetailViewProps) {
	const active = usePrsTabActive();
	const slug = status.repo?.slug ?? '';
	const [tab, setTab] = useState<SubTab>('files');
	const drafts = useReviewDraftStore((s) => s.byPr[draftKey(slug, number)]?.comments.length ?? 0);

	const query = useQuery({
		queryKey: queryKeys.ghPr(slug, number),
		queryFn: () => cmd.ghPrDetail(root, number),
		staleTime: DETAIL_POLL_MS,
		refetchInterval: active ? DETAIL_POLL_MS : false,
		retry: false,
	});
	useResetOnUnauthenticated(root, query.error);

	return (
		<div className="flex min-h-0 flex-1 flex-col" data-testid="pr-detail">
			<div className="flex shrink-0 items-center gap-1 border-border border-b px-2 py-1">
				<IconButton aria-label="Back to pull requests" title="Back" onClick={onBack}>
					<ChevronLeft />
				</IconButton>
				<span className="text-muted-foreground text-xs">Pull requests</span>
			</div>

			{query.isPending && <Note>Loading pull request…</Note>}
			{query.isError && (
				<div data-testid="pr-error" className="flex flex-col gap-2 px-3 py-2 text-xs">
					<p className="text-destructive">{ghErrorText(query.error)}</p>
					<Button
						variant="outline"
						size="sm"
						className="self-start"
						onClick={() => void query.refetch()}
					>
						Retry
					</Button>
				</div>
			)}
			{query.data && (
				<>
					<Header pr={query.data} projectId={projectId} root={root} repo={status.repo} />
					<div
						role="tablist"
						aria-label="Pull request"
						className="flex shrink-0 items-center gap-1 border-border border-b px-2 pb-1"
					>
						<SubTabButton id="files" active={tab} onSelect={setTab} label="Files" />
						<SubTabButton id="overview" active={tab} onSelect={setTab} label="Overview" />
						<SubTabButton
							id="review"
							active={tab}
							onSelect={setTab}
							label="Review"
							badge={drafts}
						/>
					</div>
					<div className="min-h-0 flex-1 overflow-y-auto">
						{tab === 'files' && <FilesTab root={root} pr={query.data} />}
						{tab === 'overview' && <Overview pr={query.data} />}
						{tab === 'review' && <ReviewTab root={root} pr={query.data} status={status} />}
					</div>
				</>
			)}
		</div>
	);
}

/**
 * "Review with agent" (F31, ADR-0078): a new session whose first message is the
 * review prompt. One button with the project's agent; with a second agent
 * installed it becomes a menu naming each, the way the sidebar's `+` chevron
 * does. Disabled while the start is in flight, so a double click is one session.
 */
function ReviewWithAgent({
	pr,
	repo,
	projectId,
	root,
}: {
	pr: PrDetail;
	repo: GhRepo;
	projectId: string;
	root: string;
}) {
	const installed = useInstalledAgents();
	const start = useStartReviewSession(projectId, root);
	const [busy, setBusy] = useState(false);
	const [failed, setFailed] = useState<string | null>(null);

	const run = (agent?: AgentId) => {
		setBusy(true);
		setFailed(null);
		start({ pr, repo, agent })
			.catch((e) => setFailed(e instanceof Error ? e.message : 'Could not start the review.'))
			.finally(() => setBusy(false));
	};
	const label = (
		<>
			<Bot />
			Review with agent
		</>
	);

	return (
		<div className="flex flex-col items-start gap-1 pt-1.5">
			{installed.length < 2 ? (
				<Button
					variant="outline"
					size="sm"
					disabled={busy}
					data-testid="pr-review-agent"
					onClick={() => run()}
				>
					{label}
				</Button>
			) : (
				<DropdownMenu>
					<DropdownMenuTrigger asChild>
						<Button variant="outline" size="sm" disabled={busy} data-testid="pr-review-agent">
							{label}
						</Button>
					</DropdownMenuTrigger>
					<DropdownMenuContent align="start" className="w-44">
						{installed.map((agent) => (
							<DropdownMenuItem
								key={agent}
								data-testid={`pr-review-agent-${agent}`}
								onSelect={() => run(agent)}
							>
								<AgentMark agent={agent} aria-hidden />
								Review with {agentName(agent)}
							</DropdownMenuItem>
						))}
					</DropdownMenuContent>
				</DropdownMenu>
			)}
			{failed && (
				<p data-testid="pr-review-error" className="text-destructive text-xs">
					{failed}
				</p>
			)}
		</div>
	);
}

function Header({
	pr,
	projectId,
	root,
	repo,
}: {
	pr: PrDetail;
	projectId: string;
	root: string;
	repo: GhRepo | null;
}) {
	const fork = forkLabel(pr);
	return (
		<div className="shrink-0 px-3 pt-2 pb-1.5">
			<div className="flex items-start gap-1">
				<p
					className="line-clamp-2 min-w-0 flex-1 font-medium text-sm"
					title={pr.title}
					data-testid="pr-title"
				>
					{pr.title}
				</p>
				<IconButton
					aria-label="Open on GitHub"
					title="Open on GitHub"
					className="shrink-0"
					data-testid="pr-open-github"
					onClick={() => void openExternally(pr.url)}
				>
					<ExternalLink />
				</IconButton>
			</div>
			<p className="flex flex-wrap items-center gap-x-1.5 pt-1 text-muted-foreground text-xs">
				<span className="tabular-nums">#{pr.number}</span>
				<span data-testid="pr-state">
					{pr.isDraft && pr.state === 'open' ? 'Draft' : stateLabel(pr.state)}
				</span>
				<span
					className="min-w-0 truncate font-mono"
					title={`${pr.baseRefName} ← ${pr.headRefName}`}
				>
					{pr.baseRefName} ← {pr.headRefName}
				</span>
				{fork && (
					<span
						className="rounded border border-border px-1"
						title="The head branch lives in a fork"
						data-testid="pr-fork"
					>
						{fork}
					</span>
				)}
			</p>
			<CheckoutPr pr={pr} projectId={projectId} root={root} />
			{repo && <ReviewWithAgent pr={pr} repo={repo} projectId={projectId} root={root} />}
			<p className="pt-1 text-muted-foreground text-xs">
				{pr.author}
				<span className="text-muted-foreground/60">
					{' · '}
					{formatRelative(pr.updatedAt)}
				</span>
			</p>
		</div>
	);
}

function stateLabel(state: PrDetail['state']): string {
	return state === 'open' ? 'Open' : state === 'merged' ? 'Merged' : 'Closed';
}

function FilesTab({ root, pr }: { root: string; pr: PrDetail }) {
	const { open } = useFileViewer();
	// **The head is in the key**, so a force-push is a new entry and the files of
	// a given head never expire (F31, `queryKeys.ghPrFiles`).
	const query = useQuery({
		queryKey: queryKeys.ghPrFiles(root, pr.number, pr.headRefOid),
		queryFn: () => cmd.ghPrFiles(root, pr.number),
		staleTime: Number.POSITIVE_INFINITY,
		retry: false,
	});
	useResetOnUnauthenticated(root, query.error);

	if (query.isPending) return <Note>Loading files…</Note>;
	if (query.isError) {
		return (
			<div data-testid="pr-error" className="flex flex-col gap-2 px-3 py-2 text-xs">
				<p className="text-destructive">{ghErrorText(query.error)}</p>
				<Button
					variant="outline"
					size="sm"
					className="self-start"
					onClick={() => void query.refetch()}
				>
					Retry
				</Button>
			</div>
		);
	}
	const { files, total, truncated } = query.data;
	if (files.length === 0) return <Note>This pull request changes no files.</Note>;

	const mode = prDiffMode(pr.number, pr.mergeBaseOid, pr.headRefOid);
	return (
		<>
			<ul className="py-1" data-testid="pr-files">
				{files.map((file) => (
					<FileChangeRow
						key={file.path}
						relPath={file.relPath}
						oldRelPath={file.oldRelPath}
						kind={file.kind}
						additions={file.additions}
						deletions={file.deletions}
						// Whether a file is binary is learned from its blob when the diff
						// opens (the same path a commit range takes), not from this row.
						isBinary={false}
						onClick={() => open(file.path, { diff: mode })}
					/>
				))}
			</ul>
			{(truncated || total > files.length) && (
				<p className="px-3 pb-2 text-muted-foreground/60 text-xs" data-testid="pr-files-truncated">
					… {total - files.length} more files on GitHub
				</p>
			)}
		</>
	);
}

function Overview({ pr }: { pr: PrDetail }) {
	return (
		<div className="flex flex-col gap-3 px-3 py-2 text-xs" data-testid="pr-overview">
			{pr.body ? (
				// Plain text, the author's own line breaks kept: no markdown in v1, and
				// nothing from the body is ever interpreted as markup.
				<p className="whitespace-pre-wrap text-muted-foreground" data-testid="pr-body">
					{pr.body}
				</p>
			) : (
				<p className="text-muted-foreground/60">
					<em>No description.</em>
				</p>
			)}

			<Section title="Checks" count={pr.checkRuns.length}>
				{pr.checkRuns.length === 0 ? (
					<p className="text-muted-foreground/60">No checks.</p>
				) : (
					<ul className="flex flex-col gap-1" data-testid="pr-checks">
						{pr.checkRuns.map((check) => (
							<li
								key={`${check.workflow ?? ''}/${check.name}`}
								className="flex items-center gap-1.5"
							>
								<CheckGlyph state={check.state} label={checkStateLabel(check.state)} />
								<span
									className="min-w-0 flex-1 truncate text-secondary-foreground"
									title={check.name}
								>
									{check.name}
									{check.workflow && (
										<span className="text-muted-foreground/60"> · {check.workflow}</span>
									)}
								</span>
								{check.url && (
									<IconButton
										aria-label={`Open ${check.name} on GitHub`}
										title="Open on GitHub"
										onClick={() => void openExternally(check.url ?? '')}
									>
										<ExternalLink />
									</IconButton>
								)}
							</li>
						))}
					</ul>
				)}
			</Section>

			<Section title="Reviews" count={pr.latestReviews.length}>
				{pr.latestReviews.length === 0 ? (
					<p className="text-muted-foreground/60">No reviews yet.</p>
				) : (
					<ul className="flex flex-col gap-1" data-testid="pr-reviews">
						{pr.latestReviews.map((review) => (
							<li key={review.author} className="text-secondary-foreground">
								{review.author}
								<span className="text-muted-foreground/60">
									{' · '}
									{reviewStateLabel(review.state)}
									{review.submittedAt !== null && ` · ${formatAbsolute(review.submittedAt)}`}
								</span>
							</li>
						))}
					</ul>
				)}
			</Section>

			{pr.reviewRequests.length > 0 && (
				<Section title="Review requested" count={pr.reviewRequests.length}>
					<p className="text-secondary-foreground" data-testid="pr-requests">
						{pr.reviewRequests.join(', ')}
					</p>
				</Section>
			)}

			{pr.labels.length > 0 && (
				<Section title="Labels" count={pr.labels.length}>
					<p className="text-secondary-foreground" data-testid="pr-labels">
						{pr.labels.join(', ')}
					</p>
				</Section>
			)}

			<Section title="Merge">
				<p className="text-secondary-foreground" data-testid="pr-mergeable">
					{mergeableLabel(pr.mergeable)}
				</p>
			</Section>
		</div>
	);
}

function Section({
	title,
	count,
	children,
}: {
	title: string;
	count?: number;
	children: ReactNode;
}) {
	return (
		<section className="flex flex-col gap-1">
			<h3 className="flex items-center gap-1.5 font-medium text-muted-foreground uppercase tracking-wide">
				{title}
				{count !== undefined && <span className="text-muted-foreground/60">{count}</span>}
			</h3>
			{children}
		</section>
	);
}

/** One of the pane's tabs: visually the panel header's `TabButton`, local
 *  state by design, like `CommitDetail`'s. */
function SubTabButton({
	id,
	active,
	onSelect,
	label,
	badge = 0,
}: {
	id: SubTab;
	active: SubTab;
	onSelect: (tab: SubTab) => void;
	label: string;
	/** A count drawn beside the label when it is above zero. */
	badge?: number;
}) {
	const selected = active === id;
	return (
		<button
			type="button"
			role="tab"
			aria-selected={selected}
			className={`rounded px-1.5 py-0.5 font-medium text-sm transition-colors ${
				selected ? 'text-foreground' : 'text-muted-foreground hover:text-foreground'
			}`}
			onClick={() => onSelect(id)}
			data-testid={`pr-${id}-tab`}
		>
			{label}
			{badge > 0 && (
				<span
					className="ml-1 rounded bg-primary/15 px-1 text-primary text-xs tabular-nums"
					data-testid={`pr-${id}-tab-badge`}
				>
					{badge}
				</span>
			)}
		</button>
	);
}

function Note({ children }: { children: string }) {
	return <p className="px-3 py-2 text-muted-foreground text-xs">{children}</p>;
}
