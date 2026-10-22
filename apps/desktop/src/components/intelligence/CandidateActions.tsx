import type { ImprovementCandidate, VerificationOutcome } from '@factorai/types';
import {
	Button,
	Input,
	Label,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
	Textarea,
} from '@factorai/ui';
import {
	canFollowUp,
	describeIntelligenceError,
	type ErrorNotice,
	verificationOf,
} from '@lib/intelligenceView';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { type ReactNode, useState } from 'react';
import { Heading } from './chips';
import { Notice } from './Notice';
import { PrepareChange } from './PrepareChange';

type Panel = 'prepare' | 'revert' | 'dismiss' | 'retain';

const OUTCOMES: readonly { value: VerificationOutcome; label: string }[] = [
	{ value: 'passed', label: 'Passed' },
	{ value: 'failed', label: 'Failed' },
	{ value: 'inconclusive', label: 'Inconclusive' },
];

/**
 * What a person can do with one suggestion, by where it is in its life
 * (specs/05-features.md F33 § Status).
 *
 * `proposed`: prepare a change (an instruction only), flag it, or dismiss it.
 * `implemented` and `retained`: document a check, keep it, or revert it. The two
 * terminal states offer nothing. Every action here is one a person asked for; a
 * button that changes anything irreversible - dismissing - asks first, and the
 * only buttons that can lead to a file being written are inside `PrepareChange`
 * and each of those is a Save.
 */
export function CandidateActions({
	candidate,
	projectId,
}: {
	candidate: ImprovementCandidate;
	projectId: string;
}) {
	const queryClient = useQueryClient();
	const [panel, setPanel] = useState<Panel | null>(null);
	const [notice, setNotice] = useState<ErrorNotice | null>(null);
	const invalidate = () =>
		queryClient.invalidateQueries({ queryKey: queryKeys.improvements(projectId) });
	const fail = (error: unknown) => {
		setNotice(describeIntelligenceError(error));
		// The list may be the thing that is out of date.
		void invalidate();
	};
	const toggle = (next: Panel) => {
		setNotice(null);
		setPanel((current) => (current === next ? null : next));
	};

	const followUp = useMutation({
		mutationFn: () => cmd.markFollowUp(candidate.id, !candidate.followUp),
		onSuccess: () => {
			setNotice(null);
			return invalidate();
		},
		onError: fail,
	});

	const { status } = candidate;
	const flaggable = canFollowUp(status);
	const followUpButton = flaggable && (
		<Button
			variant="outline"
			size="sm"
			data-testid="follow-up"
			aria-pressed={candidate.followUp}
			disabled={followUp.isPending}
			onClick={() => followUp.mutate()}
		>
			{candidate.followUp ? 'Remove follow-up flag' : 'Mark for follow-up'}
		</Button>
	);

	return (
		<div className="space-y-3" data-testid="candidate-actions">
			{notice && (
				<Notice notice={notice}>
					<Button variant="quiet" size="sm" onClick={() => setNotice(null)}>
						Close
					</Button>
				</Notice>
			)}

			{status === 'dismissed' && (
				<p className="text-muted-foreground text-xs">
					Dismissed. It is closed for good: a later analysis will not reopen it.
				</p>
			)}
			{status === 'reverted' && (
				<p className="text-muted-foreground text-xs">
					Reverted. The instruction is out of the file, and this suggestion is closed.
				</p>
			)}

			{status === 'proposed' && (
				<>
					{candidate.actionKind !== 'instruction' && (
						<p className="text-muted-foreground text-xs" data-testid="consult-only">
							This is a suggestion to consult. Nothing is run or written for it; flag it to come
							back to it.
						</p>
					)}
					<div className="flex flex-wrap gap-2">
						{candidate.actionKind === 'instruction' && (
							<Button
								size="sm"
								data-testid="prepare-change"
								aria-expanded={panel === 'prepare'}
								onClick={() => toggle('prepare')}
							>
								Prepare change
							</Button>
						)}
						{followUpButton}
						<Button
							variant="quiet"
							size="sm"
							data-testid="dismiss"
							aria-expanded={panel === 'dismiss'}
							onClick={() => toggle('dismiss')}
						>
							Dismiss…
						</Button>
					</div>
					{panel === 'dismiss' && (
						<ReasonPrompt
							testId="dismiss-confirm"
							prompt="Dismiss this suggestion? It is closed for good: a later analysis will not reopen it."
							confirm="Dismiss"
							destructive
							onCancel={() => setPanel(null)}
							onConfirm={async (reason) => {
								try {
									await cmd.dismissImprovement(candidate.id, reason || undefined);
									setPanel(null);
									await invalidate();
								} catch (e) {
									fail(e);
								}
							}}
						/>
					)}
					{panel === 'prepare' && (
						<PrepareChange
							candidate={candidate}
							projectId={projectId}
							kind="instruction"
							onClose={() => setPanel(null)}
						/>
					)}
				</>
			)}

			{(status === 'implemented' || status === 'retained') && (
				<Implemented
					candidate={candidate}
					projectId={projectId}
					panel={panel}
					toggle={toggle}
					close={() => setPanel(null)}
					fail={fail}
					invalidate={invalidate}
					followUpButton={followUpButton}
				/>
			)}
		</div>
	);
}

