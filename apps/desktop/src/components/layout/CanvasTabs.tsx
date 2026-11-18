import { TAB_LOOK } from '@components/layout/SessionTabs';
import { TabGroup } from '@components/layout/TabGroup';
import { DndContext } from '@dnd-kit/core';
import { SortableContext, horizontalListSortingStrategy } from '@dnd-kit/sortable';
import { queryKeys } from '@lib/queryKeys';
import { groupSortId } from '@lib/tabGroups';
import { cmd } from '@lib/tauri';
import { useCanvasStore } from '@store/canvasStore';
import { useQuery } from '@tanstack/react-query';
import { Shapes, X } from 'lucide-react';
import { useState } from 'react';

type CanvasTab = { projectId: string; path: string };

/**
 * The canvas tab strip, row 2 of the top bar in Canvas mode (specs/05-features.md
 * F44): the Git strip's shape, grouped under the same project chips, tabs in the
 * order they were opened. Canvases are opened from the sidebar, so no "+ open".
 */
export function CanvasTabs() {
	const tabs = useCanvasStore((s) => s.tabs);
	const selected = useCanvasStore((s) => s.selected);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const nameOf = (id: string) => projectsQ.data?.find((p) => p.id === id)?.displayName ?? id;
	const groups = [...new Set(tabs.map((t) => t.projectId))].map((projectId) => ({
		projectId,
		tabs: tabs.filter((t) => t.projectId === projectId),
	}));
	const [folded, setFolded] = useState<Set<string>>(new Set());
	const toggle = (id: string) =>
		setFolded((c) => {
			const next = new Set(c);
			if (!next.delete(id)) next.add(id);
			return next;
		});

	return (
		<div
			role="tablist"
			aria-label="Canvas tabs"
			data-testid="canvas-tabs"
			className="scrollbar-none flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto py-0.5"
		>
			{/* `TabGroup` is sortable, so it needs the contexts even with nothing to move. */}
			<DndContext>
				<SortableContext
					items={groups.map((g) => groupSortId(g.projectId))}
					strategy={horizontalListSortingStrategy}
				>
					{groups.map((g) => {
						// The active tab stays drawn when its group is folded.
						const collapsed =
							folded.has(g.projectId) && !g.tabs.some((t) => t.path === selected?.path);
						return (
							<TabGroup
								key={g.projectId}
								projectId={g.projectId}
								name={nameOf(g.projectId)}
								collapsed={collapsed}
								count={g.tabs.length}
								onToggle={toggle}
								onNudge={() => {}}
							>
								{!collapsed &&
									g.tabs.map((tab) => (
										<CanvasTabItem key={tab.path} tab={tab} active={tab.path === selected?.path} />
									))}
							</TabGroup>
						);
					})}
				</SortableContext>
			</DndContext>
		</div>
	);
}

function CanvasTabItem({ tab, active }: { tab: CanvasTab; active: boolean }) {
	const select = useCanvasStore((s) => s.select);
	const closeTab = useCanvasStore((s) => s.closeTab);
	const canvasesQ = useQuery({
		queryKey: queryKeys.canvases(tab.projectId),
		queryFn: () => cmd.listCanvases(tab.projectId),
	});
	const label =
		canvasesQ.data?.find((c) => c.path === tab.path)?.title ??
		tab.path.split(/[\\/]/).pop() ??
		tab.path;

	return (
		<div
			role="tab"
			aria-selected={active}
			tabIndex={0}
			data-testid="canvas-tab"
			data-path={tab.path}
			title={tab.path}
			className={`group flex h-8 max-w-60 shrink-0 cursor-pointer items-center gap-2 rounded-lg px-2.5 text-sm ${TAB_LOOK(active)}`}
			onClick={() => select(tab)}
			onKeyDown={(e) => {
				if (e.key === 'Enter' || e.key === ' ') {
					e.preventDefault();
					select(tab);
				}
			}}
		>
			<Shapes className="size-3.5 shrink-0" aria-hidden />
			<span className="min-w-0 flex-1 truncate">{label}</span>
			<button
				type="button"
				aria-label={`Close canvas tab ${label}`}
				className={`-mr-1 rounded-md p-0.5 text-muted-foreground/70 hover:bg-secondary hover:text-foreground focus-visible:opacity-100 ${
					active ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
				}`}
				onClick={(e) => {
					e.stopPropagation();
					closeTab(tab.path);
				}}
			>
				<X className="size-3" />
			</button>
		</div>
	);
}
