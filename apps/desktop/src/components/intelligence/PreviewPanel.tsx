import type { AnalysisPreview, SignalPreview } from '@factorai/types';
import { Button, Checkbox, Input, Label } from '@factorai/ui';
import { agentName } from '@lib/agents';
import { formatBytes } from '@lib/format';
import { intelligenceErrorCode } from '@lib/intelligence';
import {
	clampSessionLimit,
	describeIntelligenceError,
	type ErrorNotice,
	providerReason,
	SESSION_LIMIT_DEFAULT,
	SESSION_LIMIT_MAX,
	SIGNAL_LABELS,
} from '@lib/intelligenceView';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Chip } from './chips';
import { Notice } from './Notice';

/**
 * What an analysis would read and send, before anything is (specs/05-features.md
 * F33 § "Preview, consent, run"; the brief's F-01).
 *
 * **Nothing here starts a model.** The preview reads transcripts on this machine
 * and shows every excerpt that would leave it, redacted, so any can be struck.
 * The run is started by the one button at the bottom, and what that button does
 * is named in its label: with consent it sends the selected excerpts to the
 * provider, without it the run is offline and only lists the signals.
 *
 * **The preview is pinned by `previewHash`.** If the sessions changed since, the
 * backend refuses (`preview_stale`) rather than sending something other than
 * what was shown; this panel then re-reads, and clears the consent, because
 * consent was given for a set that no longer exists.
 */
