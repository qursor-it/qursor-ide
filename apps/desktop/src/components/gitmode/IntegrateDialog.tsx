import {
	Button,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from '@factorai/ui';
import { useGitHistory } from '@hooks/useGitHistory';
import { commitsOnlyIn } from '@lib/gitGraph';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import { useGitOp } from './useGitOp';

export interface IntegrateRequest {
	kind: 'merge' | 'rebase';
	target: string;
}

interface IntegrateDialogProps {
	dir: string;
	/** Null closes it. */
	request: IntegrateRequest | null;
	onClose: () => void;
}

/**
 * Confirms a merge or rebase with what it will do, and offers to stash first
 * when the working tree is dirty (specs/05-features.md F35). A rebase records
 * HEAD for "Undo rebase" before it starts.
 */
export function IntegrateDialog({ dir, request, onClose }: IntegrateDialogProps) {
	const { run, running } = useGitOp(dir);
	const setPreRebase = useGitModeStore((s) => s.setPreRebase);
	const refsQ = useQuery({ queryKey: queryKeys.gitRefs(dir), queryFn: () => cmd.gitRefs(dir) });
	const statusQ = useQuery({
		queryKey: queryKeys.gitStatus(dir),
		queryFn: () => cmd.gitStatus(dir),
	});
	const { commits } = useGitHistory(dir);
	const refs = refsQ.data;
	const dirty = (statusQ.data?.changes.length ?? 0) > 0;

	let text = '';
	if (request && refs) {
		const current = refs.head ?? 'HEAD';
		const targetSha = refs.local.find((b) => b.name === request.target)?.sha;
		const headSha = refs.headSha;
		const n =
			targetSha && headSha
				? request.kind === 'merge'
					? commitsOnlyIn(commits, targetSha, headSha)
					: commitsOnlyIn(commits, headSha, targetSha)
				: null;
		const count = n === null ? 'Commits' : `${n} commit${n === 1 ? '' : 's'}`;
		text =
			request.kind === 'merge'
				? `${count} from ${request.target} enter ${current}.`
				: `${count} of ${current} are replayed onto ${request.target}.`;
	}

	async function go(stash: boolean) {
		if (!request) return;
		onClose();
		if (stash) {
			const stashed = await run({ kind: 'stashPush', message: null });
			if (!stashed?.ok) return;
		}
		if (request.kind === 'rebase') setPreRebase(dir, refs?.headSha ?? null);
		await run(
			request.kind === 'merge'
				? { kind: 'merge', branch: request.target }
				: { kind: 'rebase', onto: request.target },
		);
	}

	const label = request?.kind === 'rebase' ? 'Rebase' : 'Merge';
	return (
		<Dialog open={request !== null} onOpenChange={(open) => !open && onClose()}>
			<DialogContent>
				<DialogHeader>
					<DialogTitle>
						{label} {request?.target}
					</DialogTitle>
					<DialogDescription>
						{text}
						{dirty && ' You have uncommitted changes.'}
					</DialogDescription>
				</DialogHeader>
				<DialogFooter>
					<Button variant="outline" onClick={onClose}>
						Cancel
					</Button>
					{dirty && (
						<Button variant="outline" disabled={running} onClick={() => void go(true)}>
							Stash and continue
						</Button>
					)}
					<Button disabled={running} onClick={() => void go(false)}>
						{label}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
