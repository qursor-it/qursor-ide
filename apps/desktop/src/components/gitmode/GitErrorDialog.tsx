import {
	Button,
	Dialog,
	DialogContent,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from '@factorai/ui';
import { useStartGitAgentSession } from '@hooks/useStartGitAgentSession';
import { isLooseRepo, useGitModeStore } from '@store/gitModeStore';
import { type GitErrorAction, explainGitError } from './gitErrors';
import { useGitOp, useGitUi } from './useGitOp';
import { useQuickAgent } from './useQuickAgent';

const LABELS: Record<GitErrorAction['kind'], string> = {
	retry: 'Retry',
	pullThenPush: 'Pull and push',
	stashAndRetry: 'Stash and retry',
	pushSetUpstream: 'Push and set upstream',
	askAgent: '✦ Ask the agent',
	copy: 'Copy',
};

const close = () => useGitUi.setState({ error: null });

/** The modal for a failed git op (specs/05-features.md F35); the OpLog keeps the history. */
export function GitErrorHost() {
	const error = useGitUi((s) => s.error);
	return (
		<Dialog open={error !== null} onOpenChange={(open) => !open && close()}>
			<DialogContent>{error && <ErrorBody {...error} />}</DialogContent>
		</Dialog>
	);
}

function ErrorBody({
	dir,
	op,
	result,
}: NonNullable<ReturnType<typeof useGitUi.getState>['error']>) {
	const { run } = useGitOp(dir);
	const quick = useQuickAgent(dir);
	const startAgent = useStartGitAgentSession();
	const projectId = useGitModeStore((s) => s.tabs.find((t) => t.dir === dir)?.projectId);
	const info = explainGitError(op, result);
	if (!info) return null;
	const output = [result.stderr, result.stdout].filter((t) => t.trim()).join('\n');
	const actions = info.actions.filter(
		(a) => a.kind !== 'askAgent' || (quick.enabled && projectId && !isLooseRepo(projectId)),
	);

	// Close first: a retry that fails again reopens the dialog with its own result.
	async function act(action: GitErrorAction) {
		if (action.kind === 'copy') {
			await navigator.clipboard.writeText(`$ ${result.command}\n${output}`);
			return;
		}
		close();
		switch (action.kind) {
			case 'retry':
				await run(op);
				break;
			case 'pullThenPush':
				if ((await run({ kind: 'pull' }))?.ok) await run({ kind: 'push', setUpstream: false });
				break;
			case 'stashAndRetry':
				if ((await run({ kind: 'stashPush', message: null }))?.ok) await run(op);
				break;
			case 'pushSetUpstream':
				await run({ kind: 'push', setUpstream: true });
				break;
			case 'askAgent':
				if (projectId)
					await startAgent(
						projectId,
						dir,
						info?.title ?? 'Git error',
						`This git command failed:\n\n$ ${result.command}\n\n${output}\n\nExplain what went wrong and fix it. Do not force-push and do not discard any work.`,
					);
				break;
		}
	}

	return (
		<div data-testid="git-error-dialog">
			<DialogHeader>
				<DialogTitle>{info.title}</DialogTitle>
			</DialogHeader>
			<div className="my-3 flex flex-col gap-2">
				<div className="font-mono text-xs text-muted-foreground">$ {result.command}</div>
				<pre className="max-h-64 overflow-auto whitespace-pre-wrap rounded border border-border bg-card p-2 font-mono text-xs">
					{output}
				</pre>
			</div>
			<DialogFooter>
				<Button variant="outline" onClick={close}>
					Close
				</Button>
				{actions.map((a, i) => (
					<Button
						key={a.kind}
						variant={i === 0 ? 'default' : 'outline'}
						onClick={() => void act(a)}
					>
						{LABELS[a.kind]}
					</Button>
				))}
			</DialogFooter>
		</div>
	);
}