export function PreviewPanel({
	projectId,
	onCancel,
	onStarted,
}: {
	projectId: string;
	onCancel: () => void;
	onStarted: (runId: string) => void;
}) {
	const [sessionLimit, setSessionLimit] = useState(SESSION_LIMIT_DEFAULT);
	const [limitText, setLimitText] = useState(String(SESSION_LIMIT_DEFAULT));
	// Ids are stable across previews (F33), so a strike made now still means the
	// same observation after the preview is read again.
	const [excluded, setExcluded] = useState<ReadonlySet<string>>(() => new Set());
	const [consent, setConsent] = useState(false);
	const [includeInstructions, setIncludeInstructions] = useState(false);
	const [notice, setNotice] = useState<ErrorNotice | null>(null);
	const [refreshed, setRefreshed] = useState(false);

	const preview = useQuery({
		queryKey: queryKeys.intelligencePreview(projectId, sessionLimit),
		queryFn: () => cmd.previewAnalysis(projectId, sessionLimit),
		// A new read happens when a person asks for one - a different limit, or a
		// stale refusal - and never because the window was focused: it would change
		// the set a consent was given for.
		staleTime: Number.POSITIVE_INFINITY,
		gcTime: 0,
		retry: false,
		refetchOnWindowFocus: false,
		refetchOnReconnect: false,
	});
	const data = preview.data;

	// Consent belongs to one set of excerpts. A different hash is a different set.
	const seenHash = useRef<string | null>(null);
	const hash = data?.previewHash ?? null;
	useEffect(() => {
		if (hash === null) return;
		if (seenHash.current !== null && seenHash.current !== hash) setConsent(false);
		seenHash.current = hash;
	}, [hash]);

	const start = useMutation({
		mutationFn: (request: Parameters<typeof cmd.startAnalysis>[0]) => cmd.startAnalysis(request),
		onSuccess: ({ runId }) => onStarted(runId),
		onError: (error) => {
			setNotice(describeIntelligenceError(error));
			if (intelligenceErrorCode(error) === 'preview_stale') {
				setConsent(false);
				setRefreshed(true);
				void preview.refetch();
			}
		},
	});

	const signals = useMemo(() => data?.signals ?? [], [data]);
	const present = useMemo(() => new Set(signals.map((s) => s.evidenceId)), [signals]);
	const struck = useMemo(() => [...excluded].filter((id) => present.has(id)), [excluded, present]);
	const selected = signals.length - struck.length;
	const provider = data?.provider ?? null;
	const canSend = provider?.available === true && selected > 0;
	// Consent only counts where something could be sent.
	const consenting = consent && canSend;

	function applyLimit() {
		const next = clampSessionLimit(limitText);
		setLimitText(String(next));
		if (next !== sessionLimit) {
			setNotice(null);
			setSessionLimit(next);
		}
	}

	function analyze(current: AnalysisPreview) {
		setNotice(null);
		setRefreshed(false);
		start.mutate({
			projectId,
			sessionLimit: current.sessionLimit,
			previewHash: current.previewHash,
			consentRemote: consenting,
			excludedEvidenceIds: struck,
			includeInstructionFile: consenting && includeInstructions && current.instructionFile !== null,
		});
	}

	function toggle(id: string, include: boolean) {
		setExcluded((current) => {
			const next = new Set(current);
			if (include) next.delete(id);
			else next.add(id);
			return next;
		});
	}

	return (
		<section
			data-testid="intelligence-preview"
			aria-labelledby="intelligence-preview-title"
			className="space-y-4"
		>
			<header className="space-y-0.5">
				<h3 id="intelligence-preview-title" className="font-medium text-sm">
					Review before analyzing
				</h3>
				<p className="text-muted-foreground text-xs">
					Nothing has been sent. This is what an analysis would read, and every excerpt that would
					leave this machine if you agree.
				</p>
			</header>

			<div className="flex items-center gap-2">
				<Label htmlFor="intelligence-session-limit" className="text-xs">
					Sessions to read
				</Label>
				<Input
					id="intelligence-session-limit"
					data-testid="intelligence-session-limit"
					type="number"
					min={1}
					max={SESSION_LIMIT_MAX}
					className="w-16"
					value={limitText}
					onChange={(e) => setLimitText(e.target.value)}
					onBlur={applyLimit}
					onKeyDown={(e) => {
						if (e.key === 'Enter') applyLimit();
					}}
				/>
				<span className="text-muted-foreground text-xs">
					newest completed sessions, {SESSION_LIMIT_DEFAULT} by default, {SESSION_LIMIT_MAX} at most
				</span>
			</div>

			{preview.isPending && (
				<p className="text-muted-foreground text-sm" data-testid="intelligence-preview-loading">
					Reading sessions on this machine…
				</p>
			)}

			{preview.isError && (
				<Notice
					notice={describeIntelligenceError(preview.error)}
					onRetry={() => void preview.refetch()}
				/>
			)}

			{refreshed && !notice && (
				<p className="text-muted-foreground text-xs" data-testid="intelligence-preview-refreshed">
					The preview was refreshed. Review it again before analyzing.
				</p>
			)}

			{data && (
				<>
					<Overview data={data} />

					<ProviderBlock data={data} />

					{data.instructionFile && (
						<div className="flex items-start gap-2 rounded-md border border-border p-3">
							<Checkbox
								id="intelligence-include-instructions"
								data-testid="intelligence-include-instructions"
								checked={includeInstructions && consenting}
								disabled={!consenting}
								onCheckedChange={(v) => setIncludeInstructions(v === true)}
							/>
							<div className="space-y-0.5">
								<Label
									htmlFor="intelligence-include-instructions"
									className="font-normal text-sm leading-5"
								>
									Also send {data.instructionFile.path}
								</Label>
								<p className="text-muted-foreground text-xs">
									{formatBytes(data.instructionFile.includedBytes)} of{' '}
									{formatBytes(data.instructionFile.bytes)}
									{data.instructionFile.truncated ? ', cut to the limit' : ''}, redacted. It lets
									the provider avoid suggesting what the file already says. Needs your consent
									below.
								</p>
							</div>
						</div>
					)}

					<SignalList
						signals={signals}
						excluded={excluded}
						selected={selected}
						capped={data.signalsCapped}
						onToggle={toggle}
						onAll={(include) => setExcluded(include ? new Set() : new Set(present))}
					/>

					<div className="space-y-3 rounded-md border border-border p-3">
						{canSend && provider ? (
							<div className="flex items-start gap-2">
								<Checkbox
									id="intelligence-consent"
									data-testid="intelligence-consent"
									checked={consent}
									onCheckedChange={(v) => setConsent(v === true)}
								/>
								<div className="space-y-0.5">
									<Label htmlFor="intelligence-consent" className="font-normal text-sm leading-5">
										Send the {selected} selected excerpt{selected === 1 ? '' : 's'} to{' '}
										{provider.displayName} for this run
									</Label>
									<p className="text-muted-foreground text-xs">
										{provider.remote
											? 'It is processed by the provider’s hosted model, not on this machine. Secrets are masked on a best-effort basis, which is not a guarantee: strike anything sensitive above.'
											: 'It is processed on this machine.'}
									</p>
								</div>
							</div>
						) : (
							<p className="text-muted-foreground text-xs" data-testid="intelligence-no-send">
								{provider && !provider.available
									? `${providerReason(provider)}, so nothing can be sent. The run will only list the signals, offline.`
									: 'There are no excerpts selected to send. The run will only list what it found, offline.'}
							</p>
						)}

						{notice && (
							<Notice
								notice={notice}
								onRetry={() => data && analyze(data)}
								testId="intelligence-start-error"
							/>
						)}

						<div className="flex flex-wrap items-center gap-2">
							<Button
								size="sm"
								data-testid="intelligence-analyze"
								disabled={start.isPending || preview.isFetching || data.sessionCount === 0}
								onClick={() => analyze(data)}
							>
								{start.isPending
									? 'Starting…'
									: consenting
										? 'Analyze'
										: 'Analyze offline (signals only)'}
							</Button>
							<Button
								variant="outline"
								size="sm"
								data-testid="intelligence-preview-cancel"
								onClick={onCancel}
							>
								Cancel
							</Button>
							<span className="text-muted-foreground text-xs" data-testid="intelligence-mode-note">
								{consenting
									? 'The answer is checked on this machine before anything is saved.'
									: 'Nothing is sent: no model reads these. The result is an unreviewed list.'}
							</span>
						</div>
					</div>
				</>
			)}
		</section>
	);
}

