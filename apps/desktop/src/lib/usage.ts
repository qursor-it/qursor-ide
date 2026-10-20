import type { AgentId, ProviderUsage, UsageSource, UsageWindow } from '@factorai/types';
import { AGENTS } from '@lib/agents';

/**
 * The status bar's rules for plan limits (usage monitor). Pure, so the
 * thresholds are tested rather than eyeballed.
 */

/** How close a window is to its limit. Colour is the only alert: `ok` draws
 *  in the muted foreground, `warn` in Usage Amber, `critical` in the
 *  destructive red (`DESIGN.md` § Colors). */
export type UsageLevel = 'ok' | 'warn' | 'critical';

export function levelOf(percent: number): UsageLevel {
	if (percent >= 90) return 'critical';
	if (percent >= 70) return 'warn';
	return 'ok';
}

/** `89%`. Floored, so the number never reaches a threshold before the colour
 *  does: 89.9 is amber and reads 89%, not a red-looking 90%. */
export function formatPercent(percent: number): string {
	const p = Number.isNaN(percent) ? 0 : Math.min(100, Math.max(0, percent));
	return `${Math.floor(p)}%`;
}

const STALE_AFTER_MS = 15 * 60_000;

/** A reading older than 15 minutes is shown muted, never as fresh. */
export function isStale(updatedAt: number, now: number): boolean {
	return now - updatedAt > STALE_AFTER_MS;
}

/** A window whose reset time has passed reads as reset — 0% and no reset
 *  time — rather than as the old number. Rust applies the same rule when it
 *  answers; this covers a snapshot that sat on screen across its own reset. */
export function windowAt(window: UsageWindow | null, now: number): UsageWindow | null {
	if (window?.resetsAt != null && window.resetsAt <= now) {
		return { usedPercent: 0, resetsAt: null };
	}
	return window;
}

/** Time until a reset: `<1m`, `45m`, `2h 14m`, `3d 4h`. Whole minutes, floored,
 *  so the figure never promises more time than there is; a reset already past
 *  clamps to `<1m`. The widget and its tooltip format the same way in Rust. */
export function formatCountdown(resetsAt: number, now: number): string {
	const min = Math.max(0, Math.floor((resetsAt - now) / 60_000));
	if (min < 1) return '<1m';
	if (min < 60) return `${min}m`;
	const hours = Math.floor(min / 60);
	if (hours < 24) return `${hours}h ${min % 60}m`;
	return `${Math.floor(hours / 24)}d ${hours % 24}h`;
}

/** `950`, `12.3k`, `1.8M`. */
export function formatTokens(n: number): string {
	if (n < 1000) return String(Math.round(n));
	const k = Math.round(n / 100) / 10;
	if (k < 1000) return `${trimZero(k)}k`;
	return `${trimZero(Math.round(n / 100_000) / 10)}M`;
}

function trimZero(n: number): string {
	return n.toFixed(1).replace(/\.0$/, '');
}

/** An estimate at API list prices, so it carries a `~`. Unknown price → `—`. */
export function formatCost(usd: number | null): string {
	return usd === null ? '—' : `~$${usd.toFixed(2)}`;
}

export interface BarRow {
	agent: AgentId;
	usage: ProviderUsage | null;
}

/** One row per installed agent, in `AGENTS` order (Claude, then Codex),
 *  carrying the default profile's usage or `null` when none has been seen. An
 *  agent that is not installed has no row; other profiles live in the
 *  popover only. */
export function barRows(usage: readonly ProviderUsage[], installed: readonly AgentId[]): BarRow[] {
	return AGENTS.filter((a) => installed.includes(a.id)).map((a) => ({
		agent: a.id,
		usage: usage.find((u) => u.agent === a.id && u.isDefault) ?? null,
	}));
}

/** Where a reading came from, in the popover's words. */
export function sourceLabel(source: UsageSource): string {
	switch (source) {
		case 'statusLine':
			return 'Claude Code status line';
		case 'claudeApi':
			return 'Anthropic usage endpoint';
		case 'codexAppServer':
			return 'Codex app-server';
		case 'codexLog':
			return 'Codex session log';
	}
}
