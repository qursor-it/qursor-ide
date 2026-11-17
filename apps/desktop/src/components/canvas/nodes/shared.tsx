import { Textarea, cn } from '@factorai/ui';
import { Handle, NodeResizer, Position } from '@xyflow/react';
import { type ReactNode, createContext, useContext } from 'react';
import type { CanvasNodeData } from '../jsonCanvas';

type Raw = CanvasNodeData['raw'];

/** What every node component needs from the board: edit state and patching. */
interface CanvasCtxValue {
	editingId: string | null;
	setEditing: (id: string | null) => void;
	/** Patch the node's raw JSON. History records on `commit`, not per keystroke. */
	patch: (id: string, patch: Partial<Raw>) => void;
	commit: () => void;
	newIds: Set<string>;
	touch: (id: string) => void;
}

export const CanvasCtx = createContext<CanvasCtxValue>({
	editingId: null,
	setEditing: () => {},
	patch: () => {},
	commit: () => {},
	newIds: new Set(),
	touch: () => {},
});

/** JSON Canvas preset colours "1"-"6" as readable pastels, in both themes. */
export const STICKY_COLORS: Record<string, string> = {
	'1': '#ffc9de',
	'2': '#ffd8a8',
	'3': '#fff1a8',
	'4': '#c9ecc4',
	'5': '#c4e2fb',
	'6': '#dccbf7',
};

export const colorOf = (raw: Raw, fallback: string): string => {
	const c = raw.color;
	return typeof c === 'string' ? (STICKY_COLORS[c] ?? c) : fallback;
};

const SIDES = [Position.Top, Position.Right, Position.Bottom, Position.Left];

/** Resizer, connection handles, the dashed "new" outline and the AGENT badge. */
export function NodeChrome({
	id,
	raw,
	selected,
	minWidth = 80,
	minHeight = 40,
	className,
	children,
}: {
	id: string;
	raw: Raw;
	selected?: boolean;
	minWidth?: number;
	minHeight?: number;
	className?: string;
	children: ReactNode;
}) {
	const { newIds } = useContext(CanvasCtx);
	const agent = raw['x-author'] === 'agent';
	return (
		<div
			className={cn(
				'group relative size-full',
				newIds.has(id) && 'outline-2 outline-primary outline-dashed outline-offset-2',
				className,
			)}
		>
			<NodeResizer isVisible={selected} minWidth={minWidth} minHeight={minHeight} />
			{SIDES.map((side) => (
				<Handle
					key={side}
					id={side}
					type="source"
					position={side}
					className="size-2! border-0! bg-primary! opacity-0 group-hover:opacity-100"
				/>
			))}
			{agent && (
				<span className="absolute -top-2.5 right-2 z-10 rounded-sm bg-primary px-1.5 font-semibold text-[10px] text-primary-foreground tracking-wide">
					AGENT
				</span>
			)}
			{children}
		</div>
	);
}

/** A node's text: plain, or a textarea while it is being edited. */
export function EditableText({
	id,
	field,
	value,
	placeholder,
	className,
}: {
	id: string;
	field: string;
	value: unknown;
	placeholder: string;
	className?: string;
}) {
	const ctx = useContext(CanvasCtx);
	const text = typeof value === 'string' ? value : '';
	if (ctx.editingId === id) {
		return (
			<Textarea
				autoFocus
				value={text}
				placeholder={placeholder}
				onFocus={(e) => e.currentTarget.setSelectionRange(text.length, text.length)}
				onChange={(e) => ctx.patch(id, { [field]: e.target.value })}
				onBlur={() => {
					ctx.setEditing(null);
					ctx.commit();
					ctx.touch(id);
				}}
				onKeyDown={(e) => {
					e.stopPropagation();
					if (e.key === 'Escape') e.currentTarget.blur();
				}}
				className={cn(
					'nodrag nowheel min-h-0 flex-1 resize-none rounded-none border-0 bg-transparent p-0 text-inherit shadow-none focus-visible:ring-0',
					className,
				)}
			/>
		);
	}
	return (
		<div
			onDoubleClick={() => ctx.setEditing(id)}
			className={cn('min-h-0 flex-1 overflow-hidden whitespace-pre-wrap break-words', className)}
		>
			{text || <span className="opacity-40">{placeholder}</span>}
		</div>
	);
}
