import type {
	AgentPolicy,
	Server,
	ServerAuth,
	ServerEnvironment,
	ServerFolder,
	ServerInput,
	SshConfigHost,
} from '@factorai/types';
import {
	Button,
	Checkbox,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
	Input,
	Label,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
	Textarea,
} from '@factorai/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { formatError } from '@lib/errors';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';

const NO_FOLDER = '__none__';

const AUTH_LABEL: Record<ServerAuth, string> = {
	password: 'Password',
	key: 'Private key (in the vault)',
	keyFile: 'Key file on this computer',
	agent: 'SSH agent',
	system: 'System ssh client',
};

const POLICY_LABEL: Record<AgentPolicy, string> = {
	free: 'Free',
	confirm: 'Confirm every command',
	readonly: 'Read only',
};

interface Draft {
	name: string;
	host: string;
	port: string;
	username: string;
	startDir: string;
	folderId: string;
	environment: ServerEnvironment;
	agentPolicy: AgentPolicy;
	auth: ServerAuth;
	keyPath: string;
	persistent: boolean;
	password: string;
	privateKey: string;
	passphrase: string;
}

function draftFrom(s: Server | null, folderId: string | null): Draft {
	return {
		name: s?.name ?? '',
		host: s?.host ?? '',
		port: String(s?.port ?? 22),
		username: s?.username ?? '',
		startDir: s?.startDir ?? '',
		folderId: s?.folderId ?? folderId ?? NO_FOLDER,
		environment: s?.environment ?? 'dev',
		agentPolicy: s?.agentPolicy ?? 'free',
		auth: s?.auth ?? 'agent',
		keyPath: s?.keyPath ?? '',
		persistent: s?.persistent ?? false,
		password: '',
		privateKey: '',
		passphrase: '',
	};
}

/** The form as the backend takes it. Secrets typed here are sent once and
 *  never read back; an untouched field leaves the saved one alone. */
function toInput(d: Draft, id?: string): ServerInput {
	const input: ServerInput = {
		id,
		name: d.name,
		host: d.host,
		port: Number.parseInt(d.port, 10) || 0,
		username: d.username,
		startDir: d.startDir || null,
		folderId: d.folderId === NO_FOLDER ? null : d.folderId,
		environment: d.environment,
		agentPolicy: d.agentPolicy,
		auth: d.auth,
		keyPath: d.auth === 'keyFile' ? d.keyPath : null,
		persistent: d.auth !== 'system' && d.persistent,
	};
	if (d.auth === 'password' && d.password) input.password = d.password;
	if (d.auth === 'key' && d.privateKey) input.privateKey = d.privateKey;
	if ((d.auth === 'key' || d.auth === 'keyFile') && d.passphrase) input.passphrase = d.passphrase;
	return input;
}

/**
 * New or edit server (F45). `server` null is "new"; `folderId` preselects the
 * folder the human started from.
 */
