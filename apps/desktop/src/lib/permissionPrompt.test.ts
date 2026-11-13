import { describe, expect, it } from 'vitest';
import { parsePermissionPrompt } from './permissionPrompt';

describe('parsePermissionPrompt', () => {
	it('reads the question and the numbered options under it', () => {
		const prompt = parsePermissionPrompt([
			'● Edit docs/install.md',
			'',
			' Do you want to make this edit to install.md?',
			' ❯ 1. Yes',
			'   2. Yes, allow all edits during this session (shift+tab)',
			'   3. No, and tell Claude what to do differently (esc)',
		]);
		expect(prompt).toEqual({
			question: 'Do you want to make this edit to install.md?',
			options: [
				{ key: '1', label: 'Yes' },
				{ key: '2', label: 'Yes, allow all edits during this session (shift+tab)' },
				{ key: '3', label: 'No, and tell Claude what to do differently' },
			],
		});
	});

	it('reads a menu drawn inside a box', () => {
		const prompt = parsePermissionPrompt([
			'│ Do you want to proceed?            │',
			'│ ❯ 1. Yes                           │',
			'│   2. No                            │',
		]);
		expect(prompt?.question).toBe('Do you want to proceed?');
		expect(prompt?.options.map((o) => o.key)).toEqual(['1', '2']);
	});

	it('is null for an ordinary question or a numbered list in prose', () => {
		expect(parsePermissionPrompt(['Which port should Caddy expose?', '> '])).toBeNull();
		expect(parsePermissionPrompt(['Steps:', '1. Install', '2. Run'])).toBeNull();
	});

	it('only offers the last menu, not one still in the scrollback', () => {
		const prompt = parsePermissionPrompt([
			'Do you want to create a.ts?',
			'❯ 1. Yes',
			'  2. No',
			'● Created a.ts',
			'Do you want to proceed?',
			'❯ 1. Yes',
			'  2. No',
		]);
		expect(prompt?.question).toBe('Do you want to proceed?');
		expect(prompt?.options).toHaveLength(2);
	});
});
