import type { Node, NodeProps } from '@xyflow/react';
import { readPoints, strokePath } from '../ink';
import type { CanvasNodeData } from '../jsonCanvas';
import { NodeChrome } from './shared';

/** A pen stroke: `x-points` through perfect-freehand, stretched to the node's box. */
export function InkNode({ id, data, selected }: NodeProps<Node<CanvasNodeData>>) {
	const { raw } = data;
	const points = readPoints(raw['x-points']);
	const w = Math.max(8, ...points.map((p) => p[0]));
	const h = Math.max(8, ...points.map((p) => p[1]));
	return (
		<NodeChrome id={id} raw={raw} selected={selected} minWidth={8} minHeight={8}>
			<svg
				className="absolute inset-0 size-full overflow-visible"
				viewBox={`0 0 ${w} ${h}`}
				preserveAspectRatio="none"
				aria-hidden="true"
			>
				<path d={strokePath(points)} fill="var(--color-foreground)" />
			</svg>
		</NodeChrome>
	);
}
