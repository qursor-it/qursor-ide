import { wheelScrollsX } from '@lib/wheel';
import type {
	Project,
	Server,
	ServerBook as ServerBookData,
	ServerFolder,
	ServerFileProblem,
	ServerInput,
	SidebarRow,
} from '@factorai/types';
import {
	Button,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuSub,
	DropdownMenuSubContent,
	DropdownMenuSubTrigger,
	DropdownMenuTrigger,
	IconButton,
	Input,
} from '@factorai/ui';
import {
	DndContext,
	type DragEndEvent,
	DragOverlay,
	PointerSensor,
	useDraggable,
	useDroppable,
	useSensor,
	useSensors,
} from '@dnd-kit/core';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
	ChevronDown,
	ChevronRight,
	Folder,
	FolderPlus,
	Lock,
	LockOpen,
	MoreHorizontal,
	Server as ServerIcon,
	ShieldAlert,
	TerminalSquare,
	TriangleAlert,
	X,
} from 'lucide-react';
import { type ReactNode, useEffect, useState } from 'react';
import { AgentMenu, AgentPane, ConfirmBanner, RunningChip } from '@components/servers/AgentPane';
import { ServerDialog } from '@components/servers/ServerDialog';
import { SftpPanel } from '@components/servers/SftpPanel';
import { TunnelsMenu } from '@components/servers/TunnelsMenu';
import { AgentMark } from '@components/layout/AgentMark';
import { ProjectIcon } from '@components/layout/ProjectIcon';
import {
	LiveRow,
	LiveSection,
	NeedsYouPill,
	SidebarSectionHeader,
	SidebarToolbar,
} from '@components/layout/SidebarParts';
import { agentName } from '@lib/agents';
import { answerHostKey, reconnectTab, ServerTerminal } from '@components/servers/ServerTerminal';
import { disposeTerminal } from '@components/terminal/Terminal';
import { formatError } from '@lib/errors';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { tabLabel, useServerStore } from '@store/serverStore';

const POLL_MS = 3_000;

function useServerBook() {
	return useQuery({
		queryKey: queryKeys.servers(),
		queryFn: () => cmd.listServers(),
		refetchInterval: POLL_MS,
	});
}

/** The production marker (ADR-0108 token): a small word, never a whole row. */
function ProdBadge() {
	return (
		<span
			data-testid="prod-badge"
			className="rounded px-1 text-[10px] font-semibold leading-4 tracking-wide text-production bg-production/10"
		>
			PROD
		</span>
	);
}

/** Close a tab: its shell is hung up and its terminal thrown away. */
function closeServerTab(key: string, persistent = false) {
	const tab = useServerStore.getState().tabs.find((t) => t.key === key);
	if (tab?.terminalId) cmd.terminalKill(tab.terminalId).catch(() => {});
	// Closing a persistent tab is the gesture that ends its tmux session (F49).
	if (tab && persistent) cmd.serverEndTab(tab.serverId, key).catch(() => {});
	disposeTerminal(key);
	useServerStore.getState().close(key);
}

/**
 * The Server mode (F45, ADR-0107): the active SSH session filling the card; the
 * address book is the shared sidebar's content (ADR-0115). The tab strip is in the top bar.
 */
