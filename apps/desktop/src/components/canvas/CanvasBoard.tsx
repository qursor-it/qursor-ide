import '@xyflow/react/dist/style.css';
import { Button } from '@factorai/ui';
import { cmd } from '@lib/tauri';
import { Background, BackgroundVariant, Controls, MiniMap, ReactFlow } from '@xyflow/react';
import type { Node, NodeProps } from '@xyflow/react';
import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { type CanvasNodeData, parseCanvas } from './jsonCanvas';

interface CanvasBoardProps {
	path: string;
	/** The error card's way out: the file in the existing viewer. */
	onOpenAsText: (path: string) => void;
}

/** Read-only for now: editing and autosave arrive with the next slice (F44). */
export function CanvasBoard({ path, onOpenAsText }: CanvasBoardProps) {
	const readQ = useQuery({
		queryKey: ['canvas-file', path],
		queryFn: () => cmd.readCanvas(path),
		// A stale copy of the file would be shown over a newer one.
		gcTime: 0,
	});
	const parsed = useMemo(
		() => (readQ.data ? parseCanvas(readQ.data.contents) : null),
		[readQ.data],
	);

	if (readQ.isError || (parsed && !parsed.ok)) {
		const message = parsed && !parsed.ok ? parsed.error : String(readQ.error);
		return (
			<div className="flex min-h-0 flex-1 items-center justify-center p-8">
				<div
					role="alert"
					data-testid="canvas-error"
					className="flex max-w-md flex-col gap-3 rounded-xl border border-border bg-background p-5"
				>
					<h2 className="font-semibold text-base">This canvas can&apos;t be read</h2>
					<p className="break-words text-muted-foreground text-sm">{message}</p>
					<Button variant="outline" className="self-start" onClick={() => onOpenAsText(path)}>
						Open as text
					</Button>
				</div>
			</div>
		);
	}
	if (!parsed?.ok) return <div className="flex-1" />;

	return (
		<div data-testid="canvas-board" className="min-h-0 min-w-0 flex-1">
			<ReactFlow
				nodes={parsed.nodes}
				edges={parsed.edges}
				nodeTypes={NODE_TYPES}
				nodesDraggable={false}
				nodesConnectable={false}
				elementsSelectable={false}
				fitView
				proOptions={{ hideAttribution: true }}
			>
				<Background variant={BackgroundVariant.Dots} />
				<MiniMap pannable zoomable />
				<Controls showInteractive={false} />
			</ReactFlow>
		</div>
	);
}

/** One plain card for every kind: the real node components come with editing. */
function ReadOnlyNode({ data }: NodeProps<Node<CanvasNodeData>>) {
	const { raw } = data;
	const text = [raw.text, raw.label, raw.file, raw.url].find((v) => typeof v === 'string' && v);
	return (
		<div
			className="size-full overflow-hidden whitespace-pre-wrap break-words rounded-md border border-border bg-card p-2 text-foreground text-sm shadow-sm"
			style={{ width: raw.width, height: raw.height }}
		>
			{String(text ?? '')}
		</div>
	);
}

const NODE_TYPES = Object.fromEntries(
	['sticky', 'shape', 'ink', 'text', 'file', 'link', 'group'].map((t) => [t, ReadOnlyNode]),
);
