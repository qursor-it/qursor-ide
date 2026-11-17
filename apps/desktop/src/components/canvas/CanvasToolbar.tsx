import { IconButton, cn } from '@factorai/ui';
import {
	Circle,
	Frame,
	Hand,
	Link2,
	MousePointer2,
	MoveUpRight,
	Square,
	StickyNote,
	Type,
	FileText,
} from 'lucide-react';
import type { ReactNode } from 'react';

export type Tool =
	| 'select'
	| 'hand'
	| 'sticky'
	| 'rect'
	| 'ellipse'
	| 'text'
	| 'connector'
	| 'frame'
	| 'file'
	| 'link';

const TOOLS: { tool: Tool; label: string; key?: string; icon: ReactNode; gap?: boolean }[] = [
	{ tool: 'select', label: 'Select', key: 'V', icon: <MousePointer2 /> },
	{ tool: 'hand', label: 'Hand', key: 'H', icon: <Hand /> },
	{ tool: 'sticky', label: 'Sticky note', key: 'N', icon: <StickyNote />, gap: true },
	{ tool: 'rect', label: 'Rectangle', key: 'R', icon: <Square /> },
	{ tool: 'ellipse', label: 'Ellipse', key: 'O', icon: <Circle /> },
	{ tool: 'text', label: 'Text', key: 'T', icon: <Type /> },
	{ tool: 'connector', label: 'Connector', icon: <MoveUpRight /> },
	{ tool: 'frame', label: 'Frame', key: 'F', icon: <Frame /> },
	{ tool: 'file', label: 'File', icon: <FileText />, gap: true },
	{ tool: 'link', label: 'Link', key: 'L', icon: <Link2 /> },
];

/** Shortcut letter to tool, for the board's key handler. */
export const TOOL_KEYS: Record<string, Tool> = Object.fromEntries(
	TOOLS.flatMap((t) => (t.key ? [[t.key.toLowerCase(), t.tool]] : [])),
);

/** The floating tool column on the board's left edge. */
export function CanvasToolbar({ tool, onTool }: { tool: Tool; onTool: (t: Tool) => void }) {
	return (
		<div
			role="toolbar"
			aria-label="Canvas tools"
			aria-orientation="vertical"
			data-testid="canvas-toolbar"
			className="flex flex-col gap-0.5 rounded-xl border border-border bg-card p-1.5 shadow-card"
		>
			{TOOLS.map((t) => (
				<div
					key={t.tool}
					className={cn('flex flex-col', t.gap && 'mt-1 border-border border-t pt-1')}
				>
					<IconButton
						size="md"
						aria-label={t.label}
						aria-pressed={tool === t.tool}
						title={t.key ? `${t.label} (${t.key})` : t.label}
						data-testid={`tool-${t.tool}`}
						onClick={() => onTool(t.tool)}
						className={cn('size-8 rounded-md', tool === t.tool && 'bg-primary/15 text-primary')}
					>
						{t.icon}
					</IconButton>
				</div>
			))}
		</div>
	);
}
