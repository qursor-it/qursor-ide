import {
	Button,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
	Tabs,
	TabsList,
	TabsTrigger,
} from '@factorai/ui';
import type { AgentId } from '@factorai/types';
import { Terminal, sendToSession } from '@components/terminal/Terminal';
import { useCanvasStore } from '@store/canvasStore';
import { useTerminalStore } from '@store/terminalStore';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Composer, type NodeRef } from './Composer';
import { buildMessage, buildPrimer } from './contextLine';

const RETRY_MS = 250;
const AGENT_NAME: Record<AgentId, string> = { claude: 'Claude', codex: 'Codex' };
const GIVE_UP_MS = 20_000;

/** Resolves true once the session's PTY took the message, false after 20 s. */
async function sendWhenLive(sessionId: string, message: string): Promise<boolean> {
	for (let waited = 0; waited <= GIVE_UP_MS; waited += RETRY_MS) {
		if (sendToSession(sessionId, message, true)) return true;
		await new Promise((r) => setTimeout(r, RETRY_MS));
	}
	return false;
}

/** The right card of Canvas mode: the canvas's own agent session (Claude or Codex) and a composer (F44). */
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
	const agent = useCanvasStore((s) => s.agent);
	const setAgent = useCanvasStore((s) => s.setAgent);
	// Re-picking the session of a canvas that already has one (F44).
	const [choosing, setChoosing] = useState(false);
	const picking = !sessionId || choosing;
	const sessionsQ = useQuery({
		queryKey: queryKeys.sessions(projectId),
		queryFn: () => cmd.listSessions(projectId),
		enabled: picking,
	});
	const existing = (sessionsQ.data ?? [])
		.filter((s) => !s.subagentOf && s.id !== sessionId)
		.sort((a, b) => b.updatedAt - a.updatedAt)
		.slice(0, 30);

	const link = async (id: string) => {
		setError(null);
		try {
			await cmd.setCanvasSession(projectId, relPath, id);
			onSession(id);
			setChoosing(false);
			await qc.invalidateQueries({ queryKey: queryKeys.canvasSession(projectId, relPath) });
			void qc.invalidateQueries({ queryKey: queryKeys.canvasOrigins(projectId) });
			// A linked session has never seen this canvas; tell it once.
			if (!(await sendWhenLive(id, buildPrimer(absPath))))
				setError('Linked, but the session did not take the canvas context yet.');
		} catch (e) {
			setError(e instanceof Error ? e.message : 'Could not link the session.');
		}
	};

	const send = async (text: string): Promise<boolean> => {
		setBusy(true);
		setError(null);
		try {
			let id = picking ? null : sessionId;
			if (!id) {
				// Fresh: never hand the canvas somebody's idle session.
				id = await cmd.startSession(projectId, agent, true);
				// The spawn reads the launch override, as for the `+` menu's agent pick (F30).
				useTerminalStore.getState().setLaunchAgent(id, agent);
				await cmd.setCanvasSession(projectId, relPath, id);
				onSession(id);
				setChoosing(false);
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
			{!picking && sessionId ? (
				<div className="flex min-h-0 flex-1 flex-col">
					<div className="flex shrink-0 items-center gap-2 border-border border-b px-3 py-1">
						<p className="min-w-0 flex-1 truncate text-muted-foreground text-xs">
							Write below to attach the canvas and the selected nodes
						</p>
						<Button
							variant="ghost"
							size="sm"
							className="h-6 text-xs"
							data-testid="canvas-change-session"
							onClick={() => setChoosing(true)}
						>
							Change session
						</Button>
					</div>
					<div className="min-h-0 flex-1">
						<Terminal
							sessionId={sessionId}
							projectId={projectId}
							projectCwd={projectCwd}
							sessionCwd={null}
							surface="canvas"
						/>
					</div>
				</div>
			) : (
				<div className="flex-1 space-y-3 p-4">
					<Tabs value={agent} onValueChange={(v) => setAgent(v as AgentId)}>
						<TabsList aria-label="Agent for this canvas">
							<TabsTrigger value="claude" data-testid="canvas-agent-claude">
								Claude
							</TabsTrigger>
							<TabsTrigger value="codex" data-testid="canvas-agent-codex">
								Codex
							</TabsTrigger>
						</TabsList>
					</Tabs>
					<p className="text-muted-foreground text-sm">
						The next message starts a new {AGENT_NAME[agent]} session for this canvas.
					</p>
					{existing.length > 0 && (
						<div className="space-y-1.5">
							<p className="text-muted-foreground text-xs">Or link a session already running:</p>
							<Select value="" onValueChange={(id) => void link(id)}>
								<SelectTrigger
									data-testid="canvas-link-session"
									aria-label="Link an existing session"
								>
									<SelectValue placeholder="Pick a session…" />
								</SelectTrigger>
								<SelectContent>
									{existing.map((s) => (
										<SelectItem key={s.id} value={s.id}>
											{s.title || s.id.slice(0, 8)}
										</SelectItem>
									))}
								</SelectContent>
							</Select>
						</div>
					)}
					{choosing && (
						<Button variant="outline" size="sm" onClick={() => setChoosing(false)}>
							Keep the current session
						</Button>
					)}
				</div>
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
