import { Brand } from '@components/brand/Brand';
import { DevBadge } from '@components/layout/DevBadge';
import { GitTabs } from '@components/layout/GitTabs';
import { ModeSwitch } from '@components/layout/ModeSwitch';
import { SessionTabs } from '@components/layout/SessionTabs';
import { IconButton } from '@factorai/ui';
import { useSettingsModal } from '@hooks/useSettingsModal';
import { useChordTitle } from '@hooks/useShortcuts';
import { useGitModeStore } from '@store/gitModeStore';
import { usePanelStore } from '@store/panelStore';
import { PanelRight, Settings } from 'lucide-react';

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
	const settings = useSettingsModal();
	// The chord rides on the tooltip these buttons already had (F28).
	const settingsTitle = useChordTitle('Settings', 'openSettings');
	const panelTitle = useChordTitle('Toggle file tree', 'toggleFilePanel');

	return (
		<div className="shrink-0 bg-chrome">
			<header className="flex h-10.5 items-center gap-2 px-3">
				<Brand />
				<DevBadge />
				<ModeSwitch />

				<div className="flex-1" />

				{/* Settings is app-level chrome, so it lives here rather than in the
			    sidebar footer — which is also already over-full (F14). Left of the
			    panel toggle, and item 6's window controls sit at the window's outer
			    edge, so this moves once by a fixed offset when they land rather than
			    competing for the same pixels. */}
				<IconButton
					size="md"
					aria-label="Settings"
					title={settingsTitle}
					data-testid="open-settings"
					onClick={() => settings.open()}
				>
					<Settings />
				</IconButton>

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
			{/* Row 2, the work row. Both strips stay mounted and the idle one is
		    hidden, so a mode flip costs the session strip nothing. */}
			<div className="flex h-9 items-center px-3">
				<div className={gitMode ? 'hidden' : 'flex min-w-0 flex-1 items-center'}>
					<SessionTabs />
				</div>
				{gitMode && <GitTabs />}
			</div>
		</div>
	);
}
