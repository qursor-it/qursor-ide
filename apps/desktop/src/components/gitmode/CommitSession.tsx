import { Button } from '@factorai/ui';
import { useOpenSession } from '@hooks/useOpenSession';
import { queryKeys } from '@lib/queryKeys';
import { sessionDuring } from '@lib/sessionDuring';
import { cmd } from '@lib/tauri';
import { isLooseRepo } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';
import { Sparkles } from 'lucide-react';

/**
 * Over a selected commit's detail in Git mode (F41): the session that was
 * running in this checkout when the commit was made, with a way back to it.
 * Absent for a folder that is no project, and when no session matches.
 */
export function CommitSession({
	projectId,
	dir,
	sha,
}: {
	projectId: string;
	dir: string;
	sha: string;
}) {
	const loose = isLooseRepo(projectId);
	const sessionsQ = useQuery({
		queryKey: queryKeys.sessions(projectId),
		queryFn: () => cmd.listSessions(projectId),
		enabled: !loose,
	});
	// The detail below asks the same question with the same key, so this is
	// one read, cached for good: a commit never changes.
	const commitQ = useQuery({
		queryKey: queryKeys.gitCommit(dir, sha),
		queryFn: () => cmd.gitCommit(dir, sha),
		staleTime: Number.POSITIVE_INFINITY,
		enabled: !loose,
	});
	const openSession = useOpenSession();
	const time = commitQ.data?.commitTime;
	const session = time !== undefined ? sessionDuring(time, sessionsQ.data ?? [], dir) : null;
	if (!session) return null;
	return (
		<div
			data-testid="commit-session"
			className="flex shrink-0 items-center gap-2 border-border border-b bg-primary/10 px-3 py-1.5 text-xs"
		>
			<Sparkles className="size-3.5 shrink-0 text-primary" aria-hidden />
			<span className="min-w-0 truncate">
				Committed while{' '}
				<span className="font-medium">{session.title.trim() || 'an untitled session'}</span> was
				running in this checkout
			</span>
			<Button
				variant="link"
				size="sm"
				className="ml-auto h-auto shrink-0 p-0 text-xs"
				onClick={() => openSession(session.id, projectId)}
			>
				Open session
			</Button>
		</div>
	);
}
