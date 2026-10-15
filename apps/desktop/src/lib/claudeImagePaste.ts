/**
 * Pasting an image into Claude Code through the terminal (specs/05-features.md
 * F5, ADR-0083).
 *
 * Claude Code does not read images from the terminal's paste: it reads them from
 * the OS clipboard when it receives its own image-paste key, `Alt+V` on Windows
 * and `Ctrl+V` on macOS and Linux. xterm turns the paste chord's `paste` event
 * into a text paste, and a clipboard holding only an image has no text, so
 * nothing would reach Claude. These two decide when to send the key instead and
 * which one it is.
 */

/** The bytes of Claude Code's image-paste key: `ESC v` is `Alt+V`, `0x16` is
 *  `Ctrl+V`. */
export function claudeImagePasteInput(windows: boolean): string {
	return windows ? '\x1bv' : '\x16';
}

/** The slice of `DataTransfer` the decision reads, so a test needs no DOM. */
interface PasteData {
	getData: (format: string) => string;
	items: ArrayLike<{ kind: string; type: string }>;
}

/**
 * Whether a paste carries an image and no text. An image copied from a browser or
 * a document often travels with a text flavour too (its URL, its alt text), and
 * that is a text paste: Claude Code gets the text, as it would from any terminal.
 */
export function isImageOnlyPaste(data: PasteData): boolean {
	if (data.getData('text/plain').trim() !== '') return false;
	for (let i = 0; i < data.items.length; i++) {
		const item = data.items[i];
		if (item.kind === 'file' && item.type.startsWith('image/')) return true;
	}
	return false;
}

/**
 * `Ctrl+V` on Windows, which xterm would write as `0x16` and cancel — so the
 * browser never fires the `paste` that carries the clipboard, and Windows has
 * no use for `0x16` in Claude Code. Letting the key through to the browser is
 * what turns it into a paste, text or image. macOS pastes with `Cmd+V`, which
 * xterm already leaves alone, and on Linux `0x16` *is* Claude Code's own key.
 */
export function isWindowsPasteChord(event: {
	key: string;
	ctrlKey: boolean;
	shiftKey: boolean;
	altKey: boolean;
	metaKey: boolean;
}): boolean {
	return (
		event.key.toLowerCase() === 'v' &&
		event.ctrlKey &&
		!event.shiftKey &&
		!event.altKey &&
		!event.metaKey
	);
}