export function ServerDialog({
	open,
	onOpenChange,
	server,
	folders,
	folderId = null,
}: {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	server: Server | null;
	folders: ServerFolder[];
	folderId?: string | null;
}) {
	const qc = useQueryClient();
	const [d, setD] = useState<Draft>(() => draftFrom(server, folderId));
	useEffect(() => {
		if (open) setD(draftFrom(server, folderId));
	}, [open, server, folderId]);
	const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((p) => ({ ...p, [k]: v }));

	const imports = useQuery({
		queryKey: ['ssh-config-hosts'],
		queryFn: () => cmd.sshConfigHosts(),
		enabled: open && !server,
	});
	const fromConfig = (h: SshConfigHost) =>
		setD((p) => ({
			...p,
			name: h.alias,
			host: h.host,
			port: String(h.port),
			username: h.username ?? p.username,
			auth: h.needsSystemSsh ? 'system' : h.identityFile ? 'keyFile' : 'agent',
			keyPath: h.identityFile ?? '',
		}));

	const save = useMutation({
		mutationFn: () => cmd.saveServer(toInput(d, server?.id)),
		onSuccess: () => {
			qc.invalidateQueries({ queryKey: queryKeys.servers() });
			onOpenChange(false);
		},
	});

	const field = (label: string, el: React.ReactNode, id: string) => (
		<div className="grid grid-cols-[9rem_1fr] items-center gap-3">
			<Label htmlFor={id} className="text-muted-foreground">
				{label}
			</Label>
			{el}
		</div>
	);

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="max-w-xl" data-testid="server-dialog">
				<DialogHeader>
					<DialogTitle>{server ? `Edit ${server.name}` : 'New server'}</DialogTitle>
					<DialogDescription>
						Passwords, keys and passphrases go to the encrypted vault and are never shown to an
						agent.
					</DialogDescription>
				</DialogHeader>
				{!server && (imports.data?.length ?? 0) > 0 && (
					<div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
						From ~/.ssh/config:
						{imports.data?.map((h) => (
							<Button
								key={h.alias}
								size="sm"
								variant="outline"
								className="h-6 px-2 text-xs"
								onClick={() => fromConfig(h)}
							>
								{h.alias}
							</Button>
						))}
					</div>
				)}
				<div className="flex flex-col gap-2.5 text-sm">
					{field(
						'Name',
						<Input id="srv-name" value={d.name} onChange={(e) => set('name', e.target.value)} />,
						'srv-name',
					)}
					{field(
						'Host',
						<div className="flex gap-2">
							<Input
								id="srv-host"
								placeholder="web01.example.com"
								value={d.host}
								onChange={(e) => set('host', e.target.value)}
							/>
							<Input
								aria-label="Port"
								className="w-20"
								inputMode="numeric"
								value={d.port}
								onChange={(e) => set('port', e.target.value.replace(/\D/g, ''))}
							/>
						</div>,
						'srv-host',
					)}
					{field(
						'User',
						<Input
							id="srv-user"
							value={d.username}
							onChange={(e) => set('username', e.target.value)}
						/>,
						'srv-user',
					)}
					{field(
						'Start folder',
						<Input
							id="srv-dir"
							className="font-mono"
							placeholder="/var/www (optional)"
							value={d.startDir}
							onChange={(e) => set('startDir', e.target.value)}
						/>,
						'srv-dir',
					)}
					{field(
						'Folder',
						<Select value={d.folderId} onValueChange={(v) => set('folderId', v)}>
							<SelectTrigger id="srv-folder">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value={NO_FOLDER}>No folder</SelectItem>
								{folders.map((f) => (
									<SelectItem key={f.id} value={f.id}>
										{f.name}
									</SelectItem>
								))}
							</SelectContent>
						</Select>,
						'srv-folder',
					)}
					<div className="my-1 border-border border-t" />
					{field(
						'Login',
						<Select value={d.auth} onValueChange={(v) => set('auth', v as ServerAuth)}>
							<SelectTrigger id="srv-auth" data-testid="server-auth">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								{(Object.keys(AUTH_LABEL) as ServerAuth[]).map((a) => (
									<SelectItem key={a} value={a}>
										{AUTH_LABEL[a]}
									</SelectItem>
								))}
							</SelectContent>
						</Select>,
						'srv-auth',
					)}
					{d.auth === 'password' &&
						field(
							'Password',
							<Input
								id="srv-pw"
								type="password"
								autoComplete="off"
								placeholder={server?.hasPassword ? 'saved in the vault — type to replace' : ''}
								value={d.password}
								onChange={(e) => set('password', e.target.value)}
							/>,
							'srv-pw',
						)}
					{d.auth === 'key' &&
						field(
							'Private key',
							<Textarea
								id="srv-key"
								className="h-20 font-mono text-xs"
								spellCheck={false}
								placeholder={
									server?.hasPrivateKey
										? 'saved in the vault — paste to replace'
										: '-----BEGIN OPENSSH PRIVATE KEY-----'
								}
								value={d.privateKey}
								onChange={(e) => set('privateKey', e.target.value)}
							/>,
							'srv-key',
						)}
					{d.auth === 'keyFile' &&
						field(
							'Key file',
							<Input
								id="srv-keyfile"
								className="font-mono"
								placeholder="~/.ssh/id_ed25519"
								value={d.keyPath}
								onChange={(e) => set('keyPath', e.target.value)}
							/>,
							'srv-keyfile',
						)}
					{(d.auth === 'key' || d.auth === 'keyFile') &&
						field(
							'Passphrase',
							<Input
								id="srv-pass"
								type="password"
								autoComplete="off"
								placeholder={server?.hasPassphrase ? 'saved in the vault' : 'none'}
								value={d.passphrase}
								onChange={(e) => set('passphrase', e.target.value)}
							/>,
							'srv-pass',
						)}
					{d.auth === 'system' && (
						<p className="pl-[9.75rem] text-xs text-muted-foreground">
							Uses your own ssh client and ~/.ssh/config (ProxyJump, Match…). Each tab is a new
							login.
						</p>
					)}
					{d.auth !== 'system' &&
						field(
							'Sessions',
							<div className="flex items-center gap-2 text-sm">
								<Checkbox
									id="srv-persistent"
									data-testid="server-persistent"
									checked={d.persistent}
									onCheckedChange={(v) => set('persistent', v === true)}
								/>
								<Label htmlFor="srv-persistent" className="font-normal">
									Keep running in tmux when the connection drops or the app quits
								</Label>
							</div>,
							'srv-persistent',
						)}
					<div className="my-1 border-border border-t" />
					{field(
						'Environment',
						<Select
							value={d.environment}
							onValueChange={(v) => {
								const env = v as ServerEnvironment;
								// Production starts from "confirm" (F46); the human can still change it.
								setD((p) => ({
									...p,
									environment: env,
									agentPolicy:
										env === 'prod' && p.agentPolicy === 'free' ? 'confirm' : p.agentPolicy,
								}));
							}}
						>
							<SelectTrigger id="srv-env" data-testid="server-environment">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="dev">Development</SelectItem>
								<SelectItem value="staging">Staging</SelectItem>
								<SelectItem value="prod">Production</SelectItem>
							</SelectContent>
						</Select>,
						'srv-env',
					)}
					{field(
						'Agents may',
						<Select
							value={d.agentPolicy}
							onValueChange={(v) => set('agentPolicy', v as AgentPolicy)}
						>
							<SelectTrigger id="srv-policy">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								{(Object.keys(POLICY_LABEL) as AgentPolicy[]).map((p) => (
									<SelectItem key={p} value={p}>
										{POLICY_LABEL[p]}
									</SelectItem>
								))}
							</SelectContent>
						</Select>,
						'srv-policy',
					)}
				</div>
				{save.error && (
					<p className="text-sm text-destructive" role="alert">
						{formatError(save.error)}
					</p>
				)}
				<DialogFooter>
					<Button variant="outline" onClick={() => onOpenChange(false)}>
						Cancel
					</Button>
					<Button
						data-testid="server-save"
						disabled={save.isPending || !d.name.trim() || !d.host.trim() || !d.username.trim()}
						onClick={() => save.mutate()}
					>
						{server ? 'Save' : 'Add server'}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
