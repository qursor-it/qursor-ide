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
