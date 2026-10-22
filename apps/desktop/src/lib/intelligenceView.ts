import type {
	ActionKind,
	CandidateStatus,
	ImprovementCandidate,
	ImprovementEvent,
	IntelligenceProgressEvent,
	IntelligenceProviderInfo,
	IntelligenceRunErrorCode,
	ProblemKind,
	SignalType,
	VerificationOutcome,
} from '@factorai/types';
import { formatError } from '@lib/errors';
import { intelligenceErrorCode, intelligenceErrorDetail } from '@lib/intelligence';

/**
 * What the Intelligence tab says (specs/05-features.md F33).
 *
 * Pure on purpose, like `settingsDraft`: the screens are components, but the
 * words they use, the order they put things in and the question "is this change
 * implemented, and has anybody checked it" are all answers worth pinning in
 * vitest rather than discovering in a browser. Nothing here touches the bridge.
 *
 * **Nothing here scores anything.** The honest quantities are the number of
 * independent sessions and whether that makes a candidate a hypothesis (ADR-0084,
 * "A confidence score on candidates" was rejected), so there is no percentage,
 * rank or star to format.
 */

// ── Session limit ───────────────────────────────────────────────────────────

export const SESSION_LIMIT_DEFAULT = 20;
export const SESSION_LIMIT_MAX = 50;

/** A session limit as the backend clamps it, 1 to 50; anything that is not a
 *  number is the default rather than `NaN`. */
export function clampSessionLimit(value: unknown): number {
	const n = typeof value === 'number' ? value : Number.parseInt(String(value), 10);
	if (!Number.isFinite(n)) return SESSION_LIMIT_DEFAULT;
	return Math.min(SESSION_LIMIT_MAX, Math.max(1, Math.trunc(n)));
}

// ── Words ───────────────────────────────────────────────────────────────────

/** The order the actions are grouped in: the remedy the decision hierarchy
 *  prefers first (ADR-0084: no change, then a test, then an instruction...) is
 *  not what a person reads first, which is the one they can act on here. */
export const ACTION_ORDER: readonly ActionKind[] = [
	'instruction',
	'test_suggestion',
	'script_suggestion',
	'manual_followup',
];

export const ACTION_LABELS: Record<ActionKind, { label: string; hint: string }> = {
	instruction: {
		label: 'Instruction',
		hint: 'A line for the instruction file. You review the diff and press Save yourself.',
	},
	test_suggestion: {
		label: 'Test suggestion',
		hint: 'A check that would catch this. For you to consider: nothing is written or run.',
	},
	script_suggestion: {
		label: 'Script suggestion',
		hint: 'A script that could do this. For you to consider: nothing is written or run.',
	},
	manual_followup: {
		label: 'Manual follow-up',
		hint: 'Something to look at yourself. Nothing is written or run.',
	},
};

export const PROBLEM_LABELS: Record<ProblemKind, string> = {
	project_convention: 'Project convention',
	recurring_correction: 'Recurring correction',
	verification_gap: 'Verification gap',
	workflow_hint: 'Workflow hint',
};

export const SIGNAL_LABELS: Record<SignalType, string> = {
	user_correction: 'You corrected the agent',
	tool_failure: 'A check failed',
	retry_after_failure: 'A check was run again',
	repeated_instruction: 'You said it twice',
};

/** How many sessions stand behind a candidate. A single one is a hypothesis and
 *  is called one: the backend computes both, the UI only words them. */
export function sessionsLabel(
	candidate: Pick<ImprovementCandidate, 'independentSessions' | 'hypothesis'>,
): string {
	const n = candidate.independentSessions;
	const sessions = `${n} independent session${n === 1 ? '' : 's'}`;
	return candidate.hypothesis ? `hypothesis · ${sessions}` : sessions;
}

// ── Verification ────────────────────────────────────────────────────────────

