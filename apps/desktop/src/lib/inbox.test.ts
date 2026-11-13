import { describe, expect, it } from 'vitest';
import { inboxSections } from './inbox';

describe('inboxSections', () => {
	it('puts waiting and unseen sessions first, then working ones, and leaves seen stopped ones out', () => {
		const tabs = [
			{ sessionId: 'a', projectId: 'p1' }, // working
			{ sessionId: 'b', projectId: 'p1' }, // waiting
			{ sessionId: 'c', projectId: 'p2' }, // stopped, seen
			{ sessionId: 'd', projectId: 'p2' }, // stopped, unseen
		];
		const bySession = {
			a: { projectId: 'p1', status: 'working' as const },
			b: { projectId: 'p1', status: 'waiting_input' as const },
			r: { projectId: 'p3', status: 'working' as const }, // a routine, no tab
		};
		const { needsYou, working } = inboxSections(tabs, bySession, { d: true });
		expect(needsYou.map((e) => e.sessionId)).toEqual(['b', 'd']);
		expect(working.map((e) => e.sessionId)).toEqual(['a', 'r']);
		expect(needsYou[1]).toEqual({ sessionId: 'd', projectId: 'p2', status: 'stopped' });
	});

	it('lists a session once even when it is both a tab and live', () => {
		const { working } = inboxSections(
			[{ sessionId: 'a', projectId: 'p1' }],
			{ a: { projectId: 'p1', status: 'working' } },
			{},
		);
		expect(working).toHaveLength(1);
	});

	it('counts a working session that is still marked unseen as wanting you', () => {
		const { needsYou, working } = inboxSections(
			[],
			{ a: { projectId: 'p1', status: 'working' } },
			{ a: true },
		);
		expect(needsYou).toHaveLength(1);
		expect(working).toHaveLength(0);
	});
});
