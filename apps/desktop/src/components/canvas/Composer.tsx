import { Button, Textarea } from '@factorai/ui';
import { type Ref, useState } from 'react';

export type NodeRef = { id: string; text: string };

const words = (t: string) => t.trim().split(/\s+/).slice(0, 4).join(' ');

/** The agent panel's input: Enter sends, Shift+Enter breaks the line, `@` cites a node (F44). */
export function Composer({
	inputRef,
	selectedCount,
	nodes,
	busy,
	error,
	onSend,
}: {
	inputRef: Ref<HTMLTextAreaElement>;
	selectedCount: number;
	nodes: NodeRef[];
	busy: boolean;
	error: string | null;
	/** Resolves true when the message went out; the text is cleared only then. */
	onSend: (text: string) => Promise<boolean>;
}) {
	const [text, setText] = useState('');
	// The `@word` being typed at the end of the text, if any.
	const query = /(?:^|\s)@(\S*)$/.exec(text)?.[1];
	const matches =
		query === undefined
			? []
			: nodes
					.filter((n) => `${n.id} ${n.text}`.toLowerCase().includes(query.toLowerCase()))
					.slice(0, 6);
	const send = async () => {
		if (!text.trim() || busy) return;
		if (await onSend(text.trim())) setText('');
	};
	const cite = (n: NodeRef) => {
		const w = words(n.text);
		setText(text.replace(/@\S*$/, `@${n.id}${w ? ` "${w}…"` : ''} `));
	};
	return (
		<div className="flex shrink-0 flex-col gap-2 border-border border-t p-3">
			{error && (
				<p role="alert" data-testid="canvas-agent-error" className="text-destructive text-xs">
					{error}
				</p>
			)}
			{matches.length > 0 && (
				<div
					data-testid="canvas-node-picker"
					className="flex flex-col rounded-md border border-border"
				>
					{matches.map((n) => (
						<Button
							key={n.id}
							variant="quiet"
							size="sm"
							className="justify-start truncate"
							onClick={() => cite(n)}
						>
							<span className="font-mono text-muted-foreground">{n.id}</span> {words(n.text)}
						</Button>
					))}
				</div>
			)}
			<Textarea
				ref={inputRef}
				data-testid="canvas-composer"
				aria-label="Message the agent"
				placeholder="Ask about this canvas…  (@ to cite a node)"
				value={text}
				disabled={busy}
				onChange={(e) => setText(e.target.value)}
				onKeyDown={(e) => {
					if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
						e.preventDefault();
						void send();
					}
				}}
			/>
			{selectedCount > 0 && (
				<span
					data-testid="canvas-selection-chip"
					className="self-start rounded-sm bg-secondary px-1.5 py-0.5 text-secondary-foreground text-xs"
				>
					◇ {selectedCount} {selectedCount === 1 ? 'node' : 'nodes'} selected
				</span>
			)}
		</div>
	);
}
