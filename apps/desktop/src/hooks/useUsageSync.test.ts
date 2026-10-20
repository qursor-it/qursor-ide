import type { ProviderUsage } from '@factorai/types';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
	listener: null as null | ((u: unknown) => void),
	resolveGet: null as null | ((u: unknown) => void),
}));

// The effect runs once, immediately: enough to exercise the race without a DOM.
vi.mock('react', () => ({ useEffect: (fn: () => unknown) => void fn() }));
vi.mock('@lib/tauri', () => ({
	events: {
		onUsageChanged: (cb: (u: unknown) => void) => {
			h.listener = cb;
			return Promise.resolve(() => {});
		},
	},
	cmd: { getUsage: () => new Promise((r) => (h.resolveGet = r)) },
}));

import { useUsageStore } from '@store/usageStore';
import { useUsageSync } from './useUsageSync';

const u = (name: string) => [{ profileName: name }] as unknown as ProviderUsage[];

describe('useUsageSync', () => {
	beforeEach(() => useUsageStore.getState().set([]));

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
});
