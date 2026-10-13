import { PrDetailView } from '@components/prs/PrDetail';
import { PrsEmpty, PrsGate } from '@components/prs/PrsView';
import { useActiveCheckout } from '@hooks/useActiveCheckout';
import { useGhStatus } from '@hooks/useGhStatus';

/**
 * A pull request as a page in the main area (specs/05-features.md F31): what a
 * PR tab in the top strip shows. The same `PrDetailView` the panel used to host,
 * in a readable column rather than stretched across the window.
 *
 * Resolves the repository the way the panel's list does, so both read the same
 * `gh_status`; anything short of `ready` is the list's own empty state.
 */
export function PrDetailPage({ number }: { number: number }) {
	const { projectId, root, isLoading } = useActiveCheckout();
	const status = useGhStatus(root);

	const body = () => {
		if (!projectId) return <PrsEmpty>Select a project to see its pull requests.</PrsEmpty>;
		if (isLoading || (root && status.isPending)) return <PrsEmpty>Loading…</PrsEmpty>;
		if (!root) return <PrsEmpty>Project folder not found on disk.</PrsEmpty>;
		if (!status.data) return <PrsEmpty>Could not check the GitHub CLI.</PrsEmpty>;
		const gh = status.data;
		if (gh.state !== 'ready' || !gh.repo) return <PrsGate status={gh} root={root} />;
		return (
			<PrDetailView
				key={`${gh.repo.slug}#${number}`}
				projectId={projectId}
				root={root}
				status={gh}
				number={number}
			/>
		);
	};

	return (
		<main className="flex h-full min-h-0 flex-col" data-testid="pr-page">
			<div className="mx-auto flex min-h-0 w-full max-w-4xl flex-1 flex-col">{body()}</div>
		</main>
	);
}
