import { diffStats, parseUnifiedDiff } from '@lib/intelligenceView';
import { useMemo } from 'react';

const LINE_CLASS = {
	file: 'text-muted-foreground',
	hunk: 'bg-secondary/40 text-muted-foreground',
	add: 'bg-git-added/10 text-git-added',
	remove: 'bg-git-deleted/10 text-git-deleted',
	context: 'text-secondary-foreground',
	note: 'text-muted-foreground italic',
} as const;

/**
 * A unified diff, for review (specs/05-features.md F33): what a Save would add
 * to, or take out of, the instruction file.
 *
 * Colour is not the only cue - every line keeps its `+` / `-` - and a line is
 * never wrapped into the next one, so a long instruction scrolls sideways
 * rather than turning into two. Read-only, and the text is the backend's: this
 * renders it, it does not make a second diff.
 */
export function UnifiedDiff({ diff }: { diff: string }) {
	const lines = useMemo(() => parseUnifiedDiff(diff), [diff]);
	const { added, removed } = useMemo(() => diffStats(lines), [lines]);
	return (
		<figure className="space-y-1" data-testid="intelligence-diff">
			<figcaption className="text-muted-foreground text-xs">
				<span className="text-git-added">+{added}</span>{' '}
				<span className="text-git-deleted">−{removed}</span> lines
			</figcaption>
			<pre
				aria-label="Proposed change as a unified diff"
				className="max-h-64 overflow-auto rounded-md border border-border bg-background py-1 font-mono text-xs leading-4"
			>
				{lines.map((line) => (
					<div
						key={line.no}
						data-kind={line.kind}
						className={`min-w-max whitespace-pre px-2 ${LINE_CLASS[line.kind]}`}
					>
						{line.text === '' ? ' ' : line.text}
					</div>
				))}
			</pre>
		</figure>
	);
}
