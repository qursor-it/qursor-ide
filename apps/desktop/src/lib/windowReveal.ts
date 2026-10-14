import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauri } from '@lib/tauri';

/**
 * Show the main window once there is something in it (PERF-22,
 * `specs/10-performance.md`).
 *
 * On Windows the window starts hidden (`tauri.windows.conf.json`), because
 * WebView2 is a process of its own that takes a few hundred milliseconds to
 * start and paint, and a window shown before that is an empty white box that
 * looks like a hang. The renderer shows it from the root's first commit, and
 * from the boot-failure screen so a broken boot is still seen; Rust shows it
 * after a few seconds regardless, for the boot that never runs a line of this.
 *
 * Elsewhere the window is already visible and `show` changes nothing, so this
 * carries no platform branch. A factory so the once-only rule is testable
 * without a webview.
 */
export function createWindowRevealer(
	show: () => Promise<void>,
	inTauri: () => boolean,
): () => void {
	let revealed = false;
	return () => {
		if (revealed || !inTauri()) return;
		revealed = true;
		void show().catch((e) => console.error('window show failed', e));
	};
}

export const revealWindow = createWindowRevealer(() => getCurrentWindow().show(), isTauri);
