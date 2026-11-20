import type { Profile } from '@factorai/types';
import { describe, expect, it } from 'vitest';
import {
	DEFAULT_TASKBAR_WIDGET,
	hasDefaultGeometry,
	moveAgent,
	profilesFor,
	sameWidget,
	thresholdError,
	thresholdSetting,
	withDefaultGeometry,
} from './taskbarWidget';

describe('moveAgent', () => {
	const agents = DEFAULT_TASKBAR_WIDGET.agents;

	it('swaps neighbours', () => {
		expect(moveAgent(agents, 0, 1).map((a) => a.agent)).toEqual(['codex', 'claude']);
		expect(moveAgent(agents, 1, -1).map((a) => a.agent)).toEqual(['codex', 'claude']);
	});

	it('leaves the list alone at either end', () => {
		expect(moveAgent(agents, 0, -1)).toBe(agents);
		expect(moveAgent(agents, 1, 1)).toBe(agents);
	});
});

describe('geometry', () => {
	it('resets position and size and nothing else', () => {
		const moved = { ...DEFAULT_TASKBAR_WIDGET, gap: 60, width: 320, fontPx: 14 };
		expect(hasDefaultGeometry(moved)).toBe(false);
		const reset = withDefaultGeometry(moved);
		expect(hasDefaultGeometry(reset)).toBe(true);
		expect(reset.fontPx).toBe(14);
	});

	it('compares configs by what they hold', () => {
		expect(sameWidget(DEFAULT_TASKBAR_WIDGET, structuredClone(DEFAULT_TASKBAR_WIDGET))).toBe(true);
		expect(sameWidget(DEFAULT_TASKBAR_WIDGET, { ...DEFAULT_TASKBAR_WIDGET, click: 'none' })).toBe(
			false,
		);
	});
});

describe('profilesFor', () => {
	it('keeps one agent’s profiles, in order', () => {
		const p = (id: string, agent: 'claude' | 'codex') => ({ id, agent }) as unknown as Profile;
		const list = [p('a', 'claude'), p('b', 'codex'), p('c', 'claude')];
		expect(profilesFor(list, 'claude').map((x) => x.id)).toEqual(['a', 'c']);
		expect(profilesFor(list, 'codex').map((x) => x.id)).toEqual(['b']);
	});
});

describe('thresholds', () => {
	it('writes whole numbers in 1..=100, and nothing otherwise', () => {
		expect(thresholdSetting('85')).toBe('85');
		expect(thresholdSetting(' 85 ')).toBe('85');
		expect(thresholdSetting('')).toBeNull();
		expect(thresholdSetting('0')).toBeNull();
		expect(thresholdSetting('101')).toBeNull();
		expect(thresholdSetting('7.5')).toBeNull();
	});

	it('accepts blanks and a warning below the critical', () => {
		expect(thresholdError('', '')).toBeNull();
		expect(thresholdError('50', '80')).toBeNull();
		expect(thresholdError('50', '')).toBeNull();
		expect(thresholdError('', '95')).toBeNull();
	});

	it('refuses out-of-range numbers and a warning at or above the critical', () => {
		expect(thresholdError('0', '80')).toMatch(/1 to 100/);
		expect(thresholdError('50', '101')).toMatch(/1 to 100/);
		expect(thresholdError('abc', '80')).toMatch(/1 to 100/);
		expect(thresholdError('80', '80')).toMatch(/below critical/);
		expect(thresholdError('95', '')).toMatch(/below critical/);
		expect(thresholdError('', '95')).toBeNull();
		expect(thresholdError('', '60')).toMatch(/below critical/);
	});
});
