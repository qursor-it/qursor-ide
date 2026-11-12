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
	return (await import('./gitModeStore')).useGitModeStore;
}

beforeEach(() => data.clear());

describe('gitModeStore', () => {
	it('openTab_dedupes_by_dir_and_activates', async () => {
		const store = await freshStore();
		store.getState().openTab('p1', '/a');
		store.getState().openTab('p1', '/b');
		store.getState().openTab('p1', '/a');
		expect(store.getState().tabs.map((t) => t.dir)).toEqual(['/a', '/b']);
		expect(store.getState().activeDir).toBe('/a');
	});

	it('closing_active_tab_activates_neighbour', async () => {
		const store = await freshStore();
		for (const dir of ['/a', '/b', '/c']) store.getState().openTab('p1', dir);
		store.getState().activate('/b');
		store.getState().closeTab('/b');
		expect(store.getState().activeDir).toBe('/c');
		store.getState().closeTab('/c');
		expect(store.getState().activeDir).toBe('/a');
		store.getState().closeTab('/a');
		expect(store.getState().activeDir).toBeNull();
	});

	it('mode_persists_under_factorai_gitmode', async () => {
		const store = await freshStore();
		store.getState().setMode('git');
		const saved = JSON.parse(data.get('factorai.gitmode') ?? '{}');
		expect(saved.state.mode).toBe('git');
		expect(saved.version).toBe(1);
	});

	it('preRebase_and_origin_are_not_persisted', async () => {
		const store = await freshStore();
		store.getState().setPreRebase('/a', 'abc');
		store.getState().setOrigin('s1', { dir: '/a', title: 't' });
		const saved = JSON.parse(data.get('factorai.gitmode') ?? '{}');
		expect(saved.state.preRebase).toBeUndefined();
		expect(saved.state.origin).toBeUndefined();
	});
});
