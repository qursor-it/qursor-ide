/** The line pasted ahead of the user's text so the agent knows which file and nodes (F44). */
export function buildContextLine(absPath: string, selectedIds: string[]): string {
	const selected = selectedIds.length ? selectedIds.join(', ') : 'none';
	return `[canvas: ${absPath} · selected nodes: ${selected} · JSON Canvas 1.0; mark nodes you add with "x-author":"agent"; keep fields you don't know]`;
}

export const buildMessage = (absPath: string, selectedIds: string[], text: string): string =>
	`${buildContextLine(absPath, selectedIds)}\n${text}`;

/** Sent once when an existing session is linked, so text typed straight into its
 *  terminal (which the canvas cannot prefix) still has the canvas in context. */
export const buildPrimer = (absPath: string): string =>
	`${buildContextLine(absPath, [])}
This canvas is now our shared whiteboard for this conversation. Read it before answering anything about it, and put your ideas on it as nodes. My later messages may come without this line: keep using the same file. Reply with one short line.`;
