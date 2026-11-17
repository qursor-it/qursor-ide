import { Button } from '@factorai/ui';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useCanvasStore } from '@store/canvasStore';
import { useGitModeStore } from '@store/gitModeStore';
import { useQuery } from '@tanstack/react-query';

/** Above the session a canvas started (F44): where it came from, and the way back. */
export function CanvasOriginStrip({
	sessionId,
	projectId,
}: {
	sessionId: string;
	projectId: string;
}) {
	const setMode = useGitModeStore((s) => s.setMode);
	const select = useCanvasStore((s) => s.select);
	const originsQ = useQuery({
		queryKey: queryKeys.canvasOrigins(projectId),
		queryFn: () => cmd.listCanvasOrigins(projectId),
	});
	// The entry carries the path the OS gave, so no separator is guessed here.
	const canvasesQ = useQuery({
		queryKey: queryKeys.canvases(projectId),
		queryFn: () => cmd.listCanvases(projectId),
	});
	const origin = originsQ.data?.find((o) => o.sessionId === sessionId);
	const entry = canvasesQ.data?.find((c) => c.relPath === origin?.relPath);
	if (!origin || !entry) return null;
	return (
		<div
			data-testid="canvas-origin-strip"
			className="flex h-9 shrink-0 items-center gap-2 border-b border-border bg-card px-4 text-xs"
		>
			<span className="shrink-0 rounded-sm bg-secondary px-1.5 py-0.5 text-secondary-foreground">
				◇ from canvas
			</span>
			<span className="min-w-0 flex-1 truncate text-muted-foreground">{origin.title}</span>
			<Button
				variant="outline"
				size="sm"
				className="h-6"
				onClick={() => {
					select({ projectId, path: entry.path });
					setMode('canvas');
				}}
			>
				Open canvas
			</Button>
		</div>
	);
}
