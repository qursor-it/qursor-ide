import { AgentPanel } from './AgentPanel';
import type { NodeRef } from './Composer';
import { PanelEmpty } from '@components/layout/PanelEmpty';
import { PanelResizer } from '@components/layout/PanelResizer';
import { LazyCanvasBoard } from '@components/viewer/lazyViews';
import { IconButton } from '@factorai/ui';
import { useTerminalStore } from '@store/terminalStore';
import { useFileViewer } from '@hooks/useFileViewer';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useCanvasStore } from '@store/canvasStore';
import { useGitModeStore } from '@store/gitModeStore';
import { clampSidebarWidth, useSidebarStore } from '@store/sidebarStore';
import { useQuery } from '@tanstack/react-query';
import { PanelRight, X } from 'lucide-react';
import { Suspense, useRef, useState } from 'react';
import { CanvasSidebar } from './CanvasSidebar';
import type { SaveState } from './useCanvasDoc';

const SAVE_LABEL: Record<SaveState, string> = {
	saved: 'Saved',
	saving: 'Saving…',
	unsaved: 'Unsaved',
	conflict: 'Changed on disk',
};

/** The full-window Canvas mode: sidebar, the board's card, the agent's slot (F44). */
export function CanvasModeView() {
	const width = useSidebarStore((s) => s.width);
	const setWidth = useSidebarStore((s) => s.setWidth);
	const selected = useCanvasStore((s) => s.selected);
	const setMode = useGitModeStore((s) => s.setMode);
	const viewer = useFileViewer();
	const openAsText = (path: string) => {
		// The viewer lives in the Sessions body, which is what the mode switch brings back.
		setMode('sessions');
		viewer.open(path);
	};
	return (
		<div data-testid="canvas-mode-view" className="flex min-h-0 min-w-0 flex-1 pr-2 pb-2">
			<aside style={{ width }} className="flex shrink-0 flex-col bg-chrome">
				<CanvasSidebar />
			</aside>
			<PanelResizer
				size={width}
				onSize={setWidth}
				edge="right"
				label="Resize sidebar"
				clamp={clampSidebarWidth}
			/>
			{selected ? (
				// Keyed on the file so one board never shows another's state.
				<CanvasCard key={selected.path} onOpenAsText={openAsText} />
			) : (
				<section className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-xl border border-border bg-background shadow-card">
					<PanelEmpty>Pick a canvas, or create one from a project.</PanelEmpty>
				</section>
			)}
		</div>
	);
}

function CanvasCard({ onOpenAsText }: { onOpenAsText: (path: string) => void }) {
	const selected = useCanvasStore((s) => s.selected);
	const select = useCanvasStore((s) => s.select);
	const panelOpen = useCanvasStore((s) => s.panelOpen);
	const togglePanel = useCanvasStore((s) => s.togglePanel);
	const panelWidth = useCanvasStore((s) => s.panelWidth);
	const setPanelWidth = useCanvasStore((s) => s.setPanelWidth);
	const [saveState, setSaveState] = useState<SaveState>('saved');
	const [selectedIds, setSelectedIds] = useState<string[]>([]);
	const [nodeRefs, setNodeRefs] = useState<NodeRef[]>([]);
	const composer = useRef<HTMLTextAreaElement>(null);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const canvasesQ = useQuery({
		queryKey: queryKeys.canvases(selected?.projectId ?? ''),
		queryFn: () => cmd.listCanvases(selected?.projectId ?? ''),
		enabled: !!selected,
	});
	const project = projectsQ.data?.find((p) => p.id === selected?.projectId);
	const entry = canvasesQ.data?.find((c) => c.path === selected?.path);
	const projectId = selected?.projectId ?? '';
	const relPath = entry?.relPath ?? '';
	const sessionQ = useQuery({
		queryKey: queryKeys.canvasSession(projectId, relPath),
		queryFn: () => cmd.getCanvasSession(projectId, relPath),
		enabled: !!entry,
	});
	const sessionId = sessionQ.data ?? null;
	const connected = useTerminalStore((s) => !!sessionId && !!s.bySession[sessionId]);
	if (!selected) return null;
	return (
		<>
			<section className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-xl border border-border bg-background shadow-card">
				<header className="flex shrink-0 items-center gap-2 border-border border-b px-4 py-2">
					<div className="flex min-w-0 flex-1 flex-col">
						<div className="flex items-center gap-2 text-muted-foreground text-xs">
							<span className="truncate">{project?.displayName}</span>
							<span className="truncate font-mono">{entry?.relPath}</span>
						</div>
						<h2 data-testid="canvas-title" className="truncate font-semibold text-base">
							{entry?.title}
						</h2>
					</div>
					<span data-testid="canvas-save-state" className="text-muted-foreground text-xs">
						{SAVE_LABEL[saveState]}
					</span>
					<IconButton
						aria-label={panelOpen ? 'Hide agent panel' : 'Show agent panel'}
						onClick={togglePanel}
					>
						<PanelRight />
					</IconButton>
					<IconButton aria-label="Close canvas" onClick={() => select(null)}>
						<X />
					</IconButton>
				</header>
				<Suspense fallback={<div className="flex-1" />}>
					<LazyCanvasBoard
						projectId={selected.projectId}
						path={selected.path}
						onOpenAsText={onOpenAsText}
						onAsk={() => {
							// The selection is already attached live; this brings the composer up.
							if (!panelOpen) togglePanel();
							setTimeout(() => composer.current?.focus(), 0);
						}}
						onSelection={setSelectedIds}
						onNodeRefs={setNodeRefs}
						onSaveState={setSaveState}
					/>
				</Suspense>
			</section>
			{panelOpen && (
				<>
					<PanelResizer
						size={panelWidth}
						onSize={setPanelWidth}
						edge="left"
						label="Resize agent panel"
						clamp={(n) => Math.min(640, Math.max(280, n))}
					/>
					<aside
						data-testid="canvas-agent-slot"
						style={{ width: panelWidth }}
						className="flex shrink-0 flex-col overflow-hidden rounded-xl border border-border bg-background shadow-card"
					>
						<header className="flex shrink-0 items-center gap-2 border-border border-b px-4 py-2">
							<h3 className="flex-1 font-semibold text-sm">Agent</h3>
							{connected && (
								<span
									data-testid="canvas-agent-connected"
									className="rounded-full bg-emerald-500/10 px-2 py-0.5 text-emerald-300 text-xs"
								>
									● Agent connected
								</span>
							)}
						</header>
						{entry && (
							<AgentPanel
								projectId={selected.projectId}
								relPath={entry.relPath}
								absPath={entry.path}
								projectCwd={projectsQ.isPending ? undefined : (project?.realPath ?? null)}
								sessionId={sessionId}
								selectedIds={selectedIds}
								nodes={nodeRefs}
								inputRef={composer}
							/>
						)}
					</aside>
				</>
			)}
		</>
	);
}
