import { describe, expect, it } from 'vitest';
import type { CommentableLines, PrComment } from '@factorai/types';
import { draftKey, isCommentable, rangeCommentable, threadsFor } from './reviewAnchors';

const lines: CommentableLines = {
	left: [[5, 8]],
	right: [
		[1, 4],
		[20, 22],
	],
};

describe('isCommentable', () => {
	it('accepts the ends and the inside of a range, on the side it belongs to', () => {
		expect(isCommentable(lines, 'RIGHT', 1)).toBe(true);
		expect(isCommentable(lines, 'RIGHT', 4)).toBe(true);
		expect(isCommentable(lines, 'RIGHT', 21)).toBe(true);
		expect(isCommentable(lines, 'LEFT', 5)).toBe(true);
	});

	it('refuses a gap, a line past the end and the other side', () => {
		expect(isCommentable(lines, 'RIGHT', 5)).toBe(false);
		expect(isCommentable(lines, 'RIGHT', 23)).toBe(false);
		expect(isCommentable(lines, 'LEFT', 1)).toBe(false);
		expect(isCommentable(lines, 'RIGHT', 0)).toBe(false);
	});

	it('refuses everything when there is nothing to comment on', () => {
		expect(isCommentable({ left: [], right: [] }, 'RIGHT', 1)).toBe(false);
	});
});

describe('rangeCommentable', () => {
	it('accepts a range inside one hunk range', () => {
		expect(rangeCommentable(lines, 'RIGHT', 2, 4)).toBe(true);
		expect(rangeCommentable(lines, 'RIGHT', 20, 22)).toBe(true);
	});

	it('refuses a range that bridges two hunks even when both ends are commentable', () => {
		expect(isCommentable(lines, 'RIGHT', 4)).toBe(true);
		expect(isCommentable(lines, 'RIGHT', 20)).toBe(true);
		expect(rangeCommentable(lines, 'RIGHT', 4, 20)).toBe(false);
	});

	it('refuses a range that runs off its hunk and an inverted one', () => {
		expect(rangeCommentable(lines, 'RIGHT', 3, 5)).toBe(false);
		expect(rangeCommentable(lines, 'RIGHT', 4, 2)).toBe(false);
		expect(rangeCommentable(lines, 'LEFT', 5, 9)).toBe(false);
	});
});

describe('draftKey', () => {
	it('names a pull request by repository and number', () => {
		expect(draftKey('github.com/o/r', 7)).toBe('github.com/o/r#7');
	});
});

function comment(over: Partial<PrComment>): PrComment {
	return {
		id: 1,
		author: 'a',
		body: 'b',
		path: 'src/a.ts',
		line: 10,
		side: 'RIGHT',
		startLine: null,
		startSide: null,
		commitOid: 'c'.repeat(40),
		createdAt: 100,
		inReplyTo: null,
		url: 'u',
		outdated: false,
		...over,
	};
}

describe('threadsFor', () => {
	it('groups replies under their root, oldest first, for the one path', () => {
		const threads = threadsFor(
			[
				comment({ id: 2, inReplyTo: 1, createdAt: 300 }),
				comment({ id: 1 }),
				comment({ id: 3, inReplyTo: 1, createdAt: 200 }),
				comment({ id: 4, path: 'other.ts' }),
				comment({ id: 5, line: 3, side: 'LEFT' }),
			],
			'src/a.ts',
		);
		expect(threads).toHaveLength(2);
		expect(threads[0]).toMatchObject({ side: 'RIGHT', line: 10 });
		expect(threads[0].comments.map((c) => c.id)).toEqual([1, 3, 2]);
		expect(threads[1]).toMatchObject({ side: 'LEFT', line: 3 });
	});

	it('leaves out a thread whose line is gone from the diff', () => {
		const threads = threadsFor(
			[
				comment({ id: 1, line: null, outdated: true }),
				comment({ id: 2, inReplyTo: 1, line: null, outdated: true }),
			],
			'src/a.ts',
		);
		expect(threads).toEqual([]);
	});

	it('keeps a reply whose root is missing as a thread of its own', () => {
		const threads = threadsFor([comment({ id: 7, inReplyTo: 99 })], 'src/a.ts');
		expect(threads.map((t) => t.comments.map((c) => c.id))).toEqual([[7]]);
	});
});