/** What a person recorded about whether a change helped, or that they have not.
 *  The status never moves on a verification (ADR-0084), so this is read off the
 *  timeline. */
export type VerificationState =
	| { kind: 'unverified' }
	| {
			kind: 'recorded';
			outcome: VerificationOutcome | null;
			method: string | null;
			at: number;
	  };

function isOutcome(value: string | null): value is VerificationOutcome {
	return value === 'passed' || value === 'failed' || value === 'inconclusive';
}

/** The latest verification event, or `unverified`. Events are oldest first by an
 *  increasing id, so the last one is the person's latest word. */
export function verificationOf(candidate: Pick<ImprovementCandidate, 'events'>): VerificationState {
	for (let i = candidate.events.length - 1; i >= 0; i--) {
		const event = candidate.events[i];
		if (event?.eventType !== 'verification') continue;
		return {
			kind: 'recorded',
			outcome: isOutcome(event.outcome) ? event.outcome : null,
			method: event.method,
			at: event.createdAt,
		};
	}
	return { kind: 'unverified' };
}

/**
 * The status chip's text.
 *
 * `implemented` and `retained` carry the verification beside them because
 * implemented is not effective and retained is not evidence that it works
 * (F33): the chip says `unverified` until a person has recorded a check, and
 * then says what **they** recorded - `check passed`, not `works`.
 */
export function statusBadge(candidate: Pick<ImprovementCandidate, 'status' | 'events'>): string {
	const { status } = candidate;
	if (status !== 'implemented' && status !== 'retained') return status;
	const verification = verificationOf(candidate);
	if (verification.kind === 'unverified') return `${status} · unverified`;
	return verification.outcome ? `${status} · check ${verification.outcome}` : `${status} · checked`;
}

// ── Grouping ────────────────────────────────────────────────────────────────

export interface ActionGroup {
	action: ActionKind;
	candidates: ImprovementCandidate[];
}

export interface CandidateSections {
	/** `proposed`, grouped by what kind of remedy they are. */
	review: ActionGroup[];
	/** `implemented` and `retained`: done, and waiting for someone to check. */
	inUse: ImprovementCandidate[];
	/** `dismissed` and `reverted`: terminal. */
	closed: ImprovementCandidate[];
}

const IN_USE: readonly CandidateStatus[] = ['implemented', 'retained'];

/** The list as the tab shows it. The backend already returns `proposed` first,
 *  and that order is kept inside each section rather than re-sorted here. */
export function sectionCandidates(candidates: readonly ImprovementCandidate[]): CandidateSections {
	const review: ActionGroup[] = ACTION_ORDER.map((action) => ({
		action,
		candidates: candidates.filter((c) => c.status === 'proposed' && c.actionKind === action),
	})).filter((group) => group.candidates.length > 0);
	return {
		review,
		inUse: candidates.filter((c) => IN_USE.includes(c.status)),
		closed: candidates.filter((c) => c.status === 'dismissed' || c.status === 'reverted'),
	};
}

/** Whether a candidate can still be flagged for follow-up. The backend refuses
 *  `dismissed` and `reverted`. */
export function canFollowUp(status: CandidateStatus): boolean {
	return status !== 'dismissed' && status !== 'reverted';
}

// ── Timeline ────────────────────────────────────────────────────────────────

/** First eight characters of a sha256, enough to tell two apart in a timeline. */
export function shortHash(hash: string | null): string | null {
	return hash ? hash.slice(0, 8) : null;
}

export interface EventLine {
	label: string;
	/** Smaller second line, or null. */
	detail: string | null;
}

/** One timeline row in words. A verification says it was typed by a person
 *  because nothing in the product ever writes one. */
