import type { GhStatus, PrDetail, ReviewEvent } from '@factorai/types';
import {
	Button,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
	IconButton,
	Textarea,
	toast,
} from '@factorai/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { X } from 'lucide-react';
import { useState } from 'react';
import { useFileViewer } from '@hooks/useFileViewer';
import { useResetOnUnauthenticated } from '@hooks/useGhStatus';
import { ghErrorText, prDiffMode } from '@lib/pullRequests';
import { queryKeys } from '@lib/queryKeys';
import { draftKey } from '@lib/reviewAnchors';
import { cmd, openExternally } from '@lib/tauri';
import { type ReviewDraftComment, useReviewDraftStore } from '@store/reviewDraftStore';

/**
 * Write the general comment, choose a verdict and publish (specs/05-features.md
 * F31, P3). The line comments come from the diff; this tab lists them, and
 * **submitting is the one write the app makes to GitHub**, so it goes through a
 * confirmation that names the account and the count (ADR-0077).
 */

const EVENTS: { id: ReviewEvent; label: string }[] = [
	{ id: 'COMMENT', label: 'Comment' },
	{ id: 'APPROVE', label: 'Approve' },
	{ id: 'REQUEST_CHANGES', label: 'Request changes' },
];

const OWN_PR_TITLE = 'GitHub does not let you approve or request changes on your own pull request.';

const EMPTY: ReviewDraftComment[] = [];

function eventLabel(event: ReviewEvent): string {
	return EVENTS.find((e) => e.id === event)?.label ?? event;
}

function excerpt(body: string): string {
	const line = body.trim().split('\n')[0] ?? '';
	return line.length > 80 ? `${line.slice(0, 80)}…` : line;
}

function plural(count: number, word: string): string {
	return `${count} ${word}${count === 1 ? '' : 's'}`;
}

