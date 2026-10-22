import { create } from 'zustand';
import type { Handoff } from '@lib/intelligenceSave';
import type { ErrorNotice } from '@lib/intelligenceView';

/**
 * Changes that were handed to the viewer and are waiting for a person to press
 * Save there (specs/05-features.md F33, ADR-0084 decision 6).
 *
 * **In the renderer and in memory, and that is the point.** The viewer is
 * app-level and the Intelligence tab is not: a draft opened from the tab is
 * saved wherever the person happens to be, so the thing that records the Save
 * cannot live in the tab. It is mounted at the root (`useIntelligenceSaveRecorder`)
 * and this is what it looks at.
 *
 * An entry is a promise to *report* a Save, never permission to make one: it
 * holds the proposal so the report can say "this exact text", and it is created
 * only by the click that opened the draft. Quitting forgets it, which is safe -
 * the backend still has the prepared change, and preparing again gives the same
 * diff.
 */
export type HandoffPhase =
	| { state: 'waiting' }
	| { state: 'recording' }
	/** The backend would not record the Save. The notice is already in words. */
	| { state: 'failed'; notice: ErrorNotice };

interface HandoffEntry {
	handoff: Handoff;
	phase: HandoffPhase;
}

interface HandoffState {
	/** By candidate id: a candidate has at most one change in flight. */
	byCandidate: Record<string, HandoffEntry>;
	begin: (handoff: Handoff) => void;
	setPhase: (candidateId: string, phase: HandoffPhase) => void;
	forget: (candidateId: string) => void;
}

export const useIntelligenceHandoffStore = create<HandoffState>()((set) => ({
	byCandidate: {},
	begin: (handoff) =>
		set((s) => ({
			byCandidate: {
				...s.byCandidate,
				[handoff.candidateId]: { handoff, phase: { state: 'waiting' } },
			},
		})),
	setPhase: (candidateId, phase) =>
		set((s) => {
			const entry = s.byCandidate[candidateId];
			if (!entry) return s;
			return { byCandidate: { ...s.byCandidate, [candidateId]: { ...entry, phase } } };
		}),
	forget: (candidateId) =>
		set((s) => {
			if (!(candidateId in s.byCandidate)) return s;
			const { [candidateId]: _gone, ...rest } = s.byCandidate;
			return { byCandidate: rest };
		}),
}));
