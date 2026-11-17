import '@xyflow/react/dist/style.css';
import './canvas.css';
import { Button, IconButton } from '@factorai/ui';
import {
	Background,
	BackgroundVariant,
	type Connection,
	ConnectionMode,
	type Edge,
	MarkerType,
	MiniMap,
	type Node,
	Panel,
	ReactFlow,
	ReactFlowProvider,
	useReactFlow,
	useViewport,
} from '@xyflow/react';
import { Maximize, Minus, Plus } from 'lucide-react';
import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import { CanvasToolbar, TOOL_KEYS, type Tool } from './CanvasToolbar';
import { SelectionToolbar } from './SelectionToolbar';
import type { CanvasEdgeData, CanvasNodeData } from './jsonCanvas';
import { FileNode } from './nodes/FileNode';
import { FrameNode } from './nodes/FrameNode';
import { LinkNode } from './nodes/LinkNode';
import { ShapeNode } from './nodes/ShapeNode';
import { StickyNode } from './nodes/StickyNode';
import { TextNode } from './nodes/TextNode';
import { CanvasCtx } from './nodes/shared';
import { type SaveState, useCanvasDoc } from './useCanvasDoc';

type CN = Node<CanvasNodeData>;
type CE = Edge<CanvasEdgeData>;
type Raw = CanvasNodeData['raw'];

interface CanvasBoardProps {
	projectId: string;
	path: string;
	/** The error card's way out: the file in the existing viewer. */
	onOpenAsText: (path: string) => void;
	/** "Ask the agent" over the selection (F44). */
	onAsk: (selectedIds: string[]) => void;
	/** What the agent panel's composer attaches: the selection and the nodes to cite. */
	onSelection: (ids: string[]) => void;
	onNodeRefs: (refs: { id: string; text: string }[]) => void;
	onSaveState: (state: SaveState) => void;
}

/** Everything a tool can drop on the board with one click. */
const ADD: Partial<Record<Tool, { type: string; w: number; h: number; raw: Partial<Raw> }>> = {
	sticky: {
		type: 'sticky',
		w: 160,
		h: 160,
		raw: { type: 'text', text: '', 'x-kind': 'sticky', color: '3' },
	},
	rect: {
		type: 'shape',
		w: 160,
		h: 90,
		raw: { type: 'text', text: '', 'x-kind': 'shape', 'x-shape': 'rect' },
	},
	ellipse: {
		type: 'shape',
		w: 160,
		h: 90,
		raw: { type: 'text', text: '', 'x-kind': 'shape', 'x-shape': 'ellipse' },
	},
	text: { type: 'text', w: 200, h: 40, raw: { type: 'text', text: '' } },
	frame: { type: 'group', w: 400, h: 300, raw: { type: 'group', label: 'Frame' } },
	file: { type: 'file', w: 220, h: 48, raw: { type: 'file', file: '' } },
	link: { type: 'link', w: 220, h: 48, raw: { type: 'link', url: '' } },
};

const NODE_TYPES = {
	sticky: StickyNode,
	shape: ShapeNode,
	text: TextNode,
	group: FrameNode,
	file: FileNode,
	link: LinkNode,
	// Strokes arrive with the pen; until then an ink node is an outline.
	ink: () => <div className="size-full rounded border border-border border-dashed" />,
};

const EDGE_OPTIONS = {
	style: { stroke: 'var(--color-primary)', strokeDasharray: '6 4', strokeWidth: 1.5 },
	markerEnd: { type: MarkerType.ArrowClosed, color: 'var(--color-primary)' },
};

const THEME_VARS = {
	'--xy-background-color': 'var(--color-background)',
	'--xy-minimap-background-color': 'var(--color-card)',
	'--xy-minimap-mask-background-color':
		'color-mix(in oklab, var(--color-foreground) 8%, transparent)',
	'--xy-minimap-node-background-color': 'var(--color-secondary)',
	'--xy-controls-button-background-color': 'var(--color-card)',
} as React.CSSProperties;

const uid = () => crypto.randomUUID().slice(0, 8);
const OPPOSITE: Record<string, string> = {
	left: 'right',
	right: 'left',
	top: 'bottom',
	bottom: 'top',
};
const typing = (t: EventTarget | null) =>
	t instanceof HTMLElement &&
	(t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName));

export function CanvasBoard(props: CanvasBoardProps) {
	return (
		<ReactFlowProvider>
			<Board {...props} />
		</ReactFlowProvider>
	);
}

