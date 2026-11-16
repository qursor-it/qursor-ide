/** The shas reachable from `from` through the loaded commits' parents (F35):
 *  what HEAD contains. A parent past the loaded page simply ends the walk. */
export function reachableFrom(
	from: string | null | undefined,
	commits: ReadonlyArray<{ sha: string; parents: readonly string[] }>,
): Set<string> {
	const parents = new Map(commits.map((c) => [c.sha, c.parents]));
	const seen = new Set<string>();
	const stack = from ? [from] : [];
	while (stack.length > 0) {
		const sha = stack.pop() as string;
		if (seen.has(sha) || !parents.has(sha)) continue;
		seen.add(sha);
		stack.push(...(parents.get(sha) ?? []));
	}
	return seen;
}
