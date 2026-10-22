import type { IntelligenceProviderInfo } from '@factorai/types';
import { SettingRow, Switch } from '@factorai/ui';
import { useIntelligenceStatus } from '@hooks/useIntelligenceStatus';
import { formatError } from '@lib/errors';
import { intelligenceEnabledSetting } from '@lib/intelligence';
import { providerReason } from '@lib/intelligenceView';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

/**
 * Project Intelligence's switch (specs/05-features.md F33, ADR-0084).
 *
 * **Like Profiles, this section writes as you click.** The row is
 * `intelligence.enabled` in SQLite because Rust enforces it, and a draft would
 * put the modal's Save between the person and the two things that happen the
 * moment it flips: the provider is probed so its availability can be shown here,
 * and turning it off cancels runs in flight. A run that is cancelling is not
 * something Cancel on the modal can take back, so the switch does not pretend to
 * be a draft. The note under it says so.
 *
 * Off by default, and what off means is stated before anything else: nothing is
 * read, nothing is run.
 */
export function IntelligenceSection() {
	const queryClient = useQueryClient();
	const status = useIntelligenceStatus();
	const [failure, setFailure] = useState<string | null>(null);

	const toggle = useMutation({
		mutationFn: (on: boolean) =>
			cmd.setSetting('projectIntelligence', intelligenceEnabledSetting(on)),
		onSuccess: async (_done, on) => {
			setFailure(null);
			// Off means no result of a past run is shown or fetched: the commands that
			// would refetch them reject with `disabled`. Only inactive entries - one a
			// mounted view still observes is dropped by that view unmounting, and
			// removing it from under the observer would be a refetch that fails.
			if (!on) {
				for (const queryKey of [
					queryKeys.intelligenceRun(),
					queryKeys.improvements(),
					['intelligence-preview'],
				]) {
					queryClient.removeQueries({ queryKey, type: 'inactive' });
				}
			}
			await queryClient.invalidateQueries({ queryKey: queryKeys.intelligenceStatus() });
		},
		onError: (e) => setFailure(formatError(e)),
	});

	const enabled = status.data?.enabled === true;
	const provider = status.data?.provider ?? null;

	return (
		<div className="space-y-3">
			<div className="divide-y divide-border">
				<SettingRow
					label="Project Intelligence"
					htmlFor="settings-intelligence"
					description="Look back over a project’s recent sessions, on demand, and get at most five suggestions for what to change: an instruction, a test, a script idea, something to look at yourself, or nothing. Off by default. While it is off nothing is read and nothing runs."
				>
					<Switch
						id="settings-intelligence"
						data-testid="settings-intelligence"
						checked={enabled}
						disabled={status.isPending || toggle.isPending}
						onCheckedChange={(on) => toggle.mutate(on)}
					/>
				</SettingRow>

				{enabled && (
					<SettingRow label="Analysis provider" description={<ProviderNote provider={provider} />}>
						<span
							data-testid="settings-intelligence-provider"
							className={`font-medium text-xs ${
								provider?.available ? 'text-secondary-foreground' : 'text-destructive'
							}`}
						>
							{provider === null ? 'checking…' : provider.available ? 'available' : 'unavailable'}
						</span>
					</SettingRow>
				)}
			</div>

			{status.isError && (
				<p data-testid="settings-intelligence-error" className="text-destructive text-xs">
					Could not read the switch: {formatError(status.error)}
				</p>
			)}
			{failure && (
				<p data-testid="settings-intelligence-error" className="text-destructive text-xs">
					Could not change it: {failure}
				</p>
			)}

			<ul
				data-testid="settings-intelligence-privacy"
				className="list-disc space-y-1 pl-4 text-muted-foreground text-xs"
			>
				<li>
					Signals are found on this machine, in the original transcripts, and never in a session
					that is still running or in another project.
				</li>
				<li>
					Sending excerpts to the agent CLI’s hosted model needs your consent for each run. Before
					that you see every excerpt and can strike any of them. Secrets are masked on a best-effort
					basis, which is not a guarantee.
				</li>
				<li>Without consent a run only lists the signals, offline, as unreviewed observations.</li>
				<li>
					An instruction file is changed only by a Save you press. This switch writes when you click
					it, not on the modal’s Save, and turning it off cancels any run in progress.
				</li>
			</ul>
		</div>
	);
}

/** What the provider is, whether it leaves the machine, and why not if it
 *  cannot run. A local CLI is not local inference (ADR-0084), so `remote` is
 *  said in the sentence and not only in a badge. */
function ProviderNote({ provider }: { provider: IntelligenceProviderInfo | null }) {
	if (provider === null) return <>Checking whether the agent’s CLI can run the analysis.</>;
	const name = provider.version
		? `${provider.displayName} ${provider.version}`
		: provider.displayName;
	if (!provider.available) {
		return (
			<>
				{providerReason(provider)}.{provider.detail ? ` ${provider.detail}.` : ''} Runs will list
				signals offline only.
			</>
		);
	}
	return (
		<>
			{name}.{' '}
			{provider.remote
				? 'It is remote: the excerpts you agree to send are processed by its hosted model, not on this machine.'
				: 'It runs on this machine.'}
		</>
	);
}
