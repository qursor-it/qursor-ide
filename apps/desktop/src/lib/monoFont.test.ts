import { describe, expect, it } from 'vitest';
import { MONO_STACK, TERMINAL_FONTS, type TerminalPrefs, terminalOptions } from './monoFont';

const BASE: TerminalPrefs = {
	terminalFont: 'jetbrains',
	terminalFontSize: 13,
	terminalLineHeight: 1,
	terminalCursor: 'bar',
};

describe('terminalOptions', () => {
	it('maps the preferences onto xterm options', () => {
		expect(terminalOptions(BASE)).toEqual({
			fontFamily: MONO_STACK,
			fontSize: 13,
			lineHeight: 1,
			cursorStyle: 'bar',
			cursorInactiveStyle: 'outline',
		});
	});

	it('gives every preset a stack with a generic monospace fallback', () => {
		for (const font of TERMINAL_FONTS) {
			expect(terminalOptions({ ...BASE, terminalFont: font.id }).fontFamily).toBe(font.stack);
			expect(font.stack, font.id).toMatch(/monospace$/);
		}
	});

	it('clamps a size and line height that are out of range', () => {
		expect(terminalOptions({ ...BASE, terminalFontSize: 400 }).fontSize).toBe(20);
		expect(terminalOptions({ ...BASE, terminalFontSize: 2 }).fontSize).toBe(10);
		expect(terminalOptions({ ...BASE, terminalLineHeight: 0 }).lineHeight).toBe(1);
		expect(terminalOptions({ ...BASE, terminalLineHeight: 9 }).lineHeight).toBe(1.6);
	});

	it('survives values that are not numbers or ids', () => {
		const odd = terminalOptions({
			terminalFont: 'gone' as never,
			terminalFontSize: Number.NaN,
			terminalLineHeight: Number.NaN,
			terminalCursor: 'beam' as never,
		});
		expect(odd.fontFamily).toBe(MONO_STACK);
		expect(odd.fontSize).toBe(13);
		expect(odd.lineHeight).toBe(1);
		expect(odd.cursorStyle).toBe('bar');
	});

	it('rounds the line height to its step and the size to a whole pixel', () => {
		expect(terminalOptions({ ...BASE, terminalLineHeight: 1.15 }).lineHeight).toBe(1.15);
		expect(terminalOptions({ ...BASE, terminalLineHeight: 1.1500000001 }).lineHeight).toBe(1.15);
		expect(terminalOptions({ ...BASE, terminalFontSize: 13.4 }).fontSize).toBe(13);
	});
});
