import type { ContentBlock, SessionEvent } from '@factorai/types';
import { describe, expect, it } from 'vitest';
import { lastAssistantText } from './agentSummary';

const assistant = (content: string | ContentBlock[]): SessionEvent => ({
	type: 'assistant',
	message: { role: 'assistant', content },
});

describe('lastAssistantText', () => {
	it('takes the newest assistant message with words, skipping tool-only ones', () => {
		const events: SessionEvent[] = [
			assistant([{ type: 'text', text: 'Older summary.' }]),
			{ type: 'user', message: { role: 'user', content: 'go on' } },
			assistant([
				{ type: 'text', text: 'The handler wrote the invoice first.' },
				{ type: 'text', text: 'Both writes now share a transaction.' },
			]),
			assistant([{ type: 'tool_use', id: 't1', name: 'Bash', input: { command: 'pnpm test' } }]),
		];
		expect(lastAssistantText(events)).toBe(
			'The handler wrote the invoice first.\n\nBoth writes now share a transaction.',
		);
	});

	it('reads string content, and is null when the agent has said nothing', () => {
		expect(lastAssistantText([assistant('  Done.  ')])).toBe('Done.');
		expect(
			lastAssistantText([{ type: 'user', message: { role: 'user', content: 'hi' } }]),
		).toBeNull();
		expect(lastAssistantText([])).toBeNull();
	});
});
