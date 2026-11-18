interface Rect {
	x: number;
	y: number;
	width: number;
	height: number;
}
interface Viewport {
	x: number;
	y: number;
	zoom: number;
}

/** A user gesture this recent means the board is theirs: never pan under them. */
const INTERACTION_QUIET_MS = 2000;

export function boundsOf(rects: Rect[]): Rect | null {
	if (!rects.length) return null;
	const x = Math.min(...rects.map((r) => r.x));
	const y = Math.min(...rects.map((r) => r.y));
	return {
		x,
		y,
		width: Math.max(...rects.map((r) => r.x + r.width)) - x,
		height: Math.max(...rects.map((r) => r.y + r.height)) - y,
	};
}

/** The part of the flow plane the screen shows. */
export function viewportRect(v: Viewport, width: number, height: number): Rect {
	return { x: -v.x / v.zoom, y: -v.y / v.zoom, width: width / v.zoom, height: height / v.zoom };
}

const intersects = (a: Rect, b: Rect) =>
	a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

/** Follow only when new nodes arrived, none is on screen, and the user is idle. */
export function shouldFollow(
	newRects: Rect[],
	view: Rect,
	lastInteractionMs: number,
	nowMs: number,
): boolean {
	if (!newRects.length) return false;
	if (nowMs - lastInteractionMs < INTERACTION_QUIET_MS) return false;
	return !newRects.some((r) => intersects(r, view));
}

/** Centre `box`, keeping `zoom` unless the box (plus margin) needs less, never below `minZoom`. */
export function followViewport(
	box: Rect,
	width: number,
	height: number,
	zoom: number,
	minZoom: number,
	margin = 64,
): Viewport {
	const fit = Math.min(width / (box.width + margin * 2), height / (box.height + margin * 2));
	const z = Math.max(minZoom, Math.min(zoom, fit));
	return {
		zoom: z,
		x: width / 2 - (box.x + box.width / 2) * z,
		y: height / 2 - (box.y + box.height / 2) * z,
	};
}
