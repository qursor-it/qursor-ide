import { ProjectIcon } from '@components/layout/ProjectIcon';
import { StatusDot } from '@components/layout/StatusDot';
import { restartSession } from '@components/terminal/Terminal';
import { Dialog, DialogContent, DialogDescription, DialogTitle, Input } from '@factorai/ui';
import { useActiveProject } from '@hooks/useActiveProject';
import { useSessionTitles } from '@hooks/useSessionTitles';
import { useSettingsModal } from '@hooks/useSettingsModal';
import { useStartSession } from '@hooks/useStartSession';
import { inboxSections } from '@lib/inbox';
import { queryKeys } from '@lib/queryKeys';
import { tabsInKnownProjects } from '@lib/sessionGroups';
import { cmd } from '@lib/tauri';
import { useAttentionStore } from '@store/attentionStore';
import { usePaletteStore } from '@store/paletteStore';
import { usePanelStore } from '@store/panelStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { Inbox, PanelRight, Plus, Search, Settings } from 'lucide-react';
import { type ReactNode, useEffect, useMemo, useState } from 'react';

interface Item {
	key: string;
	group: 'Sessions' | 'Projects' | 'Actions';
	label: string;
	detail?: string;
	icon: ReactNode;
	run: () => void;
}

/** Hits asked of the index per query; the palette shows what fits, not a page. */
const SEARCH_LIMIT = 8;

/**
 * The command palette (specs/05-features.md F37): one field for sessions,
 * projects and actions. With nothing typed it lists the sessions that need you
 * and the ones working (the Inbox's order, F36); typing searches session
 * content through the same index as F4 and filters projects by name.
 */
