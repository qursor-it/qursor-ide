import {
	type CloseGesture,
	CloseSessionConfirm,
	needsCloseConfirm,
} from '@components/dialog/CloseSessionConfirm';
import { ProjectIcon } from '@components/layout/ProjectIcon';
import { StatusDot } from '@components/layout/StatusDot';
import { disposeTerminal } from '@components/terminal/Terminal';
import { Button, IconButton } from '@factorai/ui';
import { useOpenSession } from '@hooks/useOpenSession';
import { useSessionTitles } from '@hooks/useSessionTitles';
import { useShortcuts } from '@hooks/useShortcuts';
import { type InboxEntry, inboxSections } from '@lib/inbox';
import { stepTab } from '@lib/keymap';
import { queryKeys } from '@lib/queryKeys';
import { tabsInKnownProjects } from '@lib/sessionGroups';
import { cmd } from '@lib/tauri';
import { useAttentionStore } from '@store/attentionStore';
import { usePrefsStore } from '@store/prefsStore';
import { usePrTabStore } from '@store/prTabStore';
import { useSidebarStore } from '@store/sidebarStore';
import { useTerminalStore } from '@store/terminalStore';
import { useQuery } from '@tanstack/react-query';
import { useNavigate, useParams } from '@tanstack/react-router';
import { GitPullRequest, PanelLeftClose, X } from 'lucide-react';
import { type ReactNode, useMemo, useState } from 'react';

/** One row's key in list order, for `Mod+PageUp/PageDown` (F28). */
const sessionKey = (id: string) => `s:${id}`;
const prKey = (projectId: string, number: number) => `pr:${projectId}:${number}`;

/**
 * The Inbox (specs/05-features.md F36, ADR-0092): everything you have open,
 * from every project, above the project tree — sessions that need you, sessions
 * working, open sessions that have stopped, and open pull requests. It replaced
 * the top bar's tab strip, so it carries what the strip did: a row per open
 * session, the close `×` (asking first while the agent works, middle-click
 * included), `Mod+W` and stepping with `Mod+PageUp/PageDown`. Renders nothing
 * while nothing is open or live.
 */
function useOpenList() {
	const tabs = useTerminalStore((s) => s.tabs);
	const bySession = useTerminalStore((s) => s.bySession);
	const detach = useTerminalStore((s) => s.detach);
	const unseen = useAttentionStore((s) => s.unseen);
	const prTabsAll = usePrTabStore((s) => s.tabs);
	const closePrTab = usePrTabStore((s) => s.close);
	const confirmCloseSession = usePrefsStore((s) => s.confirmCloseSession);
	const confirmCloseMiddleClick = usePrefsStore((s) => s.confirmCloseMiddleClick);
	const confirmPrefs = useMemo(
		() => ({ confirmCloseSession, confirmCloseMiddleClick }),
		[confirmCloseSession, confirmCloseMiddleClick],
	);
	const projectsQ = useQuery({ queryKey: queryKeys.projects(), queryFn: () => cmd.listProjects() });
	const {
		sessionId: activeId,
		projectId: routeProjectId,
		number: routeNumber,
	} = useParams({ strict: false }) as { sessionId?: string; projectId?: string; number?: string };
	const navigate = useNavigate();
	const openSession = useOpenSession();
	const [closing, setClosing] = useState<InboxEntry | null>(null);

	const { needsYou, working, stopped } = useMemo(() => {
		const live = Object.fromEntries(
			tabsInKnownProjects(
				Object.entries(bySession).map(([sessionId, t]) => ({ ...t, sessionId })),
				projectsQ.data,
			).map((t) => [t.sessionId, t]),
		);
		return inboxSections(tabsInKnownProjects(tabs, projectsQ.data), live, unseen);
	}, [tabs, bySession, unseen, projectsQ.data]);
	const prTabs = useMemo(
		() => tabsInKnownProjects(prTabsAll, projectsQ.data),
		[prTabsAll, projectsQ.data],
	);
	const entries = useMemo(
		() => [...needsYou, ...working, ...stopped],
		[needsYou, working, stopped],
	);

	const tabIds = useMemo(() => new Set(tabs.map((t) => t.sessionId)), [tabs]);
	const titles = useSessionTitles(entries);
	const projectName = useMemo(
		() => new Map((projectsQ.data ?? []).map((p) => [p.id, p.displayName])),
		[projectsQ.data],
	);
	const activeKey = activeId
		? sessionKey(activeId)
		: routeProjectId && routeNumber
			? prKey(routeProjectId, Number(routeNumber))
			: null;
	const order = useMemo(
		() => [
			...entries.map((e) => sessionKey(e.sessionId)),
			...prTabs.map((t) => prKey(t.projectId, t.number)),
		],
		[entries, prTabs],
	);

	const openPr = (projectId: string, number: number) =>
		void navigate({
			to: '/projects/$projectId/prs/$number',
			params: { projectId, number: String(number) },
		});

	/** A pull request has no process behind it, so closing it asks nothing;
	 *  leaving the page you were on lands on its project. */
	const closePr = (projectId: string, number: number) => {
		closePrTab(projectId, number);
		if (activeKey === prKey(projectId, number)) {
			void navigate({ to: '/projects/$id', params: { id: projectId } });
		}
	};

	/** Closing is the only thing that removes an open session (F16): kill the
	 *  PTY if there is one, forget the session, and leave it if you were on it.
	 *  A kill that fails keeps the row, where its dot goes on telling the truth. */
	const closeSession = (e: InboxEntry) => {
		const live = bySession[e.sessionId];
		setClosing(null);
		void (async () => {
			if (live) {
				try {
					await cmd.terminalKill(live.terminalId);
				} catch (err) {
					console.error('terminal_kill failed', err);
					return;
				}
			}
			detach(e.sessionId);
			disposeTerminal(e.sessionId);
			if (activeId === e.sessionId) {
				void navigate({ to: '/projects/$id', params: { id: e.projectId } });
			}
		})();
	};

	const requestClose = (e: InboxEntry, gesture: CloseGesture) => {
		if (needsCloseConfirm(bySession[e.sessionId]?.status, gesture, confirmPrefs)) setClosing(e);
		else closeSession(e);
	};

	return {
		needsYou,
		working,
		stopped,
		entries,
		prTabs,
		order,
		tabIds,
		titles,
		projectName,
		unseen,
		bySession,
		activeId,
		activeKey,
		openSession,
		openPr,
		closePr,
		requestClose,
		closing,
		setClosing,
		closeSession,
	};
}

