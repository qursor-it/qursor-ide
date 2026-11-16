import { ModeSwitch } from '@components/layout/ModeSwitch';
import { IconButton } from '@factorai/ui';
import { useSettingsModal } from '@hooks/useSettingsModal';
import { useChordTitle } from '@hooks/useShortcuts';
import { useGitModeStore } from '@store/gitModeStore';
import { useSidebarStore } from '@store/sidebarStore';
import { useNavigate, useRouterState } from '@tanstack/react-router';
import { FolderTree, LayoutGrid, Settings } from 'lucide-react';
import type { ReactNode } from 'react';

function RailButton({
	label,
	testId,
	on,
	onClick,
	children,
}: {
	label: string;
	testId: string;
	on: boolean;
	onClick: () => void;
	children: ReactNode;
}) {
	return (
		<IconButton
			size="md"
			aria-label={label}
			title={label}
			aria-pressed={on}
			data-testid={testId}
			onClick={onClick}
			className={`size-9 rounded-lg ${on ? 'bg-background text-foreground shadow-sm' : ''}`}
		>
			{children}
		</IconButton>
	);
}

/**
 * The app rail (ADR-0093): the column of places at the window's left edge, as in
 * the redesign's mock-up. The Inbox, the project tree and the Board in Sessions
 * mode, Git mode, and settings at the foot. No tile per project: initials do not
 * tell a dozen projects apart (ADR-0098). Always drawn; collapsing the sidebar hides only the
 * column beside it.
 */
export function AppRail() {
	const view = useSidebarStore((s) => s.view);
	const setView = useSidebarStore((s) => s.setView);
	const mode = useGitModeStore((s) => s.mode);
	const setMode = useGitModeStore((s) => s.setMode);
	const pathname = useRouterState({ select: (s) => s.location.pathname });
	const navigate = useNavigate();
	const settings = useSettingsModal();
	const settingsTitle = useChordTitle('Settings', 'openSettings');

	const sessions = mode === 'sessions';
	const onBoard = pathname === '/board';
	const show = (next: 'inbox' | 'projects') => {
		setMode('sessions');
		setView(next);
		if (onBoard) void navigate({ to: '/' });
	};

	return (
		<nav
			aria-label="App"
			data-testid="app-rail"
			className="flex w-14 shrink-0 flex-col items-center gap-1 pt-1 pb-2"
		>
			<ModeSwitch
				highlightSessions={view === 'inbox' && !onBoard}
				onPick={(m) => m === 'sessions' && show('inbox')}
			>
				<RailButton
					label="Projects"
					testId="rail-projects"
					on={sessions && view === 'projects' && !onBoard}
					onClick={() => show('projects')}
				>
					<FolderTree />
				</RailButton>
				<RailButton
					label="All sessions board"
					testId="open-board"
					on={sessions && onBoard}
					onClick={() => {
						setMode('sessions');
						void navigate({ to: '/board' });
					}}
				>
					<LayoutGrid />
				</RailButton>
			</ModeSwitch>

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
