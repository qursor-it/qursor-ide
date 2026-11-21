import { ServerTabs } from '@components/servers/ServerModeView';
import { CanvasTabs } from '@components/layout/CanvasTabs';
import { GitTabs } from '@components/layout/GitTabs';
import { SessionTabs } from '@components/layout/SessionTabs';
import { SidebarCollapseButton } from '@components/layout/ModeSidebar';
import { Button, IconButton } from '@factorai/ui';
import { useChordLabel, useChordTitle } from '@hooks/useShortcuts';
import { usePaletteStore } from '@store/paletteStore';
import { useGitModeStore } from '@store/gitModeStore';
import { usePanelStore } from '@store/panelStore';
import { useSidebarStore } from '@store/sidebarStore';
import { PanelRight, Search } from 'lucide-react';

/**
 * The header of everything right of the sidebar: the palette and the work row.
 * The sidebar frame runs the full height beside it and carries the brand.
 */
export function TopBar() {
	const open = usePanelStore((s) => s.open);
	const toggle = usePanelStore((s) => s.toggle);
	const mode = useGitModeStore((s) => s.mode);
	const sidebarCollapsed = useSidebarStore((s) => s.collapsed);
	// The chord rides on the tooltip these buttons already had (F28).
	const panelTitle = useChordTitle('Toggle file tree', 'toggleFilePanel');
	const paletteChord = useChordLabel('openPalette');
	const openPalette = usePaletteStore((s) => s.setOpen);

	return (
		<div className="shrink-0 bg-chrome">
			<header className="flex h-10.5 items-center gap-2 px-3">
				{/* The frame's own toggle is in its header; with the frame gone this is
				    the way back. */}
				{sidebarCollapsed && <SidebarCollapseButton />}

				{/* The palette's door (F37), centred in what the tools row leaves. */}
				<div className="flex min-w-0 flex-1 justify-center px-4">
					<Button
						variant="secondary"
						size="sm"
						data-testid="open-palette"
						onClick={() => openPalette(true)}
						className="w-full max-w-md justify-start gap-2 bg-secondary/70 font-normal text-muted-foreground hover:bg-secondary"
					>
						<Search />
						<span className="truncate">Search sessions, projects and actions…</span>
						{paletteChord && (
							<kbd className="ml-auto rounded border border-border bg-background px-1.5 font-sans text-xs">
								{paletteChord}
							</kbd>
						)}
					</Button>
				</div>

				{/* **One colour for both, changed 2026-08-20 on user feedback.** This
			    button used to take full `foreground` while the panel was open, on the
			    reasoning that open is a state rather than a hover. The rule is right
			    for a list row and wrong here: the 288px panel is either on screen or
			    it is not, so the colour was restating something impossible to miss —
			    and it made two neighbouring icons in the same row disagree about what
			    a header icon looks like. `aria-pressed` still carries the state where
			    it is not otherwise visible. */}
				{mode === 'sessions' && (
					<IconButton
						size="md"
						aria-label="Toggle file tree"
						aria-pressed={open}
						title={panelTitle}
						onClick={toggle}
					>
						<PanelRight />
					</IconButton>
				)}
			</header>
			{/* Row 2, the work row (ADR-0100): the open sessions in Sessions mode, the
			    open repositories in Git mode, each its own list. Both stay mounted and
			    the idle one is hidden, so a mode flip costs the session strip nothing. */}
			<div className="flex h-10 items-center px-2">
				<div className={mode !== 'sessions' ? 'hidden' : 'flex min-w-0 flex-1 items-center'}>
					<SessionTabs />
				</div>
				{mode === 'git' && <GitTabs />}
				{mode === 'canvas' && <CanvasTabs />}
				{mode === 'servers' && <ServerTabs />}
			</div>
		</div>
	);
}
