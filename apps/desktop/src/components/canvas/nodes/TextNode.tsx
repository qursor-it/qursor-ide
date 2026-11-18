import type { Node, NodeProps } from '@xyflow/react';
import type { CanvasNodeData } from '../jsonCanvas';
import { EditableText, NodeChrome, colorOf } from './shared';

/** A plain JSON Canvas text node: free text, or a card when it has a colour. */
export function TextNode({ id, data, selected }: NodeProps<Node<CanvasNodeData>>) {
	const { raw } = data;
	// Agents and Obsidian write coloured cards as plain `text` nodes, without `x-kind`.
	if (raw.color)
		return (
			<NodeChrome id={id} raw={raw} selected={selected} minHeight={32}>
				<div
					data-testid="canvas-text-card"
					className={`flex size-full flex-col rounded-md border border-black/15 p-3 text-[#23262f] text-sm shadow-sm ${selected ? 'ring-2 ring-primary' : ''}`}
					style={{ background: colorOf(raw, 'transparent') }}
				>
					<EditableText id={id} field="text" value={raw.text} placeholder="Text" />
				</div>
			</NodeChrome>
		);
	return (
		<NodeChrome id={id} raw={raw} selected={selected} minHeight={32}>
			<div
				className={`flex size-full flex-col rounded-md p-1.5 text-foreground text-sm ${selected ? 'ring-1 ring-primary' : ''}`}
			>
				<EditableText id={id} field="text" value={raw.text} placeholder="Text" />
			</div>
		</NodeChrome>
	);
}
