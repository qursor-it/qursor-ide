/** The line pasted ahead of the user's text so the agent knows which file and nodes (F44). */
export function buildContextLine(absPath: string, selectedIds: string[]): string {
	const selected = selectedIds.length ? selectedIds.join(', ') : 'none';
	return `[canvas: ${absPath} · selected nodes: ${selected} · JSON Canvas 1.0; mark nodes you add with "x-author":"agent"; keep fields you don't know]`;
}

export const buildMessage = (absPath: string, selectedIds: string[], text: string): string =>
	`${buildContextLine(absPath, selectedIds)}\n${text}`;
