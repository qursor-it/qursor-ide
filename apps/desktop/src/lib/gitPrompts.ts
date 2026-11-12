/** The first message of an agent session started from Git mode (F35). */
export function resolveConflictsPrompt(args: {
	op: 'merge' | 'rebase' | 'cherryPick' | 'revert';
	current: string;
	theirs: string;
	files: string[];
}): string {
	const { op, current, theirs, files } = args;
	const heads = {
		merge: `A merge of ${theirs} into ${current} stopped with conflicts.`,
		rebase: `A rebase of ${current} onto ${theirs} stopped with conflicts.`,
		cherryPick: `A cherry-pick of ${theirs} onto ${current} stopped with conflicts.`,
		revert: `A revert of ${theirs} on ${current} stopped with conflicts.`,
	};
	const tails = {
		merge: 'Do NOT create the merge commit; the user confirms it.',
		rebase: 'Continue the rebase (`git rebase --continue`) until it finishes.',
		cherryPick: 'Do NOT commit; the user continues the cherry-pick.',
		revert: 'Do NOT commit; the user continues the revert.',
	};
	const head = heads[op];
	const tail = tails[op];
	return [
		head,
		'Conflicted files:',
		...files.map((f) => `- ${f}`),
		'Resolve every conflict keeping the intent of both sides, run the tests the touched files have, and `git add` each resolved file.',
		tail,
	].join('\n');
}
