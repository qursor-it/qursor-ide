import type { Server, ServerFolder } from '@factorai/types';
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
	DropdownMenuTrigger,
	IconButton,
	Input,
} from '@factorai/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
	ChevronDown,
	ChevronRight,
	Folder,
	FolderPlus,
	Lock,
	LockOpen,
	MoreHorizontal,
	Plus,
	Server as ServerIcon,
	ShieldAlert,
	TerminalSquare,
	X,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { AgentMenu, AgentPane, RunningChip } from '@components/servers/AgentPane';
import { ServerDialog } from '@components/servers/ServerDialog';
import { AgentMark } from '@components/layout/AgentMark';
import { agentName } from '@lib/agents';
import { answerHostKey, reconnectTab, ServerTerminal } from '@components/servers/ServerTerminal';
import { disposeTerminal } from '@components/terminal/Terminal';
import { formatError } from '@lib/errors';
import { queryKeys } from '@lib/queryKeys';
import { cmd, events } from '@lib/tauri';
import { tabLabel, useServerStore } from '@store/serverStore';

const POLL_MS = 3_000;

export function useServerBook() {
	return useQuery({
		queryKey: queryKeys.servers(),
		queryFn: () => cmd.listServers(),
		refetchInterval: POLL_MS,
	});
}

