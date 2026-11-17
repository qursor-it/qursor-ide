import type { Node, NodeProps } from '@xyflow/react';
import type { CanvasNodeData } from '../jsonCanvas';
import { EditableText, NodeChrome } from './shared';

/** A plain JSON Canvas text card. */
export function TextNode({ id, data, selected }: NodeProps<Node<CanvasNodeData>>) {
	const { raw } = data;
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
