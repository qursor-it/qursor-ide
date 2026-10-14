/**
 * The top-bar tab strip, grouped by project (specs/05-features.md F16,
 * ADR-0080).
 *
 * Pure on purpose: what is open comes from `terminalStore` and `prTabStore`,
 * how it is arranged from `tabLayoutStore`, and this is the one place the two
 * meet, so the ordering rules are testable without React or a store.
 */

const SESSION_KEY_PREFIX = 's:';
const PR_KEY_PREFIX = 'pr:';
const GROUP_ID_PREFIX = 'g:';

/** The key a session tab is ordered, sorted and found by. */
export function sessionTabKey(sessionId: string): string {
	return `${SESSION_KEY_PREFIX}${sessionId}`;
}

/** The key a pull request tab is ordered, sorted and found by: the project's id
 *  keeps the same number in two projects apart. */
export function prTabKey(projectId: string, number: number): string {
	return `${PR_KEY_PREFIX}${projectId}#${number}`;
}

/** The part of a pull request key after its prefix, which is what the tab's
 *  `data-pr-tab` attribute carries. */
export function prTabDomId(key: string): string {
	return key.startsWith(PR_KEY_PREFIX) ? key.slice(PR_KEY_PREFIX.length) : key;
}

/** Whether `key` names a pull request tab of `projectId`. */
export function isPrKeyOfProject(key: string, projectId: string): boolean {
	return key.startsWith(`${PR_KEY_PREFIX}${projectId}#`);
}

/** The id a group is sorted by, kept apart from the item keys. */
export function groupSortId(projectId: string): string {
	return `${GROUP_ID_PREFIX}${projectId}`;
}

/** The project id behind a `groupSortId`, or null for anything else. */
export function projectIdOfGroupSortId(id: string): string | null {
	return id.startsWith(GROUP_ID_PREFIX) ? id.slice(GROUP_ID_PREFIX.length) : null;
}

export interface TabLayout {
	groupOrder: readonly string[];
	itemOrder: readonly string[];
	collapsed: readonly string[];
}

interface SessionRef {
	sessionId: string;
	projectId: string;
}

interface PrRef {
	projectId: string;
	number: number;
}

export type TabItem<S extends SessionRef, P extends PrRef> =
	| { kind: 'session'; key: string; projectId: string; tab: S }
	| { kind: 'pr'; key: string; projectId: string; tab: P };

export interface TabGroupModel<S extends SessionRef, P extends PrRef> {
	projectId: string;
	collapsed: boolean;
	/** Every open tab of the project, in the order the user arranged. */
	items: TabItem<S, P>[];
	/** What is drawn: all of `items`, or while collapsed only the active one. */
	visibleItems: TabItem<S, P>[];
}

/** Stable sort by position in `order`; entries it does not name keep their
 *  relative order and follow the ones it does. */
function arrange<T>(entries: readonly T[], keyOf: (e: T) => string, order: readonly string[]): T[] {
	const rank = new Map<string, number>();
	order.forEach((k, i) => {
		if (!rank.has(k)) rank.set(k, i);
	});
	const known: Array<{ e: T; r: number }> = [];
	const unknown: T[] = [];
	for (const e of entries) {
		const r = rank.get(keyOf(e));
		if (r === undefined) unknown.push(e);
		else known.push({ e, r });
	}
	known.sort((a, b) => a.r - b.r);
	return [...known.map((k) => k.e), ...unknown];
}

/**
 * Group the open tabs by project.
 *
 * Groups follow `layout.groupOrder`; a project it does not name is appended in
 * order of first appearance, sessions before pull requests, each in its store's
 * (open) order. Items inside a group follow `layout.itemOrder` the same way.
 * Keys in the layout that name nothing open are ignored, and a project with no
 * tab has no group.
 */