export function CommandPalette() {
	const open = usePaletteStore((s) => s.open);
	const setOpen = usePaletteStore((s) => s.setOpen);
	const [term, setTerm] = useState('');
	const [active, setActive] = useState(0);
	const query = term.trim();
	const navigate = useNavigate();
	const { projectId } = useActiveProject();
	const startSession = useStartSession();
	const settings = useSettingsModal();
	const togglePanel = usePanelStore((s) => s.toggle);
	const tabs = useTerminalStore((s) => s.tabs);
	const bySession = useTerminalStore((s) => s.bySession);
	const unseen = useAttentionStore((s) => s.unseen);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const hitsQ = useQuery({
		queryKey: queryKeys.search(query, null),
		queryFn: () => cmd.searchSessions(query, undefined, SEARCH_LIMIT),
		enabled: open && query.length > 0,
	});

	// Every open starts fresh: a palette that remembers the last query makes you
	// clear it before you can type the next one.
	useEffect(() => {
		if (open) {
			setTerm('');
			setActive(0);
		}
	}, [open]);
	useEffect(() => setActive(0), [query]);

	const inbox = useMemo(() => {
		const { needsYou, working } = inboxSections(
			tabsInKnownProjects(tabs, projectsQ.data),
			bySession,
			unseen,
		);
		return [...needsYou, ...working];
	}, [tabs, bySession, unseen, projectsQ.data]);
	const titles = useSessionTitles(inbox);

	const items = useMemo<Item[]>(() => {
		const projects = projectsQ.data ?? [];
		const nameOf = new Map(projects.map((p) => [p.id, p.displayName]));
		const close = () => setOpen(false);
		const openSession = (sessionId: string, pid: string) => {
			close();
			// A stopped session is a restart, as a stopped tab is (F16).
			if (!useTerminalStore.getState().bySession[sessionId]) restartSession(sessionId);
			void navigate({
				to: '/projects/$projectId/sessions/$sessionId',
				params: { projectId: pid, sessionId },
			});
		};
		const q = query.toLowerCase();

		const sessions: Item[] = q
			? [...new Map((hitsQ.data ?? []).map((h) => [h.sessionId, h])).values()].map((h) => ({
					key: `s:${h.sessionId}`,
					group: 'Sessions',
					label: h.title.trim() || 'Untitled session',
					detail: h.projectName,
					icon: <ProjectIcon name={h.projectName} size={16} />,
					run: () => openSession(h.sessionId, h.projectId),
				}))
			: inbox.map((e) => ({
					key: `s:${e.sessionId}`,
					group: 'Sessions',
					label: titles.get(e.sessionId) ?? 'New session',
					detail: `${nameOf.get(e.projectId) ?? ''} · ${
						e.status === 'working' && !unseen[e.sessionId] ? 'working' : 'needs you'
					}`,
					icon: <StatusDot status={e.status} unseen={unseen[e.sessionId] === true} />,
					run: () => openSession(e.sessionId, e.projectId),
				}));

		const matchedProjects: Item[] = q
			? projects
					.filter((p) => p.displayName.toLowerCase().includes(q))
					.slice(0, 5)
					.map((p) => ({
						key: `p:${p.id}`,
						group: 'Projects',
						label: p.displayName,
						detail: p.realPath,
						icon: <ProjectIcon name={p.displayName} size={16} />,
						run: () => {
							close();
							void navigate({ to: '/projects/$id', params: { id: p.id } });
						},
					}))
			: [];

		const actions = (
			[
				...(inbox[0] && inbox[0].status !== 'working'
					? [
							{
								key: 'a:next',
								group: 'Actions' as const,
								label: 'Jump to the next session that needs you',
								icon: <Inbox />,
								run: () => openSession(inbox[0].sessionId, inbox[0].projectId),
							},
						]
					: []),
				...(projectId
					? [
							{
								key: 'a:new',
								group: 'Actions' as const,
								label: `New session in ${nameOf.get(projectId) ?? 'this project'}`,
								icon: <Plus />,
								run: () => {
									close();
									void startSession(projectId);
								},
							},
						]
					: []),
				{
					key: 'a:panel',
					group: 'Actions',
					label: 'Toggle the file panel',
					icon: <PanelRight />,
					run: () => {
						close();
						togglePanel();
					},
				},
				{
					key: 'a:settings',
					group: 'Actions',
					label: 'Open settings',
					icon: <Settings />,
					run: () => {
						close();
						settings.open();
					},
				},
			] satisfies Item[]
		).filter((a) => !q || a.label.toLowerCase().includes(q));

		// Typing, a name match is more specific than a content hit, so projects and
		// actions lead; with nothing typed the sessions that need you do.
		return q ? [...matchedProjects, ...actions, ...sessions] : [...sessions, ...actions];
	}, [
		query,
		hitsQ.data,
		inbox,
		titles,
		unseen,
		projectsQ.data,
		projectId,
		navigate,
		setOpen,
		startSession,
		settings,
		togglePanel,
	]);

	const onKeyDown = (e: React.KeyboardEvent) => {
		if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
			e.preventDefault();
			const step = e.key === 'ArrowDown' ? 1 : -1;
			setActive((i) => (items.length ? (i + step + items.length) % items.length : 0));
		} else if (e.key === 'Enter') {
			e.preventDefault();
			items[active]?.run();
		}
	};

	return (
		<Dialog open={open} onOpenChange={setOpen}>
			<DialogContent
				hideClose
				data-testid="command-palette"
				className="top-[14%] max-w-xl translate-y-0 gap-0 overflow-hidden p-0 data-[state=closed]:slide-out-to-top-[14%] data-[state=open]:slide-in-from-top-[14%]"
			>
				<DialogTitle className="sr-only">Command palette</DialogTitle>
				<DialogDescription className="sr-only">
					Search sessions, projects and actions
				</DialogDescription>
				<div className="relative border-border border-b">
					<Search className="-translate-y-1/2 pointer-events-none absolute top-1/2 left-4 size-4 text-muted-foreground" />
					<Input
						autoFocus
						role="combobox"
						aria-expanded
						value={term}
						onChange={(e) => setTerm(e.target.value)}
						onKeyDown={onKeyDown}
						placeholder="Search sessions, projects and actions…"
						aria-label="Search sessions, projects and actions"
						aria-controls="command-palette-list"
						aria-activedescendant={items[active] ? `palette-${items[active].key}` : undefined}
						className="h-12 rounded-none border-0 bg-transparent pl-11 focus-visible:ring-0 focus-visible:ring-offset-0"
					/>
				</div>
				<div
					id="command-palette-list"
					role="listbox"
					aria-label="Results"
					className="max-h-96 overflow-y-auto p-1.5"
				>
					{items.length === 0 && (
						<div className="px-3 py-6 text-center text-muted-foreground text-sm">
							{hitsQ.isFetching ? 'Searching…' : 'Nothing matches'}
						</div>
					)}
					{items.map((item, i) => (
						<div key={item.key}>
							{(i === 0 || items[i - 1].group !== item.group) && (
								<div className="px-3 pt-2 pb-1 font-medium text-muted-foreground text-xs uppercase tracking-wider">
									{item.group}
								</div>
							)}
							<div
								id={`palette-${item.key}`}
								role="option"
								aria-selected={i === active}
								data-testid={`palette-item-${item.key}`}
								tabIndex={-1}
								onKeyDown={onKeyDown}
								onMouseMove={() => setActive(i)}
								onClick={item.run}
								className={`flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-muted-foreground ${
									i === active ? 'bg-secondary text-foreground' : 'text-secondary-foreground'
								}`}
							>
								<span className="flex w-4 justify-center">{item.icon}</span>
								<span className="truncate">{item.label}</span>
								{item.detail && (
									<span className="ml-auto max-w-[45%] shrink-0 truncate text-muted-foreground text-xs">
										{item.detail}
									</span>
								)}
							</div>
						</div>
					))}
				</div>
				<div className="flex gap-4 border-border border-t bg-muted px-4 py-2 text-muted-foreground text-xs">
					<span>↑↓ move</span>
					<span>↵ open</span>
					<span>esc close</span>
				</div>
			</DialogContent>
		</Dialog>
	);
}