function Board({
	projectId,
	path,
	onOpenAsText,
	onAsk,
	onSelection,
	onNodeRefs,
	onSaveState,
}: CanvasBoardProps) {
	const doc = useCanvasDoc(projectId, path);
	const { ctl, nodes, edges } = doc;
	const rf = useReactFlow<CN, CE>();
	const [tool, setTool] = useState<Tool>('select');
	const [editingId, setEditing] = useState<string | null>(null);
	const [pending, setPending] = useState<string | null>(null);
	const clip = useRef<{
		nodes: { type: string; w: number; h: number; x: number; y: number; raw: Raw }[];
		edges: CE[];
	}>({
		nodes: [],
		edges: [],
	});
	const pasteCount = useRef(0);
	const patched = useRef(false);
	const latest = useRef({ nodes, edges });
	latest.current = { nodes, edges };

	useEffect(() => onSaveState(doc.saveState), [doc.saveState, onSaveState]);

	const ctx = useMemo(
		() => ({
			editingId,
			setEditing,
			patch: (id: string, p: Partial<Raw>) => {
				patched.current = true;
				ctl.update(
					(s) => ({
						...s,
						nodes: s.nodes.map((n) =>
							n.id === id ? { ...n, data: { raw: { ...n.data.raw, ...p } } } : n,
						),
					}),
					{ record: false },
				);
			},
			commit: () => {
				if (!patched.current) return;
				patched.current = false;
				ctl.update((s) => s);
			},
			newIds: doc.newIds,
			touch: ctl.touch,
		}),
		[editingId, doc.newIds, ctl],
	);

	const selectedIds = useMemo(() => nodes.filter((n) => n.selected).map((n) => n.id), [nodes]);
	// Reported on content, not identity: a drag makes a new `nodes` every frame.
	const selectionKey = selectedIds.join(',');
	const refs = useMemo(
		() =>
			nodes.map((n) => {
				const raw = n.data.raw;
				const t = raw.text ?? raw.file ?? raw.url ?? raw.label;
				return { id: n.id, text: typeof t === 'string' ? t : '' };
			}),
		[nodes],
	);
	const refsKey = JSON.stringify(refs);
	const reported = useRef({ onSelection, onNodeRefs, selectedIds, refs });
	reported.current = { onSelection, onNodeRefs, selectedIds, refs };
	useEffect(() => reported.current.onSelection(reported.current.selectedIds), [selectionKey]);
	useEffect(() => reported.current.onNodeRefs(reported.current.refs), [refsKey]);

	const place = (id: string) => {
		const n = rf.getInternalNode(id);
		const p = n?.internals.positionAbsolute ?? { x: 0, y: 0 };
		return {
			...p,
			w: n?.measured.width ?? n?.width ?? 160,
			h: n?.measured.height ?? n?.height ?? 160,
		};
	};

	const connect = (source: string, target: string, from?: string | null, to?: string | null) => {
		if (source === target) return;
		const a = place(source);
		const b = place(target);
		const dx = b.x + b.w / 2 - (a.x + a.w / 2);
		const dy = b.y + b.h / 2 - (a.y + a.h / 2);
		const fromSide =
			from ??
			(Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? 'right' : 'left') : dy > 0 ? 'bottom' : 'top');
		const toSide = to ?? OPPOSITE[fromSide] ?? 'left';
		const id = `e${uid()}`;
		ctl.onConnect({
			id,
			source,
			target,
			sourceHandle: fromSide,
			targetHandle: toSide,
			data: { raw: { id, fromNode: source, toNode: target, fromSide, toSide, toEnd: 'arrow' } },
		});
	};

	const addAt = (t: Tool, at: { x: number; y: number }) => {
		const spec = ADD[t];
		if (!spec) return;
		const id = uid();
		const x = at.x - spec.w / 2;
		const y = at.y - spec.h / 2;
		const node: CN = {
			id,
			type: spec.type,
			position: { x, y },
			width: spec.w,
			height: spec.h,
			data: { raw: { id, x, y, width: spec.w, height: spec.h, ...spec.raw } as Raw },
			selected: true,
		};
		ctl.update((s) => {
			const rest = s.nodes.map((n) => ({ ...n, selected: false }));
			return { ...s, nodes: spec.type === 'group' ? [node, ...rest] : [...rest, node] };
		});
		setEditing(id);
	};

	const groupSelected = () => {
		const picked = latest.current.nodes.filter(
			(n) => n.selected && n.type !== 'group' && !n.parentId,
		);
		if (!picked.length) return;
		const boxes = picked.map((n) => ({ id: n.id, ...place(n.id) }));
		const pad = 24;
		const x = Math.min(...boxes.map((b) => b.x)) - pad;
		const y = Math.min(...boxes.map((b) => b.y)) - pad;
		const w = Math.max(...boxes.map((b) => b.x + b.w)) + pad - x;
		const h = Math.max(...boxes.map((b) => b.y + b.h)) + pad - y;
		const id = uid();
		const frame: CN = {
			id,
			type: 'group',
			position: { x, y },
			width: w,
			height: h,
			data: { raw: { id, type: 'group', label: 'Frame', x, y, width: w, height: h } },
		};
		const at = new Map(boxes.map((b) => [b.id, b]));
		ctl.update((s) => ({
			...s,
			nodes: [
				frame,
				...s.nodes.map((n) => {
					const b = at.get(n.id);
					return b ? { ...n, parentId: id, position: { x: b.x - x, y: b.y - y } } : n;
				}),
			],
		}));
		setEditing(id);
	};

	const setColor = (color: string) =>
		ctl.update((s) => ({
			...s,
			nodes: s.nodes.map((n) =>
				n.selected && (n.type === 'sticky' || n.type === 'shape')
					? { ...n, data: { raw: { ...n.data.raw, color } } }
					: n,
			),
		}));

	const copy = () => {
		const sel = latest.current.nodes.filter((n) => n.selected);
		const ids = new Set(sel.map((n) => n.id));
		pasteCount.current = 0;
		clip.current = {
			nodes: sel.map((n) => {
				const { x, y, w, h } = place(n.id);
				return { type: n.type ?? 'text', x, y, w, h, raw: n.data.raw };
			}),
			edges: latest.current.edges.filter((e) => ids.has(e.source) && ids.has(e.target)),
		};
	};

	const paste = () => {
		const { nodes: src, edges: srcEdges } = clip.current;
		if (!src.length) return;
		pasteCount.current += 1;
		const off = 24 * pasteCount.current;
		const ids = new Map<string, string>();
		const fresh: CN[] = src.map((c) => {
			const id = uid();
			ids.set(c.raw.id, id);
			const x = c.x + off;
			const y = c.y + off;
			return {
				id,
				type: c.type,
				position: { x, y },
				width: c.w,
				height: c.h,
				data: { raw: { ...c.raw, id, x, y } },
				selected: true,
			};
		});
		const newEdges: CE[] = srcEdges.flatMap((e) => {
			const source = ids.get(e.source);
			const target = ids.get(e.target);
			if (!source || !target) return [];
			const id = `e${uid()}`;
			return [
				{
					...e,
					id,
					source,
					target,
					selected: false,
					data: { raw: { ...e.data?.raw, id, fromNode: source, toNode: target } },
				} as CE,
			];
		});
		ctl.update((s) => ({
			nodes: [
				...fresh.filter((n) => n.type === 'group'),
				...s.nodes.map((n) => ({ ...n, selected: false })),
				...fresh.filter((n) => n.type !== 'group'),
			],
			edges: [...s.edges, ...newEdges],
		}));
	};

	const onKey = (e: KeyboardEvent) => {
		if (typing(e.target)) return;
		const mod = e.ctrlKey || e.metaKey;
		const k = e.key.toLowerCase();
		if (mod) {
			if (k === 'z') (e.shiftKey ? ctl.redo : ctl.undo)();
			else if (k === 'y') ctl.redo();
			else if (k === 's') void ctl.saveNow();
			else if (k === 'c' && !window.getSelection()?.toString()) copy();
			else if (k === 'v') paste();
			else return;
			e.preventDefault();
		} else if (e.key === 'Escape') {
			setTool('select');
			setPending(null);
		} else if (!e.altKey && TOOL_KEYS[k]) {
			setTool(TOOL_KEYS[k]);
			setPending(null);
		}
	};
	// The listener is registered once; it calls whichever handler the last render made.
	const onKeyRef = useRef(onKey);
	onKeyRef.current = onKey;
	useEffect(() => {
		const handler = (e: KeyboardEvent) => onKeyRef.current(e);
		window.addEventListener('keydown', handler);
		return () => window.removeEventListener('keydown', handler);
	}, []);

	if (doc.status === 'invalid') {
		return (
			<div className="flex min-h-0 flex-1 items-center justify-center p-8">
				<div
					role="alert"
					data-testid="canvas-error"
					className="flex max-w-md flex-col gap-3 rounded-xl border border-border bg-background p-5"
				>
					<h2 className="font-semibold text-base">This canvas can&apos;t be read</h2>
					<p className="break-words text-muted-foreground text-sm">{doc.error}</p>
					<Button variant="outline" className="self-start" onClick={() => onOpenAsText(path)}>
						Open as text
					</Button>
				</div>
			</div>
		);
	}
	if (doc.status === 'loading') return <div className="flex-1" />;

	return (
		<div
			data-testid="canvas-board"
			className="flex min-h-0 min-w-0 flex-1 flex-col"
			style={THEME_VARS}
		>
			{doc.conflict && (
				<Banner testid="canvas-conflict">
					The canvas changed on disk (the agent?).
					<Button size="sm" variant="outline" onClick={doc.conflict.reload}>
						Reload
					</Button>
					<Button size="sm" onClick={doc.conflict.keepMine}>
						Keep mine
					</Button>
				</Banner>
			)}
			{doc.status === 'deleted' && (
				<Banner testid="canvas-deleted">
					This canvas was deleted from disk. It will not be recreated, and further changes are not
					saved.
				</Banner>
			)}
			<div
				className="min-h-0 flex-1"
				onDoubleClick={(e) => {
					if ((e.target as Element).classList.contains('react-flow__pane')) {
						addAt('sticky', rf.screenToFlowPosition({ x: e.clientX, y: e.clientY }));
					}
				}}
			>
				<CanvasCtx.Provider value={ctx}>
					<ReactFlow
						nodes={nodes}
						edges={edges}
						nodeTypes={NODE_TYPES}
						onNodesChange={ctl.onNodesChange}
						onEdgesChange={ctl.onEdgesChange}
						onConnect={(c: Connection) =>
							connect(c.source, c.target, c.sourceHandle, c.targetHandle)
						}
						onPaneClick={(e) => {
							setPending(null);
							if (ADD[tool]) {
								addAt(tool, rf.screenToFlowPosition({ x: e.clientX, y: e.clientY }));
								setTool('select');
							}
						}}
						onNodeClick={(_, n) => {
							if (tool !== 'connector') return;
							if (pending) {
								connect(pending, n.id);
								setPending(null);
								setTool('select');
							} else setPending(n.id);
						}}
						onNodeDragStart={(_, n) => ctl.touch(n.id)}
						defaultEdgeOptions={EDGE_OPTIONS}
						connectionMode={ConnectionMode.Loose}
						panOnDrag={tool === 'hand' ? true : [1, 2]}
						selectionOnDrag={tool === 'select'}
						nodesDraggable={tool === 'select'}
						multiSelectionKeyCode={['Shift', 'Meta', 'Control']}
						deleteKeyCode={['Delete', 'Backspace']}
						zoomOnDoubleClick={false}
						snapToGrid
						snapGrid={[8, 8]}
						minZoom={0.1}
						maxZoom={3}
						fitView
						fitViewOptions={{
							maxZoom: 1,
							padding: { top: 0.15, right: 0.1, bottom: 0.15, left: '96px' },
						}}
						proOptions={{ hideAttribution: true }}
					>
						<Background
							variant={BackgroundVariant.Dots}
							gap={20}
							size={1.5}
							color="var(--color-border)"
						/>
						<Panel position="top-left" style={{ top: '50%', transform: 'translateY(-50%)' }}>
							<CanvasToolbar tool={tool} onTool={setTool} />
						</Panel>
						<Panel position="bottom-right" className="flex flex-col items-end gap-2">
							<MiniMap
								pannable
								zoomable
								style={{
									position: 'relative',
									margin: 0,
									border: '1px solid var(--color-border)',
									borderRadius: 8,
								}}
							/>
							<ZoomPanel />
						</Panel>
						<SelectionToolbar
							ids={editingId ? [] : selectedIds}
							onColor={setColor}
							onGroup={groupSelected}
							onConnector={() => setTool('connector')}
							onAsk={onAsk}
						/>
					</ReactFlow>
				</CanvasCtx.Provider>
			</div>
		</div>
	);
}

function Banner({ testid, children }: { testid: string; children: ReactNode }) {
	return (
		<div
			role="alert"
			data-testid={testid}
			className="flex shrink-0 items-center gap-3 border-border border-b bg-secondary px-4 py-2 text-sm"
		>
			{children}
		</div>
	);
}

function ZoomPanel() {
	const rf = useReactFlow();
	const { zoom } = useViewport();
	return (
		<div className="flex items-center gap-1 rounded-lg border border-border bg-card px-1.5 py-1 shadow-card">
			<IconButton size="md" aria-label="Zoom out" onClick={() => rf.zoomOut()}>
				<Minus />
			</IconButton>
			<span className="w-10 text-center text-xs tabular-nums">{Math.round(zoom * 100)}%</span>
			<IconButton size="md" aria-label="Zoom in" onClick={() => rf.zoomIn()}>
				<Plus />
			</IconButton>
			<IconButton
				size="md"
				aria-label="Fit view"
				onClick={() => rf.fitView({ maxZoom: 1, padding: 0.2 })}
			>
				<Maximize />
			</IconButton>
		</div>
	);
}
