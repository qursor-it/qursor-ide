import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { DiffSide, ReviewEvent } from '@factorai/types';
import { deferredLocalStorage } from '@lib/persistStorage';

/** One line comment written in the diff and not yet published. */
export interface ReviewDraftComment {
	id: string;
	/** Relative to the repository root, the new name for a renamed file. */
	path: string;
	side: DiffSide;
	/** The last line of the range. */
	line: number;
	/** The first line of a multi-line comment; absent for a single line. */
	startLine?: number;
	body: string;
	createdAt: number;
}

/** The review being written for one pull request. */
interface ReviewDraft {
	/** The head commit when the first comment was written. Line numbers refer to
	 *  this commit, so it is what the review is submitted against. */
	headOid: string;
	body: string;
	event: ReviewEvent;
	comments: ReviewDraftComment[];
}

interface ReviewDraftState {
	/** Keyed by `draftKey(slug, number)`. */
	byPr: Record<string, ReviewDraft>;
	addComment: (
		key: string,
		headOid: string,
		comment: Omit<ReviewDraftComment, 'id' | 'createdAt'>,
	) => string;
	updateComment: (key: string, id: string, body: string) => void;
	removeComment: (key: string, id: string) => void;
	setBody: (key: string, headOid: string, body: string) => void;
	setEvent: (key: string, headOid: string, event: ReviewEvent) => void;
	clear: (key: string) => void;
}

const EMPTY_COMMENTS: ReviewDraftComment[] = [];

function blank(headOid: string): ReviewDraft {
	return { headOid, body: '', event: 'COMMENT', comments: [] };
}

/** A draft with no comments, no text and the default verdict is nothing: a pull
 *  request nobody is reviewing leaves no entry behind. */
function isEmpty(draft: ReviewDraft): boolean {
	return draft.comments.length === 0 && draft.body.trim() === '' && draft.event === 'COMMENT';
}

function without(byPr: Record<string, ReviewDraft>, key: string): Record<string, ReviewDraft> {
	const rest = { ...byPr };
	delete rest[key];
	return rest;
}

function settle(
	byPr: Record<string, ReviewDraft>,
	key: string,
	draft: ReviewDraft,
): Record<string, ReviewDraft> {
	return isEmpty(draft) ? without(byPr, key) : { ...byPr, [key]: draft };
}

/**
 * Review drafts, one per pull request, kept across reloads (specs/05-features.md
 * F31, P3; ADR-0013: renderer-only state lives in localStorage, deferred).
 * Nothing here reaches GitHub - publishing is the Review tab's confirmed submit.
 */
export const useReviewDraftStore = create<ReviewDraftState>()(
	persist(
		(set, get) => ({
			byPr: {},
			addComment: (key, headOid, comment) => {
				const id = crypto.randomUUID();
				set((s) => {
					const draft = s.byPr[key] ?? blank(headOid);
					// The head is set by the first comment and kept: a later comment is
					// still written against the lines the first one saw.
					const anchored = draft.comments.length === 0 ? { ...draft, headOid } : draft;
					return {
						byPr: {
							...s.byPr,
							[key]: {
								...anchored,
								comments: [...anchored.comments, { ...comment, id, createdAt: Date.now() }],
							},
						},
					};
				});
				return id;
			},
			updateComment: (key, id, body) =>
				set((s) => {
					const draft = s.byPr[key];
					if (!draft) return s;
					return {
						byPr: {
							...s.byPr,
							[key]: {
								...draft,
								comments: draft.comments.map((c) => (c.id === id ? { ...c, body } : c)),
							},
						},
					};
				}),
			removeComment: (key, id) =>
				set((s) => {
					const draft = s.byPr[key];
					if (!draft) return s;
					return {
						byPr: settle(s.byPr, key, {
							...draft,
							comments: draft.comments.filter((c) => c.id !== id),
						}),
					};
				}),
			setBody: (key, headOid, body) =>
				set((s) => ({ byPr: settle(s.byPr, key, { ...(s.byPr[key] ?? blank(headOid)), body }) })),
			setEvent: (key, headOid, event) =>
				set((s) => ({ byPr: settle(s.byPr, key, { ...(s.byPr[key] ?? blank(headOid)), event }) })),
			clear: (key) => {
				if (get().byPr[key]) set((s) => ({ byPr: without(s.byPr, key) }));
			},
		}),
		{
			name: 'factorai.reviewDrafts',
			storage: deferredLocalStorage(),
			version: 1,
			partialize: (s) => ({ byPr: s.byPr }),
		},
	),
);

/** One pull request's draft comments; a stable empty array when there are none,
 *  so a selector returning it does not re-render on unrelated changes. */
export function selectDraftComments(key: string) {
	return (s: ReviewDraftState): ReviewDraftComment[] => s.byPr[key]?.comments ?? EMPTY_COMMENTS;
}
