import { beforeEach, describe, expect, it } from 'vitest';
import { DEFAULT_PR_FILTER, usePrPanelStore } from './prPanelStore';

beforeEach(() => {
	usePrPanelStore.setState({ filter: DEFAULT_PR_FILTER });
});

describe('prPanelStore', () => {
	it('opens on the open pull requests', () => {
		expect(usePrPanelStore.getState().filter).toBe('open');
	});

	it('sets the filter', () => {
		usePrPanelStore.getState().setFilter('merged');
		expect(usePrPanelStore.getState().filter).toBe('merged');
	});
});
