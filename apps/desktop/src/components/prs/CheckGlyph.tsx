import { Check, Circle, Minus, X } from 'lucide-react';

/** The mark for a check's outcome: the git-added green for a pass, destructive
 *  for a failure (or a cancelled run, which the backend counts as one), muted
 *  for everything still undecided or not applicable (specs/05-features.md F31).
 *  Static — no spinner for a running check, so a list of them holds still. */
export function CheckGlyph({
	state,
	label,
	testId,
}: {
	state: 'success' | 'failure' | 'pending' | 'skipped' | 'neutral' | 'cancelled' | 'none';
	label: string;
	testId?: string;
}) {
	const common = { 'aria-label': label, title: label, 'data-testid': testId };
	if (state === 'success') {
		return <Check {...common} className="size-3.5 shrink-0 text-git-added" />;
	}
	if (state === 'failure' || state === 'cancelled') {
		return <X {...common} className="size-3.5 shrink-0 text-destructive" />;
	}
	if (state === 'skipped' || state === 'neutral') {
		return <Minus {...common} className="size-3.5 shrink-0 text-muted-foreground" />;
	}
	return <Circle {...common} className="size-3 shrink-0 text-muted-foreground" />;
}
