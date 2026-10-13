import { beforeEach, describe, expect, it } from 'vitest';
import { DEFAULT_PR_FILTER, usePrPanelStore } from './prPanelStore';

beforeEach(() => {
	usePrPanelStore.setState({ selected: {}, filter: DEFAULT_PR_FILTER });
});

describe('prPanelStore', () => {
	it('opens on the open pull requests, with nothing selected', () => {
		expect(usePrPanelStore.getState().filter).toBe('open');
		expect(usePrPanelStore.getState().selected).toEqual({});
	});

	it('keeps one selection per project', () => {
		const { select } = usePrPanelStore.getState();
		select('a', 11);
		select('b', 12);
		expect(usePrPanelStore.getState().selected).toEqual({ a: 11, b: 12 });
	});

	it('returns to the list with null, leaving other projects alone', () => {
		const { select } = usePrPanelStore.getState();
		select('a', 11);
		select('b', 12);
		select('a', null);
		expect(usePrPanelStore.getState().selected).toEqual({ a: null, b: 12 });
	});

	it('sets the filter', () => {
		usePrPanelStore.getState().setFilter('merged');
		expect(usePrPanelStore.getState().filter).toBe('merged');
	});
});
