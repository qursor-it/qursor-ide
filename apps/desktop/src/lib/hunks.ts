/**
 * Split one file's unified diff into its header and hunks (F35 hunk staging).
 * A hunk starts at a line beginning `@@ `; body lines begin with ` `, `+`, `-`
 * or `\`, so nothing inside a hunk can be mistaken for the next one, and a
 * `\ No newline at end of file` marker stays with the hunk it follows.
 */
export function splitHunks(patch: string): { header: string; hunks: string[] } {
	const [header = '', ...hunks] = patch.split(/^(?=@@ )/m);
	return { header, hunks };
}

/** The file header plus one hunk, ending in a newline: what `git apply` takes. */
export function hunkPatch(header: string, hunk: string): string {
	const patch = header + hunk;
	return patch.endsWith('\n') ? patch : `${patch}\n`;
}
