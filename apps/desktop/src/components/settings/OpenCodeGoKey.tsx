import {
	Button,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
	Input,
	SettingRow,
} from '@factorai/ui';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { formatError } from '@lib/errors';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { VaultControls } from './VaultControls';

export function OpenCodeGoKey() {
	const queryClient = useQueryClient();
	const source = useQuery({
		queryKey: queryKeys.openCodeGoKey(),
		queryFn: cmd.openCodeGoKeySource,
	});
	const vault = useQuery({ queryKey: queryKeys.vault(), queryFn: cmd.vaultStatus });
	const [key, setKey] = useState('');
	const [saving, setSaving] = useState(false);
	const [confirmRemoval, setConfirmRemoval] = useState(false);
	const [failure, setFailure] = useState<string | null>(null);

	async function save(value: string | null) {
		setSaving(true);
		setFailure(null);
		try {
			const nextSource = await cmd.setOpenCodeGoKey(value);
			queryClient.setQueryData(queryKeys.openCodeGoKey(), nextSource);
			setKey('');
			setConfirmRemoval(false);
		} catch (error) {
			setFailure(formatError(error));
			setConfirmRemoval(false);
		} finally {
			setSaving(false);
		}
	}

	const disabled = saving || vault.isPending || Boolean(vault.data?.locked);
	return (
		<>
			<SettingRow
				label="OpenCode Go API key"
				htmlFor="settings-opencode-go-key"
				description="Reads plan limits for the default OpenCode profile. The key is stored in the encrypted vault."
			>
				<div className="flex w-64 flex-col gap-2">
					<Input
						id="settings-opencode-go-key"
						type="password"
						autoComplete="off"
						placeholder={source.data === 'saved' ? 'Enter a replacement key' : 'Enter a key'}
						value={key}
						maxLength={4096}
						disabled={disabled}
						onChange={(event) => setKey(event.target.value)}
					/>
					<div className="flex items-center justify-end gap-2">
						<span className="mr-auto text-muted-foreground text-xs" role="status">
							{source.isPending
								? 'Checking…'
								: source.isError
									? 'Unavailable'
									: source.data === 'saved'
										? 'Saved in vault'
										: source.data === 'environment'
											? 'Using environment'
											: 'No key saved'}
						</span>
						{source.data === 'saved' && (
							<Button
								size="sm"
								variant="outline"
								disabled={disabled}
								onClick={() => setConfirmRemoval(true)}
							>
								Remove
							</Button>
						)}
						<Button
							size="sm"
							variant="outline"
							disabled={disabled || !key.trim()}
							onClick={() => void save(key)}
						>
							Save key
						</Button>
					</div>
					<VaultControls />
					{failure && (
						<p role="alert" className="text-destructive text-xs">
							{failure}
						</p>
					)}
				</div>
			</SettingRow>
			<Dialog
				open={confirmRemoval}
				onOpenChange={(open) => {
					if (!open && !saving) setConfirmRemoval(false);
				}}
			>
				<DialogContent>
					<DialogHeader>
						<DialogTitle>Remove OpenCode Go API key?</DialogTitle>
						<DialogDescription>
							The key will be removed from Qursor’s encrypted vault.
						</DialogDescription>
					</DialogHeader>
					<DialogFooter>
						<Button variant="outline" disabled={saving} onClick={() => setConfirmRemoval(false)}>
							Cancel
						</Button>
						<Button variant="destructive" disabled={saving} onClick={() => void save(null)}>
							Remove key
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
		</>
	);
}
