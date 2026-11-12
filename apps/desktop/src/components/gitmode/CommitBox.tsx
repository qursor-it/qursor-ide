import { Button, Checkbox, Input, Textarea } from '@factorai/ui';
import { useState } from 'react';
import { useGitOp } from './useGitOp';
import { useQuickAgent } from './useQuickAgent';

/** Subject, body, amend and the commit button (F35). */
export function CommitBox({
	dir,
	branch,
	staged,
}: {
	dir: string;
	branch: string | null;
	staged: number;
}) {
	const { run, running } = useGitOp(dir);
	const [subject, setSubject] = useState('');
	const [body, setBody] = useState('');
	const [amend, setAmend] = useState(false);
	const quick = useQuickAgent(dir);
	const [written, setWritten] = useState(false);
	const canCommit = !running && subject.trim() !== '' && (staged > 0 || amend);

	async function writeMessage() {
		const text = await quick.ask('Writing the commit message', { kind: 'commitMessage' });
		if (!text) return;
		const [first = '', ...rest] = text.split('\n');
		setSubject(first.trim());
		setBody(rest.join('\n').trim());
		setWritten(true);
	}

	async function commit() {
		const message = subject.trim() + (body.trim() ? `\n\n${body.trim()}` : '');
		const result = await run({ kind: 'commit', message, amend });
		if (result?.ok) {
			setSubject('');
			setBody('');
			setAmend(false);
			setWritten(false);
		}
	}

	return (
		<div
			data-testid="commit-box"
			className="flex shrink-0 flex-col gap-1.5 border-t border-border p-2"
		>
			<Input
				aria-label="Commit subject"
				placeholder="Subject"
				value={subject}
				onChange={(e) => setSubject(e.target.value)}
			/>
			<Textarea
				aria-label="Commit body"
				placeholder="Body (optional)"
				value={body}
				onChange={(e) => setBody(e.target.value)}
				className="min-h-16"
			/>
			<div className="flex items-center gap-1.5 text-secondary-foreground text-xs">
				<Checkbox id="git-amend" checked={amend} onCheckedChange={(v) => setAmend(v === true)} />
				<label htmlFor="git-amend">Amend last commit</label>
			</div>
			<div className="flex gap-1.5">
				{quick.enabled && (
					<Button
						variant="outline"
						size="sm"
						disabled={quick.busy || staged === 0}
						title="Draft the message from the staged diff"
						onClick={() => void writeMessage()}
					>
						{written ? '✦ Regenerate' : '✦ Write message'}
					</Button>
				)}
				<Button
					size="sm"
					className="min-w-0 flex-1"
					disabled={!canCommit}
					onClick={() => void commit()}
				>
					<span className="truncate">{branch ? `Commit on ${branch}` : 'Commit'}</span>
				</Button>
			</div>
		</div>
	);
}