export function describeEvent(event: ImprovementEvent): EventLine {
	const note = event.note?.trim() ? event.note.trim() : null;
	switch (event.eventType) {
		case 'proposed':
			return { label: 'Proposed', detail: null };
		case 'evidence_added':
			return { label: 'More evidence attached by a later analysis', detail: null };
		case 'dismissed':
			return { label: 'Dismissed', detail: note };
		case 'follow_up':
			return {
				label: event.outcome === 'off' ? 'Follow-up flag removed' : 'Marked for follow-up',
				detail: note,
			};
		case 'implemented': {
			const target = event.artifactPath ?? 'the instruction file';
			const hashes = [shortHash(event.beforeHash), shortHash(event.afterHash)];
			return {
				label: `Saved to ${target}`,
				detail: hashes[1] ? `file ${hashes[0] ?? 'new'} to ${hashes[1]}` : null,
			};
		}
		case 'verification': {
			const outcome = isOutcome(event.outcome) ? event.outcome : 'recorded';
			const how = event.method ? `Checked by ${event.method}` : 'Checked';
			return {
				label: `${how}: ${outcome}`,
				detail: ['recorded by you', note].filter(Boolean).join(' · '),
			};
		}
		case 'retained':
			return { label: 'Kept', detail: note };
		case 'reverted':
			return { label: 'Reverted', detail: null };
	}
}

// ── Progress ────────────────────────────────────────────────────────────────

export function phaseLabel(phase: IntelligenceProgressEvent['phase']): string {
	switch (phase) {
		case 'analyzing':
			return 'Analyzing sessions';
		case 'validating':
			return 'Checking the answer';
		case 'saving':
			return 'Saving the result';
	}
}

/** Sessions done as 0..1, or null when the total is not known yet - an
 *  indeterminate bar is more honest than a full or empty one. */
export function progressFraction(
	progress: Pick<IntelligenceProgressEvent, 'sessionsDone' | 'sessionsTotal'> | null,
): number | null {
	if (!progress || progress.sessionsTotal <= 0) return null;
	return Math.min(1, Math.max(0, progress.sessionsDone / progress.sessionsTotal));
}

// ── Provider ────────────────────────────────────────────────────────────────

/** Why the provider cannot run, in a sentence. */
export function providerReason(provider: IntelligenceProviderInfo): string {
	switch (provider.reason) {
		case 'not_installed':
			return `${provider.displayName} is not installed, or is not on the PATH the app sees`;
		case 'missing_flag':
			return `this version of ${provider.displayName} lacks a flag the isolated analysis depends on`;
		case 'probe_failed':
			return `${provider.displayName} did not answer its own help`;
		default:
			return `${provider.displayName} cannot be used`;
	}
}

// ── Errors ──────────────────────────────────────────────────────────────────

/** What the person can do about an error, as the control that does it. */
export type ErrorNext =
	/** Same action again. */
	| 'retry'
	/** Review the preview again before starting. */
	| 'preview-again'
	/** Prepare the change again. */
	| 'prepare-again'
	/** Look at the file, then check again. */
	| 'check-again'
	/** The switch is the fix. */
	| 'settings'
	| 'none';

export interface ErrorNotice {
	title: string;
	/** What happened to the person's data, or what to do. */
	hint: string;
	/** The backend's own sentence, when it adds something the title does not. */
	detail: string | null;
	next: ErrorNext;
}

/** A rejected command as a notice. The code decides the words; the backend's
 *  sentence is kept as `detail` because it names the file or the count. */