/** Counts of what was read and what was left out, and the size of the payload. */
function Overview({ data }: { data: AnalysisPreview }) {
	const skipped = [
		data.excludedRunning > 0 ? `${data.excludedRunning} still running` : null,
		data.excludedSubagents > 0
			? `${data.excludedSubagents} sub-agent transcript${data.excludedSubagents === 1 ? '' : 's'}`
			: null,
		data.unreadable > 0 ? `${data.unreadable} unreadable` : null,
	].filter(Boolean);
	return (
		<dl
			data-testid="intelligence-overview"
			className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 rounded-md border border-border p-3 text-sm"
		>
			<dt className="text-muted-foreground text-xs">Sessions</dt>
			<dd data-testid="intelligence-session-count">
				{data.sessionCount === 0 ? (
					'No completed sessions to analyze yet'
				) : (
					<>
						{data.sessionCount} completed
						{data.byAgent.length > 0 && (
							<span className="text-muted-foreground">
								{' '}
								({data.byAgent.map((a) => `${agentName(a.agent)} ${a.sessions}`).join(' · ')})
							</span>
						)}
					</>
				)}
			</dd>
			{skipped.length > 0 && (
				<>
					<dt className="text-muted-foreground text-xs">Skipped</dt>
					<dd className="text-muted-foreground">{skipped.join(' · ')}</dd>
				</>
			)}
			<dt className="text-muted-foreground text-xs">Excerpts</dt>
			<dd data-testid="intelligence-payload">
				{data.signals.length} found, about {formatBytes(data.estimatedPayloadBytes)}
			</dd>
		</dl>
	);
}