/** A one-line prompt with an optional reason, for the decisions that take one. */
function ReasonPrompt({
	testId,
	prompt,
	confirm,
	destructive = false,
	onCancel,
	onConfirm,
}: {
	testId: string;
	prompt: string;
	confirm: string;
	destructive?: boolean;
	onCancel: () => void;
	onConfirm: (reason: string) => Promise<void> | void;
}) {
	const [reason, setReason] = useState('');
	const [working, setWorking] = useState(false);
	return (
		<div
			role="group"
			aria-label={prompt}
			data-testid={testId}
			className="space-y-2 rounded-md border border-border bg-background p-3"
		>
			<p className="text-sm">{prompt}</p>
			<Input
				aria-label="Reason (optional)"
				placeholder="Reason (optional)"
				maxLength={500}
				value={reason}
				onChange={(e) => setReason(e.target.value)}
			/>
			<div className="flex gap-2">
				<Button
					variant={destructive ? 'destructive' : 'default'}
					size="sm"
					data-testid={`${testId}-ok`}
					disabled={working}
					onClick={async () => {
						setWorking(true);
						try {
							await onConfirm(reason.trim());
						} finally {
							setWorking(false);
						}
					}}
				>
					{confirm}
				</Button>
				<Button variant="outline" size="sm" onClick={onCancel}>
					Cancel
				</Button>
			</div>
		</div>
	);
}

/** A change that has been saved: document a check, keep it, or take it back. */
function Implemented({
	candidate,
	projectId,
	panel,
	toggle,
	close,
	fail,
	invalidate,
	followUpButton,
}: {
	candidate: ImprovementCandidate;
	projectId: string;
	panel: Panel | null;
	toggle: (next: Panel) => void;
	close: () => void;
	fail: (error: unknown) => void;
	invalidate: () => Promise<unknown>;
	followUpButton: ReactNode;
}) {
	const verification = verificationOf(candidate);
	const where = candidate.implementation?.relativePath;
	return (
		<div className="space-y-3">
			<p className="text-sm" data-testid="implemented-note">
				{where ? `Saved to ${where}. ` : 'Saved. '}
				{candidate.status === 'retained'
					? 'You chose to keep it. That is your decision, not evidence that it works. '
					: ''}
				{verification.kind === 'unverified'
					? 'Implemented is not the same as effective, and nobody has checked yet.'
					: verification.outcome
						? `You recorded a check: ${verification.outcome}. That is your word, not a measurement.`
						: 'You recorded a check.'}
			</p>

			<VerificationForm candidateId={candidate.id} fail={fail} invalidate={invalidate} />

			<div className="flex flex-wrap gap-2">
				{candidate.status === 'implemented' && (
					<Button
						variant="outline"
						size="sm"
						data-testid="retain"
						aria-expanded={panel === 'retain'}
						onClick={() => toggle('retain')}
					>
						Retain…
					</Button>
				)}
				<Button
					variant="outline"
					size="sm"
					data-testid="revert"
					aria-expanded={panel === 'revert'}
					onClick={() => toggle('revert')}
				>
					Revert…
				</Button>
				{followUpButton}
			</div>

			{panel === 'retain' && (
				<ReasonPrompt
					testId="retain-confirm"
					prompt="Keep this instruction? You can still revert it later."
					confirm="Retain"
					onCancel={close}
					onConfirm={async (reason) => {
						try {
							await cmd.markRetained(candidate.id, reason || undefined);
							close();
							await invalidate();
						} catch (e) {
							fail(e);
						}
					}}
				/>
			)}
			{panel === 'revert' && (
				<PrepareChange candidate={candidate} projectId={projectId} kind="revert" onClose={close} />
			)}
		</div>
	);
}

