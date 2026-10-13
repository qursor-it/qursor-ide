import { describe, expect, it } from 'vitest';
import {
	isDiffMode,
	parseCommitRange,
	parsePosition,
	parsePrRange,
	type DiffMode,
} from './useFileViewer';

/**
 * `parsePosition` guards the two params a hand-edited URL can reach Monaco
 * through (F19). Everything it rejects is a position no file has.
 */
describe('parsePosition', () => {
	it('accepts a 1-based line, as a string or a number', () => {
		expect(parsePosition('42')).toBe(42);
		expect(parsePosition(42)).toBe(42);
		expect(parsePosition('1')).toBe(1);
	});

	it('rejects zero and negatives — lines start at 1', () => {
		expect(parsePosition('0')).toBeUndefined();
		expect(parsePosition('-3')).toBeUndefined();
	});

	it('rejects fractions', () => {
		expect(parsePosition('4.5')).toBeUndefined();
	});

	it('rejects anything that is not a number at all', () => {
		expect(parsePosition('abc')).toBeUndefined();
		expect(parsePosition('')).toBeUndefined();
		expect(parsePosition(undefined)).toBeUndefined();
		expect(parsePosition(null)).toBeUndefined();
		expect(parsePosition({})).toBeUndefined();
		// `Number(' ')` is 0 and `Number([])` is 0 — both would sneak past a
		// bare `Number.isInteger` check without the >= 1 floor.
		expect(parsePosition(' ')).toBeUndefined();
	});

	it('rejects Infinity and NaN', () => {
		expect(parsePosition('Infinity')).toBeUndefined();
		expect(parsePosition(Number.NaN)).toBeUndefined();
	});
});

const A = 'a'.repeat(40);
const B = 'b'.repeat(40);

/**
 * The diff modes a hand-edited URL can reach Monaco through (F13, F18, F31).
 * `COMMIT_RANGE` stays strict and anchored, and `PR_RANGE` is a separate
 * pattern, so neither can be satisfied by the other's shape.
 */
describe('isDiffMode', () => {
	it('accepts the three working-tree modes', () => {
		for (const mode of ['staged', 'unstaged', 'head']) expect(isDiffMode(mode)).toBe(true);
	});

	it('accepts a commit range, with or without a parent', () => {
		expect(isDiffMode(`${A}..${B}`)).toBe(true);
		expect(isDiffMode(`..${B}`)).toBe(true);
	});

	it('accepts a pull request range', () => {
		expect(isDiffMode(`pr:42:${A}..${B}`)).toBe(true);
		expect(isDiffMode(`pr:1:${A}..${B}`)).toBe(true);
	});

	it('rejects a malformed pull request range', () => {
		for (const bad of [
			`pr:0:${A}..${B}`,
			`pr:042:${A}..${B}`,
			`pr:-1:${A}..${B}`,
			`pr:42:${A.slice(1)}..${B}`,
			`pr:42:${A}..${B.toUpperCase()}`,
			`pr:42:..${B}`,
			`pr:42:${A}..`,
			`pr::${A}..${B}`,
			`pr:42:${A}..${B} `,
			` pr:42:${A}..${B}`,
			`pr:12345678901:${A}..${B}`,
			`PR:42:${A}..${B}`,
		]) {
			expect(isDiffMode(bad), bad).toBe(false);
		}
	});

	it('rejects loose ranges and non-strings', () => {
		for (const bad of ['..', 'abc..def', `${A}...${B}`, 'main', '', 42, null, undefined]) {
			expect(isDiffMode(bad), String(bad)).toBe(false);
		}
	});
});

describe('parsePrRange', () => {
	it('reads the number, merge base and head', () => {
		expect(parsePrRange(`pr:42:${A}..${B}` as DiffMode)).toEqual({
			number: 42,
			mergeBase: A,
			head: B,
		});
	});

	it('is null for every other mode', () => {
		expect(parsePrRange('staged')).toBeNull();
		expect(parsePrRange(`${A}..${B}`)).toBeNull();
		expect(parsePrRange(`..${B}`)).toBeNull();
	});
});

describe('parseCommitRange', () => {
	it('still reads a commit range', () => {
		expect(parseCommitRange(`${A}..${B}`)).toEqual({ left: A, right: B });
		expect(parseCommitRange(`..${B}`)).toEqual({ left: null, right: B });
	});

	it('does not match a pr: mode — the two patterns never overlap', () => {
		expect(parseCommitRange(`pr:42:${A}..${B}` as DiffMode)).toBeNull();
	});
});
