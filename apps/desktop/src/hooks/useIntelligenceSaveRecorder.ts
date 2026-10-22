import { describeIntelligenceError } from '@lib/intelligenceView';
import { onFileSaved } from '@lib/fileSaved';
import { recordSaved } from '@lib/intelligenceSave';
import { queryKeys } from '@lib/queryKeys';
import { useIntelligenceHandoffStore } from '@store/intelligenceHandoffStore';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';

/**
 * Report the viewer's Save to the backend (specs/05-features.md F33, ADR-0084
 * decision 6).
 *
 * A proposed instruction is opened in the viewer as a draft; when the person
 * presses Save there, `useEditBuffer` announces the write and this records it -
 * the backend then re-reads the file and moves the candidate to `implemented`
 * only if the disk agrees. It **writes nothing**: it hears a write that already
 * happened and reports it.
 *
 * Mounted once at the root, beside `useRoutinesChanged` and for the same reason:
 * the viewer is app-level, so the Save can come from any route - the person
 * opens the draft from the Intelligence tab and presses Save from a session.
 */
export function useIntelligenceSaveRecorder(): void {
	const queryClient = useQueryClient();

	useEffect(() => {
		return onFileSaved(({ path, contents }) => {
			const { byCandidate, setPhase, forget } = useIntelligenceHandoffStore.getState();
			for (const entry of Object.values(byCandidate)) {
				const { handoff, phase } = entry;
				if (handoff.targetPath !== path || phase.state === 'recording') continue;
				setPhase(handoff.candidateId, { state: 'recording' });
				void recordSaved(handoff, contents).then(
					() => {
						forget(handoff.candidateId);
						void queryClient.invalidateQueries({
							queryKey: queryKeys.improvements(handoff.projectId),
						});
					},
					(error: unknown) => {
						// Kept as `failed`, not forgotten: the person is looking at a
						// file they just saved, and the panel says what is wrong with it.
						setPhase(handoff.candidateId, {
							state: 'failed',
							notice: describeIntelligenceError(error),
						});
					},
				);
			}
		});
	}, [queryClient]);
}
