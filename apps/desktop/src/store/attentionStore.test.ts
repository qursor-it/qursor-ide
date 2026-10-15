import { beforeEach, describe, expect, it } from 'vitest';
import { useAttentionStore } from './attentionStore';

const reset = () =>
	useAttentionStore.setState({ unseen: {}, windowFocused: true, activeSessionId: null });
const s = () => useAttentionStore.getState();

describe('attentionStore', () => {
	beforeEach(reset);

	it('marks a session that wants you when it is not the active one', () => {
		s().setActiveSession('a');
		s().observe('b', 'working', 'waiting_input');
		expect(s().unseen).toEqual({ b: true });
	});

	it('does not mark the active session while the window is focused', () => {
		s().setActiveSession('a');
		s().observe('a', 'working', 'waiting_input');
		expect(s().unseen).toEqual({});
	});

	it('marks the active session while the window is in the background', () => {
		s().setActiveSession('a');
		s().setWindowFocused(false);
		s().observe('a', 'working', 'waiting_input');
		expect(s().unseen).toEqual({ a: true });
	});

	it('clears the active session when the window comes back', () => {
		s().setActiveSession('a');
		s().setWindowFocused(false);
		s().observe('a', 'working', 'waiting_input');
		s().setWindowFocused(true);
		expect(s().unseen).toEqual({});
	});

	it('clears a session when it becomes active in a focused window', () => {
		s().observe('b', 'working', 'stopped');
		s().setActiveSession('b');
		expect(s().unseen).toEqual({});
	});

	it('does not clear on activation while the window is in the background', () => {
		s().setWindowFocused(false);
		s().observe('b', 'working', 'waiting_input');
		s().setActiveSession('b');
		expect(s().unseen).toEqual({ b: true });
	});

	it('clears when the session goes back to working', () => {
		s().observe('b', 'working', 'waiting_input');
		s().observe('b', 'waiting_input', 'working');
		expect(s().unseen).toEqual({});
	});

	it('stopped marks and nothing clears it but viewing', () => {
		s().observe('b', 'working', 'stopped');
		s().observe('b', 'stopped', 'stopped');
		expect(s().unseen).toEqual({ b: true });
	});

	it('moves an unseen mark to the adopted id', () => {
		s().observe('prov', 'working', 'waiting_input');
		s().rebind('prov', 'adopted');
		expect(s().unseen).toEqual({ adopted: true });
	});

	it('rebind leaves the store alone when the session was not unseen', () => {
		s().observe('other', 'working', 'waiting_input');
		const before = s().unseen;
		s().rebind('prov', 'adopted');
		expect(s().unseen).toBe(before);
	});

	it('ignores transitions that are not in the table', () => {
		s().observe('b', 'unknown', 'waiting_input');
		s().observe('c', 'waiting_input', 'stopped');
		expect(s().unseen).toEqual({});
	});
});
