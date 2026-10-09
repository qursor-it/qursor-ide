/**
 * Path arithmetic the renderer does on strings the backend handed it.
 * Windows paths compare case-insensitively and accept drive, UNC and extended
 * path prefixes. POSIX paths keep their case-sensitive behavior (ADR-0074).
 */

interface ParsedWindowsPath {
	anchor: string;
	segments: string[];
}

function isWindowsPath(path: string): boolean {
	const normalized = path.replaceAll('/', '\\');
	return (
		/^[a-z]:\\/i.test(normalized) ||
		normalized.startsWith('\\\\?\\') ||
		normalized.startsWith('\\\\.\\') ||
		(normalized.startsWith('\\\\') && normalized.split('\\').filter(Boolean).length >= 2)
	);
}

function parseWindowsPath(path: string): ParsedWindowsPath | null {
	let normalized = path.replaceAll('/', '\\');
	const lower = normalized.toLowerCase();
	if (lower.startsWith('\\\\?\\unc\\')) {
		normalized = `\\\\${normalized.slice(8)}`;
	} else if (normalized.startsWith('\\\\?\\') || normalized.startsWith('\\\\.\\')) {
		normalized = normalized.slice(4);
	}

	const unc = normalized.startsWith('\\\\');
	const body = (unc ? normalized.slice(2) : normalized).replace(/\\+/g, '\\');
	normalized = unc ? `\\\\${body}` : body;

	const drive = /^([a-z]:)(?:\\|$)(.*)$/i.exec(normalized);
	if (drive) {
		return {
			anchor: drive[1].toLowerCase(),
			segments: drive[2].split('\\').filter(Boolean),
		};
	}

	const share = /^\\\\([^\\]+)\\([^\\]+)(?:\\(.*))?$/.exec(normalized);
	if (share) {
		return {
			anchor: `\\\\${share[1]}\\${share[2]}`.toLowerCase(),
			segments: (share[3] ?? '').split('\\').filter(Boolean),
		};
	}

	return null;
}

function isSegmentPrefix(path: readonly string[], root: readonly string[]): boolean {
	return root.every((segment, index) => path[index]?.toLowerCase() === segment.toLowerCase());
}

function windowsSeparator(path: string): string {
	return path.lastIndexOf('\\') > path.lastIndexOf('/') ? '\\' : '/';
}

function posixRoot(root: string): string {
	return root === '/' ? root : root.replace(/\/+$/, '');
}

/**
 * `path` expressed against the project root, for "Copy relative path" (F12).
 *
 * The root is the only base that isn't a guess — it is already threaded into
 * every tree row for `list_dir`. No leading `./`: a path you copy is a path you
 * paste into a shell or an agent prompt, and `./` there is noise. The root row
 * itself is `.`, which is the only honest name a directory has for itself.
 *
 * A path that isn't under the root comes back untouched rather than as a pile
 * of `../` — a symlink target outside the project is somewhere else, and saying
 * so absolutely is more useful than a relative path that only resolves from one
 * working directory.
 */
export function relativeToRoot(path: string, root: string): string {
	if (!root || !path) return path;

	if (isWindowsPath(path) || isWindowsPath(root)) {
		const parsedPath = parseWindowsPath(path);
		const parsedRoot = parseWindowsPath(root);
		if (!parsedPath || !parsedRoot || parsedPath.anchor !== parsedRoot.anchor) return path;
		if (!isSegmentPrefix(parsedPath.segments, parsedRoot.segments)) return path;

		const relative = parsedPath.segments
			.slice(parsedRoot.segments.length)
			.join(windowsSeparator(path));
		if (!relative) return '.';
		return /[\\/]$/.test(path) ? `${relative}${windowsSeparator(path)}` : relative;
	}

	const base = posixRoot(root);
	if (path === base || posixRoot(path) === base) return '.';
	const prefix = base === '/' ? '/' : `${base}/`;
	return path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

/**
 * Is `path` inside `root`, as paths rather than as strings?
 *
 * The separator check is what makes this a path test: `/repo-old/a.ts` starts
 * with `/repo` and is not in it, which a bare `startsWith` would get wrong.
 * Windows roots compare case-insensitively by path segment, including when one
 * side uses an extended drive or UNC prefix. A root is inside itself.
 */
export function isWithin(path: string, root: string): boolean {
	if (isWindowsPath(path) || isWindowsPath(root)) {
		const parsedPath = parseWindowsPath(path);
		const parsedRoot = parseWindowsPath(root);
		return (
			parsedPath !== null &&
			parsedRoot !== null &&
			parsedPath.anchor === parsedRoot.anchor &&
			isSegmentPrefix(parsedPath.segments, parsedRoot.segments)
		);
	}

	const base = posixRoot(root);
	const candidate = posixRoot(path);
	if (candidate === base) return true;
	const prefix = base === '/' ? '/' : `${base}/`;
	return path.startsWith(prefix);
}
