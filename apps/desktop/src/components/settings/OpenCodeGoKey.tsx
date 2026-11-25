import type { GoQuotaStatus } from '@factorai/types';
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

const quotaErrors: Partial<Record<GoQuotaStatus, string>> = {
	rejectedKey: 'OpenCode rejected this key. Use a valid key from your OpenCode Go account.',
	missingEntitlement: 'The account for this key lacks access to OpenCode Go usage.',
	unavailable: 'OpenCode Go usage is temporarily unavailable.',
};

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
			const result = await cmd.setOpenCodeGoKey(value);
			queryClient.setQueryData(queryKeys.openCodeGoKey(), result);
			setKey('');
			setConfirmRemoval(false);
		} catch (error) {
			setFailure(formatError(error));
			setConfirmRemoval(false);
		} finally {
			setSaving(false);
		}
	}

	const quotaError = source.data && quotaErrors[source.data.status];
	const disabled = saving || vault.isPending || Boolean(vault.data?.locked);
	return (
		<>
			<SettingRow
				label="OpenCode Go API key"
				htmlFor="settings-opencode-go-key"
				description="Shows remaining Go limits in the status bar details. The key is stored in the encrypted vault."
			>
				<div className="flex w-64 flex-col gap-2">
					<Input
						id="settings-opencode-go-key"
						type="password"
						autoComplete="off"
						placeholder={
							source.data?.source === 'saved' ? 'Enter a replacement key' : 'Enter a key'
						}
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
									: source.data?.source === 'saved'
										? 'Saved in vault'
										: source.data?.source === 'environment'
											? 'Using environment'
											: 'No key saved'}
						</span>
						{source.data?.source === 'saved' && (
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
					{(failure || quotaError) && (
						<p role="alert" className="text-destructive text-xs">
							{failure ?? quotaError}
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
