import type {
	ImprovementCandidate,
	ImprovementEvent,
	IntelligenceErrorCode,
	IntelligenceRunErrorCode,
} from '@factorai/types';
import { describe, expect, it } from 'vitest';
import {
	ACTION_ORDER,
	canFollowUp,
	clampSessionLimit,
	describeEvent,
	describeIntelligenceError,
	describeRunError,
	diffStats,
	firstChangedLine,
	parseUnifiedDiff,
	phaseLabel,
	progressFraction,
	providerReason,
	sectionCandidates,
	SESSION_LIMIT_DEFAULT,
	SESSION_LIMIT_MAX,
	sessionsLabel,
	shortHash,
	statusBadge,
	verificationOf,
} from './intelligenceView';

function event(
	over: Partial<ImprovementEvent> & Pick<ImprovementEvent, 'eventType'>,
): ImprovementEvent {
	return {
		id: 1,
		outcome: null,
		method: null,
		note: null,
		artifactPath: null,
		beforeHash: null,
		afterHash: null,
		createdAt: 1,
		...over,
	};
}

function candidate(over: Partial<ImprovementCandidate> = {}): ImprovementCandidate {
	return {
		id: 'c1',
		projectId: 'p1',
		latestRunId: 'r1',
		problemKind: 'project_convention',
		actionKind: 'instruction',
		title: 'Use pnpm',
		problem: 'p',
		recommendation: 'Use pnpm, not npm.',
		rationale: 'r',
		limitations: null,
		status: 'proposed',
		hypothesis: false,
		downgradeNote: null,
		followUp: false,
		independentSessions: 2,
		duplicate: null,
		evidence: [],
		events: [],
		implementation: null,
		createdAt: 1,
		updatedAt: 1,
		...over,
	};
}

describe('clampSessionLimit', () => {
	it('defaults to 20 and stops at 50, as the backend does', () => {
		expect(SESSION_LIMIT_DEFAULT).toBe(20);
		expect(SESSION_LIMIT_MAX).toBe(50);
		expect(clampSessionLimit(20)).toBe(20);
		expect(clampSessionLimit(500)).toBe(50);
		expect(clampSessionLimit(0)).toBe(1);
		expect(clampSessionLimit(-4)).toBe(1);
		expect(clampSessionLimit(7.9)).toBe(7);
	});

	it('takes a field’s text, and falls back to the default for anything else', () => {
		expect(clampSessionLimit('12')).toBe(12);
		expect(clampSessionLimit('')).toBe(20);
		expect(clampSessionLimit('abc')).toBe(20);
		expect(clampSessionLimit(Number.NaN)).toBe(20);
		expect(clampSessionLimit(undefined)).toBe(20);
	});
});

describe('sessionsLabel', () => {
	it('calls a single session a hypothesis and never a pattern', () => {
		expect(sessionsLabel({ independentSessions: 1, hypothesis: true })).toBe(
			'hypothesis · 1 independent session',
		);
	});

	it('counts several without a score', () => {
		expect(sessionsLabel({ independentSessions: 3, hypothesis: false })).toBe(
			'3 independent sessions',
		);
	});
});

describe('verification', () => {
	it('is unverified until a person has recorded a check', () => {
		const c = candidate({ status: 'implemented', events: [event({ eventType: 'implemented' })] });
		expect(verificationOf(c)).toEqual({ kind: 'unverified' });
		expect(statusBadge(c)).toBe('implemented · unverified');
	});

	it('reads the latest check off the timeline and says it is a check, not an effect', () => {
		const c = candidate({
			status: 'implemented',
			events: [
				event({ id: 1, eventType: 'implemented' }),
				event({ id: 2, eventType: 'verification', outcome: 'failed', method: 'reran tests' }),
				event({ id: 3, eventType: 'verification', outcome: 'passed', method: 'reran tests' }),
			],
		});
		expect(verificationOf(c)).toMatchObject({ kind: 'recorded', outcome: 'passed' });
		expect(statusBadge(c)).toBe('implemented · check passed');
	});

	it('keeps retained honest too: keeping it is not evidence that it works', () => {
		expect(statusBadge(candidate({ status: 'retained' }))).toBe('retained · unverified');
	});

	it('does not decorate a status that has no change behind it', () => {
		expect(statusBadge(candidate({ status: 'proposed' }))).toBe('proposed');
		expect(statusBadge(candidate({ status: 'dismissed' }))).toBe('dismissed');
		expect(statusBadge(candidate({ status: 'reverted' }))).toBe('reverted');
	});

	it('survives a verification with an outcome this build does not know', () => {
		const c = candidate({
			status: 'implemented',
			events: [event({ eventType: 'verification', outcome: 'excellent' })],
		});
		expect(verificationOf(c)).toMatchObject({ kind: 'recorded', outcome: null });
		expect(statusBadge(c)).toBe('implemented · checked');
	});
});

