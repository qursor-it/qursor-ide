import { Button, IconButton } from '@factorai/ui';
import { NodeToolbar, Position } from '@xyflow/react';
import { Group, MoveUpRight, Sparkles } from 'lucide-react';
import { STICKY_COLORS } from './nodes/shared';

const SWATCHES: { id: string; label: string }[] = [
	{ id: '3', label: 'Yellow' },
	{ id: '1', label: 'Pink' },
	{ id: '5', label: 'Blue' },
	{ id: '4', label: 'Green' },
	{ id: '6', label: 'Purple' },
];

/** The context toolbar floating above the selection. */
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
	return (
		<NodeToolbar nodeId={ids} isVisible={ids.length > 0} position={Position.Top} offset={14}>
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
		</NodeToolbar>
	);
}
