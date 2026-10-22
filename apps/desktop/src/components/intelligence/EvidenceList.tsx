import type { EvidenceView, ImprovementEvent } from '@factorai/types';
import { agentName } from '@lib/agents';
import { formatRelative } from '@lib/format';
import { describeEvent, SIGNAL_LABELS } from '@lib/intelligenceView';
import { Link } from '@tanstack/react-router';
import { Chip } from './chips';

/**
 * The observations behind a suggestion (specs/05-features.md F33 § Candidates).
 *
 * Each is a **signal, not a verdict**: what a person typed, or a check that
 * failed, with where it was said. The session it came from opens from here when
 * it is still in the index. When it is not, the words are gone from the database
 * on purpose (retention) and this says so instead of showing nothing - or worse,
 * an empty quote - so a suggestion can outlive its evidence without pretending
 * the evidence is still there.
 */
export function EvidenceList({
	evidence,
	projectId,
}: {
	evidence: readonly EvidenceView[];
	projectId: string;
}) {
	if (evidence.length === 0) {
		return <p className="text-muted-foreground text-xs">No evidence is attached.</p>;
	}
	return (
		<ul className="space-y-2" data-testid="evidence-list">
			{evidence.map((item) => (
				<EvidenceItem key={item.evidenceId} item={item} projectId={projectId} />
			))}
		</ul>
	);
}

function EvidenceItem({ item, projectId }: { item: EvidenceView; projectId: string }) {
	const title = item.sessionTitle?.trim() || `session ${item.sessionId.slice(0, 8)}`;
	return (
		<li
			data-testid="evidence-item"
			data-available={item.sessionAvailable}
			className="space-y-1 rounded-md border border-border px-2.5 py-2"
		>
			<div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
				<Chip>{SIGNAL_LABELS[item.signalType]}</Chip>
				<span className="text-muted-foreground">{agentName(item.sourceAgent)}</span>
				<span className="text-muted-foreground">line {item.locator.line}</span>
				<span className="text-muted-foreground">{formatRelative(item.observedAt)}</span>
			</div>

			<p className="text-xs">
				{item.sessionAvailable ? (
					<Link
						to="/projects/$projectId/sessions/$sessionId"
						params={{ projectId, sessionId: item.sessionId }}
						data-testid="evidence-session-link"
						className="text-primary underline-offset-4 hover:underline"
					>
						{title}
					</Link>
				) : (
					<span className="text-muted-foreground">{title}</span>
				)}
			</p>

			{item.sessionAvailable && item.excerpt !== null ? (
				<>
					<p
						data-testid="evidence-excerpt"
						className="whitespace-pre-wrap break-words font-mono text-secondary-foreground text-xs"
					>
						{item.excerpt}
					</p>
					{item.context && (
						<p className="whitespace-pre-wrap break-words font-mono text-muted-foreground text-xs">
							before it: {item.context}
						</p>
					)}
				</>
			) : (
				<p data-testid="evidence-unavailable" className="text-muted-foreground text-xs">
					Evidence no longer available. This session left the index, and its words were removed with
					it.
				</p>
			)}
		</li>
	);
}

/** The decisions made about a suggestion, oldest first, in words. */
export function Timeline({ events }: { events: readonly ImprovementEvent[] }) {
	if (events.length === 0) return null;
	return (
		<ol className="space-y-1.5" data-testid="candidate-timeline">
			{events.map((event) => {
				const line = describeEvent(event);
				return (
					<li key={event.id} data-event={event.eventType} className="text-xs">
						<div className="flex flex-wrap items-baseline gap-x-2">
							<span className="text-secondary-foreground">{line.label}</span>
							<span className="text-muted-foreground">{formatRelative(event.createdAt)}</span>
						</div>
						{line.detail && <p className="text-muted-foreground">{line.detail}</p>}
					</li>
				);
			})}
		</ol>
	);
}
