/**
 * The monospace font, and the terminal options built from the user's choices
 * (ADR-0082 "Terminal appearance", F11).
 *
 * One stack constant for the places that always want the bundled face (the
 * diff and file viewers), and a preset table for the terminal, where the font
 * is a setting. Pure on purpose, like `settingsDraft`: the mapping from saved
 * preferences to xterm options is worth pinning in vitest rather than
 * discovering in a window.
 */

/** JetBrains Mono ships with the app (`@fontsource-variable/jetbrains-mono`,
 *  imported in `main.tsx`), so the first entry is always there. The variable
 *  build registers its family as "JetBrains Mono Variable"; the plain name is
 *  kept for a machine that has the static font installed. */
export const MONO_STACK =
	'"JetBrains Mono Variable", "JetBrains Mono", "Fira Code", ui-monospace, monospace';

/** The families a terminal font preset can be, in the order Settings lists them. */
export const TERMINAL_FONTS = [
	{ id: 'jetbrains', label: 'JetBrains Mono (bundled)', stack: MONO_STACK },
	{
		id: 'cascadia',
		label: 'Cascadia Code',
		stack: '"Cascadia Code", "Cascadia Mono", Consolas, ui-monospace, monospace',
	},
	{
		id: 'firacode',
		label: 'Fira Code',
		stack: '"Fira Code", "JetBrains Mono Variable", ui-monospace, monospace',
	},
	{
		id: 'sfmono',
		label: 'SF Mono / Menlo',
		stack: '"SF Mono", SFMono-Regular, Menlo, Monaco, ui-monospace, monospace',
	},
	{
		id: 'consolas',
		label: 'Consolas',
		stack: 'Consolas, "Liberation Mono", ui-monospace, monospace',
	},
	{ id: 'system', label: 'System monospace', stack: 'ui-monospace, monospace' },
] as const;

export type TerminalFontId = (typeof TERMINAL_FONTS)[number]['id'];

export const TERMINAL_CURSORS = ['bar', 'block', 'underline'] as const;
export type TerminalCursor = (typeof TERMINAL_CURSORS)[number];

/** The bounds Settings offers and `terminalOptions` enforces. */
export const FONT_SIZE_RANGE = { min: 10, max: 20, step: 1 } as const;
export const LINE_HEIGHT_RANGE = { min: 1, max: 1.6, step: 0.05 } as const;

/** The four values the terminal reads from `prefsStore`. */
export interface TerminalPrefs {
	terminalFont: TerminalFontId;
	terminalFontSize: number;
	terminalLineHeight: number;
	terminalCursor: TerminalCursor;
}

/** What the terminal takes from the preferences: the subset of xterm's options
 *  they own. `cursorBlink` and everything else stay with the constructor. */
interface TerminalOptions {
	fontFamily: string;
	fontSize: number;
	lineHeight: number;
	cursorStyle: TerminalCursor;
	cursorInactiveStyle: 'outline';
}

function clamp(value: number, min: number, max: number, fallback: number): number {
	if (!Number.isFinite(value)) return fallback;
	return Math.min(max, Math.max(min, value));
}

/** The stack for a preset id; a stored value may predate a removed preset or
 *  be hand-edited, so an unknown id is the bundled stack. */
function presetStack(id: string): string {
	return TERMINAL_FONTS.find((font) => font.id === id)?.stack ?? MONO_STACK;
}

/**
 * The xterm options for a set of terminal preferences.
 *
 * **Clamps rather than trusts**: the values come from localStorage, which a
 * hand edit, an older build or a half-finished Save can leave out of range, and
 * a line height of 0 or a font size of 400 would make every terminal unusable
 * until the key is deleted by hand. An unknown font id falls back to the
 * bundled stack. The line height is rounded to its step so a float drift never
 * reaches the renderer.
 *
 * `cursorInactiveStyle: 'outline'` is what a bar or underline cursor shows in a
 * terminal that has lost focus, which is the hollow-cursor convention.
 */
export function terminalOptions(prefs: TerminalPrefs): TerminalOptions {
	const { min: sizeMin, max: sizeMax } = FONT_SIZE_RANGE;
	const { min: lhMin, max: lhMax, step } = LINE_HEIGHT_RANGE;
	return {
		fontFamily: presetStack(prefs.terminalFont),
		fontSize: Math.round(clamp(prefs.terminalFontSize, sizeMin, sizeMax, 13)),
		lineHeight: roundTo(clamp(prefs.terminalLineHeight, lhMin, lhMax, lhMin), step),
		cursorStyle: TERMINAL_CURSORS.includes(prefs.terminalCursor) ? prefs.terminalCursor : 'bar',
		cursorInactiveStyle: 'outline',
	};
}

function roundTo(value: number, step: number): number {
	return Number((Math.round(value / step) * step).toFixed(2));
}
