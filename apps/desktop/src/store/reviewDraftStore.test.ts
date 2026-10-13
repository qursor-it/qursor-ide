import { beforeEach, describe, expect, it } from 'vitest';
import { selectDraftComments, useReviewDraftStore } from './reviewDraftStore';

const KEY = 'github.com/o/r#7';
const HEAD_A = 'a'.repeat(40);
const HEAD_B = 'b'.repeat(40);

const comment = (line: number) => ({
	path: 'src/a.ts',
	side: 'RIGHT' as const,
	line,
	body: `note ${line}`,
});

beforeEach(() => {
	useReviewDraftStore.setState({ byPr: {} });
});

describe('reviewDraftStore', () => {
	it('starts a draft on the first comment and records the head it was written against', () => {
		useReviewDraftStore.getState().addComment(KEY, HEAD_A, comment(3));
		const draft = useReviewDraftStore.getState().byPr[KEY];
		expect(draft.headOid).toBe(HEAD_A);
		expect(draft.event).toBe('COMMENT');
		expect(draft.comments).toHaveLength(1);
		expect(draft.comments[0].id).toBeTruthy();
	});

	it('keeps the original head when a later comment arrives after a force-push', () => {
		const { addComment } = useReviewDraftStore.getState();
		addComment(KEY, HEAD_A, comment(3));
		addComment(KEY, HEAD_B, comment(9));
		const draft = useReviewDraftStore.getState().byPr[KEY];
		expect(draft.headOid).toBe(HEAD_A);
		expect(draft.comments.map((c) => c.line)).toEqual([3, 9]);
	});

	it('keeps one draft per pull request', () => {
		const { addComment } = useReviewDraftStore.getState();
		addComment(KEY, HEAD_A, comment(3));
		addComment('github.com/o/r#8', HEAD_B, comment(4));
		expect(Object.keys(useReviewDraftStore.getState().byPr).sort()).toEqual([
			'github.com/o/r#7',
			'github.com/o/r#8',
		]);
	});

	it('edits and removes a comment, and forgets a draft nothing is left of', () => {
		const { addComment, updateComment, removeComment } = useReviewDraftStore.getState();
		const id = addComment(KEY, HEAD_A, comment(3));
		updateComment(KEY, id, 'reworded');
		expect(useReviewDraftStore.getState().byPr[KEY].comments[0].body).toBe('reworded');
		removeComment(KEY, id);
		expect(useReviewDraftStore.getState().byPr[KEY]).toBeUndefined();
	});

	it('keeps a draft that still has review text or a verdict after its last comment goes', () => {
		const { addComment, removeComment, setBody, setEvent } = useReviewDraftStore.getState();
		const id = addComment(KEY, HEAD_A, comment(3));
		setBody(KEY, HEAD_A, 'overall fine');
		removeComment(KEY, id);
		expect(useReviewDraftStore.getState().byPr[KEY].body).toBe('overall fine');
		setBody(KEY, HEAD_A, '');
		expect(useReviewDraftStore.getState().byPr[KEY]).toBeUndefined();
		setEvent(KEY, HEAD_A, 'APPROVE');
		expect(useReviewDraftStore.getState().byPr[KEY].event).toBe('APPROVE');
	});

	it('sets text and a verdict before any comment exists', () => {
		const { setBody, setEvent } = useReviewDraftStore.getState();
		setBody(KEY, HEAD_A, 'LGTM');
		setEvent(KEY, HEAD_A, 'REQUEST_CHANGES');
		const draft = useReviewDraftStore.getState().byPr[KEY];
		expect(draft).toMatchObject({ body: 'LGTM', event: 'REQUEST_CHANGES', headOid: HEAD_A });
	});

	it('clears one pull request and leaves the others', () => {
		const { addComment, clear } = useReviewDraftStore.getState();
		addComment(KEY, HEAD_A, comment(3));
		addComment('github.com/o/r#8', HEAD_A, comment(4));
		clear(KEY);
		expect(Object.keys(useReviewDraftStore.getState().byPr)).toEqual(['github.com/o/r#8']);
		clear('nothing');
		expect(Object.keys(useReviewDraftStore.getState().byPr)).toEqual(['github.com/o/r#8']);
	});

	it('hands out one stable empty list for a pull request with no draft', () => {
		const select = selectDraftComments(KEY);
		const a = select(useReviewDraftStore.getState());
		const b = select(useReviewDraftStore.getState());
		expect(a).toEqual([]);
		expect(a).toBe(b);
	});
});
