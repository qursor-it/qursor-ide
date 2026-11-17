/** Snapshot stack for undo/redo: React Flow has none built in (F44). */
export function createHistory<T>(initial: T, max = 100) {
	let stack = [initial];
	let at = 0;
	return {
		/** A new state after an edit; whatever was undone is dropped. */
		record(state: T) {
			stack = [...stack.slice(0, at + 1), state].slice(-max);
			at = stack.length - 1;
		},
		undo: (): T | undefined => (at > 0 ? stack[--at] : undefined),
		redo: (): T | undefined => (at < stack.length - 1 ? stack[++at] : undefined),
		/** A fresh start, after the file was reloaded from disk. */
		reset(state: T) {
			stack = [state];
			at = 0;
		},
	};
}
