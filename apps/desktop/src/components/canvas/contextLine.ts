/** The line pasted ahead of the user's text so the agent knows which file and nodes (F44). */
export function buildContextLine(absPath: string, selectedIds: string[]): string {
	const selected = selectedIds.length ? selectedIds.join(', ') : 'none';
	return `[canvas: ${absPath} · selected nodes: ${selected} · JSON Canvas 1.0; mark nodes you add with "x-author":"agent"; use colour presets "1"-"6", not hex; prefer the canvas* tools over reading or rewriting the whole file; keep fields you don't know]`;
}

export const buildMessage = (absPath: string, selectedIds: string[], text: string): string =>
	`${buildContextLine(absPath, selectedIds)}\n${text}`;

/** What the agent is told once per conversation: the canvas is shared and the
 *  factorai tools are the way to work on it (F44, ADR-0104). */
export const CANVAS_RULES =
	'This canvas is our shared whiteboard for this conversation. Work on it with the factorai canvas tools: canvasOutline for its sections, canvasRead for a group, the loose cards ({loose:true}) or given nodes, canvasFreeSpot to place new content, canvasPatch for targeted edits. Do not read or rewrite the whole file. My later messages may come without the [canvas] line: keep using this canvas.';

/** The first message of a session the canvas starts: context, rules, then the text. */
export const buildFirstMessage = (absPath: string, selectedIds: string[], text: string): string =>
	`${buildContextLine(absPath, selectedIds)}
${CANVAS_RULES}

${text}`;

/** Sent when the canvas links or resumes a session it has not briefed in this run,
 *  so text typed straight into the terminal (which cannot be prefixed) keeps the context. */
export const buildPrimer = (absPath: string): string =>
	`${buildContextLine(absPath, [])}
${CANVAS_RULES} Reply with one short line.`;
