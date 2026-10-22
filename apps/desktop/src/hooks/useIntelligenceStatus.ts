import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useQuery } from '@tanstack/react-query';

/**
 * Whether Project Intelligence is on and, while it is, whether its provider can
 * run (specs/05-features.md F33, ADR-0084).
 *
 * One query for the settings row and the project page's tab, so they cannot
 * disagree: the switch is global, and the row that writes it invalidates this.
 * `get_intelligence_status` works while the feature is off and probes nothing,
 * which is what makes asking it on every project page free.
 *
 * `staleTime` is short rather than infinite because the other half of the
 * answer is the machine: installing the CLI while the app is open should show
 * the next time the window is looked at, and a refetch on focus is how it does.
 */
export function useIntelligenceStatus() {
	return useQuery({
		queryKey: queryKeys.intelligenceStatus(),
		queryFn: () => cmd.getIntelligenceStatus(),
		retry: false,
		staleTime: 30_000,
	});
}
