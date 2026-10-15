import type { ITheme } from '@xterm/xterm';
import { oklchToHex } from '@lib/oklch';
import type { Theme } from '@lib/theme';

/**
 * The palette → xterm mapper Q8 decided on (`specs/07-open-questions.md`): two
 * themes, light and dark, following the app's rather than offering a picker.
 *
 * xterm draws to a canvas and takes its colours as an options object, so it
 * cannot read a CSS variable. The ground, the text and the cursor are read off
 * the tokens and converted, which keeps one palette rather than a hex copy that
 * goes stale; `--terminal` and `--terminal-foreground` exist for this.
 */

type AnsiName =
	| 'black'
	| 'red'
	| 'green'
	| 'yellow'
	| 'blue'
	| 'magenta'
	| 'cyan'
	| 'white'
	| 'brightBlack'
	| 'brightRed'
	| 'brightGreen'
	| 'brightYellow'
	| 'brightBlue'
	| 'brightMagenta'
	| 'brightCyan'
	| 'brightWhite';

type TokenReader = (name: string) => string;

/**
 * The token values each theme ships with, used when a token cannot be read —
 * before the stylesheet has loaded, or in a test. Mirrors `@factorai/ui`'s
 * `globals.css`; a mismatch only matters for the frame before CSS arrives.
 */
const FALLBACK: Record<Theme, Record<string, string>> = {
	dark: {
		'--terminal': 'oklch(16.3% 0.009 264)',
		'--terminal-foreground': 'oklch(87.1% 0.005 286)',
		'--primary': 'oklch(65.9% 0.088 259)',
	},
	light: {
		'--terminal': 'oklch(100% 0 0)',
		'--terminal-foreground': 'oklch(24% 0.008 250)',
		'--primary': 'oklch(48.9% 0.102 262)',
	},
};

/**
 * ANSI colours for a white ground.
 *
 * **xterm's defaults are a dark-ground palette**: its `white` is near-white and
 * its `yellow` is a light yellow, so on paper a `git status`, a test runner's
 * warnings and half of a prompt simply disappear. These are GitHub's light
 * terminal colours (Primer), which are tuned for exactly this — the normal hues
 * hold at least 4.5:1 on white, and the lighter bright blue, magenta, cyan and
 * white sit at 3:1, the faint end of the palette.
 *
 * The dark theme has its own table, `DARK_ANSI`, below.
 */
const LIGHT_ANSI: ITheme = {
	black: '#24292f',
	red: '#cf222e',
	green: '#116329',
	yellow: '#4d2d00',
	blue: '#0969da',
	magenta: '#8250df',
	cyan: '#1b7c83',
	white: '#6e7781',
	brightBlack: '#57606a',
	brightRed: '#a40e26',
	brightGreen: '#1a7f37',
	brightYellow: '#633c01',
	brightBlue: '#218bff',
	brightMagenta: '#a475f9',
	brightCyan: '#3192aa',
	brightWhite: '#8c959f',
};

/**
 * ANSI colours for the dark Terminal Well (ADR-0082, amending Q8's "xterm's
 * defaults in dark").
 *
 * xterm's defaults are saturated and clash with the well and the status hues,
 * so these are written in oklch like the tokens and share their hues: green near
 * `status-working` and `git-added`, amber near `git-modified`, red near
 * `destructive` and `git-deleted`, blue near the `primary` family and
 * `git-renamed`, cyan near `status-background`. Lightness sits at 70-96%, which
 * holds at least 4.5:1 on `--terminal` for every colour; `black` and
 * `brightBlack` are the deliberate exception at 3:1, so they still read as dim.
 * `themes.test.ts` pins both bounds.
 */
const DARK_ANSI_OKLCH: Record<AnsiName, readonly [lightness: number, chroma: number, hue: number]> =
	{
		black: [0.5, 0.01, 264],
		red: [0.7, 0.15, 22],
		green: [0.74, 0.15, 150],
		yellow: [0.8, 0.13, 80],
		blue: [0.72, 0.11, 255],
		magenta: [0.74, 0.13, 320],
		cyan: [0.76, 0.1, 200],
		white: [0.82, 0.005, 286],
		brightBlack: [0.6, 0.01, 264],
		brightRed: [0.76, 0.16, 20],
		brightGreen: [0.8, 0.17, 155],
		brightYellow: [0.86, 0.14, 85],
		brightBlue: [0.78, 0.12, 250],
		brightMagenta: [0.8, 0.14, 325],
		brightCyan: [0.82, 0.11, 200],
		brightWhite: [0.96, 0.003, 286],
	};

const DARK_ANSI: ITheme = Object.fromEntries(
	Object.entries(DARK_ANSI_OKLCH).map(([name, [l, c, h]]) => [
		name,
		oklchToHex(`oklch(${l * 100}% ${c} ${h})`) ?? '#000000',
	]),
);

export function xtermTheme(readToken: TokenReader, theme: Theme): ITheme {
	const token = (name: string): string =>
		oklchToHex(readToken(name)) ?? oklchToHex(FALLBACK[theme][name] ?? '') ?? '#000000';

	const background = token('--terminal');
	const foreground = token('--terminal-foreground');
	return {
		background,
		foreground,
		cursor: token('--primary'),
		cursorAccent: background,
		// The text colour at 30%: a selection that reads on both grounds, where
		// xterm's own default is a white wash that vanishes on paper.
		selectionBackground: `${foreground}4d`,
		...(theme === 'light' ? LIGHT_ANSI : DARK_ANSI),
	};
}

/** The theme for the document as it is now. */
export function documentXtermTheme(theme: Theme): ITheme {
	const style = getComputedStyle(document.documentElement);
	return xtermTheme((name) => style.getPropertyValue(name), theme);
}
