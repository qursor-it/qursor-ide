import { describe, expect, it } from 'vitest';
import { sessionDuring } from './sessionDuring';

const s = (
	id: string,
	createdAt: number,
	updatedAt: number,
	over: Partial<{ cwd: string | null; worktree: string | null; subagentOf: string | null }> = {},
) => ({
	id,
	createdAt,
	updatedAt,
	cwd: '/repo',
	worktree: null,
	subagentOf: null,
	...over,
});

describe('sessionDuring', () => {
	it('finds the session running in the checkout at the commit time', () => {
		const found = sessionDuring(150, [s('a', 0, 100), s('b', 120, 200)], '/repo');
		expect(found?.id).toBe('b');
	});

	it('allows a minute after the last transcript entry, and no more', () => {
		expect(sessionDuring(100 + 60_000, [s('a', 0, 100)], '/repo')?.id).toBe('a');
		expect(sessionDuring(100 + 60_001, [s('a', 0, 100)], '/repo')).toBeNull();
	});

	it('takes a session started in a subdirectory, not one in another worktree or folder', () => {
		expect(sessionDuring(50, [s('a', 0, 100, { cwd: '/repo/web' })], '/repo')?.id).toBe('a');
		expect(sessionDuring(50, [s('a', 0, 100, { worktree: '/repo/.wt/x' })], '/repo')).toBeNull();
		expect(sessionDuring(50, [s('a', 0, 100, { cwd: '/repo-old' })], '/repo')).toBeNull();
	});

	it('matches a session in this worktree, and skips sub-agents', () => {
		expect(
			sessionDuring(50, [s('a', 0, 100, { worktree: '/repo/.wt/x' })], '/repo/.wt/x')?.id,
		).toBe('a');
		expect(sessionDuring(50, [s('a', 0, 100, { subagentOf: 'p' })], '/repo')).toBeNull();
	});

	it('compares Windows paths as paths', () => {
		expect(
			sessionDuring(50, [s('a', 0, 100, { cwd: 'c:\\Code\\Repo' })], 'C:\\code\\repo')?.id,
		).toBe('a');
	});

	it('prefers the session started last when two overlap', () => {
		expect(sessionDuring(50, [s('late', 40, 100), s('early', 0, 100)], '/repo')?.id).toBe('late');
	});
});