describe('sectionCandidates', () => {
	const list = [
		candidate({ id: 'a', actionKind: 'manual_followup' }),
		candidate({ id: 'b', actionKind: 'instruction' }),
		candidate({ id: 'c', actionKind: 'test_suggestion' }),
		candidate({ id: 'd', actionKind: 'instruction' }),
		candidate({ id: 'e', status: 'implemented' }),
		candidate({ id: 'f', status: 'retained' }),
		candidate({ id: 'g', status: 'dismissed' }),
		candidate({ id: 'h', status: 'reverted' }),
	];

	it('groups what is to review by the kind of remedy, in a fixed order', () => {
		const { review } = sectionCandidates(list);
		expect(review.map((g) => g.action)).toEqual([
			'instruction',
			'test_suggestion',
			'manual_followup',
		]);
		expect(review[0]?.candidates.map((c) => c.id)).toEqual(['b', 'd']);
	});

	it('leaves out an action nobody proposed rather than drawing an empty heading', () => {
		const { review } = sectionCandidates(list);
		expect(review.map((g) => g.action)).not.toContain('script_suggestion');
		expect(ACTION_ORDER).toHaveLength(4);
	});

	it('puts decided candidates in their own sections, in the order given', () => {
		const { inUse, closed } = sectionCandidates(list);
		expect(inUse.map((c) => c.id)).toEqual(['e', 'f']);
		expect(closed.map((c) => c.id)).toEqual(['g', 'h']);
	});

	it('is empty for no candidates', () => {
		expect(sectionCandidates([])).toEqual({ review: [], inUse: [], closed: [] });
	});
});

describe('canFollowUp', () => {
	it('mirrors the backend: not once dismissed or reverted', () => {
		expect(canFollowUp('proposed')).toBe(true);
		expect(canFollowUp('implemented')).toBe(true);
		expect(canFollowUp('retained')).toBe(true);
		expect(canFollowUp('dismissed')).toBe(false);
		expect(canFollowUp('reverted')).toBe(false);
	});
});

describe('describeEvent', () => {
	it('says a verification was typed by a person', () => {
		const line = describeEvent(
			event({
				eventType: 'verification',
				outcome: 'passed',
				method: 'ran the suite',
				note: 'twice',
			}),
		);
		expect(line.label).toBe('Checked by ran the suite: passed');
		expect(line.detail).toBe('recorded by you · twice');
	});

	it('names the file a Save went to and the hashes it moved between', () => {
		const line = describeEvent(
			event({
				eventType: 'implemented',
				artifactPath: 'AGENTS.md',
				beforeHash: 'a'.repeat(64),
				afterHash: 'b'.repeat(64),
			}),
		);
		expect(line.label).toBe('Saved to AGENTS.md');
		expect(line.detail).toBe('file aaaaaaaa to bbbbbbbb');
	});

	it('words a follow-up flag in both directions', () => {
		expect(describeEvent(event({ eventType: 'follow_up', outcome: 'on' })).label).toBe(
			'Marked for follow-up',
		);
		expect(describeEvent(event({ eventType: 'follow_up', outcome: 'off' })).label).toBe(
			'Follow-up flag removed',
		);
	});

	it('carries a reason when there is one and nothing when there is not', () => {
		expect(describeEvent(event({ eventType: 'dismissed', note: ' not relevant ' })).detail).toBe(
			'not relevant',
		);
		expect(describeEvent(event({ eventType: 'dismissed', note: '   ' })).detail).toBeNull();
		expect(describeEvent(event({ eventType: 'proposed' })).label).toBe('Proposed');
		expect(describeEvent(event({ eventType: 'retained', note: 'keeps helping' })).detail).toBe(
			'keeps helping',
		);
		expect(describeEvent(event({ eventType: 'reverted' })).label).toBe('Reverted');
		expect(describeEvent(event({ eventType: 'evidence_added' })).label).toMatch(/evidence/);
	});

	it('shortens a hash to eight characters and tolerates none', () => {
		expect(shortHash('0123456789abcdef')).toBe('01234567');
		expect(shortHash(null)).toBeNull();
	});
});

