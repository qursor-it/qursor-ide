import { describe, expect, it } from 'vitest';
import { hunkPatch, splitHunks } from './hunks';

const header = 'diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n';
const one = '@@ -1,2 +1,2 @@\n a\n-b\n+c\n';
const two = '@@ -10,2 +10,2 @@\n x\n-y\n+z\n\\ No newline at end of file\n';

describe('hunks', () => {
	it('splits_two_hunks', () => {
		const parts = splitHunks(header + one + two);
		expect(parts.header).toBe(header);
		expect(parts.hunks).toEqual([one, two]);
	});

	it('hunk_patch_keeps_file_header_and_ends_with_newline', () => {
		const patch = hunkPatch(header, '@@ -1 +1 @@\n-a\n+b');
		expect(patch.startsWith(header)).toBe(true);
		expect(patch.endsWith('+b\n')).toBe(true);
	});

	it('no_newline_at_eof_marker_stays_with_its_hunk', () => {
		const parts = splitHunks(header + one + two);
		expect(parts.hunks[1]).toContain('\\ No newline at end of file');
		expect(parts.hunks[0]).not.toContain('No newline');
	});

	it('a_patch_without_hunks_has_none', () => {
		expect(splitHunks(header).hunks).toEqual([]);
	});
});