/**
 * Document a check by hand (specs/05-features.md F33 step 4).
 *
 * **The outcome starts empty, on purpose.** A form that opens on `passed` is a
 * form that is easy to submit without having checked anything; nothing in the
 * product ever writes `passed` itself, and this is the only place a person can.
 */
function VerificationForm({
	candidateId,
	fail,
	invalidate,
}: {
	candidateId: string;
	fail: (error: unknown) => void;
	invalidate: () => Promise<unknown>;
}) {
	const [method, setMethod] = useState('');
	const [outcome, setOutcome] = useState<VerificationOutcome | ''>('');
	const [note, setNote] = useState('');
	const [saving, setSaving] = useState(false);
	const ready = method.trim() !== '' && outcome !== '';

	return (
		<form
			className="space-y-2 rounded-md border border-border bg-background p-3"
			data-testid="verification-form"
			onSubmit={async (event) => {
				event.preventDefault();
				if (outcome === '' || method.trim() === '') return;
				setSaving(true);
				try {
					await cmd.recordVerification({
						candidateId,
						method: method.trim(),
						outcome,
						note: note.trim() || null,
					});
					setMethod('');
					setOutcome('');
					setNote('');
					await invalidate();
				} catch (e) {
					fail(e);
				} finally {
					setSaving(false);
				}
			}}
		>
			<Heading>Document a check</Heading>
			<p className="text-muted-foreground text-xs">
				Say how you checked, such as the test you ran or the session you watched. The app never
				records a result for you.
			</p>
			<div className="space-y-1">
				<Label htmlFor={`verify-method-${candidateId}`} className="text-xs">
					How you checked
				</Label>
				<Input
					id={`verify-method-${candidateId}`}
					data-testid="verify-method"
					maxLength={200}
					value={method}
					onChange={(e) => setMethod(e.target.value)}
				/>
			</div>
			<div className="space-y-1">
				<Label htmlFor={`verify-outcome-${candidateId}`} className="text-xs">
					Result
				</Label>
				<Select value={outcome} onValueChange={(v) => setOutcome(v as VerificationOutcome)}>
					<SelectTrigger
						id={`verify-outcome-${candidateId}`}
						data-testid="verify-outcome"
						className="w-40"
					>
						<SelectValue placeholder="Choose" />
					</SelectTrigger>
					<SelectContent>
						{OUTCOMES.map(({ value, label }) => (
							<SelectItem key={value} value={value} data-testid={`verify-outcome-${value}`}>
								{label}
							</SelectItem>
						))}
					</SelectContent>
				</Select>
			</div>
			<div className="space-y-1">
				<Label htmlFor={`verify-note-${candidateId}`} className="text-xs">
					Note (optional)
				</Label>
				<Textarea
					id={`verify-note-${candidateId}`}
					data-testid="verify-note"
					rows={2}
					maxLength={1000}
					value={note}
					onChange={(e) => setNote(e.target.value)}
				/>
			</div>
			<Button type="submit" size="sm" data-testid="verify-submit" disabled={!ready || saving}>
				{saving ? 'Recording…' : 'Record verification'}
			</Button>
		</form>
	);
}
