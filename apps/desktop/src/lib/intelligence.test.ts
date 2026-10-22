import { describe, expect, it } from 'vitest';
import {
	intelligenceEnabledOf,
	intelligenceEnabledSetting,
	intelligenceErrorCode,
	intelligenceErrorDetail,
} from './intelligence';

describe('the Project Intelligence switch', () => {
	it('is off unless the row is exactly `on`', () => {
		expect(intelligenceEnabledOf(null)).toBe(false);
		expect(intelligenceEnabledOf(undefined)).toBe(false);
		expect(intelligenceEnabledOf('on')).toBe(true);
		for (const junk of ['ON', 'true', '1', 'yes', '', 'off']) {
			expect(intelligenceEnabledOf(junk), junk).toBe(false);
		}
	});

	it('writes `on`, or deletes the row for off', () => {
		expect(intelligenceEnabledSetting(true)).toBe('on');
		expect(intelligenceEnabledSetting(false)).toBeNull();
	});
});

describe('intelligence errors', () => {
	const error = { kind: 'Intelligence', message: 'file_conflict: the file changed: again' };

	it('splits the code from the detail at the first separator only', () => {
		expect(intelligenceErrorCode(error)).toBe('file_conflict');
		expect(intelligenceErrorDetail(error)).toBe('the file changed: again');
	});

	it('is null for other errors and for codes it does not know', () => {
		expect(intelligenceErrorCode({ kind: 'Io', message: 'disabled: x' })).toBeNull();
		expect(intelligenceErrorCode({ kind: 'Intelligence', message: 'brand_new: x' })).toBeNull();
		expect(intelligenceErrorCode(new Error('boom'))).toBeNull();
		expect(intelligenceErrorCode(null)).toBeNull();
		expect(intelligenceErrorDetail(new Error('boom'))).toBe('boom');
	});
});
