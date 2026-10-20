import type { ProviderUsage } from '@factorai/types';
import { describe, expect, it } from 'vitest';
import {
	barRows,
	formatCost,
	formatCountdown,
	formatPercent,
	formatTokens,
	isStale,
	levelOf,
	sourceLabel,
	windowAt,
} from './usage';

function usage(over: Partial<ProviderUsage>): ProviderUsage {
	return {
		agent: 'claude',
		profileId: 'default',
		profileName: 'Default',
		isDefault: true,
		fiveHour: { usedPercent: 10, resetsAt: null },
		weekly: { usedPercent: 5, resetsAt: null },
		perModel: [],
		source: 'statusLine',
		updatedAt: 0,
		...over,
	};
}

describe('formatPercent', () => {
	it('floors, so the label agrees with the colour at the thresholds', () => {
		expect(formatPercent(89.9)).toBe('89%');
		expect(levelOf(89.9)).toBe('warn');
		expect(formatPercent(69.99)).toBe('69%');
		expect(formatPercent(90)).toBe('90%');
		expect(formatPercent(0)).toBe('0%');
	});

	it('clamps to 0..100 and reads NaN as 0', () => {
		expect(formatPercent(150)).toBe('100%');
		expect(formatPercent(-3)).toBe('0%');
		expect(formatPercent(Number.NaN)).toBe('0%');
	});
});

describe('levelOf', () => {
	it('is neutral below 70, amber from 70, red from 90', () => {
		expect(levelOf(0)).toBe('ok');
		expect(levelOf(69.9)).toBe('ok');
		expect(levelOf(70)).toBe('warn');
		expect(levelOf(89.999)).toBe('warn');
		expect(levelOf(90)).toBe('critical');
		expect(levelOf(100)).toBe('critical');
	});
});

describe('isStale', () => {
	const MIN = 60_000;
	it('is fresh at exactly 15 minutes and stale one millisecond later', () => {
		expect(isStale(0, 15 * MIN)).toBe(false);
		expect(isStale(0, 15 * MIN + 1)).toBe(true);
	});
});

describe('windowAt', () => {
	it('reads a window whose reset time has passed as reset', () => {
		const w = { usedPercent: 95, resetsAt: 1000 };
		expect(windowAt(w, 999)).toBe(w);
		expect(windowAt(w, 1000)).toEqual({ usedPercent: 0, resetsAt: null });
		expect(windowAt({ usedPercent: 40, resetsAt: null }, 5000)).toEqual({
			usedPercent: 40,
			resetsAt: null,
		});
		expect(windowAt(null, 0)).toBeNull();
	});
});

describe('formatCountdown', () => {
	const now = 1_000_000_000_000;
	const min = 60_000;
	const at = (ms: number) => formatCountdown(now + ms, now);

	it('reads under a minute as <1m', () => {
		expect(at(0)).toBe('<1m');
		expect(at(59_000)).toBe('<1m');
	});
	it('shows minutes under an hour', () => {
		expect(at(min)).toBe('1m');
		expect(at(45 * min)).toBe('45m');
		expect(at(59 * min)).toBe('59m');
	});
	it('shows hours and always the minutes under a day', () => {
		expect(at(60 * min)).toBe('1h 0m');
		expect(at(3 * 60 * min)).toBe('3h 0m');
		expect(at(134 * min)).toBe('2h 14m');
		expect(at((23 * 60 + 59) * min)).toBe('23h 59m');
	});
	it('shows days and hours from 24h up', () => {
		expect(at(24 * 60 * min)).toBe('1d 0h');
		expect(at((3 * 24 + 4) * 60 * min)).toBe('3d 4h');
		expect(at(((6 * 24 + 23) * 60 + 59) * min)).toBe('6d 23h');
	});
	it('clamps a past reset to <1m', () => {
		expect(at(-5 * min)).toBe('<1m');
	});
});

describe('formatTokens', () => {
	it('is compact', () => {
		expect(formatTokens(950)).toBe('950');
		expect(formatTokens(12_300)).toBe('12.3k');
		expect(formatTokens(1_800_000)).toBe('1.8M');
	});
});

describe('formatCost', () => {
	it('is an estimate to the cent, or a dash when unknown', () => {
		expect(formatCost(4.123)).toBe('~$4.12');
		expect(formatCost(null)).toBe('—');
	});
});

describe('barRows', () => {
	it('keeps the default profile, Claude before Codex', () => {
		const codex = usage({ agent: 'codex', profileId: 'c1' });
		const claude = usage({ agent: 'claude', profileId: 'a1' });
		const work = usage({ agent: 'claude', profileId: 'a2', isDefault: false });
		expect(barRows([codex, work, claude], ['claude', 'codex'])).toEqual([
			{ agent: 'claude', usage: claude },
			{ agent: 'codex', usage: codex },
		]);
	});

	it('gives an installed agent with no data a null row', () => {
		const claude = usage({ agent: 'claude' });
		expect(barRows([claude], ['claude', 'codex'])).toEqual([
			{ agent: 'claude', usage: claude },
			{ agent: 'codex', usage: null },
		]);
	});

	it('has no row for an agent that is not installed', () => {
		const codex = usage({ agent: 'codex' });
		expect(barRows([usage({}), codex], ['claude'])).toEqual([
			{ agent: 'claude', usage: usage({}) },
		]);
	});

	it('does not show a non-default profile in the bar', () => {
		const work = usage({ isDefault: false });
		expect(barRows([work], ['claude'])).toEqual([{ agent: 'claude', usage: null }]);
	});
});

describe('sourceLabel', () => {
	it('names every source', () => {
		expect(sourceLabel('statusLine')).toBe('Claude Code status line');
		expect(sourceLabel('claudeApi')).toBe('Anthropic usage endpoint');
		expect(sourceLabel('codexAppServer')).toBe('Codex app-server');
		expect(sourceLabel('codexLog')).toBe('Codex session log');
	});
});