export function ServerModeView() {
	const book = useServerBook();
	const tabs = useServerStore((s) => s.tabs);
	const active = useServerStore((s) => s.active);
	const attach = useServerStore((s) => s.attach);
	const tab = tabs.find((t) => t.key === active) ?? null;
	const [filesOpen, setFilesOpen] = useState(false);
	const servers = book.data?.servers ?? [];
	const server = tab ? servers.find((s) => s.id === tab.serverId) : undefined;

	// A renderer reload keeps every PTY running: give each tab its live one back
	// instead of logging in a second time.
	useEffect(() => {
		cmd.terminalList().then((list) => {
			for (const t of list) {
				if (t.kind === 'ssh' && t.clientKey) attach(t.clientKey, t.id);
			}
		});
	}, [attach]);

	return (
		<div className="flex min-h-0 min-w-0 flex-1 pr-2 pb-2" data-testid="server-mode">
			<section
				className={`flex min-w-0 flex-1 flex-col overflow-hidden rounded-xl border bg-background shadow-card ${
					server?.environment === 'prod' ? 'border-production' : 'border-border'
				}`}
			>
				{tab && server ? (
					<>
						<header className="flex h-12 shrink-0 items-center gap-2 border-border border-b px-4">
							<ServerIcon className="size-4 text-muted-foreground" />
							<span className="truncate text-sm font-semibold">{server.name}</span>
							<span className="truncate font-mono text-xs text-muted-foreground">
								{server.username}@{server.host}
								{server.startDir ? `:${server.startDir}` : ''}
							</span>
							{server.environment === 'prod' && <ProdBadge />}
							<div className="flex-1" />
							{tab.dead ? (
								<Button size="sm" variant="outline" onClick={() => reconnectTab(tab.key)}>
									Reconnect
								</Button>
							) : (
								<AgentMenu tab={tab} server={server} />
							)}
							{server.auth !== 'system' && <TunnelsMenu server={server} />}
							{server.auth !== 'system' && (
								<Button
									size="sm"
									variant={filesOpen ? 'secondary' : 'ghost'}
									data-testid="toggle-files"
									aria-pressed={filesOpen}
									onClick={() => setFilesOpen((o) => !o)}
								>
									Files
								</Button>
							)}
						</header>
						<ConfirmBanner tab={tab} />
						<div className="min-h-0 flex-1">
							{tab.dead ? (
								<div className="flex h-full flex-col items-center justify-center gap-3 text-sm text-muted-foreground">
									{tab.error ?? 'The session ended.'}
									<Button
										size="sm"
										data-testid="server-reconnect"
										onClick={() => reconnectTab(tab.key)}
									>
										Reconnect to {server.name}
									</Button>
								</div>
							) : (
								<div className="flex h-full min-w-0">
									<div className="relative min-w-0 flex-1">
										<ServerTerminal key={tab.key} tab={tab} />
										<RunningChip tab={tab} />
									</div>
									{tab.agent && <AgentPane key={tab.agent.sessionId} agent={tab.agent} />}
									{filesOpen && server.auth !== 'system' && (
										<SftpPanel key={server.id} server={server} />
									)}
								</div>
							)}
						</div>
					</>
				) : (
					<div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
						<ServerIcon className="size-6" />
						{servers.length === 0
							? 'Add a server to open an SSH session on it.'
							: 'Double-click a server to open an SSH session.'}
					</div>
				)}
			</section>
			<HostKeyDialog servers={servers} />
		</div>
	);
}

const flatProjects = (rows: SidebarRow[]): Project[] =>
	rows.flatMap((r) => (r.kind === 'project' ? [r.project] : r.children.map((c) => c.project)));

const byName = <T extends { name: string }>(a: T, b: T) => a.name.localeCompare(b.name);
const fileName = (relPath: string) => relPath.split('/').pop() ?? relPath;

const EMPTY_BOOK: ServerBookData = { folders: [], servers: [], problems: [] };

