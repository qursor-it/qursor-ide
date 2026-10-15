import { describe, expect, it } from 'vitest';
import { formatDroppedPaths, toCssPoint } from './droppedPaths';

describe('formatDroppedPaths', () => {
	it('is empty for no paths', () => {
		expect(formatDroppedPaths([], false)).toBe('');
		expect(formatDroppedPaths([], true)).toBe('');
	});

	it('leaves a plain path alone, with one trailing space', () => {
		expect(formatDroppedPaths(['/home/me/shot.png'], false)).toBe('/home/me/shot.png ');
		expect(formatDroppedPaths([String.raw`C:\Users\me\shot.png`], true)).toBe(
			String.raw`C:\Users\me\shot.png `,
		);
	});

	it('joins several paths with a space', () => {
		expect(formatDroppedPaths(['/a/b.png', '/a/c.png'], false)).toBe('/a/b.png /a/c.png ');
	});

	it('backslash-escapes spaces and shell characters on POSIX', () => {
		expect(formatDroppedPaths(['/Users/me/My Shot (1).png'], false)).toBe(
			String.raw`/Users/me/My\ Shot\ \(1\).png `,
		);
		expect(formatDroppedPaths(["/tmp/it's $HOME&more"], false)).toBe(
			String.raw`/tmp/it\'s\ \$HOME\&more `,
		);
	});

	it('single-quotes a POSIX path with a line break', () => {
		expect(formatDroppedPaths(["/tmp/a\nb'c"], false)).toBe(`'/tmp/a\nb'\\''c' `);
	});

	it('double-quotes a Windows path with a space or special character', () => {
		expect(formatDroppedPaths([String.raw`C:\Users\me\My Shot.png`], true)).toBe(
			String.raw`"C:\Users\me\My Shot.png" `,
		);
		expect(formatDroppedPaths([String.raw`C:\a&b.png`, String.raw`C:\plain.png`], true)).toBe(
			String.raw`"C:\a&b.png" C:\plain.png `,
		);
	});
});

describe('toCssPoint', () => {
	it('divides physical pixels by the device pixel ratio', () => {
		expect(toCssPoint({ x: 300, y: 150 }, 1.5)).toEqual({ x: 200, y: 100 });
		expect(toCssPoint({ x: 300, y: 150 }, 1)).toEqual({ x: 300, y: 150 });
	});

	it('treats a nonsense ratio as 1', () => {
		expect(toCssPoint({ x: 10, y: 20 }, 0)).toEqual({ x: 10, y: 20 });
	});
});