export function describeIntelligenceError(error: unknown): ErrorNotice {
	const code = intelligenceErrorCode(error);
	// Another kind of error, or a code this build does not know, has no sentence
	// of its own to split: say what it is.
	const detail = (code === null ? formatError(error) : intelligenceErrorDetail(error)) || null;
	switch (code) {
		case 'disabled':
			return {
				title: 'Project Intelligence is off',
				hint: 'Turn it on in Settings, under Intelligence. Nothing was read or run.',
				detail: null,
				next: 'settings',
			};
		case 'project_not_found':
			return {
				title: 'This project is no longer in the workspace',
				hint: 'Nothing was read or changed.',
				detail,
				next: 'none',
			};
		case 'project_missing':
			return {
				title: 'The project folder is not on disk',
				hint: 'Preparing a change needs the folder. Put it back, then try again.',
				detail,
				next: 'retry',
			};
		case 'candidate_not_found':
			return {
				title: 'That suggestion no longer exists',
				hint: 'The list is out of date. It was refreshed.',
				detail,
				next: 'none',
			};
		case 'run_not_found':
			return {
				title: 'That analysis is no longer stored',
				hint: 'Start a new one from the preview.',
				detail,
				next: 'none',
			};
		case 'run_in_progress':
			return {
				title: 'An analysis of this project is already running',
				hint: 'Wait for it to finish, or cancel it.',
				detail: null,
				next: 'none',
			};
		case 'no_data':
			return {
				title: 'There are no completed sessions to analyze',
				hint: 'Finish a session in this project, then try again. Running sessions are skipped.',
				detail: null,
				next: 'retry',
			};
		case 'preview_stale':
			return {
				title: 'The sessions changed after the preview',
				hint: 'Nothing was sent. The preview was refreshed: review it again before analyzing.',
				detail: null,
				next: 'preview-again',
			};
		case 'invalid_state':
			return {
				title: 'That is no longer possible for this suggestion',
				hint: 'Its status changed. The list was refreshed.',
				detail,
				next: 'none',
			};
		case 'not_instruction':
			return {
				title: 'Only an instruction can become a file change',
				hint: 'Tests and scripts are suggestions to consult. Nothing runs and nothing is written.',
				detail: null,
				next: 'none',
			};
		case 'not_prepared':
			return {
				title: 'The change has not been prepared',
				hint: 'Prepare it first, then save.',
				detail: null,
				next: 'prepare-again',
			};
		case 'file_conflict':
			return {
				title: 'The file is not what was saved',
				hint: 'It changed since. Nothing was recorded. Prepare the change again to get a fresh diff.',
				detail,
				next: 'prepare-again',
			};
		case 'not_saved':
			return {
				title: 'The change is not in the file',
				hint: 'Nothing was recorded. Save it in the editor, then check again.',
				detail,
				next: 'check-again',
			};
		case 'unsafe_target':
			return {
				title: 'The instruction file cannot be changed safely',
				hint: 'It is outside the project, a broken link, a folder, or not text. Nothing was written.',
				detail,
				next: 'none',
			};
		case 'target_too_large':
			return {
				title: 'The instruction file is too large to edit here',
				hint: 'Edit it by hand. Nothing was written.',
				detail,
				next: 'none',
			};
		case null:
			return {
				title: 'Something went wrong',
				hint: 'Nothing was written to your project.',
				detail,
				next: 'retry',
			};
	}
}

export interface RunNotice {
	title: string;
	hint: string;
	next: ErrorNext;
}

/** A run that ended without candidates, or that degraded, in words. `completed`
 *  distinguishes the one code a completed run can carry. */
