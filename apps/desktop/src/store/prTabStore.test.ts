import { usePrefsStore } from '@store/prefsStore';
import { beforeEach, describe, expect, it } from 'vitest';
import { mergePrTabs, usePrTabStore } from './prTabStore';

const reset = () => usePrTabStore.setState({ tabs: [] });
const keys = () => usePrTabStore.getState().tabs.map((t) => `${t.projectId}#${t.number}`);

describe('prTabStore', () => {
	beforeEach(reset);

	it('appends a new tab and does not duplicate an open one', () => {
		const { open } = usePrTabStore.getState();
		open('a', 11, 'first');
		open('a', 12);
		open('a', 11);
		expect(keys()).toEqual(['a#11', 'a#12']);
	});

	it('tells the same number in two projects apart', () => {
		const { open } = usePrTabStore.getState();
		open('a', 11);
		open('b', 11);
		expect(keys()).toEqual(['a#11', 'b#11']);
	});

	it('fills a missing title on reopen and keeps an existing one', () => {
		const { open, setTitle } = usePrTabStore.getState();
		open('a', 11);
		open('a', 11, 'late');
		expect(usePrTabStore.getState().tabs[0].title).toBe('late');
		open('a', 11, 'other');
		expect(usePrTabStore.getState().tabs[0].title).toBe('late');
		setTitle('a', 11, 'renamed');
		expect(usePrTabStore.getState().tabs[0].title).toBe('renamed');
	});

	it('closes one tab only', () => {
		const { open, close } = usePrTabStore.getState();
		open('a', 11);
		open('a', 12);
		close('a', 11);
		expect(keys()).toEqual(['a#12']);
	});

	it('drops every tab of a removed project', () => {
		const { open, closeProject } = usePrTabStore.getState();
		open('a', 1);
		open('b', 2);
		open('a', 3);
		closeProject('a');
		expect(keys()).toEqual(['b#2']);
	});

	it('hydrates the persisted tabs only while restoreTabs is on', () => {
		const persisted = { tabs: [{ projectId: 'a', number: 5 }] };
		const current = usePrTabStore.getState();

		usePrefsStore.setState({ restoreTabs: true });
		expect(mergePrTabs(persisted, current).tabs).toHaveLength(1);

		usePrefsStore.setState({ restoreTabs: false });
		expect(mergePrTabs(persisted, current).tabs).toEqual([]);
		usePrefsStore.setState({ restoreTabs: true });
	});
});
