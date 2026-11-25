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
	DropdownMenuTrigger,
	IconButton,
	Input,
} from '@factorai/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Lock, LockOpen, MoreHorizontal } from 'lucide-react';
import { useState } from 'react';
import { formatError } from '@lib/errors';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';

/** Shared vault status and master password controls. */
export function VaultControls() {
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
								? 'Your saved credentials are encrypted with your master password.'
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
