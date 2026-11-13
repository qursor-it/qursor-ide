import { Brand } from '@components/brand/Brand';
import { DevBadge } from '@components/layout/DevBadge';
import { GitTabs } from '@components/layout/GitTabs';
import { Button, IconButton } from '@factorai/ui';
import { useChordLabel, useChordTitle } from '@hooks/useShortcuts';
import { usePaletteStore } from '@store/paletteStore';
import { useGitModeStore } from '@store/gitModeStore';
import { usePanelStore } from '@store/panelStore';
import { PanelRight, Search } from 'lucide-react';

/**
 * Full-window header. Deliberately spans above the sidebar too: this is the
 * shape the custom titlebar needs when we drop the OS decorations and
 * reimplement minimise / maximise / close (specs/06-milestones.md M5), so that
 * step becomes "add buttons" rather than "restructure the shell".
 */
export function TopBar() {
	const open = usePanelStore((s) => s.open);
	const toggle = usePanelStore((s) => s.toggle);
	const gitMode = useGitModeStore((s) => s.mode === 'git');
	// The chord rides on the tooltip these buttons already had (F28).
	const panelTitle = useChordTitle('Toggle file tree', 'toggleFilePanel');
	const paletteChord = useChordLabel('openPalette');
	const openPalette = usePaletteStore((s) => s.setOpen);

	return (
		<div className="shrink-0 bg-chrome">
			<header className="flex h-10.5 items-center gap-2 px-3">
				<Brand />
				<DevBadge />

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
				{!gitMode && (
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
			{/* Row 2 is Git mode's own tab strip; in Sessions mode what is open lives
			    in the sidebar's Inbox (F36, ADR-0092). */}
			{gitMode && (
				<div className="flex h-9 items-center px-3">
					<GitTabs />
				</div>
			)}
		</div>
	);
}
