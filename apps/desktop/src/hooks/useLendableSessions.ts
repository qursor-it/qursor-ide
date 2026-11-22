import type { AgentId } from '@factorai/types';
import { useSessionTitles } from '@hooks/useSessionTitles';
import { useTerminalStore } from '@store/terminalStore';
import { useMemo } from 'react';

interface LendableSession {
	sessionId: string;
	projectId: string;
	agent: AgentId;
	title: string;
}

/** Running agent sessions that can be lent an SSH terminal (F46). */
export function useLendableSessions(): {
	sessions: LendableSession[];
	titleOf: (id: string) => string;
} {
	const bySession = useTerminalStore((s) => s.bySession);
	const entries = useMemo(
		() =>
			Object.entries(bySession).flatMap(([sessionId, t]) =>
				t.agent ? [{ sessionId, projectId: t.projectId, agent: t.agent }] : [],
			),
		[bySession],
	);
	const titles = useSessionTitles(entries);
	const titleOf = (id: string) => titles.get(id) ?? id.slice(0, 8);
	return { sessions: entries.map((e) => ({ ...e, title: titleOf(e.sessionId) })), titleOf };
}
