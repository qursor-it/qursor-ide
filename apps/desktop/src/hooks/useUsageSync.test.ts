import type { ProviderUsage } from '@factorai/types';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
	listener: null as null | ((u: unknown) => void),
	resolveGet: null as null | ((u: unknown) => void),
	openDetails: null as null | (() => void),
	settings: {} as Record<string, string | null>,
	showStatusBar: true,
}));

// The effect runs once, immediately: enough to exercise the race without a DOM.
vi.mock('react', () => ({ useEffect: (fn: () => unknown) => void fn() }));
vi.mock('@lib/tauri', () => ({
	events: {
		onUsageChanged: (cb: (u: unknown) => void) => {
			h.listener = cb;
			return Promise.resolve(() => {});
		},
		onUsageOpenDetails: (cb: () => void) => {
			h.openDetails = cb;
			return Promise.resolve(() => {});
		},
	},
	cmd: {
		getUsage: () => new Promise((r) => (h.resolveGet = r)),
		getSetting: (key: string) => Promise.resolve(h.settings[key] ?? null),
	},
}));
vi.mock('@store/prefsStore', () => ({
	usePrefsStore: { getState: () => ({ showStatusBar: h.showStatusBar }) },
}));

import { DEFAULT_THRESHOLDS } from '@lib/usage';
import { useUsageStore } from '@store/usageStore';
import { useUsageSync } from './useUsageSync';

const u = (name: string) => [{ profileName: name }] as unknown as ProviderUsage[];

describe('useUsageSync', () => {
	beforeEach(() => {
		useUsageStore.getState().set([]);
		useUsageStore.setState({ thresholds: DEFAULT_THRESHOLDS, detailsOpen: false });
		h.settings = {};
		h.showStatusBar = true;
	});

	it('keeps an event that landed before the initial read answered', async () => {
		useUsageSync();
		h.listener?.(u('event'));
		h.resolveGet?.(u('stale'));
		await new Promise((r) => setTimeout(r, 0));
		expect(useUsageStore.getState().usage).toEqual(u('event'));
	});

	it('applies the initial read when no event came', async () => {
		useUsageSync();
		h.resolveGet?.(u('stored'));
		await new Promise((r) => setTimeout(r, 0));
		expect(useUsageStore.getState().usage).toEqual(u('stored'));
	});

	it('loads the colour thresholds from the two settings', async () => {
		h.settings = { usageWarnPercent: '50', usageCriticalPercent: '80' };
		useUsageSync();
		await new Promise((r) => setTimeout(r, 0));
		expect(useUsageStore.getState().thresholds).toEqual({ warn: 50, critical: 80 });
	});

	it('keeps the defaults for settings that are invalid together', async () => {
		h.settings = { usageWarnPercent: '95', usageCriticalPercent: '90' };
		useUsageSync();
		await new Promise((r) => setTimeout(r, 0));
		expect(useUsageStore.getState().thresholds).toEqual(DEFAULT_THRESHOLDS);
	});

	it('opens the details when the widget asks, and only while the status bar is shown', async () => {
		useUsageSync();
		h.openDetails?.();
		expect(useUsageStore.getState().detailsOpen).toBe(true);
		useUsageStore.setState({ detailsOpen: false });
		h.showStatusBar = false;
		h.openDetails?.();
		expect(useUsageStore.getState().detailsOpen).toBe(false);
	});
});
