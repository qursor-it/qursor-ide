import type { ContentBlock, SessionEvent } from '@factorai/types';
import { describe, expect, it } from 'vitest';
import { agentChecks, lastAssistantText } from './agentSummary';

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

describe('agentChecks', () => {
	const call = (id: string, command: string): SessionEvent => ({
		type: 'assistant',
		message: {
			role: 'assistant',
			content: [{ type: 'tool_use', id, name: 'Bash', input: { command } }],
		},
	});
	const result = (id: string, isError = false): SessionEvent => ({
		type: 'user',
		message: {
			role: 'user',
			content: [{ type: 'tool_result', tool_use_id: id, content: 'out', is_error: isError }],
		},
	});

	it('keeps the checks, not the work, with whether each passed', () => {
		const checks = agentChecks([
			call('a', 'ls src'),
			result('a'),
			call('b', 'pnpm test webhooks'),
			result('b', true),
			call('c', 'pnpm typecheck'),
			result('c'),
			call('d', 'cargo clippy -- -D warnings'),
		]);
		expect(checks).toEqual([
			{ command: 'pnpm test webhooks', ok: false },
			{ command: 'pnpm typecheck', ok: true },
			{ command: 'cargo clippy -- -D warnings', ok: null },
		]);
	});

	it('shows the latest run of a command once', () => {
		const checks = agentChecks([
			call('a', 'pnpm test'),
			result('a', true),
			call('b', 'pnpm test'),
			result('b'),
		]);
		expect(checks).toEqual([{ command: 'pnpm test', ok: true }]);
	});
});
