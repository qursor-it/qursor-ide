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
		let unlisten: (() => void) | undefined;
		const set = useUsageStore.getState().set;
		events
			.onUsageChanged((usage) => set(usage))
			.then((fn) => {
				// Torn down while `listen` was in flight (StrictMode does it): release.
				if (cancelled) fn();
				else unlisten = fn;
			});
		// After the listener is requested, so an event racing this call is not
		// overwritten by an older answer — the event carries the whole list, and
		// a list that arrives second is the newer one either way.
		void cmd
			.getUsage()
			.then((usage) => {
				if (!cancelled) set(usage);
			})
			// Nothing to surface: the bar shows "—" until the first event.
			.catch((e) => console.error('get_usage failed', e));
		return () => {
			cancelled = true;
			unlisten?.();
		};
	}, []);
}
