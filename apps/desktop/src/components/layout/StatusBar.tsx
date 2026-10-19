import type { UsageWindow } from '@factorai/types';
import { Popover, PopoverContent, PopoverTrigger } from '@factorai/ui';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { useInstalledAgents } from '@hooks/useInstalledAgents';
import { agentName } from '@lib/agents';
import { formatClock } from '@lib/cron';
import { queryKeys } from '@lib/queryKeys';
import { cmd } from '@lib/tauri';
import { type BarRow, barRows, formatCost, formatTokens, isStale, windowAt } from '@lib/usage';
import { useAttentionStore } from '@store/attentionStore';
import { usePrefsStore } from '@store/prefsStore';
import { useUsageStore } from '@store/usageStore';
import { inkOf, UsageMeter, UsagePopover } from './UsagePopover';

/**
 * The window's bottom edge: plan limits per agent on the left, the session in
 * front on the right (usage monitor). Full width, below the sidebar and the
 * session column. Hidden entirely when the user turns it off.
 */
export function StatusBar() {
	const show = usePrefsStore((s) => s.showStatusBar);
	return show ? <StatusBarRow /> : null;
}

/** Re-render once a minute: staleness and a window's reset are both about the
 *  clock, and no event arrives when only time has passed. */
function useMinuteClock(): number {
	const [now, setNow] = useState(() => Date.now());
	useEffect(() => {
		const id = window.setInterval(() => setNow(Date.now()), 60_000);
		return () => window.clearInterval(id);
	}, []);
	return now;
}

function StatusBarRow() {
	const usage = useUsageStore((s) => s.usage);
	const installed = useInstalledAgents();
	const sessionId = useAttentionStore((s) => s.activeSessionId);
	const clock24 = usePrefsStore((s) => s.clock24);
	const now = useMinuteClock();

	const session = useQuery({
		queryKey: queryKeys.sessionUsage(sessionId ?? ''),
		queryFn: () => cmd.sessionUsage(sessionId ?? ''),
		enabled: sessionId !== null,
		refetchInterval: sessionId !== null ? 15_000 : false,
	});
	const sessionUsage = sessionId !== null ? session.data : null;

	const rows = barRows(usage, installed);

	return (
		<Popover>
			<PopoverTrigger
				data-testid="status-bar"
				title="Plan limits and session usage"
				className="flex h-6 w-full min-w-0 shrink-0 items-center gap-4 overflow-hidden border-border border-t bg-card px-3 text-left text-muted-foreground text-xs outline-none hover:text-secondary-foreground focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset"
			>
				{rows.map((row) => (
					<ProviderGroup
						key={row.agent}
						agent={row.agent}
						usage={row.usage}
						clock24={clock24}
						now={now}
					/>
				))}
				<span className="min-w-0 flex-1" />
				{sessionUsage && (
					<span
						data-testid="status-bar-session"
						className="min-w-0 truncate whitespace-nowrap tabular-nums"
					>
						{formatTokens(sessionUsage.totalTokens)} tok · {formatCost(sessionUsage.costUsd)}
					</span>
				)}
			</PopoverTrigger>
			<PopoverContent
				side="top"
				align="start"
				className="max-h-(--radix-popover-content-available-height) w-96 overflow-y-auto"
			>
				<UsagePopover
					usage={usage}
					installed={installed}
					session={sessionUsage}
					clock24={clock24}
					now={now}
				/>
			</PopoverContent>
		</Popover>
	);
}

function ProviderGroup({ agent, usage, clock24, now }: BarRow & { clock24: boolean; now: number }) {
	// The short name: the bar is 24px of chrome, and "Claude Code" says nothing
	// "Claude" does not.
	const name = agentName(agent).replace(/ Code$/, '');
	if (!usage) {
		return (
			<span data-testid={`usage-group-${agent}`} className="flex shrink-0 items-center gap-1.5">
				<span className="text-secondary-foreground">{name}</span>
				<span>—</span>
			</span>
		);
	}
	const fiveHour = windowAt(usage.fiveHour, now);
	const weekly = windowAt(usage.weekly, now);
	return (
		<span
			data-testid={`usage-group-${agent}`}
			className={`flex shrink-0 items-center gap-1.5 whitespace-nowrap ${isStale(usage.updatedAt, now) ? 'opacity-60' : ''}`}
		>
			<span className="text-secondary-foreground">{name}</span>
			<WindowCell
				label="5h"
				window={fiveHour}
				testId={`usage-fill-${agent}-5h`}
				reset={fiveHour?.resetsAt != null ? resetClock(fiveHour.resetsAt, clock24) : null}
			/>
			<WindowCell label="wk" window={weekly} testId={`usage-fill-${agent}-wk`} reset={null} />
		</span>
	);
}

function WindowCell({
	label,
	window,
	testId,
	reset,
}: {
	label: string;
	window: UsageWindow | null;
	testId: string;
	reset: string | null;
}) {
	return (
		<span className="ml-1 flex items-center gap-1.5">
			<span>{label}</span>
			{window ? (
				<>
					<UsageMeter percent={window.usedPercent} testId={testId} />
					<span className={`tabular-nums ${inkOf(window.usedPercent)}`}>
						{Math.round(window.usedPercent)}%
					</span>
					{reset && <span className="tabular-nums">· {reset}</span>}
				</>
			) : (
				<span>—</span>
			)}
		</span>
	);
}

function resetClock(ms: number, clock24: boolean): string {
	const d = new Date(ms);
	return formatClock(d.getHours(), d.getMinutes(), clock24);
}
