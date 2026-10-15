import type { TerminalStatus } from '@factorai/types';
import { wantsYou } from '@lib/attention';
import { create } from 'zustand';

/**
 * Which sessions started wanting you while you were not looking (F32,
 * ADR-0081). Keyed by session id, because every surface that draws the dot is.
 *
 * Not persisted: like the terminals it describes, it does not survive a
 * restart. Not cleared on exit either — `stopped` is followed at once by
 * `terminal:exit`, and clearing there would erase the dot it just set.
 */
interface AttentionState {
	unseen: Record<string, true>;
	/** The app starts in front; Tauri reports only changes. */
	windowFocused: boolean;
	/** The route's session, mirrored here by `__root`. */
	activeSessionId: string | null;
	observe: (sessionId: string, prev: TerminalStatus | undefined, next: TerminalStatus) => void;
	setWindowFocused: (focused: boolean) => void;
	setActiveSession: (sessionId: string | null) => void;
}

function without(unseen: Record<string, true>, sessionId: string): Record<string, true> {
	if (!unseen[sessionId]) return unseen;
	const { [sessionId]: _gone, ...rest } = unseen;
	return rest;
}

export const useAttentionStore = create<AttentionState>()((set) => ({
	unseen: {},
	windowFocused: true,
	activeSessionId: null,
	observe: (sessionId, prev, next) =>
		set((s) => {
			if (next === 'working') return { unseen: without(s.unseen, sessionId) };
			if (!wantsYou(prev, next)) return s;
			if (s.windowFocused && s.activeSessionId === sessionId) return s;
			return { unseen: { ...s.unseen, [sessionId]: true } };
		}),
	setWindowFocused: (focused) =>
		set((s) => ({
			windowFocused: focused,
			unseen: focused && s.activeSessionId ? without(s.unseen, s.activeSessionId) : s.unseen,
		})),
	setActiveSession: (sessionId) =>
		set((s) => ({
			activeSessionId: sessionId,
			unseen: sessionId && s.windowFocused ? without(s.unseen, sessionId) : s.unseen,
		})),
}));
