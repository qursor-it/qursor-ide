import { describe, expect, it } from 'vitest';
import { resolveConflictsPrompt } from './gitPrompts';

const base = { current: 'main', theirs: 'feat', files: ['a.ts', 'src/b.ts'] };

describe('resolveConflictsPrompt', () => {
	it('merge_prompt_forbids_the_merge_commit', () => {
		const p = resolveConflictsPrompt({ ...base, op: 'merge' });
		expect(p).toContain('Do NOT create the merge commit');
		expect(p).not.toContain('rebase --continue');
	});

	it('rebase_prompt_asks_to_continue', () => {
		const p = resolveConflictsPrompt({ ...base, op: 'rebase' });
		expect(p).toContain('git rebase --continue');
		expect(p).not.toContain('Do NOT create');
	});

	it('prompt_lists_every_file', () => {
		const p = resolveConflictsPrompt({ ...base, op: 'merge' });
		expect(p).toContain('- a.ts');
		expect(p).toContain('- src/b.ts');
		expect(p).toContain('main');
		expect(p).toContain('feat');
	});

	it('prompt_never_mentions_co_authored_by', () => {
		for (const op of ['merge', 'rebase'] as const) {
			expect(resolveConflictsPrompt({ ...base, op }).toLowerCase()).not.toContain('co-authored');
		}
	});
});
