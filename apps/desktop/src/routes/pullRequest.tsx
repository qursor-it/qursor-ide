import { PrDetailPage } from '@components/prs/PrDetailPage';
import { createRoute, redirect, useParams } from '@tanstack/react-router';
import { rootRoute } from './__root';

/** A positive integer with no sign, fraction or leading zero: the only thing a
 *  pull request number can be, and what a hand-edited URL is checked against. */
const PR_NUMBER = /^[1-9]\d{0,9}$/;

function PullRequestView() {
	const { number } = useParams({ from: pullRequestRoute.id });
	return <PrDetailPage number={Number(number)} />;
}

/** A pull request open as a tab in the top strip (specs/05-features.md F31). */
export const pullRequestRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: '/projects/$projectId/prs/$number',
	// A bad number lands on the project rather than on a page about nothing.
	beforeLoad: ({ params }) => {
		if (!PR_NUMBER.test(params.number)) {
			throw redirect({ to: '/projects/$id', params: { id: params.projectId } });
		}
	},
	component: PullRequestView,
});
