import type { PrDetail } from '@factorai/types';
import {
	Button,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from '@factorai/ui';
import { AlertTriangle, FolderGit2 } from 'lucide-react';
import { useState } from 'react';
import { usePrCheckout } from '@hooks/usePrCheckout';
import { findPrWorktree } from '@hooks/useStartReviewSession';
import { useStartWorktreeSession } from '@hooks/useStartWorktreeSession';
import { useWorktrees } from '@hooks/useWorktrees';
import { forkLabel, ghErrorText } from '@lib/pullRequests';

/**
 * "Check out" (specs/05-features.md F31 P4, ADR-0079): fetch the pull request's
 * head into a **new** worktree beside the main checkout, on `pr/<n>`. The one
 * place the app writes to a repository, so it asks first and says exactly what it
 * will add; the current checkout, its index and HEAD are never touched.
 *
 * When a worktree already has the head out the button is "Open checkout", which
 * starts a session there — the same worktree "Review with agent" prefers.
 */
export function CheckoutPr({
	pr,
	projectId,
	root,
}: {
	pr: PrDetail;
	projectId: string;
	root: string;
}) {
	const worktrees = useWorktrees(root);
	const existing = findPrWorktree(worktrees, pr);
	const startThere = useStartWorktreeSession(projectId, root);
	const [open, setOpen] = useState(false);
	const [opening, setOpening] = useState(false);
	const [openFailed, setOpenFailed] = useState<string | null>(null);
	const { plan, checkout, pending, error, reset } = usePrCheckout(projectId, root, pr.number, open);

	if (existing) {
		return (
			<div className="flex flex-col items-start gap-1 pt-1.5">
				<Button
					variant="outline"
					size="sm"
					disabled={opening}
					data-testid="pr-open-checkout"
					title={existing.path}
					onClick={() => {
						setOpening(true);
						setOpenFailed(null);
						startThere(existing)
							.catch((e) =>
								setOpenFailed(e instanceof Error ? e.message : 'Could not start the session.'),
							)
							.finally(() => setOpening(false));
					}}
				>
					<FolderGit2 />
					Open checkout
				</Button>
				{openFailed && <p className="text-destructive text-xs">{openFailed}</p>}
			</div>
		);
	}

	const data = plan.data;
	const fork = forkLabel(pr);
	const diverged = Boolean(data?.branchExists && data.branchAt !== data.headOid);
	const close = (next: boolean) => {
		if (pending) return;
		setOpen(next);
		if (!next) reset();
	};
	const run = (detached: boolean) => {
		if (!data) return;
		// The error is shown from the mutation's own state.
		checkout(data, detached)
			.then(() => close(false))
			.catch(() => undefined);
	};

	return (
		<div className="flex flex-col items-start gap-1 pt-1.5">
			<Button variant="outline" size="sm" data-testid="pr-checkout" onClick={() => setOpen(true)}>
				<FolderGit2 />
				Check out
			</Button>
			<Dialog open={open} onOpenChange={close}>
				<DialogContent data-testid="pr-checkout-dialog">
					<DialogHeader>
						<DialogTitle>Check out pull request #{pr.number}</DialogTitle>
						<DialogDescription>
							Adds a new worktree next to your checkout. Your current checkout, its files and its
							branch are not touched.
						</DialogDescription>
					</DialogHeader>
					{plan.isPending && <p className="text-muted-foreground text-xs">Preparing…</p>}
					{plan.isError && (
						<p className="text-destructive text-xs" data-testid="pr-checkout-error">
							{ghErrorText(plan.error)}
						</p>
					)}
					{data && (
						<dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
							<dt className="text-muted-foreground">Folder</dt>
							<dd className="min-w-0 break-all font-mono" data-testid="pr-checkout-path">
								{data.worktreePath}
							</dd>
							<dt className="text-muted-foreground">Branch</dt>
							<dd className="font-mono" data-testid="pr-checkout-branch">
								{diverged ? 'detached HEAD' : data.branch}
							</dd>
							<dt className="text-muted-foreground">Commit</dt>
							<dd className="font-mono">{data.headOid.slice(0, 7)}</dd>
						</dl>
					)}
					{data?.fromFork && (
						<p
							className="flex items-start gap-1.5 text-xs text-amber-600 dark:text-amber-400"
							data-testid="pr-checkout-fork"
						>
							<AlertTriangle className="mt-px size-3.5 shrink-0" />
							This code comes from a fork ({fork ?? data.headOwner ?? 'deleted fork'}). Running it
							can execute anything it contains.
						</p>
					)}
					{data && diverged && (
						<p className="text-xs" data-testid="pr-checkout-diverged">
							The branch {data.branch} already exists at {data.branchAt?.slice(0, 7)}, not at this
							pull request's head. It will be left as it is.
						</p>
					)}
					{error != null && (
						<p className="text-destructive text-xs" data-testid="pr-checkout-error">
							{ghErrorText(error)}
						</p>
					)}
					<DialogFooter>
						<Button variant="outline" disabled={pending} onClick={() => close(false)}>
							Cancel
						</Button>
						<Button
							disabled={!data || pending}
							data-testid="pr-checkout-confirm"
							onClick={() => run(diverged)}
						>
							{pending ? 'Checking out…' : diverged ? 'Check out detached' : 'Check out'}
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
		</div>
	);
}