/** The address book, in the shared sidebar's body. */
export function ServerBook() {
	const { servers, folders, problems } = useServerBook().data ?? EMPTY_BOOK;
	const sidebar = useQuery({ queryKey: queryKeys.sidebar(), queryFn: () => cmd.listSidebar() });
	const qc = useQueryClient();
	const open = useServerStore((s) => s.open);
	const collapsed = useServerStore((s) => s.collapsed);
	const toggleFolder = useServerStore((s) => s.toggleFolder);
	const tabs = useServerStore((s) => s.tabs);
	const confirms = useServerStore((s) => s.confirms);
	const activate = useServerStore((s) => s.activate);
	const [term, setTerm] = useState('');
	const q = term.trim().toLowerCase();
	const matches = (s: Server) =>
		!q || s.name.toLowerCase().includes(q) || s.host.toLowerCase().includes(q);
	const [editing, setEditing] = useState<{ server: Server | null; folderId: string | null } | null>(
		null,
	);
	const [deleting, setDeleting] = useState<Server | null>(null);
	const [newFolder, setNewFolder] = useState<{ projectId: string | null; name: string } | null>(
		null,
	);
	const refresh = () => qc.invalidateQueries({ queryKey: queryKeys.servers() });

	const createFolder = useMutation({
		mutationFn: (f: { projectId: string | null; name: string }) =>
			cmd.createServerFolder(f.name, f.projectId ?? undefined),
		onSuccess: () => {
			setNewFolder(null);
			refresh();
		},
	});
	const deleteFolder = useMutation({
		mutationFn: (id: string) => cmd.deleteServerFolder(id),
		onSuccess: refresh,
	});

	const fix = useMutation({
		mutationFn: (p: ServerFileProblem) => cmd.fixServerFile(p.projectId, p.relPath),
		onSettled: refresh,
	});

	// Drag a server onto a folder, or onto its home's heading for no folder; the
	// same as `Move to folder` in its menu (F45). Within its home only: moving a
	// server into a project's repository is the Edit form's, where it is said.
	const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));
	const [dragging, setDragging] = useState<Server | null>(null);
	const move = useMutation({
		mutationFn: (v: { server: Server; folderId: string | null }) =>
			cmd.saveServer(moveInput(v.server, v.folderId)),
		onSettled: refresh,
	});
	const moveTo = (server: Server, folderId: string | null) => {
		if (server.folderId !== folderId) move.mutate({ server, folderId });
	};
	const onDragEnd = (e: DragEndEvent) => {
		setDragging(null);
		const server = servers.find((x) => x.id === e.active.id);
		const target = e.over?.data.current as DropData | undefined;
		if (server && target && target.home === homeOf(server)) moveTo(server, target.folderId);
	};

	const problemRow = (p: ServerFileProblem) => {
		const label =
			p.kind === 'duplicate'
				? 'Make it a separate server'
				: p.kind === 'noId'
					? 'Give it an id'
					: null;
		return (
			<div
				key={p.relPath}
				data-testid={`server-problem-${fileName(p.relPath)}`}
				title={p.message}
				className="flex h-7 items-center gap-2 pl-2 text-sm text-muted-foreground"
			>
				<TriangleAlert className="size-3.5 shrink-0" />
				<span className="min-w-0 flex-1 truncate">{fileName(p.relPath)}</span>
				{label && (
					<Button
						size="sm"
						variant="ghost"
						className="h-6 px-2 text-xs"
						disabled={fix.isPending}
						onClick={() => fix.mutate(p)}
					>
						{label}
					</Button>
				)}
			</div>
		);
	};

	const row = (s: Server, indent: boolean) => {
		const sessions = tabs.filter((t) => t.serverId === s.id).length;
		const homeFolders = folders.filter((f) => (f.projectId ?? '') === homeOf(s)).sort(byName);
		return (
			<Draggable key={s.id} id={s.id}>
				<div
					role="button"
					tabIndex={0}
					data-testid={`server-row-${s.name}`}
					title={`${s.username}@${s.host}:${s.port} · double-click to open a session`}
					onDoubleClick={() => open(s.id)}
					onKeyDown={(e) => {
						if (e.key === 'Enter') open(s.id);
					}}
					className={`group flex h-7 items-center gap-2 rounded-md pr-1 text-sm hover:bg-secondary/60 ${
						indent ? 'pl-7' : 'pl-2'
					}`}
				>
					<span
						className={`size-1.5 shrink-0 rounded-full ${s.connected ? 'bg-status-working' : 'border border-muted-foreground'}`}
					/>
					<span className="min-w-0 flex-1 truncate">{s.name}</span>
					{s.environment === 'prod' && <ProdBadge />}
					{sessions > 0 && (
						<span className="text-xs text-muted-foreground tabular-nums">{sessions}</span>
					)}
					<DropdownMenu>
						<DropdownMenuTrigger asChild>
							<IconButton
								aria-label={`${s.name} actions`}
								className="opacity-0 group-hover:opacity-100"
							>
								<MoreHorizontal />
							</IconButton>
						</DropdownMenuTrigger>
						<DropdownMenuContent align="end">
							<DropdownMenuItem onSelect={() => open(s.id)}>
								<TerminalSquare /> Open SSH session
							</DropdownMenuItem>
							<DropdownMenuItem onSelect={() => setEditing({ server: s, folderId: s.folderId })}>
								Edit…
							</DropdownMenuItem>
							{s.connected && (
								<DropdownMenuItem
									onSelect={() =>
										cmd
											.serverDisconnect(s.id)
											.then(refresh)
											.catch(() => {})
									}
								>
									Disconnect
								</DropdownMenuItem>
							)}
							{homeFolders.length > 0 && (
								<DropdownMenuSub>
									<DropdownMenuSubTrigger data-testid="server-move">
										Move to folder
									</DropdownMenuSubTrigger>
									<DropdownMenuSubContent>
										{[{ id: null, name: 'No folder' }, ...homeFolders].map((f) => (
											<DropdownMenuItem
												key={f.id ?? ''}
												disabled={s.folderId === f.id}
												onSelect={() => moveTo(s, f.id)}
											>
												{f.name}
											</DropdownMenuItem>
										))}
									</DropdownMenuSubContent>
								</DropdownMenuSub>
							)}
							<DropdownMenuSeparator />
							<DropdownMenuItem className="text-destructive" onSelect={() => setDeleting(s)}>
								Delete…
							</DropdownMenuItem>
						</DropdownMenuContent>
					</DropdownMenu>
				</div>
			</Draggable>
		);
	};

	/** A home's folders, then its loose servers, then its problem files. */
	const tree = (fs: ServerFolder[], all: Server[], ps: ServerFileProblem[] = []) => {
		const ss = all.filter(matches);
		const inFolder = (f: ServerFolder) => ss.filter((s) => s.folderId === f.id).sort(byName);
		const loose = ss.filter((s) => !fs.some((f) => f.id === s.folderId)).sort(byName);
		return (
			<>
				{[...fs]
					.sort(byName)
					.filter((f) => !q || inFolder(f).length > 0)
					.map((f) => {
						const closed = collapsed.includes(f.id);
						return (
							<Drop
								key={f.id}
								id={`folder:${f.id}`}
								data={{ home: f.projectId ?? '', folderId: f.id }}
							>
								<div className="group flex h-7 items-center gap-1.5 rounded-md pr-1 pl-1 text-sm hover:bg-secondary/60">
									<button
										type="button"
										className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
										aria-expanded={!closed}
										onClick={() => toggleFolder(f.id)}
									>
										{closed ? (
											<ChevronRight className="size-3.5 text-muted-foreground" />
										) : (
											<ChevronDown className="size-3.5 text-muted-foreground" />
										)}
										<Folder className="size-3.5 text-muted-foreground" />
										<span className="truncate">{f.name}</span>
									</button>
									<span className="text-xs text-muted-foreground tabular-nums">
										{inFolder(f).length}
									</span>
									<DropdownMenu>
										<DropdownMenuTrigger asChild>
											<IconButton
												aria-label={`${f.name} actions`}
												className="opacity-0 group-hover:opacity-100"
											>
												<MoreHorizontal />
											</IconButton>
										</DropdownMenuTrigger>
										<DropdownMenuContent align="end">
											<DropdownMenuItem
												onSelect={() => setEditing({ server: null, folderId: f.id })}
											>
												New server here…
											</DropdownMenuItem>
											<DropdownMenuItem onSelect={() => deleteFolder.mutate(f.id)}>
												Delete folder (keeps its servers)
											</DropdownMenuItem>
										</DropdownMenuContent>
									</DropdownMenu>
								</div>
								{!closed && inFolder(f).map((s) => row(s, true))}
							</Drop>
						);
					})}
				{loose.map((s) => row(s, false))}
				{ps.map(problemRow)}
			</>
		);
	};
	const projects = flatProjects(sidebar.data ?? []);

	// In progress (ADR-0117): agent commands waiting for your yes or no first
	// (ADR-0110), then each server with SSH sessions open.
	const nameOf = new Map(servers.map((s) => [s.id, s]));
	const openOn = [...new Set(tabs.filter((t) => !t.dead).map((t) => t.serverId))];
	const live = [
		...confirms.map((c) => (
			<LiveRow
				key={c.id}
				testId="live-server-confirm"
				mark={<span className="size-2 rounded-full bg-status-waiting" />}
				label={c.serverName}
				detail="command to confirm"
				meta={<NeedsYouPill n={1} />}
				onClick={() => {
					const tab = tabs.find((t) => t.terminalId === c.terminalId);
					if (tab) activate(tab.key);
				}}
			/>
		)),
		...openOn.map((id) => {
			const open = tabs.filter((t) => t.serverId === id && !t.dead);
			const s = nameOf.get(id);
			return (
				<LiveRow
					key={id}
					testId="live-server"
					mark={
						<span
							className={`size-2 rounded-full ${s?.connected ? 'bg-status-working' : 'bg-status-stopped'}`}
						/>
					}
					label={s?.name ?? id}
					detail={`${open.length} SSH session${open.length === 1 ? '' : 's'}`}
					onClick={() => open[0] && activate(open[0].key)}
				/>
			);
		}),
	];

	/** The name field of a new folder, in the home it was asked for. */
	const folderForm = (projectId: string | null) =>
		newFolder && newFolder.projectId === projectId ? (
			<form
				className="px-1 pb-1"
				onSubmit={(e) => {
					e.preventDefault();
					if (newFolder.name.trim()) createFolder.mutate(newFolder);
				}}
			>
				<Input
					autoFocus
					placeholder="Folder name"
					className="h-7"
					value={newFolder.name}
					onChange={(e) => setNewFolder({ projectId, name: e.target.value })}
					onBlur={() => !newFolder.name.trim() && setNewFolder(null)}
				/>
			</form>
		) : null;

	return (
		<div className="flex min-h-0 flex-1 flex-col">
			<SidebarToolbar
				value={term}
				onChange={setTerm}
				placeholder="Filter servers…"
				action={{
					label: 'New server',
					testId: 'new-server',
					onClick: () => setEditing({ server: null, folderId: null }),
				}}
			/>
			<LiveSection rows={live} />
			<SidebarSectionHeader
				title="Servers"
				className="mt-2"
				actions={
					<IconButton
						aria-label="New folder"
						title="New folder"
						onClick={() => setNewFolder({ projectId: null, name: '' })}
					>
						<FolderPlus />
					</IconButton>
				}
			/>
			<DndContext
				sensors={sensors}
				onDragStart={(e) => setDragging(servers.find((x) => x.id === e.active.id) ?? null)}
				onDragCancel={() => setDragging(null)}
				onDragEnd={onDragEnd}
			>
				<div className="min-h-0 flex-1 overflow-y-auto px-2" data-testid="server-book">
					{folderForm(null)}
					<Section title="This computer" testId="local" home="">
						{tree(
							folders.filter((f) => f.projectId === null),
							servers.filter((s) => s.source.kind === 'local'),
						)}
					</Section>
					{projects.map((p) => {
						const pf = folders.filter((f) => f.projectId === p.id);
						const ps = servers.filter(
							(s) => s.source.kind === 'project' && s.source.projectId === p.id,
						);
						const pp = problems.filter((x) => x.projectId === p.id);
						if (!ps.length && !pp.length) return null;
						return (
							<Section
								key={p.id}
								testId={p.id}
								home={p.id}
								title={p.displayName}
								icon={<ProjectIcon name={p.displayName} size={18} />}
								action={
									<IconButton
										aria-label={`New folder in ${p.displayName}`}
										title="New folder"
										onClick={() => setNewFolder({ projectId: p.id, name: '' })}
									>
										<FolderPlus />
									</IconButton>
								}
							>
								{folderForm(p.id)}
								{tree(pf, ps, pp)}
							</Section>
						);
					})}
				</div>
				<DragOverlay dropAnimation={null}>
					{dragging && (
						<div
							data-testid="drag-chip"
							className="pointer-events-none inline-flex max-w-40 items-center gap-2 rounded border border-border bg-card px-2 py-1 text-foreground text-sm shadow-md"
						>
							<span className="min-w-0 truncate">{dragging.name}</span>
						</div>
					)}
				</DragOverlay>
			</DndContext>
			<ServerDialog
				open={editing !== null}
				onOpenChange={(o) => !o && setEditing(null)}
				server={editing?.server ?? null}
				folderId={editing?.folderId ?? null}
				folders={folders}
				projects={projects}
			/>
			<DeleteServerDialog server={deleting} onClose={() => setDeleting(null)} />
		</div>
	);
}