describe('progress', () => {
	it('is a fraction of the sessions done, and unknown before a total is known', () => {
		expect(progressFraction({ sessionsDone: 5, sessionsTotal: 20 })).toBe(0.25);
		expect(progressFraction({ sessionsDone: 0, sessionsTotal: 0 })).toBeNull();
		expect(progressFraction(null)).toBeNull();
		expect(progressFraction({ sessionsDone: 30, sessionsTotal: 20 })).toBe(1);
	});

	it('names the three phases', () => {
		expect(phaseLabel('analyzing')).toMatch(/Analy/);
		expect(phaseLabel('validating')).toMatch(/Check/);
		expect(phaseLabel('saving')).toMatch(/Sav/);
	});
});

describe('providerReason', () => {
	const base = {
		id: 'claude_cli',
		displayName: 'Claude Code CLI',
		remote: true,
		available: false,
		version: null,
		binaryPath: null,
		detail: null,
	};

	it('says why, by reason', () => {
		expect(providerReason({ ...base, reason: 'not_installed' })).toMatch(/not installed/);
		expect(providerReason({ ...base, reason: 'missing_flag' })).toMatch(/lacks a flag/);
		expect(providerReason({ ...base, reason: 'probe_failed' })).toMatch(/did not answer/);
		expect(providerReason({ ...base, reason: null })).toMatch(/cannot be used/);
	});
});

describe('describeIntelligenceError', () => {
	const CODES: IntelligenceErrorCode[] = [
		'disabled',
		'project_not_found',
		'project_missing',
		'candidate_not_found',
		'run_not_found',
		'run_in_progress',
		'no_data',
		'preview_stale',
		'invalid_state',
		'not_instruction',
		'not_prepared',
		'file_conflict',
		'not_saved',
		'unsafe_target',
		'target_too_large',
	];

	it('has precise words for every code the backend can refuse with', () => {
		for (const code of CODES) {
			const notice = describeIntelligenceError({
				kind: 'Intelligence',
				message: `${code}: detail`,
			});
			expect(notice.title.length, code).toBeGreaterThan(8);
			expect(notice.hint.length, code).toBeGreaterThan(8);
			expect(notice.title, code).not.toContain(code);
		}
	});

	it('sends a stale preview back to the preview and a lost Save back to the file', () => {
		const next = (code: string) =>
			describeIntelligenceError({ kind: 'Intelligence', message: `${code}: x` }).next;
		expect(next('preview_stale')).toBe('preview-again');
		expect(next('file_conflict')).toBe('prepare-again');
		expect(next('not_prepared')).toBe('prepare-again');
		expect(next('not_saved')).toBe('check-again');
		expect(next('disabled')).toBe('settings');
		expect(next('not_instruction')).toBe('none');
	});

	it('keeps the backend’s own sentence where it adds a fact', () => {
		const notice = describeIntelligenceError({
			kind: 'Intelligence',
			message: 'file_conflict: AGENTS.md on disk is not the content that was saved',
		});
		expect(notice.detail).toBe('AGENTS.md on disk is not the content that was saved');
	});

	it('does not repeat itself when the title already says it', () => {
		expect(
			describeIntelligenceError({ kind: 'Intelligence', message: 'disabled: off' }).detail,
		).toBeNull();
	});

	it('is a generic, retryable notice for anything that is not an intelligence error', () => {
		const notice = describeIntelligenceError(new Error('boom'));
		expect(notice).toMatchObject({ title: 'Something went wrong', detail: 'boom', next: 'retry' });
		expect(describeIntelligenceError({ kind: 'Io', message: 'disk' }).detail).toBe('Io: disk');
	});
});

