/**
 * "A human pressed Save and the file was written" (specs/05-features.md F26,
 * ADR-0039), as something other parts of the renderer can hear.
 *
 * `useEditBuffer` is the one place a file is written from the viewer, and until
 * Project Intelligence nothing needed to know when it happened: the buffer
 * adopted the text and that was the end of it. F33 hands a proposed instruction
 * to the viewer as a draft and has to **record** the Save afterwards, and the
 * viewer is not mounted where the Intelligence tab is - so the signal is a
 * module-level subscription rather than a prop.
 *
 * Deliberately an event of the write that happened, with the text that was
 * written, and not a request to write: nothing that listens here can cause a
 * write, which keeps "every byte we write to a project traces to one Save"
 * (ADR-0039) a one-file audit.
 */
export interface FileSavedEvent {
	/** The path `write_file` was given: what the viewer had open. */
	path: string;
	/** Exactly what was written. */
	contents: string;
}

type Listener = (event: FileSavedEvent) => void;

const listeners = new Set<Listener>();

/** Listen for saves. Returns the way to stop. */
export function onFileSaved(listener: Listener): () => void {
	listeners.add(listener);
	return () => {
		listeners.delete(listener);
	};
}

/** Announce a save that has already succeeded. A listener that throws must not
 *  take the others, or the viewer's own save path, down with it. */
export function emitFileSaved(event: FileSavedEvent): void {
	for (const listener of listeners) {
		try {
			listener(event);
		} catch {
			// A listener's failure is its own: the file is written either way.
		}
	}
}
