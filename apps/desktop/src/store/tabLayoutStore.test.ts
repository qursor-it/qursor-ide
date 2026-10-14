import { beforeEach, describe, expect, it } from 'vitest';
import { useTabLayoutStore } from './tabLayoutStore';

const reset = () => useTabLayoutStore.setState({ groupOrder: [], itemOrder: [], collapsed: [] });

describe('tabLayoutStore', () => {
	beforeEach(reset);

	it('starts empty, so existing tabs group themselves', () => {
		const s = useTabLayoutStore.getState();
		expect([s.groupOrder, s.itemOrder, s.collapsed]).toEqual([[], [], []]);
	});

	it('toggles a group collapsed and back', () => {
		const { toggleCollapsed } = useTabLayoutStore.getState();
		toggleCollapsed('a');
		toggleCollapsed('b');
		expect(useTabLayoutStore.getState().collapsed).toEqual(['a', 'b']);
		toggleCollapsed('a');
		expect(useTabLayoutStore.getState().collapsed).toEqual(['b']);
	});

	it('stores the orders it is given', () => {
		useTabLayoutStore.getState().setGroupOrder(['b', 'a']);
		useTabLayoutStore.getState().setItemOrder(['s:1', 'pr:a#2']);
		expect(useTabLayoutStore.getState().groupOrder).toEqual(['b', 'a']);
		expect(useTabLayoutStore.getState().itemOrder).toEqual(['s:1', 'pr:a#2']);
	});

	it('forgets a removed project everywhere it is named', () => {
		useTabLayoutStore.setState({
			groupOrder: ['a', 'b'],
			itemOrder: ['pr:a#1', 's:x', 'pr:b#1', 'pr:ab#1'],
			collapsed: ['a', 'b'],
		});
		useTabLayoutStore.getState().forgetProject('a');
		const s = useTabLayoutStore.getState();
		expect(s.groupOrder).toEqual(['b']);
		expect(s.collapsed).toEqual(['b']);
		expect(s.itemOrder).toEqual(['s:x', 'pr:b#1', 'pr:ab#1']);
	});
});
