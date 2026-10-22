import type { ImprovementCandidate } from '@factorai/types';
import { Button } from '@factorai/ui';
import { ACTION_LABELS, PROBLEM_LABELS, sessionsLabel } from '@lib/intelligenceView';
import { ChevronRight, Flag } from 'lucide-react';
import type { ReactNode } from 'react';
import { CandidateActions } from './CandidateActions';
import { Chip, Heading, StatusChip } from './chips';
import { EvidenceList, Timeline } from './EvidenceList';

/**
 * One suggestion, collapsed to what lets you choose which to open, and open to
 * everything the backend knows (specs/05-features.md F33 § Candidates).
 *
 * **Every number on it is a count of something real** - independent sessions,
 * evidence items - and nothing is a score. A single session is called a
 * hypothesis in the chip row, where it cannot be missed, because the backend
 * computes that and the UI must not call it a pattern.
 */
export function CandidateCard({
	candidate,
	projectId,
	open,
	onToggle,
}: {
	candidate: ImprovementCandidate;
	projectId: string;
	open: boolean;
	onToggle: () => void;
}) {
	const panelId = `candidate-detail-${candidate.id}`;
	return (
		<li
			data-testid={`candidate-${candidate.id}`}
			data-status={candidate.status}
			data-action={candidate.actionKind}
			className="rounded-md border border-border bg-card"
		>
			<Button
				variant="ghost"
				data-testid="candidate-toggle"
				aria-expanded={open}
				aria-controls={panelId}
				onClick={onToggle}
				className="h-auto w-full items-start justify-start gap-2.5 whitespace-normal px-3 py-2.5 text-left font-normal"
			>
				<ChevronRight
					className={`mt-1 transition-transform ${open ? 'rotate-90' : ''}`}
					aria-hidden
				/>
				<span className="min-w-0 flex-1 space-y-1">
					<span className="flex flex-wrap items-center gap-1.5">
						<Chip testId="candidate-action">{ACTION_LABELS[candidate.actionKind].label}</Chip>
						<Chip>{PROBLEM_LABELS[candidate.problemKind]}</Chip>
						<StatusChip candidate={candidate} />
						<Chip testId="candidate-sessions">{sessionsLabel(candidate)}</Chip>
						{candidate.followUp && (
							<Chip testId="candidate-follow-up">
								<Flag className="size-3" aria-hidden />
								follow-up
							</Chip>
						)}
						{candidate.duplicate && (
							<Chip
								testId="candidate-duplicate"
								title="An instruction file already seems to say this"
							>
								may be a duplicate
							</Chip>
						)}
					</span>
					<span className="block font-medium text-sm">{candidate.title}</span>
					{!open && (
						<span className="line-clamp-2 block text-muted-foreground text-xs">
							{candidate.problem}
						</span>
					)}
				</span>
			</Button>

			{open && <CandidateDetail candidate={candidate} projectId={projectId} id={panelId} />}
		</li>
	);
}

function CandidateDetail({
	candidate,
	projectId,
	id,
}: {
	candidate: ImprovementCandidate;
	projectId: string;
	id: string;
}) {
	return (
		<div
			id={id}
			className="space-y-4 border-border border-t px-3 py-3"
			data-testid="candidate-detail"
		>
			<div className="space-y-3">
				<Fact label="Problem">{candidate.problem}</Fact>
				<Fact label="Suggestion">
					<span
						data-testid="candidate-recommendation"
						className="block rounded-md border border-border bg-secondary/30 px-2 py-1.5"
					>
						{candidate.recommendation}
					</span>
				</Fact>
				<Fact label="Why this remedy">{candidate.rationale}</Fact>
				{candidate.limitations && (
					<Fact label="Limits of this inference">{candidate.limitations}</Fact>
				)}
				{candidate.downgradeNote && (
					<p
						data-testid="candidate-downgrade"
						className="rounded-md border border-border px-2 py-1.5 text-muted-foreground text-xs"
					>
						Weakened by the app: {candidate.downgradeNote}
					</p>
				)}
				{candidate.duplicate && (
					<p
						data-testid="candidate-duplicate-warning"
						className="rounded-md border border-border px-2 py-1.5 text-xs"
					>
						<span className="text-muted-foreground">
							An instruction file may already say this:{' '}
						</span>
						<span className="font-mono">
							{candidate.duplicate.path}:{candidate.duplicate.line}
						</span>{' '}
						<q>{candidate.duplicate.text}</q>
					</p>
				)}
				<p className="text-muted-foreground text-xs">
					{ACTION_LABELS[candidate.actionKind].hint} Based on {sessionsLabel(candidate)}
					{candidate.hypothesis ? ': one session is a hypothesis, not a pattern.' : '.'}
				</p>
			</div>

			<section className="space-y-2" aria-label="Evidence">
				<Heading>Evidence ({candidate.evidence.length})</Heading>
				<EvidenceList evidence={candidate.evidence} projectId={projectId} />
			</section>

			<section className="space-y-2" aria-label="Timeline">
				<Heading>Timeline</Heading>
				<Timeline events={candidate.events} />
			</section>

			<section className="space-y-2" aria-label="Actions">
				<CandidateActions candidate={candidate} projectId={projectId} />
			</section>
		</div>
	);
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
	return (
		<div className="space-y-0.5">
			<Heading>{label}</Heading>
			<div className="text-sm">{children}</div>
		</div>
	);
}
