import { describe, expect, it } from 'vitest';
import {
	checksSummaryText,
	forkLabel,
	ghErrorText,
	ghInstallCommand,
	ghStatusText,
	isUnauthenticated,
	mergeableLabel,
	pickPrFile,
	prDiffMode,
	reviewDecisionLabel,
	stepPrFile,
} from './pullRequests';
import { isDiffMode, parsePrRange } from '@hooks/useFileViewer';

describe('ghInstallCommand', () => {
	it('names the package manager of the OS', () => {
		expect(ghInstallCommand('windows')).toBe('winget install GitHub.cli');
		expect(ghInstallCommand('mac')).toBe('brew install gh');
	});

	it('prints nothing where there is no single command', () => {
		expect(ghInstallCommand('other')).toBeNull();
	});
});

describe('isUnauthenticated', () => {
	it('recognises the error kind only', () => {
		expect(isUnauthenticated({ kind: 'Unauthenticated', message: 'x' })).toBe(true);
		expect(isUnauthenticated({ kind: 'Remote', message: 'x' })).toBe(false);
		expect(isUnauthenticated(new Error('x'))).toBe(false);
		expect(isUnauthenticated(null)).toBe(false);
		expect(isUnauthenticated('Unauthenticated')).toBe(false);
	});
});

describe('ghErrorText', () => {
	it("uses the remote's own words", () => {
		expect(ghErrorText({ kind: 'Remote', message: 'API rate limit exceeded' })).toBe(
			'API rate limit exceeded',
		);
	});

	it('falls back to a sentence', () => {
		expect(ghErrorText({ kind: 'Remote', message: '' })).toMatch(/GitHub/);
		expect(ghErrorText(undefined)).toMatch(/GitHub/);
	});
});

describe('ghStatusText', () => {
	it("prefers the backend's message", () => {
		expect(ghStatusText({ state: 'noOrigin', message: 'custom' })).toBe('custom');
	});

	it('explains each state that has no layout of its own', () => {
		for (const state of [
			'tooOld',
			'notRepository',
			'noOrigin',
			'notGitHub',
			'unreachable',
		] as const) {
			expect(ghStatusText({ state, message: null }).length).toBeGreaterThan(10);
		}
	});
});

describe('labels', () => {
	it('names a review decision, and nothing for none', () => {
		expect(reviewDecisionLabel('approved')).toBe('Approved');
		expect(reviewDecisionLabel('changesRequested')).toBe('Changes requested');
		expect(reviewDecisionLabel('reviewRequired')).toBe('Review required');
		expect(reviewDecisionLabel(null)).toBeNull();
	});

	it('summarises the checks, and is silent without any', () => {
		expect(
			checksSummaryText({ state: 'failure', total: 4, passed: 2, failed: 2, pending: 0 }),
		).toBe('2 of 4 passed, 2 failed');
		expect(
			checksSummaryText({ state: 'pending', total: 3, passed: 1, failed: 0, pending: 2 }),
		).toBe('1 of 3 passed, 2 running');
		expect(
			checksSummaryText({ state: 'none', total: 0, passed: 0, failed: 0, pending: 0 }),
		).toBeNull();
	});

	it('words mergeability', () => {
		expect(mergeableLabel('conflicting')).toBe('Has conflicts');
	});

	it('labels a fork by its owner, or as deleted', () => {
		expect(forkLabel({ isCrossRepository: false, headOwner: 'acme' })).toBeNull();
		expect(forkLabel({ isCrossRepository: true, headOwner: 'mallory' })).toBe('mallory');
		expect(forkLabel({ isCrossRepository: true, headOwner: null })).toBe('deleted fork');
	});
});

describe('prDiffMode', () => {
	it('builds a mode the viewer accepts and reads back', () => {
		const mode = prDiffMode(42, 'a'.repeat(40), 'b'.repeat(40));
		expect(isDiffMode(mode)).toBe(true);
		expect(parsePrRange(mode)).toEqual({
			number: 42,
			mergeBase: 'a'.repeat(40),
			head: 'b'.repeat(40),
		});
	});
});

describe('pickPrFile', () => {
	const files = [{ relPath: 'a.ts' }, { relPath: 'b.ts' }, { relPath: 'c.ts' }];

	it('takes the first wanted path that exists', () => {
		expect(pickPrFile(files, 'b.ts', 'c.ts')?.relPath).toBe('b.ts');
		expect(pickPrFile(files, 'gone.ts', 'c.ts')?.relPath).toBe('c.ts');
	});

	it('falls back to the first file, and to null for none', () => {
		expect(pickPrFile(files, 'gone.ts', undefined)?.relPath).toBe('a.ts');
		expect(pickPrFile(files)?.relPath).toBe('a.ts');
		expect(pickPrFile([], 'a.ts')).toBeNull();
	});
});

describe('stepPrFile', () => {
	const files = [{ relPath: 'a.ts' }, { relPath: 'b.ts' }, { relPath: 'c.ts' }];

	it('moves and wraps in both directions', () => {
		expect(stepPrFile(files, 'a.ts', 1)?.relPath).toBe('b.ts');
		expect(stepPrFile(files, 'c.ts', 1)?.relPath).toBe('a.ts');
		expect(stepPrFile(files, 'a.ts', -1)?.relPath).toBe('c.ts');
	});

	it('starts from the first file when the current one is unknown', () => {
		expect(stepPrFile(files, 'gone.ts', 1)?.relPath).toBe('b.ts');
		expect(stepPrFile([], 'a.ts', 1)).toBeNull();
	});
});
