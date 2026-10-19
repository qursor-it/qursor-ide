import type { ProviderUsage } from '@factorai/types';
import { describe, expect, it } from 'vitest';
import {
	barRows,
	formatCost,
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
