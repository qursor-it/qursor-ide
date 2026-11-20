import type { ProviderUsage, UsageThresholds } from '@factorai/types';
import { DEFAULT_THRESHOLDS } from '@lib/usage';
import { create } from 'zustand';

/**
 * The latest plan limits, every provider and profile (usage monitor). Client
 * state fed by an event stream, so Zustand rather than a query: `usage:changed`
 * carries the whole list, and `useUsageSync` writes it here. Not persisted —
 * Rust persists the snapshots and answers `get_usage` on launch.
 *
 * The colour thresholds ride along: Rust reads them from the two usage
 * settings, `useUsageSync` loads them once and the settings modal updates them
 * on save, so a bar recolours without a reload.
 */
interface UsageState {
	usage: ProviderUsage[];
	thresholds: UsageThresholds;
	set: (usage: ProviderUsage[]) => void;
	setThresholds: (thresholds: UsageThresholds) => void;
	/** The status bar's popover, open on its own so a click on the taskbar
	 *  widget can open it. */
	detailsOpen: boolean;
	setDetailsOpen: (open: boolean) => void;
}

export const useUsageStore = create<UsageState>((set) => ({
	usage: [],
	thresholds: DEFAULT_THRESHOLDS,
	set: (usage) => set({ usage }),
	setThresholds: (thresholds) => set({ thresholds }),
	detailsOpen: false,
	setDetailsOpen: (detailsOpen) => set({ detailsOpen }),
}));
