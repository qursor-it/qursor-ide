import { describe, expect, it } from 'vitest';
import { xtermTheme } from './themes';

/** The tokens as `getComputedStyle` hands them over: the declared text. */
const DARK: Record<string, string> = {
	'--terminal': 'oklch(16.3% 0.009 264)',
	'--terminal-foreground': 'oklch(87.1% 0.005 286)',
	'--primary': 'oklch(65.9% 0.088 259)',
};
const LIGHT: Record<string, string> = {
	'--terminal': 'oklch(100% 0 0)',
	'--terminal-foreground': 'oklch(24% 0.008 250)',
	'--primary': 'oklch(48.9% 0.102 262)',
};

const ANSI_KEYS = [
	'black',
	'red',
	'green',
	'yellow',
	'blue',
	'magenta',
	'cyan',
	'white',
	'brightBlack',
	'brightRed',
	'brightGreen',
	'brightYellow',
	'brightBlue',
	'brightMagenta',
	'brightCyan',
	'brightWhite',
] as const;

/** WCAG 2.x relative luminance of a `#rrggbb`. */
function luminance(hex: string): number {
	const [r, g, b] = [1, 3, 5].map((i) => {
		const v = parseInt(hex.slice(i, i + 2), 16) / 255;
		return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
	});
	return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (b ?? 0);
}

function contrast(a: string, b: string): number {
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
	return ((hi ?? 0) + 0.05) / ((lo ?? 0) + 0.05);
}

const reader = (tokens: Record<string, string>) => (name: string) => tokens[name] ?? '';

describe('xtermTheme', () => {
	it('reads the ground, text and cursor off the palette', () => {
		const theme = xtermTheme(reader(DARK), 'dark');
		expect(theme.background).toBe('#0c0e12');
		expect(theme.foreground).toBe('#d4d4d8');
		// The brand blue, exactly — the cursor is the accent.
		expect(theme.cursor).toBe('#7193c8');
		// A block cursor draws the glyph under it in this colour.
		expect(theme.cursorAccent).toBe(theme.background);
	});

	it('gives the dark theme its own ANSI colours rather than the xterm defaults', () => {
		const theme = xtermTheme(reader(DARK), 'dark');
		for (const key of ANSI_KEYS) expect(theme[key], key).toMatch(/^#[0-9a-f]{6}$/);
		// xterm's default yellow, which the tuned table replaces.
		expect(theme.yellow).not.toBe('#e5e510');
	});

	// The colours held to the 3:1 non-text floor instead of 4.5:1. Dark: the two
	// blacks, which are meant to read as dim. Light: Primer's lighter "bright"
	// variants, which are the faint end of that palette by design.
	it.each([
		['dark', DARK, ['black', 'brightBlack']],
		['light', LIGHT, ['brightBlue', 'brightMagenta', 'brightCyan', 'brightWhite']],
	] as const)('keeps every %s ANSI colour legible on its ground', (name, tokens, dim) => {
		const theme = xtermTheme(reader(tokens), name);
		const ground = theme.background ?? '';
		for (const key of ANSI_KEYS) {
			const ratio = contrast(theme[key] ?? '', ground);
			const floor = (dim as readonly string[]).includes(key) ? 3 : 4.5;
			expect(ratio, `${name} ${key}`).toBeGreaterThanOrEqual(floor);
		}
	});

	it('gives the light theme ANSI colours that read on white', () => {
		const theme = xtermTheme(reader(LIGHT), 'light');
		expect(theme.background).toBe('#ffffff');
		for (const key of ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan'] as const) {
			expect(theme[key], key).toMatch(/^#[0-9a-f]{6}$/);
		}
		// `white` is the one that would vanish if left to xterm's default.
		expect(theme.white).not.toBe('#ffffff');
	});

	it('falls back to the dark values rather than to black when a token is missing', () => {
		const theme = xtermTheme(() => '', 'dark');
		expect(theme.background).toBe('#0c0e12');
		expect(theme.foreground).toBe('#d4d4d8');
	});

	it('falls back to the light values in the light theme', () => {
		const theme = xtermTheme(() => '', 'light');
		expect(theme.background).toBe('#ffffff');
	});
});
