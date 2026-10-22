import type { IntelligenceProgressEvent } from '@factorai/types';
import { Button } from '@factorai/ui';
import { phaseLabel, progressFraction } from '@lib/intelligenceView';
import { cmd } from '@lib/tauri';
import { useMutation } from '@tanstack/react-query';

/**
 * An analysis in progress (specs/05-features.md F33; the brief's "Running":
 * progress, Cancel, no writes to files).
 *
 * The bar is the sessions done out of the sessions read - counts the backend
 * reports, not an estimate - and it is absent until a total is known, because
 * an empty or full bar before then would be a number made up to fill the space.
 * Cancel asks the backend to stop the run and its process; the run ends with
 * `intelligence:run-finished` as `cancelled`, and **nothing is saved** from a
 * cancelled run, which is why the wording can promise it.
 */
export function RunningPanel({
	runId,
	progress,
	offline,
}: {
	runId: string;
	progress: IntelligenceProgressEvent | null;
	/** Known only for a run started in this window. */
	offline?: boolean;
}) {
	const cancel = useMutation({ mutationFn: () => cmd.cancelAnalysis(runId) });
	const fraction = progressFraction(progress);
	return (
		<section
			data-testid="intelligence-running"
			aria-labelledby="intelligence-running-title"
			className="space-y-3 rounded-md border border-border bg-card p-4"
		>
			<div className="space-y-0.5">
				<h3 id="intelligence-running-title" className="font-medium text-sm">
					{offline ? 'Listing signals' : 'Analyzing'}
				</h3>
				<p className="text-muted-foreground text-xs" data-testid="intelligence-running-phase">
					{progress
						? `${phaseLabel(progress.phase)} · ${progress.sessionsDone} of ${progress.sessionsTotal} sessions · ${progress.signals} signal${progress.signals === 1 ? '' : 's'}`
						: 'Starting…'}
				</p>
			</div>

			{fraction !== null && (
				<div
					role="progressbar"
					aria-label="Sessions analyzed"
					aria-valuemin={0}
					aria-valuemax={100}
					aria-valuenow={Math.round(fraction * 100)}
					className="h-1 overflow-hidden rounded-full bg-secondary"
				>
					<div
						className="h-full bg-status-working transition-[width]"
						style={{ width: `${fraction * 100}%` }}
					/>
				</div>
			)}

			<p className="text-muted-foreground text-xs">
				No file in your project is written during an analysis. A cancelled or failed run saves
				nothing.
			</p>

			<Button
				variant="outline"
				size="sm"
				data-testid="intelligence-cancel"
				disabled={cancel.isPending || cancel.isSuccess}
				onClick={() => cancel.mutate()}
			>
				{cancel.isSuccess ? 'Cancelling…' : 'Cancel analysis'}
			</Button>
			{cancel.isError && (
				<p role="alert" className="text-destructive text-xs">
					Could not cancel: the run may already have finished.
				</p>
			)}
		</section>
	);
}
