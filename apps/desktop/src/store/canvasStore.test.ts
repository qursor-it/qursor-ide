import { beforeEach, describe, expect, it, vi } from 'vitest';

/** The tests run in node, so `localStorage` is a two-method stand-in. */
const data = new Map<string, string>();

async function freshStore() {
	vi.resetModules();
	vi.stubGlobal('localStorage', {
		getItem: (k: string) => data.get(k) ?? null,
		setItem: (k: string, v: string) => void data.set(k, v),
		removeItem: (k: string) => void data.delete(k),
	});
	return (await import('./canvasStore')).useCanvasStore;
}

const at = (path: string) => ({ projectId: 'p', path });

beforeEach(() => data.clear());

describe('canvasStore tabs', () => {
	it('select_dedupes_and_appends_in_opening_order', async () => {
		const store = await freshStore();
		for (const p of ['/a', '/b', '/a']) store.getState().select(at(p));
		expect(store.getState().tabs.map((t) => t.path)).toEqual(['/a', '/b']);
		expect(store.getState().selected?.path).toBe('/a');
		store.getState().select(null);
		expect(store.getState().selected).toBeNull();
		expect(store.getState().tabs).toHaveLength(2);
	});

	it('closeTab_of_selected_picks_right_then_left_then_null', async () => {
		const store = await freshStore();
		for (const p of ['/a', '/b', '/c']) store.getState().select(at(p));
		store.getState().select(at('/b'));
		store.getState().closeTab('/b');
		expect(store.getState().selected?.path).toBe('/c');
		store.getState().closeTab('/c');
		expect(store.getState().selected?.path).toBe('/a');
		store.getState().closeTab('/a');
		expect(store.getState().selected).toBeNull();
		expect(store.getState().tabs).toEqual([]);
	});

	it('closeTab_of_a_non_selected_keeps_the_selection', async () => {
		const store = await freshStore();
		for (const p of ['/a', '/b']) store.getState().select(at(p));
		store.getState().closeTab('/a');
		expect(store.getState().selected?.path).toBe('/b');
		expect(store.getState().tabs.map((t) => t.path)).toEqual(['/b']);
	});
	it('restored_selection_without_tabs_gets_a_tab', async () => {
		data.set('factorai.canvas', JSON.stringify({ state: { selected: at('/old') }, version: 1 }));
		const store = await freshStore();
		expect(store.getState().tabs.map((t) => t.path)).toEqual(['/old']);
	});
});
