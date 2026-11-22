import { LiveRow, LiveSection } from '@components/layout/SidebarParts';
import { StatusDot } from '@components/layout/StatusDot';
import { useOpenSession } from '@hooks/useOpenSession';
import { useSessionTitles } from '@hooks/useSessionTitles';
import { inboxSections } from '@lib/inbox';
import { queryKeys } from '@lib/queryKeys';
import { tabsInKnownProjects } from '@lib/sessionGroups';
import { cmd } from '@lib/tauri';
import { useAttentionStore } from '@store/attentionStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';

/** Sessions' "In progress" (ADR-0117): the Inbox's two lists (F36), needing you first. */
export function SessionsLive() {
	const tabs = useTerminalStore((s) => s.tabs);
	const bySession = useTerminalStore((s) => s.bySession);
	const unseen = useAttentionStore((s) => s.unseen);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const openSession = useOpenSession();
	const entries = useMemo(() => {
		const { needsYou, working } = inboxSections(
			tabsInKnownProjects(tabs, projectsQ.data),
			bySession,
			unseen,
		);
		return [...needsYou, ...working];
	}, [tabs, bySession, unseen, projectsQ.data]);
	const titles = useSessionTitles(entries);
	const nameOf = new Map((projectsQ.data ?? []).map((p) => [p.id, p.displayName]));

	return (
		<LiveSection
			rows={entries.map((e) => (
				<LiveRow
					key={e.sessionId}
					testId="live-session"
					mark={<StatusDot status={e.status} unseen={unseen[e.sessionId] === true} />}
					label={titles.get(e.sessionId) ?? 'New session'}
					detail={nameOf.get(e.projectId)}
					onClick={() => openSession(e.sessionId, e.projectId)}
				/>
			))}
		/>
	);
}
