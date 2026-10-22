import type { ImprovementCandidate, PreparedChange } from '@factorai/types';
import {
	Button,
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from '@factorai/ui';
import { useFileViewer } from '@hooks/useFileViewer';
import { formatError } from '@lib/errors';
import {
	draftBlock,
	type DraftBlock,
	type Handoff,
	handoffOf,
	manualRevertTarget,
	recordFromDisk,
	saveDirectly,
	type RecordTarget,
} from '@lib/intelligenceSave';
import {
	describeIntelligenceError,
	type ErrorNotice,
	firstChangedLine,
} from '@lib/intelligenceView';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { useDraftStore } from '@store/draftStore';
import { type HandoffPhase, useIntelligenceHandoffStore } from '@store/intelligenceHandoffStore';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Notice } from './Notice';
import { UnifiedDiff } from './UnifiedDiff';

/**
 * Prepare a change and hand it to a Save (specs/05-features.md F33 § "Changing
 * an instruction file"; ADR-0084 decision 6; ADR-0039).
 *
 * Preparing **writes nothing**: the backend returns a diff and the text a Save
 * would write. Everything after that is a click, and there are exactly two ways
 * to Save, both explicit:
 *
 * - **Open as a draft in the editor.** The proposal becomes the unsaved draft of
 *   `targetPath`, the viewer opens on the changed line, and the person presses
 *   the editor's own Save. `useIntelligenceSaveRecorder` hears that write and
 *   reports it.
 * - **Save to the file** from a confirm dialog, which calls the same `write_file`
 *   and reports it itself. This is the way for a file that does not exist yet
 *   (the viewer cannot open one) and for a file the editor already has open.
 *
 * `kind` is `revert` for the same flow run backwards. A revert the backend
 * could not propose a patch for (the file changed since the Save) is shown as
 * what to do by hand and nothing else: **nothing is overwritten, ever.**
 */
export function PrepareChange({
	candidate,
	projectId,
	kind,
	onClose,
}: {
	candidate: ImprovementCandidate;
	projectId: string;
	kind: 'instruction' | 'revert';
	onClose: () => void;
}) {
	const [allowDuplicate, setAllowDuplicate] = useState(false);
	const prepared = useQuery({
		queryKey: queryKeys.intelligencePrepared(candidate.id, kind, allowDuplicate),
		queryFn: () =>
			kind === 'revert'
				? cmd.prepareRevert(candidate.id)
				: cmd.prepareInstructionChange(candidate.id, allowDuplicate),
		// It describes the file as it is now, and a refetch would swap the diff out
		// from under someone reading it. Only the person asking again does that.
		staleTime: Number.POSITIVE_INFINITY,
		gcTime: 0,
		retry: false,
		refetchOnWindowFocus: false,
		refetchOnReconnect: false,
	});

	return (
		<section
			data-testid={`prepare-${kind}`}
			aria-label={kind === 'revert' ? 'Revert' : 'Prepared change'}
			className="space-y-3 rounded-md border border-border bg-background p-3"
		>
			{prepared.isPending && (
				<p className="text-muted-foreground text-sm">Preparing the change. Nothing is written.</p>
			)}

			{prepared.isError && (
				<Notice
					notice={describeIntelligenceError(prepared.error)}
					onRetry={() => void prepared.refetch()}
				>
					<Button variant="quiet" size="sm" onClick={onClose}>
						Close
					</Button>
				</Notice>
			)}

			{prepared.data?.status === 'duplicate' && (
				<Duplicate
					change={prepared.data}
					onProceed={() => setAllowDuplicate(true)}
					onClose={onClose}
				/>
			)}

			{prepared.data?.status === 'conflict' && (
				<Conflict
					change={prepared.data}
					projectId={projectId}
					onClose={onClose}
					onRefresh={() => void prepared.refetch()}
				/>
			)}

			{prepared.data?.status === 'ready' && (
				<Ready
					change={prepared.data}
					projectId={projectId}
					onClose={onClose}
					onPrepareAgain={() => void prepared.refetch()}
				/>
			)}
		</section>
	);
}

/** The file already says it. No patch exists, so there is nothing to save; the
 *  person may still ask for one. */