/**
 * The open list's keys, mounted once at the shell so they work whichever list
 * the column shows (F28, ADR-0093), with its own copy of the close confirm.
 */
export function OpenListKeys() {
	const {
		entries,
		prTabs,
		order,
		titles,
		bySession,
		activeId,
		activeKey,
		openSession,
		openPr,
		closePr,
		requestClose,
		closing,
		setClosing,
		closeSession,
	} = useOpenList();
	// What the strip's own bindings did, in list order (F28): `Mod+W` closes
	// what you are looking at, through the same confirm the × takes.
	useShortcuts({
		closeFocusedTab: () => {
			const entry = entries.find((e) => e.sessionId === activeId);
			if (entry) return requestClose(entry, 'button');
			const pr = prTabs.find((t) => prKey(t.projectId, t.number) === activeKey);
			if (pr) closePr(pr.projectId, pr.number);
		},
		nextTab: () => step(1),
		previousTab: () => step(-1),
	});
	const step = (delta: 1 | -1) => {
		if (!activeKey) return;
		const next = stepTab(order, activeKey, delta);
		if (!next || next === activeKey) return;
		const entry = entries.find((e) => sessionKey(e.sessionId) === next);
		if (entry) return openSession(entry.sessionId, entry.projectId);
		const pr = prTabs.find((t) => prKey(t.projectId, t.number) === next);
		if (pr) openPr(pr.projectId, pr.number);
	};

	return (
		<CloseSessionConfirm
			sessionName={closing ? (titles.get(closing.sessionId) ?? 'this session') : null}
			canConfirm={closing !== null && Boolean(bySession[closing.sessionId])}
			onCancel={() => setClosing(null)}
			onConfirm={() => closing && closeSession(closing)}
		/>
	);
}

