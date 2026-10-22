import type { ImprovementCandidate } from '@factorai/types';
import { CHIP_SHAPE } from '@lib/gitGraph';
import { statusBadge, verificationOf } from '@lib/intelligenceView';
import type { ReactNode } from 'react';

/** A neutral chip: the app's shape in its neutral hue (DESIGN.md § Chips). The
 *  coloured chips mean something by their colour; these name a kind. */
export function Chip({
	children,
	testId,
	title,
}: {
	children: ReactNode;
	testId?: string;
	title?: string;
}) {
	return (
		<span
			data-testid={testId}
			title={title}
			className={`${CHIP_SHAPE} border-border bg-secondary text-secondary-foreground`}
		>
			{children}
		</span>
	);
}

/**
 * The status chip. **Blue only where it is the person's move**: a change that
 * is saved and that nobody has checked asks to be checked (DESIGN.md § The One
 * Accent Rule). Every other status is a neutral fact.
 */
export function StatusChip({
	candidate,
}: {
	candidate: Pick<ImprovementCandidate, 'status' | 'events'>;
}) {
	const unverified =
		candidate.status === 'implemented' && verificationOf(candidate).kind === 'unverified';
	return (
		<span
			data-testid="candidate-status"
			className={`${CHIP_SHAPE} ${
				unverified
					? 'border-primary/30 bg-primary/10 text-primary'
					: 'border-border bg-secondary text-secondary-foreground'
			}`}
		>
			{statusBadge(candidate)}
		</span>
	);
}

/** The section label style: 12px, uppercase, tracked (DESIGN.md § Typography). */
export function Heading({ children }: { children: ReactNode }) {
	return (
		<h4 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
			{children}
		</h4>
	);
}
