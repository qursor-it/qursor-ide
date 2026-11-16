/** A numbered choice the agent is asking you to make in its terminal. */
export interface PermissionPrompt {
	question: string;
	options: Array<{ key: string; label: string }>;
}

const QUESTION = /^\s*(?:│\s*)?(Do you want to .+\?)\s*(?:│\s*)?$/;
const OPTION = /^\s*(?:│\s*)?(?:[❯>›]\s*)?(\d)\.\s+(.+?)\s*(?:│\s*)?$/;

/**
 * The permission menu Claude Code draws when it asks before acting (F39):
 * a "Do you want to …?" line followed by numbered options. Read off the last
 * lines of the terminal; null when they are not that menu, so the caller falls
 * back to "answer in the terminal" rather than guessing at keys.
 *
 * Only the last question counts, and only options under it, so an earlier menu
 * still in the scrollback is never offered again.
 */
export function parsePermissionPrompt(lines: readonly string[]): PermissionPrompt | null {
	let at = -1;
	for (let i = lines.length - 1; i >= 0; i--) {
		if (QUESTION.test(lines[i])) {
			at = i;
			break;
		}
	}
	if (at < 0) return null;
	const question = lines[at].match(QUESTION)?.[1] ?? '';
	const options: PermissionPrompt['options'] = [];
	for (const line of lines.slice(at + 1)) {
		const m = line.match(OPTION);
		if (m) options.push({ key: m[1], label: m[2].replace(/\s*\(esc\)$/i, '') });
	}
	// One option is not a choice; and the menu always starts at 1.
	return options.length >= 2 && options[0].key === '1' ? { question, options } : null;
}
