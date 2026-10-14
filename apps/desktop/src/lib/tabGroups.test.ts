import { describe, expect, it } from 'vitest';
import {
	groupSortId,
	groupTabs,
	moveGroupByDelta,
	moveGroupTo,
	moveItemByDelta,
	moveItemTo,
	prTabKey,
	projectIdOfGroupSortId,
	sessionTabKey,
	visibleKeys,
} from './tabGroups';

const s = (sessionId: string, projectId: string) => ({ sessionId, projectId });
const pr = (projectId: string, number: number) => ({ projectId, number });
const empty = { groupOrder: [], itemOrder: [], collapsed: [] };

const keysOf = (groups: Array<{ projectId: string; items: Array<{ key: string }> }>) =>
	groups.map((g) => [g.projectId, g.items.map((i) => i.key)]);

describe('groupTabs', () => {
	it('groups by project in order of first appearance, sessions before PRs', () => {
		const groups = groupTabs({
			sessions: [s('1', 'a'), s('2', 'b'), s('3', 'a')],
			prs: [pr('c', 1), pr('a', 7)],
			layout: empty,
			activeKey: null,
		});
		expect(keysOf(groups)).toEqual([
			['a', ['s:1', 's:3', 'pr:a#7']],
			['b', ['s:2']],
			['c', ['pr:c#1']],
		]);
	});

	it('orders groups and items by the layout, appending unknowns', () => {
		const groups = groupTabs({
			sessions: [s('1', 'a'), s('2', 'b'), s('3', 'a'), s('4', 'a')],
			prs: [],
			layout: { groupOrder: ['b'], itemOrder: ['s:4', 's:1'], collapsed: [] },
			activeKey: null,
		});
		expect(keysOf(groups)).toEqual([
			['b', ['s:2']],
			['a', ['s:4', 's:1', 's:3']],
		]);
	});

	it('ignores stale keys and projects in the layout', () => {
		const groups = groupTabs({
			sessions: [s('1', 'a')],
			prs: [],
			layout: { groupOrder: ['gone', 'a'], itemOrder: ['s:dead', 's:1'], collapsed: ['gone'] },
			activeKey: null,
		});
		expect(keysOf(groups)).toEqual([['a', ['s:1']]]);
	});

	it('has no group for a project with no tab', () => {
		expect(groupTabs({ sessions: [], prs: [], layout: empty, activeKey: null })).toEqual([]);
	});

	it('shows everything while expanded', () => {
		const [g] = groupTabs({
			sessions: [s('1', 'a'), s('2', 'a')],
			prs: [],
			layout: empty,
			activeKey: 's:1',
		});
		expect(g?.collapsed).toBe(false);
		expect(g?.visibleItems.map((i) => i.key)).toEqual(['s:1', 's:2']);
	});

	it('shows nothing while collapsed unless the active tab is in the group', () => {
		const layout = { ...empty, collapsed: ['a'] };
		const input = { sessions: [s('1', 'a'), s('2', 'a')], prs: [pr('a', 3)], layout };
		const [none] = groupTabs({ ...input, activeKey: null });
		expect(none?.collapsed).toBe(true);
		expect(none?.visibleItems).toEqual([]);
		expect(none?.items).toHaveLength(3);
		const [other] = groupTabs({ ...input, activeKey: 's:other' });
		expect(other?.visibleItems).toEqual([]);
		const [active] = groupTabs({ ...input, activeKey: prTabKey('a', 3) });
		expect(active?.visibleItems.map((i) => i.key)).toEqual(['pr:a#3']);
	});

	it('mixes sessions and PRs of one project in one group', () => {
		const [g] = groupTabs({
			sessions: [s('1', 'a')],
			prs: [pr('a', 2)],
			layout: { ...empty, itemOrder: ['pr:a#2', 's:1'] },
			activeKey: null,
		});
		expect(g?.items.map((i) => i.kind)).toEqual(['pr', 'session']);
	});
});

describe('visibleKeys', () => {
	it('skips the hidden tabs of collapsed groups', () => {
		const groups = groupTabs({
			sessions: [s('1', 'a'), s('2', 'a'), s('3', 'b')],
			prs: [],
			layout: { ...empty, collapsed: ['a'] },
			activeKey: 's:2',
		});
		expect(visibleKeys(groups)).toEqual(['s:2', 's:3']);
	});
});

describe('moves', () => {
	const groups = groupTabs({
		sessions: [s('1', 'a'), s('2', 'b'), s('3', 'a'), s('4', 'a')],
		prs: [],
		layout: empty,
		activeKey: null,
	});

	it('moves an item to another item of its group, writing the full order', () => {
		expect(moveItemTo(groups, 's:4', 's:1')).toEqual(['s:4', 's:1', 's:3', 's:2']);
	});

	it('ignores a drop onto another group, onto itself, or onto nothing', () => {
		expect(moveItemTo(groups, 's:1', 's:2')).toBeNull();
		expect(moveItemTo(groups, 's:1', 's:1')).toBeNull();
		expect(moveItemTo(groups, 's:1', 's:nope')).toBeNull();
	});

	it('nudges an item within its group and stops at the ends', () => {
		expect(moveItemByDelta(groups, 's:1', 1)).toEqual(['s:3', 's:1', 's:4', 's:2']);
		expect(moveItemByDelta(groups, 's:1', -1)).toBeNull();
		expect(moveItemByDelta(groups, 's:4', 1)).toBeNull();
		expect(moveItemByDelta(groups, 's:2', 1)).toBeNull();
	});

	it('moves and nudges groups', () => {
		expect(moveGroupTo(groups, 'b', 'a')).toEqual(['b', 'a']);
		expect(moveGroupTo(groups, 'a', 'a')).toBeNull();
		expect(moveGroupByDelta(groups, 'a', 1)).toEqual(['b', 'a']);
		expect(moveGroupByDelta(groups, 'a', -1)).toBeNull();
		expect(moveGroupByDelta(groups, 'b', 1)).toBeNull();
	});
});

describe('keys', () => {
	it('round-trips a group sort id and keeps item keys apart from it', () => {
		expect(projectIdOfGroupSortId(groupSortId('a'))).toBe('a');
		expect(projectIdOfGroupSortId(sessionTabKey('a'))).toBeNull();
	});
});