function Section({
	title,
	testId,
	home,
	icon,
	action,
	children,
}: {
	title: string;
	testId: string;
	/** `''` for this computer, else the project id: dropping on the heading
	 *  takes a server of this home out of its folder. */
	home: string;
	icon?: React.ReactNode;
	action?: React.ReactNode;
	children: React.ReactNode;
}) {
	return (
		<section data-testid="server-section" data-section={testId} className="pb-1">
			<Drop id={`home:${home}`} data={{ home, folderId: null }}>
				<div className="flex h-7 items-center gap-1.5 px-1 text-xs font-medium text-muted-foreground">
					{icon}
					<span className="min-w-0 flex-1 truncate">{title}</span>
					{action}
				</div>
			</Drop>
			{children}
		</section>
	);
}

interface DropData {
	home: string;
	folderId: string | null;
}

const homeOf = (s: Server) => (s.source.kind === 'project' ? s.source.projectId : '');

/** The save that only changes the folder: secrets are left out, so the vault keeps
 *  them, and a project file is written only if it is still the one read. */
function moveInput(s: Server, folderId: string | null): ServerInput {
	return {
		id: s.id,
		name: s.name,
		host: s.host,
		port: s.port,
		username: s.username,
		startDir: s.startDir,
		folderId,
		baseSha: s.source.kind === 'project' ? s.source.sha256 : null,
		environment: s.environment,
		agentPolicy: s.agentPolicy,
		auth: s.auth,
		keyPath: s.keyPath,
		persistent: s.persistent,
	};
}

