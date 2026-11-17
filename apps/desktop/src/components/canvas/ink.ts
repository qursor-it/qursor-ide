import { getStroke } from 'perfect-freehand';
import type { JcNode } from './jsonCanvas';

export type InkPoint = [number, number, number];

const MIN_SIZE = 8;

/** A finished stroke as a JSON Canvas node: the bbox is the node, `x-points`
 *  are relative to its origin, so moving the node moves the ink. */
export function strokeToNode(points: InkPoint[], id: string): JcNode {
	const xs = points.map((p) => p[0]);
	const ys = points.map((p) => p[1]);
	const x = Math.min(...xs);
	const y = Math.min(...ys);
	return {
		id,
		type: 'text',
		text: '',
		'x-kind': 'ink',
		x,
		y,
		width: Math.max(MIN_SIZE, Math.max(...xs) - x),
		height: Math.max(MIN_SIZE, Math.max(...ys) - y),
		'x-points': points.map(([px, py, pr]) => [px - x, py - y, pr]),
	};
}

/** The outline `getStroke` makes of the points, as an SVG path. */
export function strokePath(points: InkPoint[], size = 4): string {
	const outline = getStroke(points, { size, thinning: 0.5, smoothing: 0.5, streamline: 0.5 });
	if (!outline.length) return '';
	return `M${outline.map(([px, py]) => `${px.toFixed(1)},${py.toFixed(1)}`).join('L')}Z`;
}

/** `x-points` as a typed list; anything malformed (an agent's edit) is dropped. */
export function readPoints(raw: unknown): InkPoint[] {
	if (!Array.isArray(raw)) return [];
	return raw
		.filter(
			(p): p is InkPoint =>
				Array.isArray(p) && p.length >= 2 && p.every((n) => typeof n === 'number'),
		)
		.map(([px, py, pr]) => [px, py, pr ?? 0.5]);
}