function Duplicate({
	change,
	onProceed,
	onClose,
}: {
	change: PreparedChange;
	onProceed: () => void;
	onClose: () => void;
}) {
	const where = change.duplicateOf;
	return (
		<div className="space-y-2" data-testid="prepare-duplicate">
			<p className="font-medium text-sm">This looks already covered</p>
			{where && (
				<p className="text-muted-foreground text-xs">
					<span className="font-mono">
						{where.path}:{where.line}
					</span>{' '}
					says: <q>{where.text}</q>
				</p>
			)}
			<p className="text-muted-foreground text-xs">
				No patch was prepared, and nothing was written. You can still ask for one, or close this and
				dismiss the suggestion.
			</p>
			<div className="flex flex-wrap gap-2">
				<Button variant="outline" size="sm" data-testid="prepare-anyway" onClick={onProceed}>
					Prepare anyway
				</Button>
				<Button variant="quiet" size="sm" onClick={onClose}>
					Close
				</Button>
			</div>
		</div>
	);
}

/** A revert the backend will not propose: the file is no longer as the Save left
 *  it. Shown as instructions for a person, never as a write. */
function Conflict({
	change,
	projectId,
	onClose,
	onRefresh,
}: {
	change: PreparedChange;
	projectId: string;
	onClose: () => void;
	onRefresh: () => void;
}) {
	const queryClient = useQueryClient();
	const [checking, setChecking] = useState(false);
	const [notice, setNotice] = useState<ErrorNotice | null>(null);
	const conflict = change.conflict;
	const target: RecordTarget = manualRevertTarget(change, projectId);

	async function check() {
		setChecking(true);
		setNotice(null);
		const outcome = await recordFromDisk(target);
		setChecking(false);
		if (outcome.state === 'recorded') {
			await queryClient.invalidateQueries({ queryKey: queryKeys.improvements(projectId) });
		} else {
			setNotice(describeIntelligenceError(outcome.error));
		}
	}

	return (
		<div className="space-y-2" data-testid="prepare-conflict">
			<p className="font-medium text-sm">
				{change.relativePath} has changed since the instruction was saved
			</p>
			<p className="text-muted-foreground text-xs">
				An automatic revert would overwrite what is there now, so none is offered and nothing was
				written. Remove the line yourself:
			</p>
			{conflict && (
				<>
					<p className="text-muted-foreground text-xs">{conflict.reason}</p>
					<pre
						data-testid="manual-instructions"
						className="whitespace-pre-wrap break-words rounded-md border border-border bg-secondary/30 p-2 font-mono text-secondary-foreground text-xs"
					>
						{conflict.manualInstructions}
					</pre>
				</>
			)}
			{notice && (
				<Notice notice={notice} onRetry={retryFor(notice, () => void check(), onRefresh)} />
			)}
			<div className="flex flex-wrap gap-2">
				<Button
					variant="outline"
					size="sm"
					data-testid="check-removed"
					disabled={checking}
					onClick={() => void check()}
				>
					{checking ? 'Checking…' : 'I removed it - check'}
				</Button>
				<Button variant="outline" size="sm" onClick={onRefresh}>
					Prepare again
				</Button>
				<Button variant="quiet" size="sm" onClick={onClose}>
					Close
				</Button>
			</div>
		</div>
	);
}

/** What a notice's retry control does. The label comes from the notice (`next`),
 *  so the action has to come from it too: a conflict means the diff is stale and
 *  the way out is a fresh one, while a change that is not in the file yet is
 *  answered by looking at the file again. */
function retryFor(notice: ErrorNotice, check: () => void, prepareAgain: () => void): () => void {
	return notice.next === 'prepare-again' ? prepareAgain : check;
}

const DRAFT_BLOCK_TEXT: Record<DraftBlock, (name: string) => string> = {
	'new-file': (name) =>
		`${name} does not exist yet, so the editor cannot open it. Saving from here creates it.`,
	'open-in-viewer': (name) =>
		`${name} is already open in the editor. Close it there to review this as a draft, or save from here.`,
	'has-draft': (name) =>
		`${name} has unsaved edits in the editor. Save or discard them there to review this as a draft, or save from here.`,
};

