import type { CommentableLines, DiffSide, PrComment } from '@factorai/types';

/**
 * Where a review comment may anchor (specs/05-features.md F31, P3). GitHub only
 * accepts a comment on a line the pull request's diff shows, and answers any
 * other with a 422 after the whole review has been written, so the diff editor
 * asks here before it opens a composer. The ranges come from Rust
 * (`CommentableLines`), already merged hunk by hunk.
 */

function rangesOf(lines: CommentableLines, side: DiffSide): [number, number][] {
	return side === 'LEFT' ? lines.left : lines.right;
}

/** Whether `line` of `side` is shown by the diff. */
export function isCommentable(lines: CommentableLines, side: DiffSide, line: number): boolean {
	return rangesOf(lines, side).some(([start, end]) => line >= start && line <= end);
}

/** Whether a multi-line comment from `start` to `end` of `side` fits in **one**
 *  hunk range. GitHub refuses a range that spans two hunks, so two commentable
 *  lines with a gap between them are not a commentable range. */
export function rangeCommentable(
	lines: CommentableLines,
	side: DiffSide,
	start: number,
	end: number,
): boolean {
	if (start > end) return false;
	return rangesOf(lines, side).some(([from, to]) => start >= from && end <= to);
}

/** The key one pull request's review draft is stored under. */
export function draftKey(slug: string, number: number): string {
	return `${slug}#${number}`;
}

/** One conversation on GitHub that the diff can show: a root comment, then its
 *  replies in the order they were written. */
interface AnchoredThread {
	side: DiffSide;
	/** The last line of the root comment. */
	line: number;
	comments: PrComment[];
}

/**
 * The threads of `path` to draw in its diff. A thread whose line is gone from
 * the latest diff (`outdated`) has nowhere to be drawn and is left out; replies
 * follow their root, and a reply whose root is missing stands as its own thread
 * rather than vanishing.
 */
export function threadsFor(comments: PrComment[], path: string): AnchoredThread[] {
	const here = comments.filter((c) => c.path === path);
	const ids = new Set(here.map((c) => c.id));
	const replies = new Map<number, PrComment[]>();
	const roots: PrComment[] = [];
	for (const comment of here) {
		if (comment.inReplyTo !== null && ids.has(comment.inReplyTo)) {
			const list = replies.get(comment.inReplyTo) ?? [];
			list.push(comment);
			replies.set(comment.inReplyTo, list);
		} else {
			roots.push(comment);
		}
	}
	const threads: AnchoredThread[] = [];
	for (const root of roots) {
		if (root.outdated || root.line === null || root.side === null) continue;
		const rest = (replies.get(root.id) ?? []).sort((a, b) => a.createdAt - b.createdAt);
		threads.push({ side: root.side, line: root.line, comments: [root, ...rest] });
	}
	return threads;
}