/** A server row you can pick up; the drag starts after 4px, so clicks and
 *  double-clicks still reach the row. */
function Draggable({ id, children }: { id: string; children: ReactNode }) {
	const { setNodeRef, listeners, isDragging } = useDraggable({ id });
	return (
		<div ref={setNodeRef} {...listeners} className={isDragging ? 'opacity-40' : undefined}>
			{children}
		</div>
	);
}

/** A folder or a home heading a server can be dropped on. Ringed, as a project
 *  group is (F1), only while the server held is from the same home. */
function Drop({ id, data, children }: { id: string; data: DropData; children: ReactNode }) {
	const { setNodeRef, isOver, active } = useDroppable({ id, data });
	const held = useServerBook().data?.servers.find((s) => s.id === active?.id);
	const lit = isOver && held !== undefined && homeOf(held) === data.home;
	return (
		<div
			ref={setNodeRef}
			data-testid={`server-drop-${id}`}
			className={lit ? 'rounded-md bg-secondary ring-1 ring-primary' : undefined}
		>
			{children}
		</div>
	);
}

function DeleteServerDialog({ server, onClose }: { server: Server | null; onClose: () => void }) {
	const qc = useQueryClient();
	const del = useMutation({
		mutationFn: async (id: string) => {
			// A deleted server's persistent tabs end with it, while it can still be reached.
			if (server?.persistent) {
				const tabs = useServerStore.getState().tabs.filter((t) => t.serverId === id);
				await Promise.allSettled(tabs.map((t) => cmd.serverEndTab(id, t.key)));
			}
			await cmd.deleteServer(id);
		},
		onSuccess: (_v, id) => {
			for (const t of useServerStore.getState().tabs.filter((t) => t.serverId === id)) {
				closeServerTab(t.key);
			}
			qc.invalidateQueries({ queryKey: queryKeys.servers() });
			onClose();
		},
	});
	return (
		<Dialog open={server !== null} onOpenChange={(o) => !o && onClose()}>
			<DialogContent>
				<DialogHeader>
					<DialogTitle>Delete {server?.name}?</DialogTitle>
					<DialogDescription>
						Its open sessions are closed and its saved password and keys are removed from the vault.
						This cannot be undone.
					</DialogDescription>
				</DialogHeader>
				{del.error && <p className="text-sm text-destructive">{formatError(del.error)}</p>}
				<DialogFooter>
					<Button variant="outline" onClick={onClose}>
						Cancel
					</Button>
					<Button variant="destructive" onClick={() => server && del.mutate(server.id)}>
						Delete server
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}

/** "Trust this host?" — the one decision about a server only the human makes. */
function HostKeyDialog({ servers }: { servers: Server[] }) {
	const pending = useServerStore((s) => s.pendingHostKey);
	const server = servers.find((s) => s.id === pending?.serverId);
	return (
		<Dialog
			open={pending !== null}
			onOpenChange={(o) => !o && pending && answerHostKey(pending.tabKey, false)}
		>
			<DialogContent data-testid="host-key-dialog">
				<DialogHeader>
					<DialogTitle className="flex items-center gap-2">
						<ShieldAlert className="size-5 text-muted-foreground" />
						First connection to {server?.name ?? 'this server'}
					</DialogTitle>
					<DialogDescription>
						{server ? `${server.host}:${server.port}` : ''} presents a key this computer has never
						seen. Compare it with the server's own fingerprint before trusting it.
					</DialogDescription>
				</DialogHeader>
				<code className="block rounded-md border border-border bg-secondary px-3 py-2 font-mono text-xs break-all">
					{pending?.fingerprint}
				</code>
				<DialogFooter>
					<Button variant="outline" onClick={() => pending && answerHostKey(pending.tabKey, false)}>
						Cancel
					</Button>
					<Button
						data-testid="trust-host-key"
						onClick={() => pending && answerHostKey(pending.tabKey, true)}
					>
						Trust and connect
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}

/** The vault's state in the sidebar footer's row, and its master password. */
export function VaultFooter() {
	const qc = useQueryClient();
	const status = useQuery({ queryKey: queryKeys.vault(), queryFn: () => cmd.vaultStatus() });
	const [dialog, setDialog] = useState<'unlock' | 'set' | null>(null);
	const [pw, setPw] = useState('');
	const done = () => {
		setPw('');
		setDialog(null);
		qc.invalidateQueries({ queryKey: queryKeys.vault() });
		qc.invalidateQueries({ queryKey: queryKeys.servers() });
	};
	const act = useMutation({
		mutationFn: async () => {
			if (dialog === 'unlock') return cmd.vaultUnlock(pw);
			return cmd.vaultSetMasterPassword(pw || null);
		},
		onSuccess: done,
	});
	const s = status.data;
	// No keychain on this machine (a headless Linux session): nothing to unlock,
	// the way forward is a master password.
	const needsSetup = s?.mode === 'keychain' && !!s.keychainError;
	const locked = (s?.locked ?? false) && !needsSetup;
	const vaultText = needsSetup
		? 'No OS keychain'
		: locked
			? 'Vault locked'
			: s?.mode === 'masterPassword'
				? 'Vault unlocked'
				: 'Vault (OS keychain)';
	return (
		<div className="flex min-w-0 shrink items-center gap-1.5 text-xs text-muted-foreground">
			{locked ? <Lock className="size-3.5" /> : <LockOpen className="size-3.5" />}
			<span
				className="flex-1 truncate"
				title={s?.keychainError ?? vaultText}
				data-testid="vault-state"
			>
				{vaultText}
			</span>
			{needsSetup ? (
				<Button
					size="sm"
					variant="ghost"
					className="h-6 px-2 text-xs"
					onClick={() => setDialog('set')}
				>
					Set up
				</Button>
			) : locked ? (
				<Button
					size="sm"
					variant="ghost"
					className="h-6 px-2 text-xs"
					onClick={() => setDialog('unlock')}
				>
					Unlock
				</Button>
			) : (
				<DropdownMenu>
					<DropdownMenuTrigger asChild>
						<IconButton aria-label="Vault actions" title="Vault actions">
							<MoreHorizontal />
						</IconButton>
					</DropdownMenuTrigger>
					<DropdownMenuContent align="end">
						<DropdownMenuItem onSelect={() => setDialog('set')}>
							{s?.mode === 'masterPassword' ? 'Change master password…' : 'Set a master password…'}
						</DropdownMenuItem>
						{s?.mode === 'masterPassword' && (
							<DropdownMenuItem onSelect={() => cmd.vaultLock().then(done)}>
								Lock now
							</DropdownMenuItem>
						)}
					</DropdownMenuContent>
				</DropdownMenu>
			)}
			<Dialog open={dialog !== null} onOpenChange={(o) => !o && setDialog(null)}>
				<DialogContent>
					<DialogHeader>
						<DialogTitle>
							{dialog === 'unlock' ? 'Unlock the vault' : 'Master password'}
						</DialogTitle>
						<DialogDescription>
							{dialog === 'unlock'
								? 'Your saved SSH passwords and keys are encrypted with your master password.'
								: 'With a master password the vault is locked at every start, and the key is no longer kept in the OS keychain. Leave it empty to go back to the keychain.'}
						</DialogDescription>
					</DialogHeader>
					<form
						onSubmit={(e) => {
							e.preventDefault();
							act.mutate();
						}}
					>
						<Input
							autoFocus
							type="password"
							autoComplete="off"
							aria-label="Master password"
							value={pw}
							onChange={(e) => setPw(e.target.value)}
						/>
					</form>
					{act.error && <p className="text-sm text-destructive">{formatError(act.error)}</p>}
					<DialogFooter>
						<Button variant="outline" onClick={() => setDialog(null)}>
							Cancel
						</Button>
						<Button
							disabled={act.isPending || (dialog === 'unlock' && !pw)}
							onClick={() => act.mutate()}
						>
							{dialog === 'unlock' ? 'Unlock' : 'Save'}
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
		</div>
	);
}

/** The Server mode's row of the top bar: one tab per SSH session. */
export function ServerTabs() {
	const book = useServerBook();
	const tabs = useServerStore((s) => s.tabs);
	const active = useServerStore((s) => s.active);
	const activate = useServerStore((s) => s.activate);
	const running = useServerStore((s) => s.running);
	const confirms = useServerStore((s) => s.confirms);
	const byId = new Map((book.data?.servers ?? []).map((s) => [s.id, s]));
	return (
		<div
			role="tablist"
			aria-label="SSH sessions"
			className="scrollbar-hairline flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
			onWheel={wheelScrollsX}
		>
			{tabs.map((t) => {
				const s = byId.get(t.serverId);
				const name = s?.name ?? 'removed server';
				return (
					<div
						key={t.key}
						role="tab"
						tabIndex={0}
						aria-selected={t.key === active}
						data-testid="server-tab"
						onClick={() => activate(t.key)}
						onKeyDown={(e) => e.key === 'Enter' && activate(t.key)}
						onAuxClick={(e) => e.button === 1 && closeServerTab(t.key, s?.persistent)}
						className={`relative flex h-7.5 shrink-0 items-center gap-2 rounded-lg pr-1 pl-2.5 text-sm ${
							t.key === active
								? 'bg-background text-foreground shadow-sm'
								: 'text-muted-foreground hover:bg-secondary/60'
						}`}
					>
						{s?.environment === 'prod' && (
							<span className="absolute inset-x-2 top-0 h-0.5 rounded-full bg-production" />
						)}
						<span
							className={`size-1.5 rounded-full ${t.dead || !t.terminalId ? 'border border-muted-foreground' : 'bg-status-working'}`}
						/>
						<span className="max-w-48 truncate">{tabLabel(tabs, t, name)}</span>
						{s?.environment === 'prod' && <ProdBadge />}
						{t.agent && (
							<span
								data-testid="tab-agent"
								className={`flex h-5 items-center gap-1 rounded-full px-1.5 text-xs ${
									t.terminalId && running[t.terminalId]
										? 'bg-status-working/15 text-foreground'
										: 'bg-secondary text-muted-foreground'
								}`}
							>
								<AgentMark agent={t.agent.agent} className="size-3" />
								{confirms.some((c) => c.terminalId === t.terminalId)
									? 'confirm'
									: t.agent.mode === 'own'
										? `${agentName(t.agent.agent)}'s`
										: t.terminalId && running[t.terminalId]
											? 'running'
											: agentName(t.agent.agent)}
							</span>
						)}
						<IconButton
							aria-label={`Close ${name}`}
							onClick={(e) => {
								e.stopPropagation();
								closeServerTab(t.key, s?.persistent);
							}}
						>
							<X />
						</IconButton>
					</div>
				);
			})}
		</div>
	);
}
