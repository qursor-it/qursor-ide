import type { AgentId, Profile, TaskbarWidgetConfig } from '@factorai/types';
import { DEFAULT_THRESHOLDS } from '@lib/usage';

/**
 * The taskbar widget's settings, as the form edits them. Pure, so the rules the
 * form enforces are tested rather than eyeballed.
 */

/** The widget before anyone configured it. Mirrors Rust
 *  `WidgetConfig::default()`; the form's "reset" and the browser lane's mock
 *  both start from it. */
export const DEFAULT_TASKBAR_WIDGET: TaskbarWidgetConfig = {
	gap: 8,
	width: 244,
	show: {
		fiveHourBar: true,
		fiveHourPercent: true,
		fiveHourReset: true,
		weeklyBar: false,
		weeklyPercent: true,
		weeklyReset: true,
	},
	agents: [
		{ agent: 'claude', visible: true, profileId: null },
		{ agent: 'codex', visible: true, profileId: null },
	],
	layout: 'rows',
	labels: 'letter',
	fontPx: 11,
	theme: 'auto',
	backgroundOpacity: 0,
	colors: { normal: null, warn: '#E8A33D', critical: '#E5484D' },
	click: 'focus',
};

/** The font sizes the form offers, in logical px. Rust clamps to the same range. */
export const FONT_PX_RANGE = { min: 8, max: 20 } as const;

/** The position and size, put back to where a new install has them. */
export function withDefaultGeometry(config: TaskbarWidgetConfig): TaskbarWidgetConfig {
	return { ...config, gap: DEFAULT_TASKBAR_WIDGET.gap, width: DEFAULT_TASKBAR_WIDGET.width };
}

export function hasDefaultGeometry(config: TaskbarWidgetConfig): boolean {
	return config.gap === DEFAULT_TASKBAR_WIDGET.gap && config.width === DEFAULT_TASKBAR_WIDGET.width;
}

/** The agents list with the one at `index` moved by `by` places; a move off
 *  either end changes nothing. */
export function moveAgent(
	agents: TaskbarWidgetConfig['agents'],
	index: number,
	by: -1 | 1,
): TaskbarWidgetConfig['agents'] {
	const to = index + by;
	if (to < 0 || to >= agents.length) return agents;
	const next = [...agents];
	[next[index], next[to]] = [next[to], next[index]];
	return next;
}

/** The profiles an agent's slot can pick from: its own, default first, as the
 *  profile list comes. */
export function profilesFor(profiles: readonly Profile[], agent: AgentId): Profile[] {
	return profiles.filter((p) => p.agent === agent);
}

/** Two configs are the same widget: compared as the JSON each side writes. */
export function sameWidget(a: TaskbarWidgetConfig, b: TaskbarWidgetConfig): boolean {
	return JSON.stringify(a) === JSON.stringify(b);
}

/** A threshold's text field on its way to the table: the digits, or `null` for
 *  "unset, use the default". Anything that is not a whole number in 1..=100 is
 *  treated as unset rather than written, as the routines' fields are. */
export function thresholdSetting(text: string): string | null {
	const trimmed = text.trim();
	if (trimmed === '') return null;
	const n = Number(trimmed);
	if (!Number.isInteger(n) || n < 1 || n > 100) return null;
	return String(n);
}

/** Why the two threshold fields cannot be saved, or `null` when they can. A
 *  blank field is the default and is fine. */
export function thresholdError(warnText: string, criticalText: string): string | null {
	const valid = (text: string) => text.trim() === '' || thresholdSetting(text) !== null;
	if (!valid(warnText) || !valid(criticalText)) return 'Enter a whole number from 1 to 100.';
	const warn = Number(thresholdSetting(warnText) ?? DEFAULT_THRESHOLDS.warn);
	const critical = Number(thresholdSetting(criticalText) ?? DEFAULT_THRESHOLDS.critical);
	return warn < critical ? null : 'Warning must be below critical.';
}
