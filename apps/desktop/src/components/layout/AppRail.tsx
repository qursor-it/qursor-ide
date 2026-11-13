import { ModeSwitch } from '@components/layout/ModeSwitch';
import { ProjectIcon } from '@components/layout/ProjectIcon';
import { IconButton } from '@factorai/ui';
import { useActiveProject } from '@hooks/useActiveProject';
import { useAddProject } from '@hooks/useAddProject';
import { useSettingsModal } from '@hooks/useSettingsModal';
import { useChordTitle } from '@hooks/useShortcuts';
import { queryKeys } from '@lib/queryKeys';
import { openSessions, projectStatus, projectUnseen } from '@lib/sessionGroups';
import { cmd } from '@lib/tauri';
import { useAddProjectStore } from '@store/addProjectStore';
import { useAttentionStore } from '@store/attentionStore';
import { useGitModeStore } from '@store/gitModeStore';
import { useSidebarStore } from '@store/sidebarStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQuery } from '@tanstack/react-query';
import { useNavigate, useRouterState } from '@tanstack/react-router';
import { FolderTree, LayoutGrid, Plus, Settings } from 'lucide-react';
import type { ReactNode } from 'react';
import { useMemo } from 'react';

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
 * mode, Git mode, then a tile per project with its state, adding a project, and
 * settings at the foot. Always drawn; collapsing the sidebar hides only the
 * column beside it.
 */
export function AppRail() {
	const view = useSidebarStore((s) => s.view);
	const setView = useSidebarStore((s) => s.setView);
	const mode = useGitModeStore((s) => s.mode);
	const setMode = useGitModeStore((s) => s.setMode);
	const pathname = useRouterState({ select: (s) => s.location.pathname });
	const navigate = useNavigate();
	const { projectId: activeProjectId } = useActiveProject();
	const settings = useSettingsModal();
	const settingsTitle = useChordTitle('Settings', 'openSettings');
	const addProject = useAddProject();
	const adding = useAddProjectStore((s) => s.adding);
	const tabs = useTerminalStore((s) => s.tabs);
	const bySession = useTerminalStore((s) => s.bySession);
	const unseen = useAttentionStore((s) => s.unseen);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const open = useMemo(() => openSessions(tabs, bySession), [tabs, bySession]);

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

			<div className="my-1.5 h-px w-6 shrink-0 bg-border" />

			<div
				data-testid="rail-projects-list"
				className="scrollbar-none flex min-h-0 flex-1 flex-col items-center gap-1.5 overflow-y-auto"
			>
				{(projectsQ.data ?? []).map((p) => (
					<IconButton
						key={p.id}
						aria-label={p.displayName}
						title={p.displayName}
						data-testid={`rail-project-${p.id}`}
						onClick={() => {
							setMode('sessions');
							void navigate({ to: '/projects/$id', params: { id: p.id } });
						}}
						className={`size-9 rounded-lg ${
							sessions && p.id === activeProjectId ? 'bg-background shadow-sm' : ''
						}`}
					>
						<ProjectIcon
							name={p.displayName}
							size={26}
							status={projectStatus(open, p.id)}
							unseen={projectUnseen(open, unseen, p.id)}
						/>
					</IconButton>
				))}
				<IconButton
					size="md"
					aria-label="Add a project"
					title="Add a project"
					data-testid="rail-add"
					disabled={adding}
					onClick={() => void addProject()}
					className="size-9 rounded-lg"
				>
					<Plus />
				</IconButton>
			</div>

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
