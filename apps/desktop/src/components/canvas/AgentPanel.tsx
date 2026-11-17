import { Terminal, sendToSession } from '@components/terminal/Terminal';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Composer, type NodeRef } from './Composer';
import { buildMessage } from './contextLine';

const RETRY_MS = 250;
const GIVE_UP_MS = 20_000;

/** Resolves true once the session's PTY took the message, false after 20 s. */
async function sendWhenLive(sessionId: string, message: string): Promise<boolean> {
	for (let waited = 0; waited <= GIVE_UP_MS; waited += RETRY_MS) {
		if (sendToSession(sessionId, message, true)) return true;
		await new Promise((r) => setTimeout(r, RETRY_MS));
	}
	return false;
}

/** The right card of Canvas mode: the canvas's own Claude session and a composer (F44). */
export function AgentPanel({
	projectId,
	relPath,
	absPath,
	projectCwd,
	sessionId,
	onSession,
	selectedIds,
	nodes,
	inputRef,
}: {
	projectId: string;
	relPath: string;
	absPath: string;
	projectCwd: string | null | undefined;
	/** The linked session, `null` before the first message. */
	sessionId: string | null;
	/** Called with a freshly minted session id, before it is indexed. */
	onSession: (id: string) => void;
	selectedIds: string[];
	nodes: NodeRef[];
	inputRef: React.Ref<HTMLTextAreaElement>;
}) {
	const qc = useQueryClient();
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const send = async (text: string): Promise<boolean> => {
		setBusy(true);
		setError(null);
		try {
			let id = sessionId;
			if (!id) {
				// Fresh: never hand the canvas somebody's idle session.
				id = await cmd.startSession(projectId, 'claude', true);
				await cmd.setCanvasSession(projectId, relPath, id);
				onSession(id);
				// Mounts the Terminal, which is what spawns the PTY to send into.
				await qc.invalidateQueries({ queryKey: queryKeys.canvasSession(projectId, relPath) });
				void qc.invalidateQueries({ queryKey: queryKeys.canvasOrigins(projectId) });
			}
			if (await sendWhenLive(id, buildMessage(absPath, selectedIds, text))) return true;
			setError('The session did not start in time. Your message is still here.');
		} catch (e) {
			setError(e instanceof Error ? e.message : 'Could not reach the agent.');
		} finally {
			setBusy(false);
		}
		return false;
	};

	return (
		<div data-testid="canvas-agent-panel" className="flex min-h-0 flex-1 flex-col">
			{sessionId ? (
				<div className="min-h-0 flex-1">
					<Terminal
						sessionId={sessionId}
						projectId={projectId}
						projectCwd={projectCwd}
						sessionCwd={null}
						surface="canvas"
					/>
				</div>
			) : (
				<p className="flex-1 p-4 text-muted-foreground text-sm">
					The first message starts a Claude session for this canvas.
				</p>
			)}
			<Composer
				inputRef={inputRef}
				selectedCount={selectedIds.length}
				nodes={nodes}
				busy={busy}
				error={error}
				onSend={send}
			/>
		</div>
	);
}
