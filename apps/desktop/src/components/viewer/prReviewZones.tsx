import type { CommentableLines, DiffSide, PrComment } from '@factorai/types';
import { toast } from '@factorai/ui';
import { type Root, createRoot } from 'react-dom/client';
import { monaco } from '@components/viewer/monaco';
import {
	ReviewZoneContent,
	type ZoneHandlers,
	type ZoneModel,
} from '@components/viewer/ReviewZoneContent';
import { isCommentable, rangeCommentable, threadsFor } from '@lib/reviewAnchors';
import { type ReviewDraftComment, useReviewDraftStore } from '@store/reviewDraftStore';

/**
 * Line comments on a pull request's diff (specs/05-features.md F31, P3).
 *
 * `DiffView` owns the Monaco diff editor; this module bolts the review onto it
 * and is the only place that knows how. It is imperative on purpose, like the
 * editor itself: created beside the editor, handed new data through `update`,
 * disposed before the editor is. Each zone is its own small React root inside a
 * Monaco view zone, so the zone's content is ordinary components and the
 * lifecycle stays one `dispose()`.
 *
 * Three ways to start a comment, one composer:
 *
 * - hover a commentable line and press the `+` in the glyph margin;
 * - `Ctrl/Cmd+Alt+M`, or "Add review comment" in the editor's context menu,
 *   on the cursor or selection;
 * - a multi-line selection makes it a multi-line comment.
 *
 * What it writes is a **draft**, in `reviewDraftStore`. Nothing reaches GitHub
 * from here; publishing is the Review tab's confirmed submit.
 */

/** What the layer needs to know about the file in front of it. */
export interface ReviewLayerInput {
	/** `draftKey(slug, number)`. */
	key: string;
	/** The pull request's head commit, which a first draft is anchored to. */
	headOid: string;
	file: { relPath: string; noPatch: boolean; commentable: CommentableLines };
	/** Every comment on the pull request; the layer picks this file's. */
	comments: PrComment[];
}

export interface PrReviewLayer {
	update: (input: ReviewLayerInput | null) => void;
	dispose: () => void;
}

const ADD_COMMENT_ACTION = 'factorai.addReviewComment';

interface Anchor {
	side: DiffSide;
	line: number;
}

interface ComposerState extends Anchor {
	startLine?: number;
}

interface Zone {
	id: string;
	side: DiffSide;
	domNode: HTMLElement;
	wrapper: HTMLElement;
	root: Root;
	observer: ResizeObserver;
	view: monaco.editor.IViewZone;
}

const anchorKey = (a: Anchor) => `${a.side}:${a.line}`;

/** The last line a selection covers: a selection that ends at column 1 of a line
 *  has not touched that line. */
function lastLineOf(selection: monaco.Selection): number {
	return selection.endColumn === 1 && selection.endLineNumber > selection.startLineNumber
		? selection.endLineNumber - 1
		: selection.endLineNumber;
}