export function ReviewTab({ root, pr, status }: { root: string; pr: PrDetail; status: GhStatus }) {
	const slug = status.repo?.slug ?? '';
	const key = draftKey(slug, pr.number);
	const draft = useReviewDraftStore((s) => s.byPr[key]);
	const comments = draft?.comments ?? EMPTY;
	const store = useReviewDraftStore.getState;
	const queryClient = useQueryClient();
	const [confirming, setConfirming] = useState(false);

	// Ordinarily a cache hit: the Files tab ran it for this head. It is what maps
	// a draft's path back to the file to open.
	const files = useQuery({
		queryKey: queryKeys.ghPrFiles(root, pr.number, pr.headRefOid),
		queryFn: () => cmd.ghPrFiles(root, pr.number),
		staleTime: Number.POSITIVE_INFINITY,
		retry: false,
	});
	const { open } = useFileViewer();

	// The stored verdict may be one this pull request cannot take: it is the
	// reader's own, and GitHub refuses both on it.
	const stored = draft?.event ?? 'COMMENT';
	const event: ReviewEvent = pr.viewerIsAuthor && stored !== 'COMMENT' ? 'COMMENT' : stored;
	const body = draft?.body ?? '';
	const stale = comments.length > 0 && draft !== undefined && draft.headOid !== pr.headRefOid;
	// Same rule the backend enforces: only an approval may say nothing.
	const empty = body.trim() === '' && comments.length === 0 && event !== 'APPROVE';

	const submit = useMutation({
		// **Never retried**: a second POST is a second review on the pull request.
		retry: 0,
		mutationFn: () =>
			cmd.ghPrSubmitReview(root, pr.number, {
				commitOid: draft?.headOid ?? pr.headRefOid,
				event,
				body,
				comments: comments.map((c) => ({
					path: c.path,
					line: c.line,
					side: c.side,
					startLine: c.startLine ?? null,
					startSide: c.startLine === undefined ? null : c.side,
					body: c.body,
				})),
			}),
		onSuccess: (published) => {
			store().clear(key);
			void queryClient.invalidateQueries({ queryKey: queryKeys.ghPr(slug, pr.number) });
			void queryClient.invalidateQueries({ queryKey: queryKeys.ghPrComments(slug, pr.number) });
			toast('Review published', {
				action: { label: 'Open on GitHub', onClick: () => void openExternally(published.url) },
			});
		},
	});
	useResetOnUnauthenticated(root, submit.error);

	const openDraft = (draftComment: ReviewDraftComment) => {
		const file = files.data?.files.find((f) => f.relPath === draftComment.path);
		if (file) open(file.path, { diff: prDiffMode(pr.number, pr.mergeBaseOid, pr.headRefOid) });
	};

	const login = status.login ? `@${status.login}` : 'you';
	const count = comments.length === 0 ? 'no line comments' : plural(comments.length, 'comment');
	const summary = `Publish as ${login}: ${count}, ${eventLabel(event)}`;

	return (
		<div className="flex flex-col gap-3 px-3 py-2 text-xs" data-testid="pr-review-panel">
			{stale && (
				<div
					className="flex flex-col gap-1.5 rounded-md border border-primary/40 bg-card p-2"
					data-testid="pr-review-stale"
				>
					<p>
						The pull request changed since you started this review; comments stay anchored to{' '}
						<span className="font-mono">{draft?.headOid.slice(0, 7)}</span>.
					</p>
					<Button
						variant="outline"
						size="sm"
						className="self-start"
						onClick={() => store().clear(key)}
					>
						Discard drafts
					</Button>
				</div>
			)}

			<section className="flex flex-col gap-1">
				<h3 className="flex items-center gap-1.5 font-medium text-muted-foreground uppercase tracking-wide">
					Line comments
					<span className="text-muted-foreground/60">{comments.length}</span>
				</h3>
				{comments.length === 0 ? (
					<p className="text-muted-foreground/60">
						None yet. In a file&apos;s diff, press + beside a line, or select lines and press
						Ctrl+Alt+M.
					</p>
				) : (
					<ul className="flex flex-col gap-1" data-testid="pr-review-drafts">
						{comments.map((c) => (
							<li
								key={c.id}
								className="flex items-start gap-1 rounded border border-border px-1.5 py-1"
								data-testid="pr-review-draft"
							>
								<Button
									variant="quiet"
									size="sm"
									className="h-auto min-w-0 flex-1 flex-col items-start gap-0 whitespace-normal px-0 py-0 text-left font-normal text-xs"
									onClick={() => openDraft(c)}
									title="Open the diff"
								>
									<span className="max-w-full truncate font-mono text-foreground">
										{c.path}:{c.startLine !== undefined ? `${c.startLine}–` : ''}
										{c.line}
										<span className="text-muted-foreground">
											{' '}
											{c.side === 'LEFT' ? '(old)' : '(new)'}
										</span>
									</span>
									<span className="max-w-full truncate text-muted-foreground">
										{excerpt(c.body)}
									</span>
								</Button>
								<IconButton
									aria-label="Remove pending comment"
									title="Remove"
									data-testid="pr-review-draft-remove"
									onClick={() => store().removeComment(key, c.id)}
								>
									<X />
								</IconButton>
							</li>
						))}
					</ul>
				)}
			</section>

			<section className="flex flex-col gap-1">
				<h3 className="font-medium text-muted-foreground uppercase tracking-wide">
					Review comment
				</h3>
				<Textarea
					value={body}
					onChange={(e) => store().setBody(key, pr.headRefOid, e.target.value)}
					placeholder="Leave a comment on the pull request"
					aria-label="Review comment"
					rows={4}
					className="text-xs md:text-xs"
					data-testid="pr-review-body"
				/>
			</section>

			<section className="flex flex-col gap-1">
				<h3 className="font-medium text-muted-foreground uppercase tracking-wide">Verdict</h3>
				<div role="radiogroup" aria-label="Review verdict" className="flex flex-wrap gap-1">
					{EVENTS.map(({ id, label }) => {
						const forbidden = pr.viewerIsAuthor && id !== 'COMMENT';
						return (
							<Button
								key={id}
								type="button"
								role="radio"
								aria-checked={event === id}
								variant={event === id ? 'default' : 'outline'}
								size="sm"
								disabled={forbidden}
								title={forbidden ? OWN_PR_TITLE : undefined}
								data-testid={`pr-review-event-${id}`}
								onClick={() => store().setEvent(key, pr.headRefOid, id)}
							>
								{label}
							</Button>
						);
					})}
				</div>
				{pr.viewerIsAuthor && (
					<p className="text-muted-foreground/60">
						This is your pull request: GitHub only allows plain comments on it.
					</p>
				)}
			</section>

			<div className="flex flex-col items-start gap-1.5">
				<Button
					size="sm"
					disabled={empty || submit.isPending}
					title={empty ? 'Write a comment first' : undefined}
					data-testid="pr-review-submit"
					onClick={() => setConfirming(true)}
				>
					{submit.isPending ? 'Publishing…' : 'Submit review…'}
				</Button>
				{submit.isError && (
					<p className="text-destructive" data-testid="pr-review-submit-error">
						{ghErrorText(submit.error)} Your drafts are kept.
					</p>
				)}
			</div>

			<Dialog open={confirming} onOpenChange={setConfirming}>
				<DialogContent data-testid="pr-review-confirm">
					<DialogHeader>
						<DialogTitle>Publish this review?</DialogTitle>
						<DialogDescription data-testid="pr-review-confirm-text">{summary}</DialogDescription>
					</DialogHeader>
					<p className="text-muted-foreground text-sm">
						This is posted to GitHub on pull request #{pr.number} and cannot be taken back from
						here.
					</p>
					<DialogFooter>
						<Button variant="outline" onClick={() => setConfirming(false)}>
							Cancel
						</Button>
						<Button
							data-testid="pr-review-confirm-submit"
							onClick={() => {
								setConfirming(false);
								submit.mutate();
							}}
						>
							Publish
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
		</div>
	);
}