export function groupTabs<S extends SessionRef, P extends PrRef>({
	sessions,
	prs,
	layout,
	activeKey,
}: {
	sessions: readonly S[];
	prs: readonly P[];
	layout: TabLayout;
	activeKey: string | null;
}): TabGroupModel<S, P>[] {
	const all: TabItem<S, P>[] = [
		...sessions.map((tab) => ({
			kind: 'session' as const,
			key: sessionTabKey(tab.sessionId),
			projectId: tab.projectId,
			tab,
		})),
		...prs.map((tab) => ({
			kind: 'pr' as const,
			key: prTabKey(tab.projectId, tab.number),
			projectId: tab.projectId,
			tab,
		})),
	];

	const byProject = new Map<string, TabItem<S, P>[]>();
	for (const item of all) {
		const list = byProject.get(item.projectId);
		if (list) list.push(item);
		else byProject.set(item.projectId, [item]);
	}

	const projectIds = arrange([...byProject.keys()], (id) => id, layout.groupOrder);
	const collapsed = new Set(layout.collapsed);

	return projectIds.map((projectId) => {
		const items = arrange(byProject.get(projectId) ?? [], (i) => i.key, layout.itemOrder);
		const isCollapsed = collapsed.has(projectId);
		return {
			projectId,
			collapsed: isCollapsed,
			items,
			visibleItems: isCollapsed ? items.filter((i) => i.key === activeKey) : items,
		};
	});
}

/** The keys that are drawn, left to right: what next/previous tab steps
 *  through. A collapsed group contributes only its active tab. */
export function visibleKeys<S extends SessionRef, P extends PrRef>(
	groups: readonly TabGroupModel<S, P>[],
): string[] {
	return groups.flatMap((g) => g.visibleItems.map((i) => i.key));
}

function move<T>(list: readonly T[], from: number, to: number): T[] {
	const next = [...list];
	const [moved] = next.splice(from, 1);
	if (moved === undefined) return next;
	next.splice(Math.max(0, Math.min(next.length, to)), 0, moved);
	return next;
}

/** Every open key, group by group: the full `itemOrder` as drawn, with stale
 *  keys left out. */
function flatItemOrder<S extends SessionRef, P extends PrRef>(
	groups: readonly TabGroupModel<S, P>[],
	replace: { projectId: string; keys: string[] },
): string[] {
	return groups.flatMap((g) =>
		g.projectId === replace.projectId ? replace.keys : g.items.map((i) => i.key),
	);
}

/** The new `itemOrder` after dropping `activeKey` where `overKey` is, or null
 *  when nothing moves. Both must be in the same group; a drop across groups
 *  does nothing. */
export function moveItemTo<S extends SessionRef, P extends PrRef>(
	groups: readonly TabGroupModel<S, P>[],
	activeKey: string,
	overKey: string,
): string[] | null {
	if (activeKey === overKey) return null;
	const group = groups.find((g) => g.items.some((i) => i.key === activeKey));
	if (!group) return null;
	const keys = group.items.map((i) => i.key);
	const from = keys.indexOf(activeKey);
	const to = keys.indexOf(overKey);
	if (from < 0 || to < 0) return null;
	return flatItemOrder(groups, { projectId: group.projectId, keys: move(keys, from, to) });
}

/** The new `itemOrder` after nudging a tab one place within its group, or null
 *  at either end. */
export function moveItemByDelta<S extends SessionRef, P extends PrRef>(
	groups: readonly TabGroupModel<S, P>[],
	key: string,
	delta: -1 | 1,
): string[] | null {
	const group = groups.find((g) => g.items.some((i) => i.key === key));
	if (!group) return null;
	const keys = group.items.map((i) => i.key);
	const from = keys.indexOf(key);
	const to = from + delta;
	if (to < 0 || to >= keys.length) return null;
	return flatItemOrder(groups, { projectId: group.projectId, keys: move(keys, from, to) });
}

/** The new `groupOrder` after dropping group `projectId` where `overProjectId`
 *  is, or null when nothing moves. */
export function moveGroupTo<S extends SessionRef, P extends PrRef>(
	groups: readonly TabGroupModel<S, P>[],
	projectId: string,
	overProjectId: string,
): string[] | null {
	if (projectId === overProjectId) return null;
	const ids = groups.map((g) => g.projectId);
	const from = ids.indexOf(projectId);
	const to = ids.indexOf(overProjectId);
	if (from < 0 || to < 0) return null;
	return move(ids, from, to);
}

/** The new `groupOrder` after nudging a group one place, or null at either
 *  end. */
export function moveGroupByDelta<S extends SessionRef, P extends PrRef>(
	groups: readonly TabGroupModel<S, P>[],
	projectId: string,
	delta: -1 | 1,
): string[] | null {
	const ids = groups.map((g) => g.projectId);
	const from = ids.indexOf(projectId);
	const to = from + delta;
	if (from < 0 || to < 0 || to >= ids.length) return null;
	return move(ids, from, to);
}
