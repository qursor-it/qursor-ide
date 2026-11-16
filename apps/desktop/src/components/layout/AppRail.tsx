import { ModeSwitch } from '@components/layout/ModeSwitch';
import { IconButton } from '@factorai/ui';
import { useSettingsModal } from '@hooks/useSettingsModal';
import { useChordTitle } from '@hooks/useShortcuts';
import { Settings } from 'lucide-react';

/**
 * The app rail (ADR-0093, ADR-0100): the column at the window's left edge with
 * the two modes — Sessions, whose column is the project tree, and Git — and
 * settings at the foot. Always drawn; collapsing the sidebar hides only the
 * column beside it.
 */
export function AppRail() {
	const settings = useSettingsModal();
	const settingsTitle = useChordTitle('Settings', 'openSettings');

	return (
		<nav
			aria-label="App"
			data-testid="app-rail"
			className="flex w-14 shrink-0 flex-col items-center gap-1 pt-1 pb-2"
		>
			<ModeSwitch />
			<div className="flex-1" />
			<IconButton
				size="md"
				aria-label="Settings"
				title={settingsTitle}
				data-testid="open-settings"
				onClick={() => settings.open()}
				className="size-9 rounded-lg"
			>
				<Settings />
			</IconButton>
		</nav>
	);
}
