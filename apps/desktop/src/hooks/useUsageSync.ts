import { useEffect } from 'react';
import { cmd, events } from '@lib/tauri';
import { parseThresholds } from '@lib/usage';
import { usePrefsStore } from '@store/prefsStore';
import { useUsageStore } from '@store/usageStore';

/**
 * Keep `usageStore` current: the stored snapshot once, then every
 * `usage:changed`; the colour thresholds once; and the taskbar widget's
 * request to open the details. Mounted once in `__root.tsx` for the app's
 * life, the same shape as `useSessionsSync`, so the status bar can come and go
 * (its pref) without dropping events.
 */
export function useUsageSync(): void {
	useEffect(() => {
		let cancelled = false;
		// An event carries the whole list and is always newer than the stored
		// snapshot, so once one has landed the initial read must not overwrite it.
		let eventSeen = false;
		const unlisteners: (() => void)[] = [];
		const { set, setThresholds, setDetailsOpen } = useUsageStore.getState();
		const listen = (subscribe: Promise<() => void>) =>
			subscribe.then((fn) => {
				// Torn down while `listen` was in flight (StrictMode does it): release.
				if (cancelled) fn();
				else unlisteners.push(fn);
			});
		void listen(
			events.onUsageChanged((usage) => {
				eventSeen = true;
				set(usage);
			}),
		);
		// A hidden status bar has no popover to open; the click then only focused
		// the window, and opening it later from stale state would surprise.
		void listen(
			events.onUsageOpenDetails(() => {
				if (usePrefsStore.getState().showStatusBar) setDetailsOpen(true);
			}),
		);
		// Requested after the listener, but its answer can still arrive after an
		// event; `eventSeen` keeps that older answer from winning.
		void cmd
			.getUsage()
			.then((usage) => {
				if (!cancelled && !eventSeen) set(usage);
			})
			// Nothing to surface: the bar shows "—" until the first event.
			.catch((e) => console.error('get_usage failed', e));
		// Unset is the defaults, which is what the store starts with.
		void Promise.all([cmd.getSetting('usageWarnPercent'), cmd.getSetting('usageCriticalPercent')])
			.then(([warn, critical]) => {
				if (!cancelled) setThresholds(parseThresholds(warn, critical));
			})
			.catch((e) => console.error('reading the usage thresholds failed', e));
		return () => {
			cancelled = true;
			for (const fn of unlisteners) fn();
		};
	}, []);
}
