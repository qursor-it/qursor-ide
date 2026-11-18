import type { Node, NodeProps } from '@xyflow/react';
import type { CanvasNodeData } from '../jsonCanvas';
import { EditableText, NodeChrome, colorOf } from './shared';

/** A JSON Canvas `group`, drawn as a Miro frame: its label sits above the box. */
export function FrameNode({ id, data, selected }: NodeProps<Node<CanvasNodeData>>) {
	const { raw } = data;
	const tint = colorOf(raw, '');
	return (
		<NodeChrome id={id} raw={raw} selected={selected} minWidth={120} minHeight={80}>
			<div
				className="absolute -top-6 left-0 flex h-5 max-w-full text-muted-foreground text-xs"
				style={tint ? { color: tint } : undefined}
			>
				<EditableText id={id} field="label" value={raw.label} placeholder="Frame" />
			</div>
			<div
				data-testid="canvas-frame"
				className={`size-full rounded-lg border ${tint ? '' : 'bg-card/60'} ${selected ? 'border-primary' : 'border-border'}`}
				style={
					tint
						? {
								background: `color-mix(in srgb, ${tint} 18%, transparent)`,
								...(selected ? {} : { borderColor: tint }),
							}
						: undefined
				}
			/>
		</NodeChrome>
	);
}
