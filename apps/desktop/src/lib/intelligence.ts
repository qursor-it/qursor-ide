import type { AppError, IntelligenceErrorCode } from '@factorai/types';

/**
 * Project Intelligence's switch (F33, ADR-0084) as a row in the `settings` table
 * (`intelligence.enabled`, TS key `projectIntelligence`). Rust enforces it, so
 * it lives there and not in `prefsStore`.
 *
 * **Off is the default and anything but the exact value `on` is off**: unlike
 * desktop notifications, whose absence means on. A typo or a hand-edited row can
 * therefore never turn the feature on.
 */
export function intelligenceEnabledOf(value: string | null | undefined): boolean {
	return value === 'on';
}

/** The row to write: `on`, or no row at all for off. */
export function intelligenceEnabledSetting(on: boolean): string | null {
	return on ? 'on' : null;
}

const CODES: readonly IntelligenceErrorCode[] = [
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

function isIntelligenceError(error: unknown): error is Extract<AppError, { kind: 'Intelligence' }> {
	return (
		typeof error === 'object' &&
		error !== null &&
		(error as { kind?: unknown }).kind === 'Intelligence' &&
		typeof (error as { message?: unknown }).message === 'string'
	);
}

/**
 * The code of a rejected intelligence command, or `null` for any other error
 * (including a code this build does not know, which is then just a message).
 */
export function intelligenceErrorCode(error: unknown): IntelligenceErrorCode | null {
	if (!isIntelligenceError(error)) return null;
	const code = error.message.split(':', 1)[0] as IntelligenceErrorCode;
	return CODES.includes(code) ? code : null;
}

/** The sentence after the code: what to show a person. */
export function intelligenceErrorDetail(error: unknown): string {
	if (!isIntelligenceError(error)) return error instanceof Error ? error.message : String(error);
	const at = error.message.indexOf(': ');
	return at === -1 ? error.message : error.message.slice(at + 2);
}
