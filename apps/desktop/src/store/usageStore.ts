import type { ProviderUsage } from '@factorai/types';
import { create } from 'zustand';

/**
 * The latest plan limits, every provider and profile (usage monitor). Client
 * state fed by an event stream, so Zustand rather than a query: `usage:changed`
 * carries the whole list, and `useUsageSync` writes it here. Not persisted —
 * Rust persists the snapshots and answers `get_usage` on launch.
 */
interface UsageState {
	usage: ProviderUsage[];
	set: (usage: ProviderUsage[]) => void;
}

export const useUsageStore = create<UsageState>((set) => ({
	usage: [],
	set: (usage) => set({ usage }),
}));
