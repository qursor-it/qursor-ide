import { describe, expect, it } from 'vitest';
import { queryKeys } from './queryKeys';

describe('queryKeys', () => {
	it('projects() is a stable tuple', () => {
		expect(queryKeys.projects()).toEqual(['projects']);
	});

	it('sessions() includes the project id', () => {
		expect(queryKeys.sessions('-Users-alice-foo')).toEqual(['sessions', '-Users-alice-foo']);
	});

	it('session() includes id, offset, limit', () => {
		expect(queryKeys.session('abc-123', 0, 500)).toEqual(['session', 'abc-123', 0, 500]);
	});

	it('different offsets produce different keys', () => {
		const a = queryKeys.session('abc', 0, 500);
		const b = queryKeys.session('abc', 500, 500);
		expect(a).not.toEqual(b);
	});

	it('sessionTail() includes id and limit', () => {
		expect(queryKeys.sessionTail('abc', 100)).toEqual(['session-tail', 'abc', 100]);
	});

	it('sessionTail key is distinct from session key', () => {
		expect(queryKeys.sessionTail('abc', 100)).not.toEqual(queryKeys.session('abc', 0, 100));
	});

	it('dir() keys per absolute path', () => {
		expect(queryKeys.dir('/a/b')).toEqual(['dir', '/a/b']);
		expect(queryKeys.dir('/a/b')).not.toEqual(queryKeys.dir('/a/c'));
	});

	it('file() separates capped from uncapped reads', () => {
		expect(queryKeys.file('/a/b.ts', false)).toEqual(['file', '/a/b.ts', 'capped']);
		expect(queryKeys.file('/a/b.ts', true)).not.toEqual(queryKeys.file('/a/b.ts', false));
	});

	it('ghPrSide() is content-addressed and distinct per side and path', () => {
		const left = queryKeys.ghPrSide('/r', 7, 'a'.repeat(40), '/r/x.ts');
		const right = queryKeys.ghPrSide('/r', 7, 'b'.repeat(40), '/r/x.ts');
		expect(left).not.toEqual(right);
		expect(left[0]).toBe('gh-pr-side');
	});

	it('the PR keys do not share a prefix the refresh button could over-match', () => {
		// The tab refreshes ['gh-status'], ['gh-prs'] and ['gh-pr'] by first element;
		// files and sides are SHA-keyed and must stay out of it.
		expect(queryKeys.ghPrFiles('/r', 7, 'a')[0]).toBe('gh-pr-files');
		expect(queryKeys.ghPrSide('/r', 7, 'a', 'p')[0]).toBe('gh-pr-side');
		expect(queryKeys.ghPr('s', 7)[0]).toBe('gh-pr');
	});
	it('the intelligence keys: a project’s results share a prefix with nothing the preview uses', () => {
		expect(queryKeys.intelligenceStatus()).toEqual(['intelligence-status']);
		expect(queryKeys.intelligenceRun('p1')).toEqual(['intelligence-run', 'p1']);
		expect(queryKeys.improvements('p1')).toEqual(['improvements', 'p1']);
		// The prefixes turning the switch off drops every project's results by.
		expect(queryKeys.intelligenceRun()).toEqual(['intelligence-run']);
		expect(queryKeys.improvements()).toEqual(['improvements']);
		// A refetch changes the signals a consent was given for, so no
		// invalidation aimed at a project's results may match the preview.
		const preview = queryKeys.intelligencePreview('p1', 20);
		expect(preview[0]).toBe('intelligence-preview');
		expect(preview[0]).not.toBe(queryKeys.intelligenceRun('p1')[0]);
		expect(preview[0]).not.toBe(queryKeys.improvements('p1')[0]);
		expect(queryKeys.intelligencePreview('p1', 50)).not.toEqual(preview);
		expect(queryKeys.intelligencePrepared('c1', 'instruction', false)[0]).toBe(
			'intelligence-prepared',
		);
		expect(queryKeys.intelligencePrepared('c1', 'instruction', true)).not.toEqual(
			queryKeys.intelligencePrepared('c1', 'instruction', false),
		);
		expect(queryKeys.intelligencePrepared('c1', 'revert', false)).not.toEqual(
			queryKeys.intelligencePrepared('c1', 'instruction', false),
		);
	});
});
