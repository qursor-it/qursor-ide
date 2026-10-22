import type { ImprovementCandidate, IntelligenceRunDetail } from '@factorai/types';
import { Button } from '@factorai/ui';
import { EmptyHero } from '@components/layout/EmptyHero';
import { formatRelative } from '@lib/format';
import { ACTION_LABELS, describeRunError, sectionCandidates } from '@lib/intelligenceView';
import { ChevronRight, Lightbulb } from 'lucide-react';
import { useState } from 'react';
import { CandidateCard } from './CandidateCard';
import { Heading } from './chips';
import { EvidenceList } from './EvidenceList';
import { Notice } from './Notice';

/**
 * What the last analysis found, and what is still open (specs/05-features.md F33).
 *
 * Four honest outcomes, each in its own words and none dressed as another:
 * **suggestions** (at most five per run, grouped by the kind of remedy);
 * **no actionable patterns**, which is a result and shows no invented content;
 * **unreviewed signals**, when no model looked, which are observations and not
 * insights; and a **failed or cancelled run**, which says what was and was not
 * saved. Suggestions from earlier runs stay below, by where they are in their
 * life, because a re-run never resets a decision.
 */
export function ResultsPanel({
	projectId,
	latest,
	candidates,
	onAnalyze,
}: {
	projectId: string;
	latest: IntelligenceRunDetail | null;
	candidates: readonly ImprovementCandidate[];
	onAnalyze: () => void;
}) {
	const [openId, setOpenId] = useState<string | null>(null);
	const [showClosed, setShowClosed] = useState(false);
	const run = latest?.run ?? null;
	const sections = sectionCandidates(candidates);

	if (run === null && candidates.length === 0) {
		return (
			<EmptyHero
				icon={<Lightbulb />}
				title="What do this project’s sessions say?"
				description="Look back over the last completed sessions and get at most five suggestions for what to change, or none if nothing is justified. You see exactly what would be read and sent before anything is."
				action={
					<Button size="sm" data-testid="intelligence-start" onClick={onAnalyze}>
						Analyze sessions
					</Button>
				}
			/>
		);
	}

	const card = (candidate: ImprovementCandidate) => (
		<CandidateCard
			key={candidate.id}
			candidate={candidate}
			projectId={projectId}
			open={openId === candidate.id}
			onToggle={() => setOpenId((current) => (current === candidate.id ? null : candidate.id))}
		/>
	);

	return (
		<div className="space-y-5" data-testid="intelligence-results">
			<header className="flex items-start gap-3">
				<div className="min-w-0 flex-1 space-y-0.5">
					{run && (
						<p className="text-muted-foreground text-xs" data-testid="intelligence-last-run">
							Last analysis {formatRelative(run.endedAt ?? run.startedAt)} · {run.sessionCount}{' '}
							session{run.sessionCount === 1 ? '' : 's'} · {run.signalCount} signal
							{run.signalCount === 1 ? '' : 's'}
							{run.droppedCount > 0 &&
								` · ${run.droppedCount} suggestion${run.droppedCount === 1 ? '' : 's'} discarded because they could not be checked against the evidence`}
						</p>
					)}
				</div>
				<Button
					variant="outline"
					size="sm"
					className="shrink-0"
					data-testid="intelligence-start"
					onClick={onAnalyze}
				>
					Analyze again
				</Button>
			</header>

			{run && <RunBanner run={run} onRetry={onAnalyze} />}

			{run?.state === 'completed' && run.outcome === 'no_change' && (
				<div
					data-testid="intelligence-no-change"
					className="space-y-1 rounded-md border border-border bg-card px-4 py-3"
				>
					<p className="font-medium text-sm">No actionable patterns</p>
					<p className="text-muted-foreground text-xs">
						These sessions do not justify a change, so none is suggested. Nothing was added to your
						project.
					</p>
				</div>
			)}

			{run?.state === 'completed' && run.outcome === 'signals_only' && latest && (
				<section
					className="space-y-2"
					aria-label="Unreviewed signals"
					data-testid="intelligence-unreviewed"
				>
					<Heading>Unreviewed signals ({latest.signals.length})</Heading>
					<p className="text-muted-foreground text-xs">
						These are observations found on this machine. No model reviewed them, so they are not
						findings and nothing was suggested from them.
					</p>
					<EvidenceList evidence={latest.signals} projectId={projectId} />
				</section>
			)}

			{sections.review.length > 0 && (
				<section className="space-y-4" aria-label="To review" data-testid="intelligence-review">
					<Heading>To review</Heading>
					{sections.review.map((group) => (
						<div key={group.action} className="space-y-2" data-testid={`group-${group.action}`}>
							<div>
								<h5 className="font-medium text-sm">
									{ACTION_LABELS[group.action].label}
									{group.candidates.length > 1 ? ` (${group.candidates.length})` : ''}
								</h5>
								<p className="text-muted-foreground text-xs">{ACTION_LABELS[group.action].hint}</p>
							</div>
							<ul className="space-y-2">{group.candidates.map(card)}</ul>
						</div>
					))}
				</section>
			)}

			{sections.inUse.length > 0 && (
				<section className="space-y-2" aria-label="In use" data-testid="intelligence-in-use">
					<Heading>Saved changes</Heading>
					<ul className="space-y-2">{sections.inUse.map(card)}</ul>
				</section>
			)}

			{sections.closed.length > 0 && (
				<section className="space-y-2" aria-label="Closed" data-testid="intelligence-closed">
					<Button
						variant="quiet"
						size="sm"
						className="-ml-2 gap-1.5 px-2"
						aria-expanded={showClosed}
						data-testid="intelligence-closed-toggle"
						onClick={() => setShowClosed((v) => !v)}
					>
						<ChevronRight className={`transition-transform ${showClosed ? 'rotate-90' : ''}`} />
						{showClosed ? 'Hide' : 'Show'} {sections.closed.length} closed
					</Button>
					{showClosed && <ul className="space-y-2">{sections.closed.map(card)}</ul>}
				</section>
			)}
		</div>
	);
}

/** A run that did not complete, or that completed degraded, in plain words. */
function RunBanner({ run, onRetry }: { run: IntelligenceRunDetail['run']; onRetry: () => void }) {
	if (run.state === 'cancelled') {
		return (
			<p className="text-muted-foreground text-xs" data-testid="intelligence-cancelled">
				The last analysis was cancelled. Nothing was saved from it.
			</p>
		);
	}
	if (run.state === 'failed') {
		return (
			<Notice
				testId="intelligence-run-error"
				notice={{
					...describeRunError(run.errorCode ?? 'internal', false, run.errorMessage),
					detail: null,
				}}
				onRetry={onRetry}
			/>
		);
	}
	if (run.state === 'completed' && run.errorCode === 'provider_unavailable') {
		return (
			<Notice
				tone="info"
				testId="intelligence-run-degraded"
				notice={{
					...describeRunError('provider_unavailable', true, run.errorMessage),
					detail: null,
				}}
				onRetry={onRetry}
			/>
		);
	}
	return null;
}