export function describeRunError(
	code: IntelligenceRunErrorCode,
	completed: boolean,
	message: string | null,
): RunNotice {
	switch (code) {
		case 'invalid_output':
			return {
				title: 'The provider’s answer could not be used',
				hint: 'It was not the structure the analysis requires. Nothing was saved. Trying again usually works.',
				next: 'retry',
			};
		case 'timeout':
			return {
				title: 'The analysis took too long',
				hint: 'It was stopped after two minutes and nothing was saved. Try fewer sessions.',
				next: 'retry',
			};
		case 'cancelled':
			return {
				title: 'The analysis was cancelled',
				hint: 'Nothing was saved.',
				next: 'retry',
			};
		case 'disabled':
			return {
				title: 'Project Intelligence was turned off during the analysis',
				hint: 'It stopped and nothing was saved.',
				next: 'settings',
			};
		case 'interrupted':
			return {
				title: 'The app stopped during the analysis',
				hint: 'Nothing was saved.',
				next: 'retry',
			};
		case 'output_too_large':
			return {
				title: 'The provider’s answer was too large',
				hint: 'It was stopped and nothing was saved. Try fewer sessions.',
				next: 'retry',
			};
		case 'provider_failed':
			return {
				title: 'The provider failed',
				hint: message ?? 'It exited with an error. Nothing was saved.',
				next: 'retry',
			};
		case 'provider_unauthenticated':
			return {
				title: 'The provider is not signed in',
				hint: 'Sign in with the agent’s own CLI in a terminal, then try again. Nothing was saved.',
				next: 'retry',
			};
		case 'provider_unavailable':
			return completed
				? {
						title: 'The provider could not be used, so no model reviewed these',
						hint: 'Only the signals found on this machine are shown, as unreviewed observations.',
						next: 'retry',
					}
				: {
						title: 'The provider became unavailable during the analysis',
						hint: 'Nothing was saved.',
						next: 'retry',
					};
		case 'provider_unsafe':
			return {
				title: 'The provider’s answer was refused',
				hint: 'It used a tool or replied with more than one turn, which the isolated analysis does not allow. Nothing was saved.',
				next: 'none',
			};
		case 'internal':
			return {
				title: 'An internal error stopped the analysis',
				hint: message ?? 'Nothing was saved.',
				next: 'retry',
			};
	}
}

// ── Unified diff ────────────────────────────────────────────────────────────

export type DiffLineKind = 'file' | 'hunk' | 'add' | 'remove' | 'context' | 'note';

export interface DiffLine {
	/** Position in the diff, from 1: a line's identity, for a list key. */
	no: number;
	kind: DiffLineKind;
	text: string;
}

/**
 * A unified diff as classified lines, for colouring.
 *
 * **A hunk body is classified by its first character and only that.** Before the
 * first `@@` header, `---` and `+++` name the files; after it they are ordinary
 * lines, because removing a line that begins `-- ` produces `--- ` in the diff
 * and calling that a file header would drop it from the count the reader checks
 * against. The trailing newline of the diff is not a line.
 */
export function parseUnifiedDiff(diff: string): DiffLine[] {
	const lines = diff.split('\n');
	if (lines.at(-1) === '') lines.pop();
	let inHunk = false;
	return lines.map((text, at): DiffLine => {
		const no = at + 1;
		if (text.startsWith('@@')) {
			inHunk = true;
			return { no, kind: 'hunk', text };
		}
		if (!inHunk) return { no, kind: 'file', text };
		if (text.startsWith('+')) return { no, kind: 'add', text };
		if (text.startsWith('-')) return { no, kind: 'remove', text };
		if (text.startsWith('\\')) return { no, kind: 'note', text };
		return { no, kind: 'context', text };
	});
}

/** Added and removed line counts of a diff, for the summary beside it. */
export function diffStats(lines: readonly DiffLine[]): { added: number; removed: number } {
	return {
		added: lines.filter((l) => l.kind === 'add').length,
		removed: lines.filter((l) => l.kind === 'remove').length,
	};
}

/**
 * The line of the file after the change where it starts to differ, so the
 * viewer can open on it: the first added line, or for a change that only removes
 * (a revert) the line the first hunk starts at. `null` when the diff has no hunk.
 *
 * Read off the hunk headers (`@@ -a,b +c,d @@`) rather than guessed from the
 * file's length, because the diff is the one thing both sides agree on.
 */
export function firstChangedLine(diff: string): number | null {
	let next: number | null = null;
	let hunkStart: number | null = null;
	for (const line of parseUnifiedDiff(diff)) {
		if (line.kind === 'hunk') {
			const match = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line.text);
			next = match ? Number(match[1]) : null;
			hunkStart ??= next;
			continue;
		}
		if (next === null) continue;
		if (line.kind === 'add') return next;
		if (line.kind === 'context') next += 1;
	}
	return hunkStart;
}
