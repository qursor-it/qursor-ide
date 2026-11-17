import type { Node, NodeProps } from '@xyflow/react';
import type { CanvasNodeData } from '../jsonCanvas';
import { EditableText, NodeChrome, colorOf } from './shared';

export function StickyNode({ id, data, selected }: NodeProps<Node<CanvasNodeData>>) {
	const { raw } = data;
	const agent = raw['x-author'] === 'agent';
	return (
		<NodeChrome id={id} raw={raw} selected={selected} minWidth={100} minHeight={80}>
			<div
				className={`flex size-full flex-col gap-1 rounded-sm p-3 text-[#23262f] text-sm shadow-md ${selected ? 'ring-2 ring-primary' : ''}`}
				style={{ background: colorOf(raw, '#fff1a8') }}
			>
				<EditableText id={id} field="text" value={raw.text} placeholder="Type something" />
				<span className="text-[11px] opacity-60">{agent ? 'Agent' : 'You'}</span>
			</div>
		</NodeChrome>
	);
}
