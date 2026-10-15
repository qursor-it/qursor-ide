/**
 * What dropping files on a terminal types (specs/05-features.md F5, ADR-0083).
 *
 * The text is what a real terminal puts on the line when you drop a file on it:
 * the path, quoted so the shell and Claude Code read it back as one argument,
 * separated by spaces, with a trailing space so the next thing typed does not
 * glue onto the last path. Claude Code turns a pasted image path into an
 * `[Image #N]` attachment itself and reads any other file from its path.
 */

/** Characters that are safe unquoted in a POSIX shell word. Everything else is
 *  backslash-escaped, which is what macOS Terminal and iTerm do. */
const POSIX_SAFE = /[A-Za-z0-9_\-.,:+@%/=]/;

const BACKSLASH = '\\';

function quotePosix(path: string): string {
	// A line break cannot be backslash-escaped (`\` + newline is a continuation
	// and vanishes), so a path that has one takes single quotes instead.
	if (/[\r\n]/.test(path)) return `'${path.replaceAll("'", `'${BACKSLASH}''`)}'`;
	let out = '';
	for (const ch of path) out += POSIX_SAFE.test(ch) ? ch : `${BACKSLASH}${ch}`;
	return out;
}

/** What `cmd` and PowerShell both treat as a word break or an operator. A Windows
 *  path cannot contain `"`, so double quotes always round-trip. */
const WINDOWS_SPECIAL = /[\s&()[\]{}^=;!'+,`~%$@#]/;

function quoteWindows(path: string): string {
	return WINDOWS_SPECIAL.test(path) ? `"${path}"` : path;
}

/**
 * The text a drop of `paths` puts on the terminal's input line, or `''` for no
 * paths. `windows` picks the quoting — double quotes there, backslash escapes
 * everywhere else — and is passed in rather than read, so both are testable.
 */
export function formatDroppedPaths(paths: readonly string[], windows: boolean): string {
	if (paths.length === 0) return '';
	const quote = windows ? quoteWindows : quotePosix;
	return `${paths.map(quote).join(' ')} `;
}

/**
 * Tauri reports a drop in **physical** pixels; `elementFromPoint` wants CSS
 * pixels. `devicePixelRatio` is the right divisor and the window's scale factor
 * is not: it already folds in the webview zoom (F15), which scales CSS pixels
 * without moving the window, so physical / ratio is the CSS pixel the pointer is
 * over at any zoom.
 */
export function toCssPoint(
	position: { x: number; y: number },
	devicePixelRatio: number,
): { x: number; y: number } {
	const ratio = devicePixelRatio > 0 ? devicePixelRatio : 1;
	return { x: position.x / ratio, y: position.y / ratio };
}