export function attachPrReview(
	diff: monaco.editor.IStandaloneDiffEditor,
	options: { inline: boolean },
): PrReviewLayer {
	const sides: { side: DiffSide; editor: monaco.editor.IStandaloneCodeEditor }[] = [
		// In inline mode the original editor is not on screen, so a comment on the
		// left has no line to sit on: only the modified side takes comments.
		...(options.inline ? [] : [{ side: 'LEFT' as const, editor: diff.getOriginalEditor() }]),
		{ side: 'RIGHT' as const, editor: diff.getModifiedEditor() },
	];
	const editorOf = (side: DiffSide) => sides.find((s) => s.side === side)?.editor;

	let input: ReviewLayerInput | null = null;
	let composer: ComposerState | null = null;
	let disposed = false;
	const zones = new Map<string, Zone>();
	const hovers = new Map<DiffSide, monaco.editor.IEditorDecorationsCollection>();
	const hovered = new Map<DiffSide, number>();
	const disposables: { dispose: () => void }[] = [];

	const enabled = () => input !== null && !input.file.noPatch;

	const clearHover = (side: DiffSide) => {
		if (hovered.has(side)) {
			hovers.get(side)?.clear();
			hovered.delete(side);
		}
	};

	const showHover = (side: DiffSide, line: number) => {
		if (hovered.get(side) === line) return;
		hovered.set(side, line);
		hovers.get(side)?.set([
			{
				range: new monaco.Range(line, 1, line, 1),
				options: { glyphMarginClassName: 'factorai-review-add' },
			},
		]);
	};

	// ---- what each anchor shows ---------------------------------------------

	const draftsOf = (): ReviewDraftComment[] =>
		input ? (useReviewDraftStore.getState().byPr[input.key]?.comments ?? []) : [];

	function models(): Map<string, { anchor: Anchor; model: ZoneModel }> {
		const found = new Map<string, { anchor: Anchor; model: ZoneModel }>();
		if (!input) return found;
		const slot = (anchor: Anchor) => {
			const key = anchorKey(anchor);
			let entry = found.get(key);
			if (!entry) {
				entry = { anchor, model: { threads: [], drafts: [], composer: null } };
				found.set(key, entry);
			}
			return entry.model;
		};
		const shown = new Set(sides.map((s) => s.side));
		for (const thread of threadsFor(input.comments, input.file.relPath)) {
			if (shown.has(thread.side)) {
				slot({ side: thread.side, line: thread.line }).threads.push(thread.comments);
			}
		}
		for (const draft of draftsOf()) {
			if (draft.path === input.file.relPath && shown.has(draft.side)) {
				slot({ side: draft.side, line: draft.line }).drafts.push(draft);
			}
		}
		if (composer && shown.has(composer.side)) {
			slot(composer).composer = { line: composer.line, startLine: composer.startLine };
		}
		return found;
	}

	// ---- zones ----------------------------------------------------------------

	const handlersFor = (anchor: Anchor): ZoneHandlers => ({
		onSubmit: (body) => {
			const active = composer;
			if (!input || !active || anchorKey(active) !== anchorKey(anchor)) return;
			useReviewDraftStore.getState().addComment(input.key, input.headOid, {
				path: input.file.relPath,
				side: active.side,
				line: active.line,
				...(active.startLine !== undefined ? { startLine: active.startLine } : {}),
				body,
			});
			composer = null;
			sync();
		},
		onCancel: () => {
			composer = null;
			sync();
			editorOf(anchor.side)?.focus();
		},
		onRemoveDraft: (id) => {
			if (input) useReviewDraftStore.getState().removeComment(input.key, id);
		},
	});

	function relayout(zone: Zone) {
		const editor = editorOf(zone.side);
		const height = Math.ceil(zone.wrapper.getBoundingClientRect().height);
		if (!editor || height === 0 || height === zone.view.heightInPx) return;
		editor.changeViewZones((accessor) => {
			zone.view.heightInPx = height;
			accessor.layoutZone(zone.id);
		});
	}

	function addZone(anchor: Anchor, model: ZoneModel) {
		const editor = editorOf(anchor.side);
		if (!editor) return;
		const domNode = document.createElement('div');
		// Above the text layer, which is painted over the zones and would take every
		// click meant for the composer's buttons and field.
		domNode.style.zIndex = '1';
		const wrapper = document.createElement('div');
		domNode.appendChild(wrapper);
		const root = createRoot(wrapper);
		root.render(<ReviewZoneContent model={model} handlers={handlersFor(anchor)} />);
		const view: monaco.editor.IViewZone = {
			afterLineNumber: anchor.line,
			heightInPx: 60,
			domNode,
			// `suppressMouseDown` is deliberately off: despite the name, `true` makes
			// Monaco take the press itself (a selection, then `preventDefault`), and
			// the composer's field could never be clicked into.
		};
		let id = '';
		editor.changeViewZones((accessor) => {
			id = accessor.addZone(view);
		});
		const zone: Zone = {
			id,
			side: anchor.side,
			domNode,
			wrapper,
			root,
			view,
			observer: new ResizeObserver(() => relayout(zone)),
		};
		zone.observer.observe(wrapper);
		zones.set(anchorKey(anchor), zone);
	}

	function removeZone(key: string) {
		const zone = zones.get(key);
		if (!zone) return;
		zones.delete(key);
		zone.observer.disconnect();
		if (!disposed) {
			editorOf(zone.side)?.changeViewZones((accessor) => accessor.removeZone(zone.id));
		}
		// Not while React may be rendering: unmounting a root from an effect or
		// an event it is dispatching warns.
		setTimeout(() => zone.root.unmount(), 0);
	}

	function sync() {
		if (disposed) return;
		const wanted = enabled() ? models() : new Map();
		for (const key of zones.keys()) {
			if (!wanted.has(key)) removeZone(key);
		}
		for (const [key, { anchor, model }] of wanted) {
			const zone = zones.get(key);
			if (zone) {
				zone.root.render(<ReviewZoneContent model={model} handlers={handlersFor(anchor)} />);
			} else {
				addZone(anchor, model);
			}
		}
	}

	// ---- starting a comment ---------------------------------------------------

	const refuse = (message: string) => {
		toast(message);
	};

	function openComposer(next: ComposerState) {
		composer = next;
		sync();
	}

	/** From a click on the glyph (`clicked` set) or from the action (`clicked`
	 *  unset, the cursor decides). */
	function startComment(side: DiffSide, editor: monaco.editor.ICodeEditor, clicked?: number) {
		if (!input) return;
		if (input.file.noPatch) {
			refuse("This file's diff is too large for GitHub to comment on.");
			return;
		}
		const commentable = input.file.commentable;
		const selection = editor.getSelection();
		const cursor = clicked ?? selection?.getPosition().lineNumber ?? 1;
		if (selection) {
			const first = selection.startLineNumber;
			const last = lastLineOf(selection);
			const inside = clicked === undefined || (clicked >= first && clicked <= last);
			if (first < last && inside && rangeCommentable(commentable, side, first, last)) {
				openComposer({ side, line: last, startLine: first });
				return;
			}
		}
		if (!isCommentable(commentable, side, cursor)) {
			refuse("This line isn't part of the diff.");
			return;
		}
		openComposer({ side, line: cursor });
	}

	// ---- wiring ---------------------------------------------------------------

	for (const { side, editor } of sides) {
		hovers.set(side, editor.createDecorationsCollection());
		disposables.push(
			editor.onMouseMove((event) => {
				const line = event.target.position?.lineNumber;
				if (
					!enabled() ||
					!input ||
					line === undefined ||
					event.target.type === monaco.editor.MouseTargetType.UNKNOWN
				) {
					clearHover(side);
				} else if (isCommentable(input.file.commentable, side, line)) {
					showHover(side, line);
				} else {
					clearHover(side);
				}
			}),
			editor.onMouseLeave(() => clearHover(side)),
			editor.onMouseDown((event) => {
				if (event.target.type !== monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN) return;
				if (!event.event.leftButton || !enabled()) return;
				const line = event.target.position?.lineNumber;
				if (line === undefined) return;
				event.event.preventDefault();
				startComment(side, editor, line);
			}),
			editor.addAction({
				id: ADD_COMMENT_ACTION,
				label: 'Add review comment',
				keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyMod.Alt | monaco.KeyCode.KeyM],
				contextMenuGroupId: 'navigation',
				run: (focused) => startComment(side, focused),
			}),
		);
	}

	const unsubscribe = useReviewDraftStore.subscribe(() => sync());

	return {
		update(next) {
			input = next;
			// The glyph column costs the editor its width, so it is only there when a
			// comment can go in it.
			diff.updateOptions({ glyphMargin: enabled() });
			if (!enabled()) {
				for (const { side } of sides) clearHover(side);
				composer = null;
			}
			sync();
		},
		dispose() {
			disposed = true;
			unsubscribe();
			for (const item of disposables) item.dispose();
			for (const collection of hovers.values()) collection.clear();
			for (const zone of zones.values()) {
				zone.observer.disconnect();
				setTimeout(() => zone.root.unmount(), 0);
			}
			zones.clear();
		},
	};
}
