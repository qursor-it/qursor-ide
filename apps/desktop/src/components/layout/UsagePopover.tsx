import type { AgentId, ProviderUsage, SessionUsage, UsageWindow } from '@factorai/types';
import { agentName } from '@lib/agents';
import { formatRelative, formatReset } from '@lib/format';
import {
	formatCost,
	formatTokens,
	isStale,
	levelOf,
	sourceLabel,
	type UsageLevel,
	windowAt,
} from '@lib/usage';

/** The fill for each level. Neutral until it has something to say, then
 *  Usage Amber, then the destructive red (`DESIGN.md` § Colors). */
const FILL: Record<UsageLevel, string> = {
	ok: 'bg-muted-foreground',
	warn: 'bg-usage-warn',
	critical: 'bg-destructive',
};

/** The percentage's ink: muted like the rest of the bar until a threshold. */
const INK: Record<UsageLevel, string> = {
	ok: '',
	warn: 'text-usage-warn',
	critical: 'text-destructive',
};

export function inkOf(percent: number): string {
	return INK[levelOf(percent)];
}

/** One window's bar: a hairline-coloured track and a fill to the percent. */
export function UsageMeter({
	percent,
	testId,
	className = 'w-12',
}: {
	percent: number;
	testId?: string;
	className?: string;
}) {
	const width = Math.min(100, Math.max(0, percent));
	return (
		<span
			aria-hidden="true"
			className={`relative inline-block h-1.5 shrink-0 overflow-hidden rounded-full bg-border ${className}`}
		>
			<span
				data-testid={testId}
				className={`absolute inset-y-0 left-0 ${FILL[levelOf(percent)]}`}
				style={{ width: `${width}%` }}
			/>
		</span>
	);
}

interface UsagePopoverProps {
	usage: readonly ProviderUsage[];
	installed: readonly AgentId[];
	session: SessionUsage | null | undefined;
	clock24: boolean;
	now: number;
}

/**
 * The status bar's detail: every profile of every installed agent, both
 * windows with their exact reset, per-model limits when known, where the
 * reading came from and how old it is — then the session in front, broken
 * down.
 */
export function UsagePopover({ usage, installed, session, clock24, now }: UsagePopoverProps) {
	return (
		<div data-testid="usage-popover" className="flex flex-col gap-3 text-xs">
			{installed.map((agent) => {
				const profiles = usage
					.filter((u) => u.agent === agent)
					.sort(
						(a, b) =>
							Number(b.isDefault) - Number(a.isDefault) ||
							a.profileName.localeCompare(b.profileName),
					);
				return (
					<section key={agent} className="flex flex-col gap-2">
						<h3 className="font-medium text-muted-foreground uppercase tracking-wider">
							{agentName(agent)}
						</h3>
						{profiles.length === 0 ? (
							<p className="text-muted-foreground">No reading yet.</p>
						) : (
							profiles.map((p) => (
								<ProfileBlock key={p.profileId} usage={p} clock24={clock24} now={now} />
							))
						)}
					</section>
				);
			})}
			{session && <SessionBlock session={session} />}
		</div>
	);
}

function ProfileBlock({
	usage,
	clock24,
	now,
}: {
	usage: ProviderUsage;
	clock24: boolean;
	now: number;
}) {
	const stale = isStale(usage.updatedAt, now);
	return (
		<div className="flex flex-col gap-1">
			<div className="flex items-baseline justify-between gap-2">
				<span className="truncate font-medium text-foreground text-sm">{usage.profileName}</span>
				<span className={`shrink-0 text-muted-foreground ${stale ? 'opacity-60' : ''}`}>
					updated {formatRelative(usage.updatedAt, now)}
				</span>
			</div>
			<WindowRow label="5-hour" window={windowAt(usage.fiveHour, now)} clock24={clock24} />
			<WindowRow label="Weekly" window={windowAt(usage.weekly, now)} clock24={clock24} />
			{usage.perModel.map((m) => (
				<WindowRow
					key={m.label}
					label={m.label}
					window={windowAt(m.window, now)}
					clock24={clock24}
				/>
			))}
			<span className="text-muted-foreground">{sourceLabel(usage.source)}</span>
		</div>
	);
}

function WindowRow({
	label,
	window,
	clock24,
}: {
	label: string;
	window: UsageWindow | null;
	clock24: boolean;
}) {
	return (
		<div className="grid grid-cols-[4.5rem_4rem_2.5rem_1fr] items-center gap-2 text-secondary-foreground">
			<span className="truncate">{label}</span>
			{window ? (
				<>
					<UsageMeter percent={window.usedPercent} className="w-16" />
					<span className={`text-right tabular-nums ${inkOf(window.usedPercent)}`}>
						{Math.round(window.usedPercent)}%
					</span>
					<span className="truncate text-muted-foreground">
						{window.resetsAt !== null ? `resets ${formatReset(window.resetsAt, clock24)}` : ''}
					</span>
				</>
			) : (
				<span className="col-span-3 text-muted-foreground">—</span>
			)}
		</div>
	);
}

function SessionBlock({ session }: { session: SessionUsage }) {
	const rows: [string, number][] = [
		['Input', session.inputTokens],
		['Output', session.outputTokens],
		['Cache read', session.cacheReadTokens],
		['Cache write', session.cacheWriteTokens],
	];
	// Codex reports reasoning separately but it is already inside output.
	if (session.reasoningTokens > 0) rows.push(['of which reasoning', session.reasoningTokens]);
	return (
		<section className="flex flex-col gap-1 border-border border-t pt-3">
			<h3 className="mb-1 flex items-baseline justify-between gap-2 font-medium text-muted-foreground uppercase tracking-wider">
				<span>This session</span>
				{session.model && (
					<span className="truncate font-mono font-normal normal-case tracking-normal">
						{session.model}
					</span>
				)}
			</h3>
			{rows.map(([label, n]) => (
				<div key={label} className="flex justify-between gap-2 text-secondary-foreground">
					<span>{label}</span>
					<span className="tabular-nums">{formatTokens(n)}</span>
				</div>
			))}
			<div className="flex justify-between gap-2 font-medium text-foreground">
				<span>Total</span>
				<span className="tabular-nums">
					{formatTokens(session.totalTokens)} tok · {formatCost(session.costUsd)}
				</span>
			</div>
			<p className="text-muted-foreground">Estimated at API list prices.</p>
		</section>
	);
}
