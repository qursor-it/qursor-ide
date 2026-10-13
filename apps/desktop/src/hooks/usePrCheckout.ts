import type { GitWorktree, PrCheckoutPlan } from '@factorai/types';
import { toast } from '@factorai/ui';
import { type UseQueryResult, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { checkoutLabel } from '@hooks/useWorktrees';
import { useStartWorktreeSession } from '@hooks/useStartWorktreeSession';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';

/**
 * Checking a pull request out into a new worktree (specs/05-features.md F31 P4,
 * ADR-0079).
 *
 * The plan is read only while the confirm dialog is open, and never reused: where
 * the worktree would go and whether `pr/<n>` is free are facts about the disk
 * right now. The mutation is `retry: 0` — it adds a directory and a branch, and
 * a repeat is a second worktree. On success the worktree list is re-read, which
 * is what turns the button into "Open checkout", and a toast offers a session
 * there.
 */
export function usePrCheckout(
	projectId: string,
	root: string,
	number: number,
	open: boolean,
): {
	plan: UseQueryResult<PrCheckoutPlan>;
	checkout: (plan: PrCheckoutPlan, detached: boolean) => Promise<GitWorktree>;
	pending: boolean;
	error: unknown;
	reset: () => void;
} {
	const queryClient = useQueryClient();
	const startThere = useStartWorktreeSession(projectId, root);

	const plan = useQuery({
		queryKey: queryKeys.ghPrCheckoutPlan(root, number),
		queryFn: () => cmd.ghPrCheckoutPlan(root, number),
		enabled: open,
		staleTime: 0,
		gcTime: 0,
		retry: false,
	});

	const mutation = useMutation({
		retry: 0,
		mutationFn: ({ plan, detached }: { plan: PrCheckoutPlan; detached: boolean }) =>
			cmd.ghPrCheckout(root, number, plan.headOid, detached),
		onSuccess: (worktree) => {
			void queryClient.invalidateQueries({ queryKey: queryKeys.gitWorktrees(root) });
			toast.success(`Checked out PR #${number} in ${checkoutLabel(worktree)}`, {
				action: {
					label: 'Start session there',
					onClick: () =>
						void startThere(worktree).catch((e) => console.error('start session failed', e)),
				},
			});
		},
	});

	return {
		plan,
		checkout: (p, detached) => mutation.mutateAsync({ plan: p, detached }),
		pending: mutation.isPending,
		error: mutation.error,
		reset: mutation.reset,
	};
}
