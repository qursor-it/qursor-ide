import type { TerminalStatus } from '@factorai/types';
import { describe, expect, it } from 'vitest';
import { desktopNotificationsOf, desktopNotificationsSetting, wantsYou } from './attention';

const ALL: TerminalStatus[] = ['working', 'waiting_input', 'stopped', 'unknown'];

describe('wantsYou', () => {
	it('matches the F32 table', () => {
		for (const prev of ALL) {
			for (const next of ALL) {
				const expected = prev === 'working' && (next === 'waiting_input' || next === 'stopped');
				expect(wantsYou(prev, next), `${prev} -> ${next}`).toBe(expected);
			}
		}
	});

	it('never fires without a previous status', () => {
		expect(wantsYou(undefined, 'waiting_input')).toBe(false);
	});
});

describe('desktop notifications setting', () => {
	it('is on unless the row says off', () => {
		expect(desktopNotificationsOf(null)).toBe(true);
		expect(desktopNotificationsOf('off')).toBe(false);
	});

	it('writes off, or deletes the row for on', () => {
		expect(desktopNotificationsSetting(false)).toBe('off');
		expect(desktopNotificationsSetting(true)).toBeNull();
	});
});
