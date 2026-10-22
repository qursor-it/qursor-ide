import type { IntelligenceProgressEvent } from '@factorai/types';
import { Button } from '@factorai/ui';
import { describeIntelligenceError } from '@lib/intelligenceView';
import { queryKeys } from '@lib/queryKeys';
import { cmd, events } from '@lib/tauri';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { Notice } from './Notice';
import { PreviewPanel } from './PreviewPanel';
import { ResultsPanel } from './ResultsPanel';
import { RunningPanel } from './RunningPanel';

/**
 * A project's Intelligence tab (specs/05-features.md F33, ADR-0084).
 *
 * Rendered only while the switch is on; the project page decides that. The states
 * follow the brief's table: empty, preview, running, results (which includes
 * "no actionable patterns", "unreviewed signals" and a failed run), and the
 * detail and actions inside each result.
 *
 * **What the run is doing comes from two places on purpose.** `startAnalysis`
 * answers with a run id, and the events say when it ends; but a window reloaded
 * mid-run has neither, and the latest run's row says `running`. The view treats
 * either as running, and treats a `run-finished` it has seen as final even if the
 * row it refetches has not caught up yet, so a fast run is never left spinning.
 */
export function IntelligenceView({ projectId }: { projectId: string }) {
	const queryClient = useQueryClient();
	const [previewing, setPreviewing] = useState(false);
	const [activeRunId, setActiveRunId] = useState<string | null>(null);
	const [progress, setProgress] = useState<IntelligenceProgressEvent | null>(null);
	// Runs whose end has been heard. A run can finish before `startAnalysis` has
	// even resolved (an offline run takes a moment), and then the id arrives for a
	// run that is already over - so "running" is derived against this, not set.
	const [finished, setFinished] = useState<ReadonlySet<string>>(() => new Set());

	const latest = useQuery({
		queryKey: queryKeys.intelligenceRun(projectId),
		queryFn: () => cmd.latestIntelligenceRun(projectId),
		retry: false,
	});
	const candidates = useQuery({
		queryKey: queryKeys.improvements(projectId),
		queryFn: () => cmd.listImprovements(projectId),
		retry: false,
	});

	useEffect(() => {
		const progressed = events.onIntelligenceProgress((p) => {
			if (p.projectId === projectId) setProgress(p);
		});
		const ended = events.onIntelligenceRunFinished((p) => {
			if (p.projectId !== projectId) return;
			setFinished((current) => new Set(current).add(p.runId));
			setProgress(null);
		});
		return () => {
			void progressed.then((un) => un());
			void ended.then((un) => un());
		};
	}, [projectId]);

	// A missed event must not strand the view: the row is the other witness.
	const run = latest.data?.run ?? null;
	useEffect(() => {
		if (activeRunId !== null && run?.id === activeRunId && run.state !== 'running') {
			setActiveRunId(null);
			setProgress(null);
		}
	}, [activeRunId, run]);

	const activeLive = activeRunId !== null && !finished.has(activeRunId) ? activeRunId : null;
	const rowRunning = run?.state === 'running' && !finished.has(run.id) ? run.id : null;
	const runningId = activeLive ?? rowRunning;

	function started(runId: string) {
		setPreviewing(false);
		setActiveRunId(runId);
		void queryClient.invalidateQueries({ queryKey: queryKeys.intelligenceRun(projectId) });
		void queryClient.invalidateQueries({ queryKey: queryKeys.improvements(projectId) });
	}

	const failed = latest.isError ? latest.error : candidates.isError ? candidates.error : null;

	return (
		<div className="flex min-h-full flex-col gap-4 pb-2" data-testid="intelligence-view">
			{runningId !== null ? (
				<RunningPanel runId={runningId} progress={progress} />
			) : previewing ? (
				<PreviewPanel
					projectId={projectId}
					onCancel={() => setPreviewing(false)}
					onStarted={started}
				/>
			) : failed !== null ? (
				<Notice
					notice={describeIntelligenceError(failed)}
					onRetry={() => {
						void latest.refetch();
						void candidates.refetch();
					}}
				/>
			) : latest.isPending || candidates.isPending ? (
				<p className="text-muted-foreground text-sm">Loading…</p>
			) : (
				<ResultsPanel
					projectId={projectId}
					latest={latest.data ?? null}
					candidates={candidates.data ?? []}
					onAnalyze={() => setPreviewing(true)}
				/>
			)}
			{/* Kept out of the panels so a failed first load still offers a way in
			    once the switch is known to be on. */}
			{failed !== null && !previewing && runningId === null && (
				<div>
					<Button variant="outline" size="sm" onClick={() => setPreviewing(true)}>
						Analyze sessions
					</Button>
				</div>
			)}
		</div>
	);
}
