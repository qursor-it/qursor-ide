import { Button, IconButton } from '@factorai/ui';
import { useReactFlow, useViewport } from '@xyflow/react';
import { Group, MoveUpRight, Sparkles } from 'lucide-react';
import { useLayoutEffect, useRef, useState } from 'react';
import { STICKY_COLORS } from './nodes/shared';

const SWATCHES: { id: string; label: string }[] = [
	{ id: '3', label: 'Yellow' },
	{ id: '1', label: 'Pink' },
	{ id: '5', label: 'Blue' },
	{ id: '4', label: 'Green' },
	{ id: '6', label: 'Purple' },
];

const MARGIN = 8;

/** The context toolbar floating above the selection. It is not a `NodeToolbar`:
 *  that one lives inside the viewport layer, which the floating tool palette
 *  paints over, so the first swatches vanished under it near the left edge. */
export function SelectionToolbar({
	ids,
	onColor,
	onGroup,
	onConnector,
	onAsk,
}: {
	ids: string[];
	onColor: (color: string) => void;
	onGroup: () => void;
	onConnector: () => void;
	onAsk: (ids: string[]) => void;
}) {
	const rf = useReactFlow();
	const { x: vx, y: vy, zoom } = useViewport();
	const ref = useRef<HTMLDivElement>(null);
	const [size, setSize] = useState({ w: 0, h: 0, board: 0 });
	const shown = ids.length > 0;
	// Measure the toolbar and the board it must stay inside, and again when either resizes.
	useLayoutEffect(() => {
		const el = ref.current;
		const board = el?.parentElement;
		if (!el || !board) return;
		const measure = () =>
			setSize((p) => {
				const next = { w: el.offsetWidth, h: el.offsetHeight, board: board.clientWidth };
				return p.w === next.w && p.h === next.h && p.board === next.board ? p : next;
			});
		measure();
		const ro = new ResizeObserver(measure);
		ro.observe(el);
		ro.observe(board);
		return () => ro.disconnect();
	}, [shown]);
	if (!ids.length) return null;
	const b = rf.getNodesBounds(ids);
	const centre = (b.x + b.width / 2) * zoom + vx;
	const left = Math.max(MARGIN, Math.min(centre - size.w / 2, size.board - size.w - MARGIN));
	const top = Math.max(MARGIN, b.y * zoom + vy - 14 - size.h);
	return (
		<div ref={ref} className="nodrag nopan absolute z-10" style={{ left, top }}>
			<div
				data-testid="selection-toolbar"
				className="flex items-center gap-1 rounded-lg border border-border bg-card p-1 shadow-card"
			>
				{SWATCHES.map((s) => (
					<IconButton
						key={s.id}
						size="md"
						aria-label={`${s.label} colour`}
						onClick={() => onColor(s.id)}
					>
						<span
							className="size-4 rounded-full border border-black/10"
							style={{ background: STICKY_COLORS[s.id] }}
						/>
					</IconButton>
				))}
				<span className="mx-1 h-5 w-px bg-border" />
				<Button variant="quiet" size="sm" onClick={onGroup}>
					<Group /> Group into frame
				</Button>
				<IconButton size="md" aria-label="Connect" title="Connector" onClick={onConnector}>
					<MoveUpRight />
				</IconButton>
				<Button size="sm" onClick={() => onAsk(ids)}>
					<Sparkles /> Ask the agent
				</Button>
			</div>
		</div>
	);
}
