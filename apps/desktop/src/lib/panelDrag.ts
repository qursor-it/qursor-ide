/**
 * Whether a `PanelResizer` drag is in progress (PERF-30).
 *
 * Module state rather than a store, because nothing renders from it: the one
 * reader is the terminal's `pushSize`, which asks at the moment a fit lands.
 * A drag changes the session column's width on every frame, and a terminal
 * that told the PTY about each of those would make the agent behind it redraw
 * its whole screen each time, with that output coming back through the stream
 * while the drag is still going. So the grid follows the column on every
 * frame, and the PTY hears the last size once, when the drag ends.
 */

let dragging = false;
const onEnd = new Set<() => void>();

export function isPanelDragging(): boolean {
	return dragging;
}

export function beginPanelDrag(): void {
	dragging = true;
}

/** Idempotent: `pointerup` and `lostpointercapture` both arrive for one drag. */
export function endPanelDrag(): void {
	if (!dragging) return;
	dragging = false;
	for (const fn of onEnd) fn();
}

/** Run `fn` each time a drag ends. Returns the unsubscribe. */
export function onPanelDragEnd(fn: () => void): () => void {
	onEnd.add(fn);
	return () => {
		onEnd.delete(fn);
	};
}