/** The production marker (ADR-0106 token): a small word, never a whole row. */
export function ProdBadge() {
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
export function closeServerTab(key: string) {
	const tab = useServerStore.getState().tabs.find((t) => t.key === key);
	if (tab?.terminalId) cmd.terminalKill(tab.terminalId).catch(() => {});
	disposeTerminal(key);
	useServerStore.getState().close(key);
}

/**
 * The Server mode (F44, ADR-0105): the address book beside the rail and the
 * active SSH session filling the card. The tab strip is in the top bar.
 */
export function ServerModeView() {
	const book = useServerBook();
	const tabs = useServerStore((s) => s.tabs);
	const active = useServerStore((s) => s.active);
	const attach = useServerStore((s) => s.attach);
	const tab = tabs.find((t) => t.key === active) ?? null;
	const servers = book.data?.servers ?? [];
	const server = tab ? servers.find((s) => s.id === tab.serverId) : undefined;

	// A renderer reload keeps every PTY running: give each tab its live one back
	// instead of logging in a second time.
	// "Claude is running …" on the tab and over its terminal (F45).
	useEffect(() => {
		let off: (() => void) | undefined;
		let gone = false;
		events
			.onAgentTerminalRun((e) =>
				useServerStore.getState().setRunning(e.terminalId, e.running ? e.command : null),
			)
			.then((fn) => {
				if (gone) fn();
				else off = fn;
			});
		return () => {
			gone = true;
			off?.();
		};
	}, []);

	useEffect(() => {
		cmd.terminalList().then((list) => {
			for (const t of list) {
				if (t.kind === 'ssh' && t.clientKey) attach(t.clientKey, t.id);
			}
		});
	}, [attach]);

	return (
		<div className="flex min-h-0 min-w-0 flex-1 pr-2 pb-2" data-testid="server-mode">
			<aside className="flex w-64 shrink-0 flex-col bg-chrome">
				<ServerSidebar servers={servers} folders={book.data?.folders ?? []} />
			</aside>
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
						</header>
						<div className="min-h-0 flex-1">
							{tab.dead ? (
								<div className="flex h-full items-center justify-center text-sm text-muted-foreground">
									The session ended.
								</div>
							) : (
								<div className="flex h-full min-w-0">
									<div className="relative min-w-0 flex-1">
										<ServerTerminal key={tab.key} tab={tab} />
										<RunningChip tab={tab} />
									</div>
									{tab.agent && <AgentPane key={tab.agent.sessionId} agent={tab.agent} />}
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

function ServerSidebar({ servers, folders }: { servers: Server[]; folders: ServerFolder[] }) {
	const qc = useQueryClient();
	const open = useServerStore((s) => s.open);
	const collapsed = useServerStore((s) => s.collapsed);
	const toggleFolder = useServerStore((s) => s.toggleFolder);
	const tabs = useServerStore((s) => s.tabs);
	const [editing, setEditing] = useState<{ server: Server | null; folderId: string | null } | null>(
		null,
	);
	const [deleting, setDeleting] = useState<Server | null>(null);
	const [newFolder, setNewFolder] = useState<string | null>(null);
	const refresh = () => qc.invalidateQueries({ queryKey: queryKeys.servers() });

	const createFolder = useMutation({
		mutationFn: (name: string) => cmd.createServerFolder(name),
		onSuccess: () => {
			setNewFolder(null);
			refresh();
		},
	});
	const deleteFolder = useMutation({
		mutationFn: (id: string) => cmd.deleteServerFolder(id),
		onSuccess: refresh,
	});

	const byFolder = useMemo(() => {
		const m = new Map<string | null, Server[]>();
		for (const s of servers) {
			const k = folders.some((f) => f.id === s.folderId) ? s.folderId : null;
			m.set(k, [...(m.get(k) ?? []), s]);
		}
		return m;
	}, [servers, folders]);

	const row = (s: Server, indent: boolean) => {
		const sessions = tabs.filter((t) => t.serverId === s.id).length;
		return (
			<div
				key={s.id}
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
						<DropdownMenuSeparator />
						<DropdownMenuItem className="text-destructive" onSelect={() => setDeleting(s)}>
							Delete…
						</DropdownMenuItem>
					</DropdownMenuContent>
				</DropdownMenu>
			</div>
		);
	};

	return (
		<div className="flex min-h-0 flex-1 flex-col px-2">
			<div className="flex h-9 items-center gap-1 px-1 text-xs font-medium tracking-wider text-muted-foreground uppercase">
				<span className="flex-1">Servers</span>
				<IconButton aria-label="New folder" title="New folder" onClick={() => setNewFolder('')}>
					<FolderPlus />
				</IconButton>
				<IconButton
					aria-label="New server"
					title="New server"
					data-testid="new-server"
					onClick={() => setEditing({ server: null, folderId: null })}
				>
					<Plus />
				</IconButton>
			</div>
			<div className="min-h-0 flex-1 overflow-y-auto" data-testid="server-book">
				{newFolder !== null && (
					<form
						className="px-1 pb-1"
						onSubmit={(e) => {
							e.preventDefault();
							if (newFolder.trim()) createFolder.mutate(newFolder);
						}}
					>
						<Input
							autoFocus
							placeholder="Folder name"
							className="h-7"
							value={newFolder}
							onChange={(e) => setNewFolder(e.target.value)}
							onBlur={() => !newFolder.trim() && setNewFolder(null)}
						/>
					</form>
				)}
				{folders.map((f) => {
					const closed = collapsed.includes(f.id);
					return (
						<div key={f.id}>
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
									{byFolder.get(f.id)?.length ?? 0}
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
										<DropdownMenuItem onSelect={() => setEditing({ server: null, folderId: f.id })}>
											New server here…
										</DropdownMenuItem>
										<DropdownMenuItem onSelect={() => deleteFolder.mutate(f.id)}>
											Delete folder (keeps its servers)
										</DropdownMenuItem>
									</DropdownMenuContent>
								</DropdownMenu>
							</div>
							{!closed && (byFolder.get(f.id) ?? []).map((s) => row(s, true))}
						</div>
					);
				})}
				{(byFolder.get(null) ?? []).map((s) => row(s, false))}
			</div>
			<VaultFooter />
			<ServerDialog
				open={editing !== null}
				onOpenChange={(o) => !o && setEditing(null)}
				server={editing?.server ?? null}
				folderId={editing?.folderId ?? null}
				folders={folders}
			/>
			<DeleteServerDialog server={deleting} onClose={() => setDeleting(null)} />
		</div>
	);
}

function DeleteServerDialog({ server, onClose }: { server: Server | null; onClose: () => void }) {
	const qc = useQueryClient();
	const del = useMutation({
		mutationFn: (id: string) => cmd.deleteServer(id),
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

/** The vault's state at the foot of the book, and its master password. */
function VaultFooter() {
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
	return (
		<div className="flex h-9 shrink-0 items-center gap-1.5 px-1 text-xs text-muted-foreground">
			{locked ? <Lock className="size-3.5" /> : <LockOpen className="size-3.5" />}
			<span
				className="flex-1 truncate"
				title={s?.keychainError ?? undefined}
				data-testid="vault-state"
			>
				{needsSetup
					? 'No OS keychain'
					: locked
						? 'Vault locked'
						: s?.mode === 'masterPassword'
							? 'Vault unlocked'
							: 'Vault (OS keychain)'}
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
						<IconButton aria-label="Vault actions">
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
	const byId = new Map((book.data?.servers ?? []).map((s) => [s.id, s]));
	return (
		<div
			role="tablist"
			aria-label="SSH sessions"
			className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
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
						onAuxClick={(e) => e.button === 1 && closeServerTab(t.key)}
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
								{t.agent.mode === 'own'
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
								closeServerTab(t.key);
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
