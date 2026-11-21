import { CanvasSidebar } from '@components/canvas/CanvasSidebar';
import { GitSidebar } from '@components/gitmode/GitSidebar';
import { PanelEmpty } from '@components/layout/PanelEmpty';
import { Brand } from '@components/brand/Brand';
import { DevBadge } from '@components/layout/DevBadge';
import { PanelResizer } from '@components/layout/PanelResizer';
import { ServerBook, VaultFooter } from '@components/servers/ServerModeView';
import { Sidebar } from '@components/layout/Sidebar';
import { UpdateBadge } from '@components/layout/UpdateBadge';
import { ZoomControls } from '@components/layout/ZoomControls';
import { IconButton } from '@factorai/ui';
import { useGitModeStore } from '@store/gitModeStore';
import { useIndexerStore } from '@store/indexerStore';
import { clampSidebarWidth, useSidebarStore } from '@store/sidebarStore';
import { PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import type { ReactNode } from 'react';

/**
 * The left column every mode shares (ADR-0100): brand and collapse toggle on
 * top, the mode's own body in the middle, the footer under it. Full height, so
 * the top bar starts at its right edge. Gone while collapsed — the rail is what
 * is left, and the top bar carries the expand button.
 */
export function ModeSidebar() {
	const mode = useGitModeStore((s) => s.mode);
	const width = useSidebarStore((s) => s.width);
	const collapsed = useSidebarStore((s) => s.collapsed);
	const setWidth = useSidebarStore((s) => s.setWidth);
	if (collapsed) return null;

	return (
		<>
			<aside data-testid="sidebar" style={{ width }} className="flex shrink-0 flex-col bg-chrome">
				<header className="flex h-10.5 shrink-0 items-center gap-2 pr-2 pl-3">
					<Brand />
					<DevBadge />
					<span className="flex-1" />
					<SidebarCollapseButton />
				</header>
				{/* Kept mounted and hidden in the other modes: the project tree holds
				    the search term and the open groups. */}
				<div className={mode !== 'sessions' ? 'hidden' : 'flex min-h-0 flex-1 flex-col'}>
					<Sidebar />
				</div>
				{mode === 'canvas' && (
					<div className="flex min-h-0 flex-1 flex-col">
						<CanvasSidebar />
					</div>
				)}
				{mode === 'git' && (
					<div className="flex min-h-0 flex-1 flex-col">
						<GitSlot />
					</div>
				)}
				{mode === 'servers' && (
					<div className="flex min-h-0 flex-1 flex-col">
						<ServerBook />
					</div>
				)}
				<SidebarFooter extra={mode === 'servers' ? <VaultFooter /> : undefined} />
			</aside>
			{/* On the sidebar's right edge, so dragging right widens it. Not rendered
			    while collapsed: a handle that resizes nothing lies (ADR-0038). */}
			<PanelResizer
				size={width}
				onSize={setWidth}
				edge="right"
				label="Resize sidebar"
				clamp={clampSidebarWidth}
			/>
		</>
	);
}

/** The active Git tab's tree; its section is kept per repository in the store. */
function GitSlot() {
	const tab = useGitModeStore((s) => s.tabs.find((t) => t.dir === s.activeDir));
	const section = useGitModeStore((s) => (tab ? s.sectionByDir[tab.dir] : undefined)) ?? 'history';
	const setSection = useGitModeStore((s) => s.setSection);
	if (!tab) return <PanelEmpty>Open a repository from the tab bar.</PanelEmpty>;
	return (
		<GitSidebar
			key={tab.dir}
			dir={tab.dir}
			projectId={tab.projectId}
			section={section}
			onSection={(next) => setSection(tab.dir, next)}
		/>
	);
}

/** The collapse / expand toggle, in the frame's header and, while collapsed, in
 *  the top bar. */
export function SidebarCollapseButton() {
	const collapsed = useSidebarStore((s) => s.collapsed);
	const toggleCollapsed = useSidebarStore((s) => s.toggleCollapsed);

	// **Focus must not be left on a button that is about to unmount** (F1,
	// ADR-0038). Collapsing removes the frame and expanding removes the top bar's
	// button, and focus on a detached node falls to `<body>`. Whichever instance
	// is left takes it — only when focus was here, so a rescue never pulls it off
	// whatever the rest of the app had it on.
	function onClick(e: React.MouseEvent<HTMLButtonElement>) {
		const here =
			document.activeElement === e.currentTarget ||
			document.activeElement?.closest('[data-testid="sidebar"]') != null;
		toggleCollapsed();
		if (here) {
			requestAnimationFrame(() =>
				document.querySelector<HTMLElement>('[data-testid="sidebar-collapse"]')?.focus(),
			);
		}
	}

	return (
		<IconButton
			// `md` in the top bar, `sm` in the frame's header (DESIGN.md, The Rail Rule).
			size={collapsed ? 'md' : 'sm'}
			aria-expanded={!collapsed}
			aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
			title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
			data-testid="sidebar-collapse"
			onClick={onClick}
		>
			{collapsed ? <PanelLeftOpen /> : <PanelLeftClose />}
		</IconButton>
	);
}

/**
 * **A fixed height, not `py-*`.** The updater's badge is 24px tall and everything
 * else in this row is 18px or less, so a footer that hugged its content grew by
 * 6px the moment an update staged itself. `h-9` is the file panel header's
 * height, and it is the badge's 24px plus the padding it wants.
 */
export function SidebarFooter({ extra }: { extra?: ReactNode }) {
	const progress = useIndexerStore((s) => s.progress);
	return (
		<footer
			data-testid="sidebar-footer"
			className="flex h-9 shrink-0 items-center gap-2 pr-1.5 pl-3 text-muted-foreground text-xs"
		>
			{/* Indexing is transient and worth saying; in its place, the updater.
			    `@container` so the badge can drop its label when this cell gets narrow. */}
			<span className="@container min-w-0 flex-1 truncate">
				{progress && progress.phase !== 'idle' ? (
					`Indexing… ${progress.processed}/${progress.total}`
				) : (
					<UpdateBadge />
				)}
			</span>
			{extra}
			<ZoomControls />
		</footer>
	);
}
