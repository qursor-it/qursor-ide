import type { Edge, Node } from '@xyflow/react';

/** JSON Canvas 1.0 (F44, ADR-0103). Unknown fields, including our `x-` extras
 *  and anything an agent adds, ride along in the index signature and are never
 *  dropped. */
// Not exported until a consumer needs it (knip fails on unused exports).
type JcNode = {
	id: string;
	type: string;
	x: number;
	y: number;
	width: number;
	height: number;
	[k: string]: unknown;
};
type JcEdge = { id: string; fromNode: string; toNode: string; [k: string]: unknown };
export type CanvasDoc = {
	title?: string;
	nodes: JcNode[];
	edges: JcEdge[];
	[k: string]: unknown;
};

/** The raw JSON node is the single source of truth: components read and patch
 *  it, and position and size are written back into it on serialize. */
export type CanvasNodeData = { raw: JcNode };
export type CanvasEdgeData = { raw: JcEdge };

type CanvasNodeType = 'sticky' | 'shape' | 'ink' | 'text' | 'file' | 'link' | 'group';

type ParsedCanvas =
	| {
			ok: true;
			doc: CanvasDoc;
			nodes: Node<CanvasNodeData>[];
			edges: Edge<CanvasEdgeData>[];
	  }
	| { ok: false; error: string };

const DEFAULT_SIZE = 160;
const X_KINDS = ['sticky', 'shape', 'ink'];
const JC_TYPES = ['text', 'file', 'link', 'group'];

const isObject = (v: unknown): v is Record<string, unknown> =>
	typeof v === 'object' && v !== null && !Array.isArray(v);

const num = (v: unknown, fallback: number): number =>
	typeof v === 'number' && Number.isFinite(v) ? v : fallback;

const round = (v: number): number => Math.round(v * 100) / 100;

function nodeType(raw: JcNode): CanvasNodeType {
	const kind = raw['x-kind'];
	if (typeof kind === 'string' && X_KINDS.includes(kind)) return kind as CanvasNodeType;
	return JC_TYPES.includes(raw.type) ? (raw.type as CanvasNodeType) : 'text';
}

type Box = { x: number; y: number; w: number; h: number };
const box = (n: JcNode): Box => ({
	x: num(n.x, 0),
	y: num(n.y, 0),
	w: num(n.width, DEFAULT_SIZE),
	h: num(n.height, DEFAULT_SIZE),
});
const contains = (outer: Box, inner: Box): boolean =>
	inner.x >= outer.x &&
	inner.y >= outer.y &&
	inner.x + inner.w <= outer.x + outer.w &&
	inner.y + inner.h <= outer.y + outer.h &&
	inner.w * inner.h < outer.w * outer.h;

export function parseCanvas(text: string): ParsedCanvas {
	let json: unknown;
	try {
		json = JSON.parse(text);
	} catch (e) {
		return { ok: false, error: e instanceof Error ? e.message : 'Invalid JSON' };
	}
	if (!isObject(json)) return { ok: false, error: 'A canvas must be a JSON object' };
	const rawNodes = json.nodes ?? [];
	const rawEdges = json.edges ?? [];
	if (!Array.isArray(rawNodes)) return { ok: false, error: '"nodes" must be an array' };
	if (!Array.isArray(rawEdges)) return { ok: false, error: '"edges" must be an array' };
	for (const [i, n] of rawNodes.entries()) {
		if (!isObject(n) || typeof n.id !== 'string') {
			return { ok: false, error: `Node ${i} has no string "id"` };
		}
	}
	for (const [i, e] of rawEdges.entries()) {
		if (
			!isObject(e) ||
			typeof e.id !== 'string' ||
			typeof e.fromNode !== 'string' ||
			typeof e.toNode !== 'string'
		) {
			return { ok: false, error: `Edge ${i} needs string "id", "fromNode" and "toNode"` };
		}
	}
	const jcNodes = rawNodes as JcNode[];
	const jcEdges = rawEdges as JcEdge[];
	const doc: CanvasDoc = { ...json, nodes: jcNodes, edges: jcEdges };

	// The smallest group that fully contains a node is its parent.
	const groups = jcNodes.filter((n) => n.type === 'group');
	const parentOf = (n: JcNode): JcNode | undefined =>
		groups
			.filter((g) => g !== n && contains(box(g), box(n)))
			.sort((a, b) => box(a).w * box(a).h - box(b).w * box(b).h)[0];

	const toRf = (raw: JcNode): Node<CanvasNodeData> => {
		const b = box(raw);
		const parent = parentOf(raw);
		const origin = parent ? box(parent) : { x: 0, y: 0 };
		return {
			id: raw.id,
			type: nodeType(raw),
			position: { x: b.x - origin.x, y: b.y - origin.y },
			width: b.w,
			height: b.h,
			data: { raw },
			...(parent ? { parentId: parent.id } : {}),
		};
	};
	// React Flow needs a parent before its children; groups first, order kept.
	const nodes = [
		...jcNodes.filter((n) => n.type === 'group'),
		...jcNodes.filter((n) => n.type !== 'group'),
	].map(toRf);
	const edges = jcEdges.map((raw): Edge<CanvasEdgeData> => ({
		id: raw.id,
		source: raw.fromNode,
		target: raw.toNode,
		...(typeof raw.fromSide === 'string' ? { sourceHandle: raw.fromSide } : {}),
		...(typeof raw.toSide === 'string' ? { targetHandle: raw.toSide } : {}),
		data: { raw },
	}));
	return { ok: true, doc, nodes, edges };
}

export function serializeCanvas(
	doc: CanvasDoc,
	nodes: Node<CanvasNodeData>[],
	edges: Edge<CanvasEdgeData>[],
): string {
	const byId = new Map(nodes.map((n) => [n.id, n]));
	const absolute = (n: Node<CanvasNodeData>): { x: number; y: number } => {
		let { x, y } = n.position;
		for (let p = n.parentId && byId.get(n.parentId); p; p = p.parentId && byId.get(p.parentId)) {
			x += p.position.x;
			y += p.position.y;
		}
		return { x, y };
	};
	const jcNodes = nodes.map((n): JcNode => {
		const { x, y } = absolute(n);
		return {
			...n.data.raw,
			x: round(x),
			y: round(y),
			width: round(n.width ?? n.measured?.width ?? num(n.data.raw.width, DEFAULT_SIZE)),
			height: round(n.height ?? n.measured?.height ?? num(n.data.raw.height, DEFAULT_SIZE)),
		};
	});
	const jcEdges = edges.map((e): JcEdge => ({
		...e.data?.raw,
		id: e.id,
		fromNode: e.source,
		toNode: e.target,
		...(e.sourceHandle ? { fromSide: e.sourceHandle } : {}),
		...(e.targetHandle ? { toSide: e.targetHandle } : {}),
	}));
	return `${JSON.stringify({ ...doc, nodes: jcNodes, edges: jcEdges }, null, 2)}\n`;
}

/** Ids in `next` that `prev` did not have: what an agent just added. */
export function newNodeIds(prev: CanvasDoc | null, next: CanvasDoc): Set<string> {
	if (!prev) return new Set();
	const known = new Set(prev.nodes.map((n) => n.id));
	return new Set(next.nodes.map((n) => n.id).filter((id) => !known.has(id)));
}
