import type { PrComment } from '@factorai/types';
import { Button, IconButton, Textarea } from '@factorai/ui';
import { X } from 'lucide-react';
import { type KeyboardEvent, useEffect, useRef, useState } from 'react';
import { formatRelative } from '@lib/format';
import type { ReviewDraftComment } from '@store/reviewDraftStore';

/**
 * What one review zone in the diff shows (specs/05-features.md F31, P3): the
 * conversation already on GitHub at a line, the reader's own pending comments,
 * and the composer when they are writing one. Rendered into a Monaco view zone
 * by `prReviewZones`, which owns the zone and its height.
 *
 * **Bodies are plain text.** A comment is another person's words; nothing in it
 * is ever interpreted as markup, the way the pull request's own body is not.
 */

export interface ZoneModel {
	/** Existing threads, each a root comment followed by its replies. */
	threads: PrComment[][];
	drafts: ReviewDraftComment[];
	composer: { line: number; startLine?: number } | null;
}

export interface ZoneHandlers {
	onSubmit: (body: string) => void;
	onCancel: () => void;
	onRemoveDraft: (id: string) => void;
}

function linesLabel(line: number, startLine: number | null | undefined): string {
	return startLine != null && startLine < line ? `Lines ${startLine}–${line}` : `Line ${line}`;
}

export function ReviewZoneContent({
	model,
	handlers,
}: {
	model: ZoneModel;
	handlers: ZoneHandlers;
}) {
	return (
		<div
			className="flex flex-col gap-1.5 py-1 pr-3 pl-2 font-sans text-foreground text-xs leading-normal"
			data-testid="review-zone"
		>
			{model.threads.map((thread) => (
				<Thread key={thread[0].id} thread={thread} />
			))}
			{model.drafts.map((draft) => (
				<Draft key={draft.id} draft={draft} onRemove={() => handlers.onRemoveDraft(draft.id)} />
			))}
			{model.composer && (
				<Composer
					line={model.composer.line}
					startLine={model.composer.startLine}
					onSubmit={handlers.onSubmit}
					onCancel={handlers.onCancel}
				/>
			)}
		</div>
	);
}

function Thread({ thread }: { thread: PrComment[] }) {
	const [root] = thread;
	return (
		<div
			className="flex flex-col divide-y divide-border rounded-md border border-border bg-card"
			data-testid="review-thread"
		>
			{thread.map((comment) => (
				<div key={comment.id} className="flex flex-col gap-0.5 px-2 py-1.5">
					<p className="flex items-center gap-1.5 text-muted-foreground">
						<span className="font-medium text-foreground">{comment.author}</span>
						<span title={new Date(comment.createdAt).toLocaleString()}>
							{formatRelative(comment.createdAt)}
						</span>
						{comment === root && (
							<span className="text-muted-foreground/60">
								{linesLabel(comment.line ?? 0, comment.startLine)}
							</span>
						)}
					</p>
					<p className="whitespace-pre-wrap break-words" data-testid="review-comment-body">
						{comment.body}
					</p>
				</div>
			))}
		</div>
	);
}

function Draft({ draft, onRemove }: { draft: ReviewDraftComment; onRemove: () => void }) {
	return (
		<div
			className="flex flex-col gap-0.5 rounded-md border border-primary/40 bg-card px-2 py-1.5"
			data-testid="review-draft"
		>
			<p className="flex items-center gap-1.5 text-muted-foreground">
				<span className="rounded bg-primary/15 px-1 font-medium text-primary">Pending</span>
				<span>{linesLabel(draft.line, draft.startLine)}</span>
				<span className="flex-1" />
				<IconButton
					aria-label="Remove pending comment"
					title="Remove"
					data-testid="review-draft-remove"
					onClick={onRemove}
				>
					<X />
				</IconButton>
			</p>
			<p className="whitespace-pre-wrap break-words">{draft.body}</p>
		</div>
	);
}

function Composer({
	line,
	startLine,
	onSubmit,
	onCancel,
}: {
	line: number;
	startLine?: number;
	onSubmit: (body: string) => void;
	onCancel: () => void;
}) {
	const [text, setText] = useState('');
	const ref = useRef<HTMLTextAreaElement>(null);

	// The zone is attached to the editor's DOM on Monaco's next frame, and an
	// element that is not in the document cannot take focus. Try again until it
	// does, for a bounded number of frames.
	useEffect(() => {
		let frame = 0;
		let tries = 0;
		const focus = () => {
			ref.current?.focus();
			if (document.activeElement !== ref.current && tries++ < 30) {
				frame = requestAnimationFrame(focus);
			}
		};
		focus();
		return () => cancelAnimationFrame(frame);
	}, []);

	const ready = text.trim() !== '';
	const submit = () => {
		if (ready) onSubmit(text.trim());
	};
	const onKeyDown = (event: KeyboardEvent) => {
		// **Nothing typed here reaches Monaco.** Its keybinding service listens on
		// the editor's DOM node, which contains this one, and would run `Ctrl+F`,
		// `Ctrl+Z` and the rest against the file behind the box.
		event.stopPropagation();
		if (event.key === 'Escape') {
			event.preventDefault();
			onCancel();
		} else if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
			event.preventDefault();
			submit();
		}
	};

	return (
		<div
			className="flex flex-col gap-1.5 rounded-md border border-input bg-card p-2"
			data-testid="review-composer"
			onKeyDown={onKeyDown}
			onKeyUp={(event) => event.stopPropagation()}
			onKeyPress={(event) => event.stopPropagation()}
		>
			<p className="text-muted-foreground">{linesLabel(line, startLine)}</p>
			<Textarea
				ref={ref}
				value={text}
				onChange={(event) => setText(event.target.value)}
				placeholder="Leave a comment"
				aria-label="Review comment"
				rows={3}
				className="min-h-[60px] text-xs md:text-xs"
				data-testid="review-composer-text"
			/>
			<div className="flex items-center justify-end gap-1.5">
				<Button
					variant="outline"
					size="sm"
					onClick={onCancel}
					title="Cancel (Esc)"
					data-testid="review-composer-cancel"
				>
					Cancel
				</Button>
				<Button
					size="sm"
					disabled={!ready}
					onClick={submit}
					title="Add to review (Ctrl+Enter)"
					data-testid="review-composer-add"
				>
					Add to review
				</Button>
			</div>
		</div>
	);
}