/** Which engine would run, and whether the excerpts leave the machine. */
function ProviderBlock({ data }: { data: AnalysisPreview }) {
	const { provider } = data;
	return (
		<div
			data-testid="intelligence-provider"
			className="flex flex-wrap items-center gap-x-2 gap-y-1 rounded-md border border-border p-3 text-sm"
		>
			<span>
				{provider.displayName}
				{provider.version ? ` ${provider.version}` : ''}
			</span>
			<Chip testId="intelligence-provider-remote">{provider.remote ? 'remote' : 'local'}</Chip>
			<Chip testId="intelligence-provider-available">
				{provider.available ? 'available' : 'unavailable'}
			</Chip>
			<span className="basis-full text-muted-foreground text-xs">
				{provider.available
					? provider.remote
						? 'A local command, but not local inference: what you agree to send is processed by its hosted model.'
						: 'Runs on this machine.'
					: `${providerReason(provider)}.${provider.detail ? ` ${provider.detail}` : ''}`}
			</span>
		</div>
	);
}

function SignalList({
	signals,
	excluded,
	selected,
	capped,
	onToggle,
	onAll,
}: {
	signals: readonly SignalPreview[];
	excluded: ReadonlySet<string>;
	selected: number;
	capped: boolean;
	onToggle: (id: string, include: boolean) => void;
	onAll: (include: boolean) => void;
}) {
	if (signals.length === 0) {
		return (
			<p className="text-muted-foreground text-sm" data-testid="intelligence-no-signals">
				No signals were found in these sessions, so there is nothing to send.
			</p>
		);
	}
	return (
		<section className="space-y-2" aria-label="Excerpts">
			<div className="flex items-center justify-between gap-2">
				<p className="text-sm" data-testid="intelligence-selected">
					{selected} of {signals.length} excerpts selected
				</p>
				<Button
					variant="quiet"
					size="sm"
					data-testid="intelligence-toggle-all"
					onClick={() => onAll(selected < signals.length)}
				>
					{selected < signals.length ? 'Select all' : 'Clear all'}
				</Button>
			</div>
			{capped && (
				<p className="text-muted-foreground text-xs">
					There were more signals than a run takes. These are the ones kept, spread across the
					sessions.
				</p>
			)}
			<ul
				data-testid="intelligence-signals"
				className="max-h-80 divide-y divide-border overflow-y-auto rounded-md border border-border"
			>
				{signals.map((signal) => (
					<SignalRow
						key={signal.evidenceId}
						signal={signal}
						included={!excluded.has(signal.evidenceId)}
						onToggle={onToggle}
					/>
				))}
			</ul>
		</section>
	);
}

function SignalRow({
	signal,
	included,
	onToggle,
}: {
	signal: SignalPreview;
	included: boolean;
	onToggle: (id: string, include: boolean) => void;
}) {
	const id = `signal-${signal.evidenceId}`;
	return (
		<li
			data-testid="intelligence-signal"
			data-included={included}
			className={`flex items-start gap-3 px-3 py-2 ${included ? '' : 'opacity-50'}`}
		>
			<Checkbox
				id={id}
				data-testid="intelligence-signal-include"
				aria-label={`Send this excerpt: ${signal.excerpt.slice(0, 60)}`}
				className="mt-0.5"
				checked={included}
				onCheckedChange={(v) => onToggle(signal.evidenceId, v === true)}
			/>
			<div className="min-w-0 flex-1 space-y-0.5">
				<div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
					<Chip>{SIGNAL_LABELS[signal.signalType]}</Chip>
					<span className="text-muted-foreground">{agentName(signal.sourceAgent)}</span>
					<span className="truncate text-muted-foreground">
						{signal.sessionTitle?.trim() || `session ${signal.sessionId.slice(0, 8)}`}
					</span>
				</div>
				<p className="whitespace-pre-wrap break-words font-mono text-secondary-foreground text-xs">
					{signal.excerpt}
				</p>
				{signal.context && (
					<p className="whitespace-pre-wrap break-words font-mono text-muted-foreground text-xs">
						before it: {signal.context}
					</p>
				)}
			</div>
		</li>
	);
}