function Inbox({ empty = null }: { empty?: ReactNode }) {
	const {
		needsYou,
		working,
		stopped,
		entries,
		prTabs,
		tabIds,
		titles,
		projectName,
		unseen,
		bySession,
		activeId,
		activeKey,
		openSession,
		openPr,
		closePr,
		requestClose,
		closing,
		setClosing,
		closeSession,
	} = useOpenList();

	if (entries.length === 0 && prTabs.length === 0) return empty;

	const row = (
		key: string,
		testId: string,
		active: boolean,
		accent: boolean,
		body: ReactNode,
		onOpen: () => void,
		onClose: (gesture: CloseGesture) => void,
		closeLabel: string,
	) => (
		<li
			key={key}
			className={`group relative flex items-center rounded-lg ${
				active ? 'bg-background shadow-sm' : 'hover:bg-secondary/60'
			} ${accent ? 'shadow-[inset_3px_0_0_var(--primary)]' : ''}`}
		>
			<Button
				variant="quiet"
				data-testid={testId}
				aria-current={active ? 'page' : undefined}
				onClick={onOpen}
				onAuxClick={(ev) => {
					if (ev.button === 1) {
						ev.preventDefault();
						onClose('middle-click');
					}
				}}
				className="h-auto min-w-0 flex-1 flex-col items-stretch gap-0.5 rounded-lg px-2.5 py-1.5 text-left font-normal text-foreground hover:text-foreground"
			>
				{body}
			</Button>
			<IconButton
				aria-label={closeLabel}
				title={closeLabel}
				data-testid={`${testId}-close`}
				onClick={() => onClose('button')}
				className={`mr-1.5 shrink-0 ${active ? '' : 'invisible group-focus-within:visible group-hover:visible'}`}
			>
				<X />
			</IconButton>
		</li>
	);

	const sessionRow = (e: InboxEntry, accent: boolean) => {
		const name = projectName.get(e.projectId) ?? '';
		const title = titles.get(e.sessionId) ?? 'New session';
		return row(
			e.sessionId,
			`inbox-row-${e.sessionId}`,
			e.sessionId === activeId,
			accent,
			<>
				<span className="flex min-w-0 items-center gap-2">
					<StatusDot
						status={e.status}
						background={!tabIds.has(e.sessionId)}
						unseen={unseen[e.sessionId] === true}
					/>
					<span className="truncate text-foreground">{title}</span>
				</span>
				<span className="flex min-w-0 items-center gap-1.5 pl-4 text-muted-foreground text-xs">
					<ProjectIcon name={name} size={14} />
					<span className="truncate">{name}</span>
				</span>
			</>,
			() => openSession(e.sessionId, e.projectId),
			(gesture) => requestClose(e, gesture),
			`Close ${title}`,
		);
	};

	const section = (label: string, count: number, testId: string, rows: ReactNode) =>
		count > 0 && (
			<section aria-label={label} data-testid={testId}>
				<h2 className="flex items-center gap-1.5 px-3 pt-2 pb-1 font-medium text-muted-foreground text-xs uppercase tracking-wider">
					{label}
					<span className="rounded bg-secondary px-1.5 tabular-nums tracking-normal">{count}</span>
				</h2>
				<ul className="space-y-0.5 px-1.5">{rows}</ul>
			</section>
		);

	return (
		<div data-testid="inbox" className="min-h-0 flex-1 overflow-y-auto pb-2">
			{section(
				'Needs you',
				needsYou.length,
				'inbox-needs-you',
				needsYou.map((e) => sessionRow(e, true)),
			)}
			{section(
				'Working',
				working.length,
				'inbox-working',
				working.map((e) => sessionRow(e, false)),
			)}
			{section(
				'Open',
				stopped.length,
				'inbox-open',
				stopped.map((e) => sessionRow(e, false)),
			)}
			{section(
				'Pull requests',
				prTabs.length,
				'inbox-prs',
				prTabs.map((t) => {
					const label = t.title ? `#${t.number} ${t.title}` : `#${t.number}`;
					return row(
						prKey(t.projectId, t.number),
						`inbox-pr-${t.projectId}-${t.number}`,
						activeKey === prKey(t.projectId, t.number),
						false,
						<>
							<span className="flex min-w-0 items-center gap-2">
								<GitPullRequest className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
								<span className="truncate text-foreground">{label}</span>
							</span>
							<span className="flex min-w-0 items-center gap-1.5 pl-5 text-muted-foreground text-xs">
								<ProjectIcon name={projectName.get(t.projectId) ?? ''} size={14} />
								<span className="truncate">{projectName.get(t.projectId) ?? ''}</span>
							</span>
						</>,
						() => openPr(t.projectId, t.number),
						() => closePr(t.projectId, t.number),
						`Close ${label}`,
					);
				}),
			)}
			<CloseSessionConfirm
				sessionName={closing ? (titles.get(closing.sessionId) ?? 'this session') : null}
				canConfirm={closing !== null && Boolean(bySession[closing.sessionId])}
				onCancel={() => setClosing(null)}
				onConfirm={() => closing && closeSession(closing)}
			/>
		</div>
	);
}

/**
 * The column beside the app rail when it shows the Inbox (ADR-0093): a header
 * that names it and can fold the column away, the list, and — with nothing
 * open — a way to the projects instead of an empty pane.
 */
export function InboxColumn() {
	const toggleCollapsed = useSidebarStore((s) => s.toggleCollapsed);
	const setView = useSidebarStore((s) => s.setView);
	return (
		<>
			<div className="flex h-11 shrink-0 items-center gap-2 pr-2 pl-3">
				<h1 className="flex-1 font-semibold text-sm">Inbox</h1>
				<IconButton
					size="md"
					aria-label="Collapse sidebar"
					title="Collapse sidebar"
					data-testid="sidebar-collapse"
					onClick={toggleCollapsed}
				>
					<PanelLeftClose />
				</IconButton>
			</div>
			<Inbox
				empty={
					<div
						data-testid="inbox-empty"
						className="flex flex-col items-start gap-2 px-3 py-2 text-muted-foreground text-sm"
					>
						<p>Nothing is open. Sessions that need you, and the ones working, show up here.</p>
						<Button
							variant="link"
							size="sm"
							className="h-auto p-0"
							onClick={() => setView('projects')}
						>
							Browse projects
						</Button>
					</div>
				}
			/>
		</>
	);
}
