interface Rect {
	x: number;
	y: number;
	w: number;
	h: number;
}

/** Shrink-wrap a frame to its children. Children are in frame-relative coordinates;
 *  the result holds the frame's new rect and each child's new relative position, so
 *  every child keeps its absolute place. Null when there is nothing to fit. */
export function fitFrame(
	frame: Rect,
	children: (Rect & { id: string })[],
	pad: number,
): { frame: Rect; children: { id: string; x: number; y: number }[] } | null {
	if (!children.length) return null;
	const dx = Math.min(...children.map((c) => c.x)) - pad;
	const dy = Math.min(...children.map((c) => c.y)) - pad;
	return {
		frame: {
			x: frame.x + dx,
			y: frame.y + dy,
			w: Math.max(...children.map((c) => c.x + c.w)) + pad - dx,
			h: Math.max(...children.map((c) => c.y + c.h)) + pad - dy,
		},
		children: children.map((c) => ({ id: c.id, x: c.x - dx, y: c.y - dy })),
	};
}
