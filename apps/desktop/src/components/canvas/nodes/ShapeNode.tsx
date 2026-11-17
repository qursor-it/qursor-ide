import type { Node, NodeProps } from '@xyflow/react';
import type { CanvasNodeData } from '../jsonCanvas';
import { EditableText, NodeChrome, colorOf } from './shared';

/** One SVG outline for the three shapes, stretched to the node's box. */
export function ShapeNode({ id, data, selected }: NodeProps<Node<CanvasNodeData>>) {
	const { raw } = data;
	const shape = raw['x-shape'];
	const common = {
		fill: colorOf(raw, 'var(--color-card)'),
		stroke: 'var(--color-border)',
		strokeWidth: 1.5,
		vectorEffect: 'non-scaling-stroke' as const,
	};
	return (
		<NodeChrome id={id} raw={raw} selected={selected}>
			<svg
				className="absolute inset-0 size-full"
				viewBox="0 0 100 100"
				preserveAspectRatio="none"
				aria-hidden="true"
			>
				{shape === 'ellipse' ? (
					<ellipse cx="50" cy="50" rx="49" ry="49" {...common} />
				) : shape === 'diamond' ? (
					<polygon points="50,1 99,50 50,99 1,50" {...common} />
				) : (
					<rect x="1" y="1" width="98" height="98" rx="4" {...common} />
				)}
			</svg>
			<div className="relative flex size-full items-center justify-center p-3 text-center text-[#23262f] text-sm">
				<EditableText
					id={id}
					field="text"
					value={raw.text}
					placeholder="Text"
					className="text-center"
				/>
			</div>
		</NodeChrome>
	);
}
