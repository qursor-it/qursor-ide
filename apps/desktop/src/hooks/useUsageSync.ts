import { useEffect } from 'react';
import { cmd, events } from '@lib/tauri';
import { useUsageStore } from '@store/usageStore';

/**
 * Keep `usageStore` current: the stored snapshot once, then every
 * `usage:changed`. Mounted once in `__root.tsx` for the app's life, the same
 * shape as `useSessionsSync`, so the status bar can come and go (its pref)
 * without dropping events.
 */
export function useUsageSync(): void {
	useEffect(() => {
		let cancelled = false;
		// An event carries the whole list and is always newer than the stored
		// snapshot, so once one has landed the initial read must not overwrite it.
		let eventSeen = false;
		let unlisten: (() => void) | undefined;
		const set = useUsageStore.getState().set;
		events
			.onUsageChanged((usage) => {
				eventSeen = true;
				set(usage);
			})
			.then((fn) => {
				// Torn down while `listen` was in flight (StrictMode does it): release.
				if (cancelled) fn();
				else unlisten = fn;
			});
		// Requested after the listener, but its answer can still arrive after an
		// event; `eventSeen` keeps that older answer from winning.
		void cmd
			.getUsage()
			.then((usage) => {
				if (!cancelled && !eventSeen) set(usage);
			})
			// Nothing to surface: the bar shows "—" until the first event.
			.catch((e) => console.error('get_usage failed', e));
		return () => {
			cancelled = true;
			unlisten?.();
		};
	}, []);
}
