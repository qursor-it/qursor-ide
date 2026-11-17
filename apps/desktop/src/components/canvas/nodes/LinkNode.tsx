import type { Node, NodeProps } from '@xyflow/react';
import { Link2 } from 'lucide-react';
import type { CanvasNodeData } from '../jsonCanvas';
import { EditableText, NodeChrome } from './shared';

/** A web link. Shown, not followed: the board is for thinking. */
export function LinkNode({ id, data, selected }: NodeProps<Node<CanvasNodeData>>) {
	const { raw } = data;
	return (
		<NodeChrome id={id} raw={raw} selected={selected} minHeight={40}>
			<div
				className={`flex size-full items-center gap-2 rounded-md border bg-card px-3 text-foreground text-sm shadow-sm ${selected ? 'border-primary' : 'border-border'}`}
			>
				<Link2 className="size-4 shrink-0 text-muted-foreground" />
				<EditableText
					id={id}
					field="url"
					value={raw.url}
					placeholder="https://"
					className="font-mono text-xs"
				/>
			</div>
		</NodeChrome>
	);
}
