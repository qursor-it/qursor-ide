import type { Node, NodeProps } from '@xyflow/react';
import type { CanvasNodeData } from '../jsonCanvas';
import { EditableText, NodeChrome } from './shared';

/** A JSON Canvas `group`, drawn as a Miro frame: its label sits above the box. */
export function FrameNode({ id, data, selected }: NodeProps<Node<CanvasNodeData>>) {
	const { raw } = data;
	return (
		<NodeChrome id={id} raw={raw} selected={selected} minWidth={120} minHeight={80}>
			<div className="absolute -top-6 left-0 flex h-5 max-w-full text-muted-foreground text-xs">
				<EditableText id={id} field="label" value={raw.label} placeholder="Frame" />
			</div>
			<div
				className={`size-full rounded-lg border bg-card/60 ${selected ? 'border-primary' : 'border-border'}`}
			/>
		</NodeChrome>
	);
}
