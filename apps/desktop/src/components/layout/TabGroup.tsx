import { ProjectIcon } from '@components/layout/ProjectIcon';
import { useSortable } from '@dnd-kit/sortable';
// Aliased for the same reason as in `SessionTabs`: the global `CSS.escape`.
import { CSS as DndCss } from '@dnd-kit/utilities';
import { useProjectSwatch } from '@hooks/useProjectSwatch';
import { groupSortId } from '@lib/tabGroups';
import type { ReactNode } from 'react';

interface TabGroupProps {
	projectId: string;
	name: string;
	collapsed: boolean;
	/** How many tabs the project has open, shown on the chip while folded. */
	count: number;
	onToggle: (projectId: string) => void;
	onNudge: (projectId: string, delta: -1 | 1) => void;
	children: ReactNode;
}

/**
 * One project's run of tabs: a label chip in the project's colour, the tabs,
 * and a 2px line of the same colour under the lot (specs/05-features.md F16,
 * ADR-0080).
 *
 * The chip is the drag handle for the whole group — `setActivatorNodeRef` on
 * it, so a press on a tab still drags the tab — and a click on it folds the
 * group. The 4px activation constraint on the sensor is what keeps the second
 * from being swallowed by the first. As on the tabs, dnd-kit's `attributes` are
 * not spread: they would make the chip a `button` pointing at lift-and-drop
 * instructions, when the keyboard path is `Alt`+arrows.
 */
export function TabGroup({
	projectId,
	name,
	collapsed,
	count,
	onToggle,
	onNudge,
	children,
}: TabGroupProps) {
	const swatch = useProjectSwatch(name);
	const { setNodeRef, setActivatorNodeRef, listeners, transform, transition, isDragging } =
		useSortable({ id: groupSortId(projectId) });

	return (
		<div
			ref={setNodeRef}
			role="group"
			aria-label={name}
			data-testid="tab-group"
			data-project-id={projectId}
			data-collapsed={collapsed}
			style={{ transform: DndCss.Translate.toString(transform), transition }}
			className={`relative flex shrink-0 items-center gap-1 pb-0.5 ${isDragging ? 'z-10' : ''}`}
		>
			<div
				ref={setActivatorNodeRef}
				role="button"
				tabIndex={0}
				aria-expanded={!collapsed}
				aria-label={`${name} tabs`}
				aria-keyshortcuts="Alt+ArrowLeft Alt+ArrowRight"
				data-testid="tab-group-chip"
				data-project-id={projectId}
				{...listeners}
				title={`${name} · ${collapsed ? 'Show' : 'Hide'} ${count} ${count === 1 ? 'tab' : 'tabs'}`}
				className={`relative flex h-6 max-w-40 shrink-0 cursor-pointer touch-none items-center gap-1 rounded px-1.5 text-xs font-medium text-foreground ${
					isDragging ? 'bg-secondary shadow-lg' : ''
				}`}
				onClick={() => onToggle(projectId)}
				onKeyDown={(e) => {
					if (e.altKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
						e.preventDefault();
						onNudge(projectId, e.key === 'ArrowLeft' ? -1 : 1);
						return;
					}
					if (e.key === 'Enter' || e.key === ' ') {
						e.preventDefault();
						onToggle(projectId);
					}
				}}
			>
				{/* A tint of the swatch rather than the swatch itself, so the avatar —
				    solid in the same colour — still reads as a tile on it (DESIGN.md
				    "Session Tabs"). A layer at an opacity rather than `color-mix`,
				    which older WebKitGTK does not parse. */}
				<span
					aria-hidden
					className="pointer-events-none absolute inset-0 rounded opacity-25"
					style={{ backgroundColor: swatch.fill }}
				/>
				<ProjectIcon name={name} size={16} />
				<span className="min-w-0 truncate">{name}</span>
				{collapsed && (
					<span data-testid="tab-group-count" className="shrink-0 tabular-nums opacity-80">
						· {count}
					</span>
				)}
			</div>
			{children}
			<span
				aria-hidden
				data-testid="tab-group-line"
				className="absolute inset-x-0 bottom-0 h-0.5 rounded-full"
				style={{ backgroundColor: swatch.fill }}
			/>
		</div>
	);
}
