import {
	Button,
	Dialog,
	DialogContent,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from '@factorai/ui';
import { useStartGitAgentSession } from '@hooks/useStartGitAgentSession';
import { cmd } from '@lib/tauri';
import { useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { parseProposal, useAgentUi, useQuickAgent } from './useQuickAgent';

/**
 * Spinner with a cancel while a question runs, its error as text, and the
 * answer: plain text, or for a proposal the `# ` explanation and the git lines
 * with `Run` (only when every line passes the allowlist) and `Open in a session`.
 */
export function AgentPopover({ dir, projectId }: { dir: string; projectId: string }) {
	const { cancel } = useQuickAgent(dir);
	const pending = useAgentUi((s) => s.pending[dir]);
	const error = useAgentUi((s) => s.error[dir]);
	const answer = useAgentUi((s) => s.answer[dir]);
	const close = () => useAgentUi.setState((s) => ({ answer: { ...s.answer, [dir]: undefined } }));
	return (
		<>
			{(pending || error) && (
				<div
					data-testid="agent-status"
					className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-1 text-xs"
				>
					{pending ? (
						<>
							<span className="animate-spin [animation-duration:600ms]">✦</span>
							<span className="min-w-0 flex-1 truncate text-secondary-foreground">{pending}…</span>
							<Button variant="ghost" size="sm" className="h-6 px-2 text-xs" onClick={cancel}>
								Cancel
							</Button>
						</>
					) : (
						<span className="min-w-0 flex-1 truncate text-destructive">{error}</span>
					)}
				</div>
			)}
			<Dialog open={answer !== undefined} onOpenChange={(open) => !open && close()}>
				<DialogContent>
					{answer && (
						<AnswerBody
							key={answer.text}
							dir={dir}
							projectId={projectId}
							answer={answer}
							onClose={close}
						/>
					)}
				</DialogContent>
			</Dialog>
		</>
	);
}

function AnswerBody({
	dir,
	projectId,
	answer: { title, text, propose, request },
	onClose,
}: {
	dir: string;
	projectId: string;
	answer: { title: string; text: string; propose: boolean; request: string };
	onClose: () => void;
}) {
	const startAgent = useStartGitAgentSession();
	const queryClient = useQueryClient();
	const { explanation, lines } = parseProposal(text);
	const [ran, setRan] = useState<string | null>(null);
	// Undefined until the allowlist has answered; one reason blocks Run.
	const [reasons, setReasons] = useState<(string | null)[] | undefined>(undefined);
	const [checked, setChecked] = useState(false);
	if (propose && !checked) {
		setChecked(true);
		void cmd.gitCheckProposed(lines).then(setReasons);
	}
	const runnable =
		propose && lines.length > 0 && reasons !== undefined && reasons.every((r) => r === null);

	async function run() {
		try {
			const results = await cmd.gitRunProposed(dir, lines);
			setRan(results.map((r) => `${r.command}: ${r.ok ? 'ok' : r.stderr}`).join('\n'));
		} catch (e) {
			setRan(String((e as { message?: unknown }).message ?? e));
		}
		void queryClient.invalidateQueries({
			predicate: (q) => String(q.queryKey[0]).startsWith('git-'),
		});
	}

	return (
		<div data-testid="agent-popover">
			<DialogHeader>
				<DialogTitle>{title}</DialogTitle>
			</DialogHeader>
			{propose ? (
				<div className="my-3 flex flex-col gap-2 text-sm">
					{explanation && <p>{explanation}</p>}
					<pre data-testid="proposed-lines" className="whitespace-pre-wrap font-mono text-xs">
						{lines.join('\n') || 'The agent proposed no command.'}
					</pre>
					{reasons?.some((r) => r !== null) && (
						<p className="text-destructive text-xs">
							Not runnable here: {reasons.find((r) => r !== null)}
						</p>
					)}
					{ran && <pre className="whitespace-pre-wrap font-mono text-xs">{ran}</pre>}
				</div>
			) : (
				<pre
					data-testid="agent-answer"
					className="my-3 max-h-96 overflow-auto whitespace-pre-wrap text-sm"
				>
					{text}
				</pre>
			)}
			<DialogFooter>
				{propose && (
					<>
						{runnable && !ran && <Button onClick={() => void run()}>Run</Button>}
						<Button
							variant="outline"
							onClick={() => {
								onClose();
								void startAgent(projectId, dir, title, request);
							}}
						>
							Open in a session
						</Button>
					</>
				)}
				<Button variant="outline" onClick={onClose}>
					{propose ? 'Cancel' : 'Close'}
				</Button>
			</DialogFooter>
		</div>
	);
}
