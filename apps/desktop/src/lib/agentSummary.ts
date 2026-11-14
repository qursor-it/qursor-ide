import type { SessionEvent } from '@factorai/types';

/**
 * What the agent last said in words (F43): the text of the newest assistant
 * message that has any, its text blocks joined. Tool calls and results are not
 * words; null when no assistant message has text.
 */
export function lastAssistantText(events: readonly SessionEvent[]): string | null {
	for (let i = events.length - 1; i >= 0; i--) {
		const message = events[i].message;
		if (!message || message.role !== 'assistant') continue;
		const text =
			typeof message.content === 'string'
				? message.content
				: message.content.flatMap((b) => (b.type === 'text' ? [b.text] : [])).join('\n\n');
		if (text.trim()) return text.trim();
	}
	return null;
}

/** A check the agent ran (F43): the command, and whether it passed — null
 *  while its result has not come back. */
export interface AgentCheck {
	command: string;
	ok: boolean | null;
}

/** Commands that are a check rather than work: tests, linters, type checks. */
const CHECK =
	/\b(test|tests|vitest|jest|pytest|playwright|e2e|lint|eslint|oxlint|clippy|tsc|typecheck|cargo (check|test|clippy)|go (test|vet)|mypy|ruff)\b/;

/**
 * The checks the agent ran, read from its Bash tool calls and their results
 * (F43): each command that looks like a test, a lint or a type check, with
 * whether its result was an error. The latest run of a command wins; at most
 * `limit`, newest last.
 */
export function agentChecks(events: readonly SessionEvent[], limit = 6): AgentCheck[] {
	const byId = new Map<string, AgentCheck>();
	const order: string[] = [];
	for (const event of events) {
		const content = event.message?.content;
		if (!Array.isArray(content)) continue;
		for (const block of content as Array<Record<string, unknown>>) {
			if (block.type === 'tool_use' && block.name === 'Bash') {
				const command = (block.input as { command?: unknown } | undefined)?.command;
				if (typeof command === 'string' && typeof block.id === 'string' && CHECK.test(command)) {
					byId.set(block.id, { command: command.trim(), ok: null });
					order.push(block.id);
				}
			} else if (block.type === 'tool_result' && typeof block.tool_use_id === 'string') {
				const check = byId.get(block.tool_use_id);
				if (check) check.ok = block.is_error !== true;
			}
		}
	}
	const latest = new Map<string, AgentCheck>();
	for (const id of order) {
		const check = byId.get(id);
		if (!check) continue;
		latest.delete(check.command);
		latest.set(check.command, check);
	}
	return [...latest.values()].slice(-limit);
}
