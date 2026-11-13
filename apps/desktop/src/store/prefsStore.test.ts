import { describe, expect, it } from 'vitest';
import { migratePrefs } from './prefsStore';

describe('migratePrefs', () => {
	it('moves the old default system theme to light once', () => {
		expect(migratePrefs({ theme: 'system', clock24: false }, 1)).toEqual({
			theme: 'light',
			clock24: false,
		});
	});

	it('keeps a theme somebody chose, and anything already at version 2', () => {
		expect(migratePrefs({ theme: 'dark' }, 1)).toEqual({ theme: 'dark' });
		expect(migratePrefs({ theme: 'system' }, 2)).toEqual({ theme: 'system' });
		expect(migratePrefs(null, 1)).toBeNull();
	});
});