/** A change with a patch: the diff, and the two ways to Save it. */
function Ready({
	change,
	projectId,
	onClose,
	onPrepareAgain,
}: {
	change: PreparedChange;
	projectId: string;
	onClose: () => void;
	onPrepareAgain: () => void;
}) {
	const handoff = handoffOf(change, projectId);
	const queryClient = useQueryClient();
	const viewer = useFileViewer();
	const entry = useIntelligenceHandoffStore((s) => s.byCandidate[change.candidateId]);
	const existingDraft = useDraftStore((s) => s.drafts[change.targetPath]);
	const [confirming, setConfirming] = useState(false);
	const [saving, setSaving] = useState(false);
	const [writeError, setWriteError] = useState<string | null>(null);
	const [recordNotice, setRecordNotice] = useState<ErrorNotice | null>(null);
	const [checking, setChecking] = useState(false);

	if (!handoff) return null;
	const revert = change.kind === 'revert';
	const name = change.relativePath;
	const block = draftBlock({
		exists: change.exists,
		targetPath: change.targetPath,
		viewerPath: viewer.path,
		existingDraft,
	});
	const line = change.diff ? firstChangedLine(change.diff) : null;

	function openAsDraft(h: Handoff) {
		// Seeded before the open: the editor reads its draft once, when the file
		// loads, so it has to be there when the view mounts.
		useDraftStore.getState().setDraft(h.targetPath, h.proposedContent);
		useIntelligenceHandoffStore.getState().begin(h);
		viewer.open(h.targetPath, { line: line ?? undefined });
	}

	async function afterRecorded() {
		await Promise.all([
			queryClient.invalidateQueries({ queryKey: queryKeys.improvements(projectId) }),
			// A viewer showing this file re-reads what was just written.
			queryClient.invalidateQueries({ queryKey: ['file', change.targetPath] }),
		]);
	}

	async function confirmSave(h: Handoff) {
		setSaving(true);
		setWriteError(null);
		setRecordNotice(null);
		const outcome = await saveDirectly(h);
		setSaving(false);
		if (outcome.state === 'write-failed') {
			// The file is as it was. The dialog stays so it can be tried again.
			setWriteError(formatError(outcome.error));
			return;
		}
		setConfirming(false);
		if (outcome.state === 'record-failed') {
			// Written, and the backend would not believe it. Said as exactly that.
			setRecordNotice(describeIntelligenceError(outcome.error));
			void queryClient.invalidateQueries({ queryKey: ['file', change.targetPath] });
			return;
		}
		await afterRecorded();
	}

	async function checkNow(h: Handoff) {
		setChecking(true);
		const outcome = await recordFromDisk(h);
		setChecking(false);
		const store = useIntelligenceHandoffStore.getState();
		if (outcome.state === 'recorded') {
			store.forget(h.candidateId);
			setRecordNotice(null);
			await afterRecorded();
		} else {
			const notice = describeIntelligenceError(outcome.error);
			if (store.byCandidate[h.candidateId])
				store.setPhase(h.candidateId, { state: 'failed', notice });
			else setRecordNotice(notice);
		}
	}

	function dropHandoff(h: Handoff) {
		// The draft goes only if it is still exactly what was seeded: anything the
		// person typed into it is theirs.
		const draft = useDraftStore.getState().drafts[h.targetPath];
		if (draft === h.proposedContent) useDraftStore.getState().clearDraft(h.targetPath);
		useIntelligenceHandoffStore.getState().forget(h.candidateId);
	}

	function stopWaiting(h: Handoff) {
		dropHandoff(h);
		onClose();
	}

	/** A fresh diff against the file as it is now. What was handed to the editor is
	 *  let go first: it was prepared against a file that has since moved. */
	function prepareAgain() {
		setRecordNotice(null);
		if (entry) dropHandoff(entry.handoff);
		onPrepareAgain();
	}

	return (
		<div className="space-y-3">
			<div className="space-y-0.5">
				<p className="font-medium text-sm">
					{revert ? `Review the revert of ${name}` : `Review the change to ${name}`}
				</p>
				<p
					className="break-all font-mono text-muted-foreground text-xs"
					data-testid="prepare-target"
				>
					{change.targetPath}
				</p>
				{change.createdFile && (
					<p className="text-muted-foreground text-xs">
						This file does not exist yet. Saving creates it.
					</p>
				)}
				{change.note && <p className="text-muted-foreground text-xs">{change.note}</p>}
			</div>

			{change.diff ? <UnifiedDiff diff={change.diff} /> : null}

			<p className="text-muted-foreground text-xs" data-testid="prepare-nothing-written">
				Nothing has been written. The file changes only when you save it.
			</p>

			{recordNotice && (
				<Notice
					notice={recordNotice}
					onRetry={retryFor(recordNotice, () => void checkNow(handoff), prepareAgain)}
				>
					<span className="text-muted-foreground text-xs">
						The file was written; only the record is missing.
					</span>
				</Notice>
			)}

			{entry ? (
				<Waiting
					name={name}
					phase={entry.phase}
					checking={checking}
					onShow={() => viewer.open(entry.handoff.targetPath, { line: line ?? undefined })}
					onCheck={() => void checkNow(entry.handoff)}
					onPrepareAgain={prepareAgain}
					onStop={() => stopWaiting(entry.handoff)}
				/>
			) : (
				<div className="space-y-2">
					<div className="flex flex-wrap gap-2">
						{block === null && (
							<Button size="sm" data-testid="open-as-draft" onClick={() => openAsDraft(handoff)}>
								Open as draft in editor
							</Button>
						)}
						<Button
							variant={block === null ? 'outline' : 'default'}
							size="sm"
							data-testid="save-directly"
							onClick={() => {
								setWriteError(null);
								setConfirming(true);
							}}
						>
							Save to {name}…
						</Button>
						<Button variant="quiet" size="sm" onClick={onClose}>
							Cancel
						</Button>
					</div>
					{block !== null && (
						<p className="text-muted-foreground text-xs" data-testid="draft-blocked">
							{DRAFT_BLOCK_TEXT[block](name)}
						</p>
					)}
				</div>
			)}

			<Dialog
				open={confirming}
				onOpenChange={(next) => {
					if (!next && !saving) setConfirming(false);
				}}
			>
				<DialogContent className="sm:max-w-md" data-testid="intelligence-save-dialog">
					<DialogHeader>
						<DialogTitle>{revert ? `Save the revert to ${name}?` : `Save to ${name}?`}</DialogTitle>
						<DialogDescription>
							This writes the file now, with the same Save the editor uses. It adds or removes only
							what the diff showed.
						</DialogDescription>
					</DialogHeader>
					<p className="break-all font-mono text-muted-foreground text-xs">{change.targetPath}</p>
					{writeError && (
						<p
							role="alert"
							data-testid="intelligence-save-error"
							className="text-destructive text-sm"
						>
							Could not write the file: {writeError}. Nothing was changed.
						</p>
					)}
					<DialogFooter>
						<Button variant="outline" disabled={saving} onClick={() => setConfirming(false)}>
							Cancel
						</Button>
						<Button
							data-testid="intelligence-confirm-save"
							disabled={saving}
							onClick={() => void confirmSave(handoff)}
						>
							{saving ? 'Saving…' : `Save to ${name}`}
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
		</div>
	);
}