describe('describeRunError', () => {
	const CODES: IntelligenceRunErrorCode[] = [
		'invalid_output',
		'timeout',
		'cancelled',
		'disabled',
		'interrupted',
		'output_too_large',
		'provider_failed',
		'provider_unauthenticated',
		'provider_unavailable',
		'provider_unsafe',
		'internal',
	];

	it('has words for every run code, and every failure says nothing was saved or what was', () => {
		for (const code of CODES) {
			const notice = describeRunError(code, false, null);
			expect(notice.title.length, code).toBeGreaterThan(8);
			expect(notice.hint, code).toMatch(/saved|Sign in|signals|Nothing|stopped|Try|sent/i);
		}
	});

	it('tells a degraded completed run from a provider that vanished mid-run', () => {
		expect(describeRunError('provider_unavailable', true, null).title).toMatch(/no model reviewed/);
		expect(describeRunError('provider_unavailable', false, null).title).toMatch(/during/);
	});

	it('uses the backend’s message for the two codes that carry one', () => {
		expect(describeRunError('provider_failed', false, 'exit code 3').hint).toBe('exit code 3');
		expect(describeRunError('internal', false, 'db is locked').hint).toBe('db is locked');
	});

	it('points the unauthenticated at the CLI and an unsafe answer nowhere', () => {
		expect(describeRunError('provider_unauthenticated', false, null).hint).toMatch(/Sign in/);
		expect(describeRunError('provider_unsafe', false, null).next).toBe('none');
	});
});

describe('parseUnifiedDiff', () => {
	const DIFF = [
		'--- a/AGENTS.md',
		'+++ b/AGENTS.md',
		'@@ -1,2 +1,4 @@',
		' # Project',
		' ',
		'+',
		'+- Use pnpm, not npm.',
		'',
	].join('\n');

	it('separates the file header from the hunk and classifies the body', () => {
		expect(parseUnifiedDiff(DIFF).map((l) => l.kind)).toEqual([
			'file',
			'file',
			'hunk',
			'context',
			'context',
			'add',
			'add',
		]);
	});

	it('does not take a removed line that starts with dashes for a file header', () => {
		const lines = parseUnifiedDiff(
			['--- a/x', '+++ b/x', '@@ -1,1 +0,0 @@', '--- a/dash'].join('\n'),
		);
		expect(lines.at(-1)).toEqual({ kind: 'remove', text: '--- a/dash' });
		expect(diffStats(lines)).toEqual({ added: 0, removed: 1 });
	});

	it('keeps a "no newline" marker out of the counts', () => {
		const lines = parseUnifiedDiff(
			['@@ -1 +1 @@', '-a', '\\ No newline at end of file', '+b'].join('\n'),
		);
		expect(lines.map((l) => l.kind)).toEqual(['hunk', 'remove', 'note', 'add']);
	});

	it('is empty for an empty diff', () => {
		expect(parseUnifiedDiff('')).toEqual([]);
	});
});

describe('firstChangedLine', () => {
	it('is the line in the new file where the first addition lands', () => {
		const diff = ['--- a', '+++ b', '@@ -1,2 +1,4 @@', ' one', ' two', '+', '+three'].join('\n');
		expect(firstChangedLine(diff)).toBe(3);
	});

	it('walks past context after a later hunk header too', () => {
		const diff = ['--- a', '+++ b', '@@ -10,3 +10,4 @@', ' a', ' b', ' c', '+d'].join('\n');
		expect(firstChangedLine(diff)).toBe(13);
	});

	it('falls back to where the hunk starts when the change only removes', () => {
		const diff = ['--- a', '+++ b', '@@ -5,3 +5,2 @@', ' a', '-gone', ' b'].join('\n');
		expect(firstChangedLine(diff)).toBe(5);
	});

	it('is null when there is no hunk', () => {
		expect(firstChangedLine('')).toBeNull();
		expect(firstChangedLine('--- a\n+++ b\n')).toBeNull();
	});
});
