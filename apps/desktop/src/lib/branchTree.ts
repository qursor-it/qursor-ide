/** A branch list as folders by `/` prefix, as Git clients show it (F35). */
export type BranchNode =
	| { kind: 'folder'; name: string; path: string; children: BranchNode[] }
	| { kind: 'branch'; name: string; full: string };

/** Folders first, then branches, each by name; a folder holds its subtree. */
export function branchTree(names: readonly string[]): BranchNode[] {
	const build = (entries: Array<{ rest: string; full: string }>, prefix: string): BranchNode[] => {
		const folders = new Map<string, Array<{ rest: string; full: string }>>();
		const leaves: BranchNode[] = [];
		for (const e of entries) {
			const slash = e.rest.indexOf('/');
			if (slash < 0) {
				leaves.push({ kind: 'branch', name: e.rest, full: e.full });
				continue;
			}
			const head = e.rest.slice(0, slash);
			const list = folders.get(head) ?? [];
			list.push({ rest: e.rest.slice(slash + 1), full: e.full });
			folders.set(head, list);
		}
		const byName = (a: BranchNode, b: BranchNode) => a.name.localeCompare(b.name);
		return [...folders]
			.map(([name, list]): BranchNode => ({
				kind: 'folder',
				name,
				path: `${prefix}${name}`,
				children: build(list, `${prefix}${name}/`),
			}))
			.sort(byName)
			.concat(leaves.sort(byName));
	};
	return build(
		names.map((full) => ({ rest: full, full })),
		'',
	);
}