/** The draft is in the editor and the person has not saved it yet. */
function Waiting({
	name,
	phase,
	checking,
	onShow,
	onCheck,
	onPrepareAgain,
	onStop,
}: {
	name: string;
	phase: HandoffPhase;
	checking: boolean;
	onShow: () => void;
	onCheck: () => void;
	onPrepareAgain: () => void;
	onStop: () => void;
}) {
	return (
		<div className="space-y-2" data-testid="prepare-waiting">
			{phase.state === 'failed' ? (
				<Notice notice={phase.notice} onRetry={retryFor(phase.notice, onCheck, onPrepareAgain)} />
			) : (
				<p className="text-sm">
					{phase.state === 'recording'
						? 'Recording the Save…'
						: `${name} is open in the editor with the change as an unsaved draft. Review it and press Save there. It is recorded once the file is saved.`}
				</p>
			)}
			<div className="flex flex-wrap gap-2">
				<Button variant="outline" size="sm" onClick={onShow}>
					Show in editor
				</Button>
				<Button
					variant="outline"
					size="sm"
					data-testid="check-saved"
					disabled={checking || phase.state === 'recording'}
					onClick={onCheck}
				>
					{checking ? 'Checking…' : 'I saved it - check now'}
				</Button>
				<Button variant="quiet" size="sm" onClick={onStop}>
					Stop waiting
				</Button>
			</div>
		</div>
	);
}
