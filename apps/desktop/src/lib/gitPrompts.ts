/** The first message of an agent session started from Git mode (F35). */
export function resolveConflictsPrompt(args: {
	op: 'merge' | 'rebase';
	current: string;
	theirs: string;
	files: string[];
}): string {
	const { op, current, theirs, files } = args;
	const head =
		op === 'merge'
			? `A merge of ${theirs} into ${current} stopped with conflicts.`
			: `A rebase of ${current} onto ${theirs} stopped with conflicts.`;
	const tail =
		op === 'merge'
			? 'Do NOT create the merge commit; the user confirms it.'
			: 'Continue the rebase (`git rebase --continue`) until it finishes.';
	return [
		head,
		'Conflicted files:',
		...files.map((f) => `- ${f}`),
		'Resolve every conflict keeping the intent of both sides, run the tests the touched files have, and `git add` each resolved file.',
		tail,
	].join('\n');
}
