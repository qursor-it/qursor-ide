import { Button } from '@factorai/ui';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';

/** Above a session that was started from Git mode (F35): where it came from. */
export function GitOriginStrip({ sessionId }: { sessionId: string }) {
	const origin = useGitModeStore((s) => s.origin[sessionId]);
	const setMode = useGitModeStore((s) => s.setMode);
	const activate = useGitModeStore((s) => s.activate);
	const dir = origin?.dir ?? '';
	const stateQ = useQuery({
		queryKey: queryKeys.gitRepoState(dir),
		queryFn: () => cmd.gitRepoState(dir),
		enabled: Boolean(origin),
		refetchInterval: 3_000,
	});
	if (!origin) return null;
	const left = stateQ.data?.conflicted.length;
	return (
		<div
			data-testid="git-origin-strip"
			className="flex h-9 shrink-0 items-center gap-2 border-b border-border bg-card px-4 text-xs"
		>
			<span className="shrink-0 rounded-sm bg-secondary px-1.5 py-0.5 text-secondary-foreground">
				✦ started from Git
			</span>
			<span className="min-w-0 flex-1 truncate text-muted-foreground">{origin.title}</span>
			{left !== undefined && (
				<span data-testid="git-origin-conflicts" className="shrink-0 tabular-nums">
					{left} conflicted
				</span>
			)}
			<Button
				variant="outline"
				size="sm"
				className="h-6"
				onClick={() => {
					activate(origin.dir);
					setMode('git');
				}}
			>
				Back to Git
			</Button>
		</div>
	);
}
